use anyhow::{Context, Result};
use embedded_svc::http::client::Client as HttpClient;
use esp_idf_svc::http::client::EspHttpConnection;
use serde::{Deserialize, Serialize};

const RESPONSE_BUFFER_BYTES: usize = 4096;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Channel9HttpClient {
    api_base_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct CreateDeviceCodeRequest<'a> {
    workspace_id: &'a str,
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

    pub fn create_device_code(
        &self,
        workspace_id: &str,
        device_id: &str,
        interfaces: &[String],
    ) -> Result<DeviceCode> {
        let request = CreateDeviceCodeRequest {
            workspace_id,
            device_id,
            interfaces,
        };
        let response =
            self.post_json::<_, DeviceCode>("/api/v1/channel9/device-codes", &request)?;
        Ok(response)
    }

    pub fn poll_device_token(&self, device_code: &str) -> Result<PollToken> {
        let request = PollDeviceTokenRequest { device_code };
        match self.post_json::<_, DeviceToken>("/api/v1/channel9/device-codes/token", &request) {
            Ok(token) => Ok(PollToken::Approved(token)),
            Err(error) if error.to_string().contains("authorization_pending") => {
                Ok(PollToken::Pending)
            }
            Err(error) => Err(error),
        }
    }

    fn post_json<T, R>(&self, path: &str, payload: &T) -> Result<R>
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
            .with_context(|| format!("failed to start POST {url}"))?;
        write_all(&mut request, &body)?;
        request.flush()?;
        let mut response = request.submit()?;
        let status = response.status();

        let mut buffer = [0_u8; RESPONSE_BUFFER_BYTES];
        let mut body = Vec::new();
        loop {
            let read = response.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            body.extend_from_slice(&buffer[..read]);
        }

        let envelope: ApiEnvelope<R> = serde_json::from_slice(&body)
            .with_context(|| format!("failed to parse Channel9 response from {url}"))?;
        if !envelope.ok || !(200..300).contains(&status) {
            let error = envelope.error.unwrap_or(ApiErrorBody {
                code: "channel9_request_failed".to_owned(),
                message: format!("Channel9 request failed with HTTP {status}"),
            });
            anyhow::bail!("{}: {}", error.code, error.message);
        }
        envelope
            .data
            .ok_or_else(|| anyhow::anyhow!("Channel9 response missing data"))
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
