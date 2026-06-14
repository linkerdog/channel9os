use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use channel9_ui::StatusChannel9;
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceCode {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub verification_uri_complete: String,
    pub expires_at: i64,
    pub activation_expires_at: i64,
    pub interval_seconds: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
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

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Channel9LoginState {
    pub active_code: Option<DeviceCode>,
    pub message: String,
    pub pending_request: Option<Channel9LoginRequest>,
    pub inflight_request: Option<Channel9LoginRequest>,
    pub next_poll_at: Option<Instant>,
    pub logout_confirm: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Channel9PushState {
    pub status: StatusChannel9,
    pub suggestion: String,
    pub detail: String,
    pub message_count: usize,
    pub last_cursor: Option<String>,
    pub next_fetch_at: Option<Instant>,
    pub consecutive_failures: u8,
    pub fetch_in_flight: bool,
}

impl Default for Channel9PushState {
    fn default() -> Self {
        Self {
            status: StatusChannel9::Off,
            suggestion: "No pushes yet".to_owned(),
            detail: "Waiting for push".to_owned(),
            message_count: 0,
            last_cursor: None,
            next_fetch_at: None,
            consecutive_failures: 0,
            fetch_in_flight: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Channel9SseDrain {
    pub changed: bool,
    pub new_messages: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel9LoginRequest {
    Create,
    Poll,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Channel9NetworkRequest {
    CreateDeviceCode {
        api_base_url: String,
        device_id: String,
        description: String,
        interfaces: Vec<String>,
    },
    PollDeviceToken {
        api_base_url: String,
        device_code: String,
    },
    FetchDeviceEvents {
        api_base_url: String,
        device_id: String,
        access_token: String,
        interfaces: Vec<String>,
        cursor: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Channel9NetworkResponse {
    DeviceCode {
        device_id: String,
        result: Result<DeviceCode, String>,
    },
    DeviceToken {
        device_code: String,
        result: Result<PollToken, String>,
    },
    DeviceEvents(Result<DeviceEvents, String>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Channel9NetworkDrain {
    pub changed: bool,
    pub new_messages: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Channel9RuntimeConfig {
    pub api_base_url: String,
    pub device_id: String,
    pub description: String,
    pub interfaces: Vec<String>,
    pub workspace_id: String,
    pub access_token: Option<String>,
    pub token_expires_at: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel9LoginBlock {
    DeviceMissing,
    WifiOffline,
}

impl Channel9LoginBlock {
    pub fn label(self) -> &'static str {
        match self {
            Self::DeviceMissing => "Set Device first",
            Self::WifiOffline => "WiFi offline",
        }
    }
}

pub fn channel9_logged_in(config: &Channel9RuntimeConfig) -> bool {
    config.access_token.is_some()
}

pub fn channel9_login_blocked_reason(
    config: &Channel9RuntimeConfig,
    wifi_connected: bool,
) -> Option<Channel9LoginBlock> {
    if config.device_id.trim().is_empty() {
        return Some(Channel9LoginBlock::DeviceMissing);
    }
    if !wifi_connected {
        return Some(Channel9LoginBlock::WifiOffline);
    }
    None
}

pub fn channel9_poll_interval(code: &DeviceCode) -> Duration {
    let seconds = code.interval_seconds.clamp(1, 60) as u64;
    Duration::from_secs(seconds)
}

pub fn channel9_active_code_expired(login: &Channel9LoginState) -> bool {
    let Some(code) = login.active_code.as_ref() else {
        return false;
    };
    let Ok(elapsed) = SystemTime::now().duration_since(UNIX_EPOCH) else {
        return false;
    };
    let now = elapsed.as_secs() as i64;
    now >= code.activation_expires_at
}

pub fn maybe_schedule_channel9_auto_poll(
    config: &Channel9RuntimeConfig,
    login: &mut Channel9LoginState,
) -> bool {
    if channel9_logged_in(config)
        || login.active_code.is_none()
        || login.pending_request.is_some()
        || login.inflight_request.is_some()
    {
        return false;
    }
    if channel9_active_code_expired(login) {
        login.active_code = None;
        login.next_poll_at = None;
        login.message = "Code expired".to_owned();
        return false;
    }

    let should_poll = login
        .next_poll_at
        .map(|next_poll_at| Instant::now() >= next_poll_at)
        .unwrap_or(true);
    if !should_poll {
        return false;
    }

    login.pending_request = Some(Channel9LoginRequest::Poll);
    login.message = "Checking approval".to_owned();
    true
}

pub fn build_channel9_login_request(
    config: &Channel9RuntimeConfig,
    login: &mut Channel9LoginState,
    wifi_connected: bool,
) -> Option<Channel9NetworkRequest> {
    if login.inflight_request.is_some() {
        return None;
    }
    let request = login.pending_request.take()?;
    let network_request = match request {
        Channel9LoginRequest::Create => {
            login.message.clear();
            if let Some(reason) = channel9_login_blocked_reason(config, wifi_connected) {
                login.message = reason.label().to_owned();
                return None;
            }
            Channel9NetworkRequest::CreateDeviceCode {
                api_base_url: config.api_base_url.clone(),
                device_id: config.device_id.clone(),
                description: config.description.clone(),
                interfaces: config.interfaces.clone(),
            }
        }
        Channel9LoginRequest::Poll => {
            login.message.clear();
            if !wifi_connected {
                login.next_poll_at = login
                    .active_code
                    .as_ref()
                    .map(|code| Instant::now() + channel9_poll_interval(code));
                login.message = "WiFi offline".to_owned();
                return None;
            }
            if login.active_code.is_none() {
                login.message = "Create code first".to_owned();
                return None;
            }
            if channel9_active_code_expired(login) {
                login.active_code = None;
                login.next_poll_at = None;
                login.message = "Code expired".to_owned();
                return None;
            }
            let Some(code) = login.active_code.as_ref() else {
                login.message = "Create code first".to_owned();
                return None;
            };
            Channel9NetworkRequest::PollDeviceToken {
                api_base_url: config.api_base_url.clone(),
                device_code: code.device_code.clone(),
            }
        }
    };
    login.inflight_request = Some(request);
    Some(network_request)
}

pub fn backoff_channel9_login_request(
    login: &mut Channel9LoginState,
    request: Channel9LoginRequest,
    delay: Duration,
) {
    login.pending_request = Some(request);
    login.message = "Network busy".to_owned();
    login.next_poll_at = Some(Instant::now() + delay);
}

pub fn maybe_build_channel9_sse_request(
    config: &Channel9RuntimeConfig,
    wifi_connected: bool,
    push: &mut Channel9PushState,
    interval: Duration,
) -> (Channel9SseDrain, Option<Channel9NetworkRequest>) {
    if !channel9_sse_ready(config, wifi_connected) {
        let changed = push.status != StatusChannel9::Off;
        push.status = StatusChannel9::Off;
        push.consecutive_failures = 0;
        push.fetch_in_flight = false;
        if !channel9_logged_in(config) {
            let default_push = Channel9PushState::default();
            if *push != default_push {
                *push = default_push;
                return (
                    Channel9SseDrain {
                        changed: true,
                        new_messages: 0,
                    },
                    None,
                );
            }
        }
        return (
            Channel9SseDrain {
                changed,
                new_messages: 0,
            },
            None,
        );
    }
    if push.fetch_in_flight {
        return (Channel9SseDrain::default(), None);
    }

    let should_poll = push
        .next_fetch_at
        .map(|next_fetch_at| Instant::now() >= next_fetch_at)
        .unwrap_or(true);
    if !should_poll {
        return (Channel9SseDrain::default(), None);
    }
    push.next_fetch_at = Some(Instant::now() + interval);

    let Some(access_token) = config.access_token.as_deref() else {
        let changed = push.status != StatusChannel9::Off;
        push.status = StatusChannel9::Off;
        push.consecutive_failures = 0;
        push.fetch_in_flight = false;
        return (
            Channel9SseDrain {
                changed,
                new_messages: 0,
            },
            None,
        );
    };

    push.fetch_in_flight = true;
    (
        Channel9SseDrain::default(),
        Some(Channel9NetworkRequest::FetchDeviceEvents {
            api_base_url: config.api_base_url.clone(),
            device_id: config.device_id.clone(),
            access_token: access_token.to_owned(),
            interfaces: config.interfaces.clone(),
            cursor: push.last_cursor.clone(),
        }),
    )
}

pub fn channel9_sse_ready(config: &Channel9RuntimeConfig, wifi_connected: bool) -> bool {
    channel9_logged_in(config) && wifi_connected
}

pub fn mark_channel9_sse_failure(
    push: &mut Channel9PushState,
    failure_threshold: u8,
) -> Channel9SseDrain {
    push.fetch_in_flight = false;
    push.consecutive_failures = push.consecutive_failures.saturating_add(1);
    if push.consecutive_failures < failure_threshold {
        return Channel9SseDrain::default();
    }
    let changed = push.status != StatusChannel9::Failed;
    push.status = StatusChannel9::Failed;
    Channel9SseDrain {
        changed,
        new_messages: 0,
    }
}

pub fn apply_channel9_sse_messages(
    push: &mut Channel9PushState,
    messages: Vec<DeviceMessage>,
) -> Channel9SseDrain {
    let mut drain = Channel9SseDrain::default();
    let status_changed = push.status != StatusChannel9::Online;
    push.status = StatusChannel9::Online;
    push.consecutive_failures = 0;
    push.fetch_in_flight = false;
    if messages.is_empty() {
        if push.message_count == 0 {
            let detail_changed = push.detail != "Listening for pushes";
            if detail_changed {
                push.detail = "Listening for pushes".to_owned();
            }
            drain.changed = status_changed || detail_changed;
        } else {
            drain.changed = status_changed;
        }
        return drain;
    }
    for message in messages {
        drain.new_messages = drain.new_messages.saturating_add(1);
        push.message_count = push.message_count.saturating_add(1);
        push.last_cursor = Some(message.cursor.clone());
        push.suggestion = message.display_text();
        push.detail = message.detail_text();
    }
    drain.changed = true;
    drain
}

pub fn apply_channel9_network_response(
    response: Channel9NetworkResponse,
    config: &mut Channel9RuntimeConfig,
    login: &mut Channel9LoginState,
    push: &mut Channel9PushState,
    wifi_connected: bool,
    failure_threshold: u8,
) -> Channel9NetworkDrain {
    match response {
        Channel9NetworkResponse::DeviceCode { device_id, result } => {
            login.inflight_request = None;
            if config.device_id != device_id {
                return Channel9NetworkDrain::default();
            }
            match result {
                Ok(code) => {
                    let user_code = format_user_code(code.user_code.as_str());
                    login.next_poll_at = Some(Instant::now() + channel9_poll_interval(&code));
                    login.message = format!("Waiting {user_code}");
                    login.active_code = Some(code);
                    config.access_token = None;
                    config.token_expires_at = None;
                }
                Err(err) => {
                    login.message = error_label_from_string("Create failed", &err);
                }
            }
            Channel9NetworkDrain {
                changed: true,
                new_messages: 0,
            }
        }
        Channel9NetworkResponse::DeviceToken {
            device_code,
            result,
        } => {
            login.inflight_request = None;
            if login
                .active_code
                .as_ref()
                .map(|code| code.device_code.as_str())
                != Some(device_code.as_str())
            {
                return Channel9NetworkDrain::default();
            }
            let Some(code) = login.active_code.as_ref() else {
                return Channel9NetworkDrain::default();
            };
            match result {
                Ok(PollToken::Pending) => {
                    let user_code = format_user_code(code.user_code.as_str());
                    login.next_poll_at = Some(Instant::now() + channel9_poll_interval(code));
                    login.message = format!("Waiting {user_code}");
                }
                Ok(PollToken::Approved(token)) => {
                    config.access_token = Some(token.access_token);
                    config.token_expires_at = Some(token.expires_at);
                    config.workspace_id = token.workspace_id;
                    config.device_id = token.device_id;
                    login.message = "Activated".to_owned();
                    login.active_code = None;
                    login.next_poll_at = None;
                }
                Err(err) => {
                    login.next_poll_at = login
                        .active_code
                        .as_ref()
                        .map(|code| Instant::now() + channel9_poll_interval(code));
                    login.message = error_label_from_string("Poll failed", &err);
                }
            }
            Channel9NetworkDrain {
                changed: true,
                new_messages: 0,
            }
        }
        Channel9NetworkResponse::DeviceEvents(result) => {
            if !channel9_sse_ready(config, wifi_connected) {
                push.fetch_in_flight = false;
                return Channel9NetworkDrain::default();
            }
            let sse_drain = match result {
                Ok(events) => {
                    if !events.connected {
                        mark_channel9_sse_failure(push, failure_threshold)
                    } else {
                        apply_channel9_sse_messages(push, events.messages)
                    }
                }
                Err(_) => mark_channel9_sse_failure(push, failure_threshold),
            };
            Channel9NetworkDrain {
                changed: sse_drain.changed,
                new_messages: sse_drain.new_messages,
            }
        }
    }
}

pub fn format_user_code(value: &str) -> String {
    let mut output = String::new();
    let normalized = value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .map(|ch| ch.to_ascii_uppercase());
    for (index, ch) in normalized.take(12).enumerate() {
        if index == 4 {
            output.push('-');
        }
        output.push(ch);
    }
    output
}

pub fn error_label_from_string(prefix: &str, error: &str) -> String {
    if error.is_empty() {
        return prefix.to_owned();
    }
    let mut message = String::with_capacity(prefix.len() + 2 + 32);
    message.push_str(prefix);
    message.push_str(": ");
    message.extend(error.chars().take(32));
    message
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn runtime_config() -> Channel9RuntimeConfig {
        Channel9RuntimeConfig {
            api_base_url: "https://app.linkerdog.work".to_owned(),
            device_id: "device-a".to_owned(),
            description: "test device".to_owned(),
            interfaces: vec!["display".to_owned(), "speaker".to_owned()],
            workspace_id: String::new(),
            access_token: None,
            token_expires_at: None,
        }
    }

    fn device_code(value: &str) -> DeviceCode {
        DeviceCode {
            device_code: value.to_owned(),
            user_code: "abcd1234".to_owned(),
            verification_uri: "https://example.test/activate".to_owned(),
            verification_uri_complete: "https://example.test/activate?code=ABCD-1234".to_owned(),
            expires_at: i64::MAX,
            activation_expires_at: i64::MAX,
            interval_seconds: 5,
        }
    }

    fn device_message(cursor: &str, text: &str) -> DeviceMessage {
        DeviceMessage {
            cursor: cursor.to_owned(),
            message_id: format!("msg-{cursor}"),
            target_interface: Some("display".to_owned()),
            payload: json!({ "message": text }),
        }
    }

    #[test]
    fn backs_off_login_request_when_network_queue_is_full() {
        let mut login = Channel9LoginState {
            pending_request: Some(Channel9LoginRequest::Poll),
            ..Default::default()
        };
        backoff_channel9_login_request(
            &mut login,
            Channel9LoginRequest::Poll,
            Duration::from_secs(1),
        );

        assert_eq!(login.pending_request, Some(Channel9LoginRequest::Poll));
        assert_eq!(login.message, "Network busy");
        assert!(login.next_poll_at.is_some());
    }

    #[test]
    fn device_code_response_sets_active_code_and_clears_token() {
        let mut config = Channel9RuntimeConfig {
            access_token: Some("old-token".to_owned()),
            token_expires_at: Some(123),
            ..runtime_config()
        };
        let mut login = Channel9LoginState {
            inflight_request: Some(Channel9LoginRequest::Create),
            ..Default::default()
        };
        let mut push = Channel9PushState::default();

        let drain = apply_channel9_network_response(
            Channel9NetworkResponse::DeviceCode {
                device_id: "device-a".to_owned(),
                result: Ok(device_code("dc-1")),
            },
            &mut config,
            &mut login,
            &mut push,
            true,
            3,
        );

        assert!(drain.changed);
        assert!(login.active_code.is_some());
        assert_eq!(login.message, "Waiting ABCD-1234");
        assert_eq!(config.access_token, None);
        assert_eq!(config.token_expires_at, None);
    }

    #[test]
    fn stale_device_code_response_is_ignored() {
        let mut config = runtime_config();
        config.device_id = "device-b".to_owned();
        let mut login = Channel9LoginState {
            inflight_request: Some(Channel9LoginRequest::Create),
            ..Default::default()
        };
        let mut push = Channel9PushState::default();

        let drain = apply_channel9_network_response(
            Channel9NetworkResponse::DeviceCode {
                device_id: "device-a".to_owned(),
                result: Ok(device_code("dc-1")),
            },
            &mut config,
            &mut login,
            &mut push,
            true,
            3,
        );

        assert!(!drain.changed);
        assert!(login.active_code.is_none());
    }

    #[test]
    fn approved_token_activates_login_metadata() {
        let mut config = runtime_config();
        let mut login = Channel9LoginState {
            active_code: Some(device_code("dc-1")),
            inflight_request: Some(Channel9LoginRequest::Poll),
            ..Default::default()
        };
        let mut push = Channel9PushState::default();

        let drain = apply_channel9_network_response(
            Channel9NetworkResponse::DeviceToken {
                device_code: "dc-1".to_owned(),
                result: Ok(PollToken::Approved(DeviceToken {
                    access_token: "token".to_owned(),
                    token_type: "Bearer".to_owned(),
                    expires_at: 456,
                    workspace_id: "workspace".to_owned(),
                    device_id: "device-server".to_owned(),
                })),
            },
            &mut config,
            &mut login,
            &mut push,
            true,
            3,
        );

        assert!(drain.changed);
        assert_eq!(config.access_token.as_deref(), Some("token"));
        assert_eq!(config.workspace_id, "workspace");
        assert_eq!(config.device_id, "device-server");
        assert!(login.active_code.is_none());
        assert_eq!(login.message, "Activated");
    }

    #[test]
    fn stale_token_response_is_ignored() {
        let mut config = runtime_config();
        let mut login = Channel9LoginState {
            active_code: Some(device_code("current")),
            inflight_request: Some(Channel9LoginRequest::Poll),
            ..Default::default()
        };
        let mut push = Channel9PushState::default();

        let drain = apply_channel9_network_response(
            Channel9NetworkResponse::DeviceToken {
                device_code: "old".to_owned(),
                result: Ok(PollToken::Pending),
            },
            &mut config,
            &mut login,
            &mut push,
            true,
            3,
        );

        assert!(!drain.changed);
        assert_eq!(
            login
                .active_code
                .as_ref()
                .map(|code| code.device_code.as_str()),
            Some("current")
        );
    }

    #[test]
    fn sse_empty_message_response_does_not_repaint_when_unchanged() {
        let mut push = Channel9PushState {
            status: StatusChannel9::Online,
            detail: "Listening for pushes".to_owned(),
            fetch_in_flight: true,
            ..Default::default()
        };

        let drain = apply_channel9_sse_messages(&mut push, Vec::new());

        assert!(!drain.changed);
        assert_eq!(drain.new_messages, 0);
        assert!(!push.fetch_in_flight);
    }

    #[test]
    fn sse_messages_update_cursor_and_count() {
        let mut push = Channel9PushState {
            fetch_in_flight: true,
            ..Default::default()
        };

        let drain = apply_channel9_sse_messages(
            &mut push,
            vec![device_message("c1", "Hello"), device_message("c2", "World")],
        );

        assert!(drain.changed);
        assert_eq!(drain.new_messages, 2);
        assert_eq!(push.message_count, 2);
        assert_eq!(push.last_cursor.as_deref(), Some("c2"));
        assert_eq!(push.suggestion, "World");
        assert_eq!(push.detail, "display push received");
        assert!(!push.fetch_in_flight);
    }

    #[test]
    fn logout_or_offline_clears_push_state() {
        let config = runtime_config();
        let mut push = Channel9PushState {
            status: StatusChannel9::Online,
            suggestion: "Old".to_owned(),
            detail: "Old detail".to_owned(),
            message_count: 3,
            last_cursor: Some("cursor".to_owned()),
            fetch_in_flight: true,
            ..Default::default()
        };

        let (drain, request) =
            maybe_build_channel9_sse_request(&config, false, &mut push, Duration::from_secs(10));

        assert!(drain.changed);
        assert!(request.is_none());
        assert_eq!(push, Channel9PushState::default());
    }

    #[test]
    fn logged_in_state_builds_sse_request_once() {
        let config = Channel9RuntimeConfig {
            access_token: Some("token".to_owned()),
            ..runtime_config()
        };
        let mut push = Channel9PushState::default();

        let (drain, request) =
            maybe_build_channel9_sse_request(&config, true, &mut push, Duration::from_secs(10));

        assert!(!drain.changed);
        assert!(matches!(
            request,
            Some(Channel9NetworkRequest::FetchDeviceEvents { .. })
        ));
        assert!(push.fetch_in_flight);

        let (_drain, second_request) =
            maybe_build_channel9_sse_request(&config, true, &mut push, Duration::from_secs(10));
        assert!(second_request.is_none());
    }
}
