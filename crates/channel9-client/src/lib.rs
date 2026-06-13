use anyhow::{Context, Result};
use core::time::Duration;
use embedded_svc::http::client::{Client as HttpClient, Method};
use esp_idf_svc::http::client::{Configuration as HttpConfiguration, EspHttpConnection};
use esp_idf_svc::sys::esp_crt_bundle_attach;
use serde::{Deserialize, Serialize};
use serde_json::Value;

const RESPONSE_BUFFER_BYTES: usize = 1024;
const MAX_RESPONSE_BYTES: usize = 16 * 1024;
const HTTP_TIMEOUT: Duration = Duration::from_secs(15);
const USER_AGENT: &str = "channel9os/0.1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Channel9HttpClient {
    api_base_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct CreateDeviceCodeRequest<'a> {
    device_id: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<&'a str>,
    interfaces: &'a [String],
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct PollDeviceTokenRequest<'a> {
    device_code: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct ApiEnvelope<T> {
    ok: bool,
    data: Option<T>,
    error: Option<ApiErrorBody>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct ApiErrorBody {
    code: String,
    message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct DeviceCode {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub verification_uri_complete: String,
    pub expires_at: i64,
    pub activation_expires_at: i64,
    pub interval_seconds: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct DeviceToken {
    pub access_token: String,
    pub token_type: String,
    pub expires_at: i64,
    pub workspace_id: String,
    pub device_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PollToken {
    Pending,
    Approved(DeviceToken),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceEvents {
    pub connected: bool,
    pub messages: Vec<DeviceMessage>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceMessage {
    pub cursor: String,
    pub message_id: String,
    pub target_interface: Option<String>,
    pub payload: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct DeviceEventBody {
    event_type: String,
    cursor: Option<String>,
    message: Option<MessageSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct MessageSummary {
    message_id: String,
    target: MessageTarget,
    payload: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct MessageTarget {
    interface: Option<String>,
}

impl Channel9HttpClient {
    pub fn new(api_base_url: impl Into<String>) -> Self {
        Self {
            api_base_url: trim_base_url(api_base_url.into()),
        }
    }

    pub fn create_device_code(
        &self,
        device_id: &str,
        description: &str,
        interfaces: &[String],
    ) -> Result<DeviceCode> {
        let request = CreateDeviceCodeRequest {
            device_id,
            description: non_empty_description(description),
            interfaces,
        };
        let envelope =
            self.post_json::<_, DeviceCode>("/api/v1/channel9/device-codes", &request)?;
        if envelope.ok {
            envelope
                .data
                .ok_or_else(|| anyhow::anyhow!("Channel9 response missing data"))
        } else {
            let error = envelope.error.unwrap_or_else(default_api_error);
            anyhow::bail!("{}: {}", error.code, error.message);
        }
    }

    pub fn poll_device_token(&self, device_code: &str) -> Result<PollToken> {
        let request = PollDeviceTokenRequest { device_code };
        let envelope =
            self.post_json::<_, DeviceToken>("/api/v1/channel9/device-codes/token", &request)?;
        if envelope.ok {
            let token = envelope
                .data
                .ok_or_else(|| anyhow::anyhow!("Channel9 response missing data"))?;
            Ok(PollToken::Approved(token))
        } else {
            let error = envelope.error.unwrap_or_else(default_api_error);
            if error.code == "authorization_pending" {
                Ok(PollToken::Pending)
            } else {
                anyhow::bail!("{}: {}", error.code, error.message);
            }
        }
    }

    pub fn open_device_events(
        &self,
        device_id: &str,
        access_token: &str,
        interfaces: &[String],
        cursor: Option<&str>,
    ) -> Result<DeviceEvents> {
        let path = device_events_path(device_id, interfaces, cursor);
        let body = self.get_text(path.as_str(), access_token)?;
        Ok(parse_sse_events(body.as_str()))
    }

    fn post_json<T, R>(&self, path: &str, payload: &T) -> Result<ApiEnvelope<R>>
    where
        T: Serialize,
        R: for<'de> Deserialize<'de>,
    {
        let body = serde_json::to_vec(payload)?;
        let content_length = body.len().to_string();
        let headers = [
            ("content-type", "application/json"),
            ("content-length", content_length.as_str()),
            ("user-agent", USER_AGENT),
        ];
        let url = format!("{}{}", self.api_base_url, path);
        let host = endpoint_host(self.api_base_url.as_str()).unwrap_or("unknown");
        log::info!(
            "channel9 https request: url={}, host={}, sni={}, ca_bundle=esp_crt_bundle_attach, timeout_ms={}",
            url,
            host,
            host,
            HTTP_TIMEOUT.as_millis()
        );
        let http_config = HttpConfiguration {
            timeout: Some(HTTP_TIMEOUT),
            crt_bundle_attach: Some(esp_crt_bundle_attach),
            ..Default::default()
        };
        let mut client = HttpClient::wrap(EspHttpConnection::new(&http_config)?);
        let mut request = client
            .post(url.as_str(), &headers)
            .with_context(|| {
                format!(
                    "failed to connect to Channel9 HTTPS endpoint {url}; check WiFi IP, DNS, TLS time, and server reachability"
                )
            })?;
        write_all(&mut request, &body)?;
        request.flush()?;
        let mut response = request.submit()?;
        let status = response.status();

        let mut buffer = vec![0_u8; RESPONSE_BUFFER_BYTES];
        let mut body = Vec::new();
        loop {
            let read = response.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            if body.len() + read > MAX_RESPONSE_BYTES {
                anyhow::bail!("Channel9 response exceeded {MAX_RESPONSE_BYTES} bytes");
            }
            body.extend_from_slice(&buffer[..read]);
        }

        match serde_json::from_slice::<ApiEnvelope<R>>(&body) {
            Ok(envelope) => {
                if envelope.ok && !(200..300).contains(&status) {
                    anyhow::bail!("Channel9 request failed with HTTP {status}");
                }
                Ok(envelope)
            }
            Err(error) if !(200..300).contains(&status) => {
                anyhow::bail!("Channel9 request failed with HTTP {status}: {error}");
            }
            Err(error) => {
                Err(error).with_context(|| format!("failed to parse Channel9 response from {url}"))
            }
        }
    }

    fn get_text(&self, path: &str, bearer_token: &str) -> Result<String> {
        let auth_header = format!("Bearer {bearer_token}");
        let headers = [
            ("accept", "text/event-stream"),
            ("user-agent", USER_AGENT),
            ("authorization", auth_header.as_str()),
        ];
        let url = format!("{}{}", self.api_base_url, path);
        let host = endpoint_host(self.api_base_url.as_str()).unwrap_or("unknown");
        log::info!(
            "channel9 https request: url={}, host={}, sni={}, ca_bundle=esp_crt_bundle_attach, timeout_ms={}",
            url,
            host,
            host,
            HTTP_TIMEOUT.as_millis()
        );
        let http_config = HttpConfiguration {
            timeout: Some(HTTP_TIMEOUT),
            crt_bundle_attach: Some(esp_crt_bundle_attach),
            ..Default::default()
        };
        let mut client = HttpClient::wrap(EspHttpConnection::new(&http_config)?);
        let mut response = client
            .request(Method::Get, url.as_str(), &headers)
            .with_context(|| {
                format!(
                    "failed to connect to Channel9 HTTPS endpoint {url}; check WiFi IP, DNS, TLS time, and server reachability"
                )
            })?
            .submit()?;
        let status = response.status();
        let mut buffer = vec![0_u8; RESPONSE_BUFFER_BYTES];
        let mut body = Vec::new();
        loop {
            let read = response.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            if body.len() + read > MAX_RESPONSE_BYTES {
                anyhow::bail!("Channel9 response exceeded {MAX_RESPONSE_BYTES} bytes");
            }
            body.extend_from_slice(&buffer[..read]);
        }
        let body = String::from_utf8_lossy(&body).into_owned();
        if !(200..300).contains(&status) {
            anyhow::bail!("Channel9 request failed with HTTP {status}: {body}");
        }
        Ok(body)
    }
}

impl DeviceMessage {
    pub fn display_text(&self) -> String {
        if let Some(object) = self.payload.as_object() {
            for key in ["suggestion", "title", "message", "text", "body", "content"] {
                if let Some(value) = object.get(key).and_then(|value| value.as_str()) {
                    let value = value.trim();
                    if !value.is_empty() {
                        return value.chars().take(64).collect();
                    }
                }
            }
        }
        if let Some(value) = self.payload.as_str() {
            let value = value.trim();
            if !value.is_empty() {
                return value.chars().take(64).collect();
            }
        }
        "New push".to_owned()
    }

    pub fn detail_text(&self) -> String {
        match self.target_interface.as_deref() {
            Some(interface) if !interface.is_empty() => format!("{interface} push received"),
            _ => "Push received".to_owned(),
        }
    }
}

fn default_api_error() -> ApiErrorBody {
    ApiErrorBody {
        code: "channel9_request_failed".to_owned(),
        message: "Channel9 request failed".to_owned(),
    }
}

fn write_all<C>(request: &mut embedded_svc::http::client::Request<C>, mut body: &[u8]) -> Result<()>
where
    C: embedded_svc::http::client::Connection,
    C::Error: std::error::Error + Send + Sync + 'static,
{
    while !body.is_empty() {
        let written = request.write(body)?;
        if written == 0 {
            anyhow::bail!("failed to write Channel9 request body");
        }
        body = &body[written..];
    }
    Ok(())
}

fn trim_base_url(mut value: String) -> String {
    while value.ends_with('/') {
        value.pop();
    }
    value
}

fn device_events_path(device_id: &str, interfaces: &[String], cursor: Option<&str>) -> String {
    let mut path = format!("/api/v1/channel9/devices/{device_id}/events?limit=32&heartbeat=true");
    if !interfaces.is_empty() {
        path.push_str("&interfaces=");
        path.push_str(interfaces.join(",").as_str());
    }
    if let Some(cursor) = cursor.filter(|cursor| !cursor.trim().is_empty()) {
        path.push_str("&cursor=");
        path.push_str(cursor.trim());
    }
    path
}

fn parse_sse_events(body: &str) -> DeviceEvents {
    let mut connected = false;
    let mut messages = Vec::new();
    for block in body.split("\n\n") {
        let mut event_name = "";
        let mut data = "";
        for line in block.lines() {
            if let Some(value) = line.strip_prefix("event:") {
                event_name = value.trim();
            } else if let Some(value) = line.strip_prefix("data:") {
                data = value.trim();
            }
        }
        match event_name {
            "heartbeat" => connected = true,
            "message" => {
                connected = true;
                if let Ok(event) = serde_json::from_str::<DeviceEventBody>(data) {
                    if event.event_type == "message" {
                        if let (Some(cursor), Some(message)) = (event.cursor, event.message) {
                            messages.push(DeviceMessage {
                                cursor,
                                message_id: message.message_id,
                                target_interface: message.target.interface,
                                payload: message.payload,
                            });
                        }
                    }
                }
            }
            _ => {}
        }
    }
    DeviceEvents {
        connected,
        messages,
    }
}

fn non_empty_description(value: &str) -> Option<&str> {
    let value = value.trim();
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

fn endpoint_host(value: &str) -> Option<&str> {
    let without_scheme = value
        .strip_prefix("https://")
        .or_else(|| value.strip_prefix("http://"))
        .unwrap_or(value);
    without_scheme
        .split('/')
        .next()
        .filter(|host| !host.is_empty())
}
