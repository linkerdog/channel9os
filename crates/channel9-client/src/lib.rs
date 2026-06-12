use anyhow::{Context, Result};
use embedded_svc::http::client::Client as HttpClient;
use esp_idf_svc::http::client::EspHttpConnection;
use serde::{Deserialize, Serialize};

const RESPONSE_BUFFER_BYTES: usize = 1024;
const MAX_RESPONSE_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Channel9HttpClient {
    api_base_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct CreateDeviceCodeRequest<'a> {
    device_id: &'a str,
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

impl Channel9HttpClient {
    pub fn new(api_base_url: impl Into<String>) -> Self {
        Self {
            api_base_url: trim_base_url(api_base_url.into()),
        }
    }

    pub fn create_device_code(&self, device_id: &str, interfaces: &[String]) -> Result<DeviceCode> {
        let request = CreateDeviceCodeRequest {
            device_id,
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
        ];
        let url = format!("{}{}", self.api_base_url, path);
        let mut client = HttpClient::wrap(EspHttpConnection::new(&Default::default())?);
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
