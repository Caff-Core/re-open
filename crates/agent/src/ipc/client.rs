//! A desktop-role client, used by the CLI (and later the Tauri shell).

use std::path::Path;
use std::time::Duration;

use reopen_protocol::{ErrorCode, PROTOCOL_VERSION, decode_envelope, decode_error_response};
use serde_json::{Map, Value, json};
use tokio::net::UnixStream;
use tokio::time::timeout;
use uuid::Uuid;

use super::frame::{read_frame, write_frame};

/// Time allowed for one request and its response.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientError {
    /// No agent is listening for this user.
    AgentNotRunning,
    Timeout,
    Io(std::io::ErrorKind),
    /// The response was malformed or did not match the request.
    Protocol,
    /// The agent rejected the request.
    Remote {
        code: ErrorCode,
        retryable: bool,
        message: String,
    },
}

impl std::fmt::Display for ClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AgentNotRunning => f.write_str("the Re-Open agent is not running"),
            Self::Timeout => f.write_str("the Re-Open agent did not respond in time"),
            Self::Io(kind) => write!(f, "cannot talk to the agent: {kind}"),
            Self::Protocol => f.write_str("the agent sent an unexpected response"),
            Self::Remote { code, message, .. } => write!(f, "{message} ({code:?})"),
        }
    }
}

impl std::error::Error for ClientError {}

/// Sends one request and returns the payload of its `<kind>.result` response.
///
/// # Errors
///
/// [`ClientError`] for connection, timeout, protocol or remote failures.
pub async fn request(
    socket: &Path,
    kind: &str,
    payload: Value,
) -> Result<Map<String, Value>, ClientError> {
    timeout(REQUEST_TIMEOUT, exchange(socket, kind, payload))
        .await
        .map_err(|_| ClientError::Timeout)?
}

async fn exchange(
    socket: &Path,
    kind: &str,
    payload: Value,
) -> Result<Map<String, Value>, ClientError> {
    let mut stream = UnixStream::connect(socket)
        .await
        .map_err(|error| match error.kind() {
            std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused => {
                ClientError::AgentNotRunning
            }
            other => ClientError::Io(other),
        })?;
    let request_id = Uuid::new_v4();
    let body = json!({
        "protocol_version": PROTOCOL_VERSION,
        "request_id": request_id,
        "kind": kind,
        "payload": payload,
    });
    let body = serde_json::to_vec(&body).map_err(|_| ClientError::Protocol)?;
    write_frame(&mut stream, &body)
        .await
        .map_err(|_| ClientError::Protocol)?;
    let frame = read_frame(&mut stream)
        .await
        .map_err(|_| ClientError::Protocol)?
        .ok_or(ClientError::Protocol)?;

    let response = decode_envelope(&frame).map_err(|_| ClientError::Protocol)?;
    if response.request_id != request_id && !response.request_id.is_nil() {
        return Err(ClientError::Protocol);
    }
    if response.kind.as_str() == "error" {
        let bytes = serde_json::to_vec(&response.payload).map_err(|_| ClientError::Protocol)?;
        let error = decode_error_response(&bytes).map_err(|_| ClientError::Protocol)?;
        return Err(ClientError::Remote {
            code: error.code,
            retryable: error.retryable,
            message: error.message.to_string(),
        });
    }
    if response.kind.as_str() != format!("{kind}.result") {
        return Err(ClientError::Protocol);
    }
    Ok(response.payload)
}
