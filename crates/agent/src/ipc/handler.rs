//! Desktop-role request handling.
//!
//! Every request is decoded as an envelope, its payload validated against the
//! desktop schema definition for its operation, and every response validated
//! before it is sent. Only the operations below exist on this endpoint.

use reopen_core::{CheckpointId, ResourcePayload, SessionId};
use reopen_protocol::{
    DecodeError, ErrorCode, PROTOCOL_VERSION, decode_desktop_payload, decode_envelope,
    encode_desktop_payload, generated as wire,
};
use reopen_storage::{
    CheckpointDetail, CheckpointSummary, Store, StoreError, WriterError, WriterHandle,
};
use serde_json::{Map, Value, json};
use uuid::Uuid;

use crate::scheduler::CaptureTrigger;

/// A failed request: stable code, retry hint and a user-safe message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    pub code: ErrorCode,
    pub retryable: bool,
    pub message: &'static str,
}

impl Failure {
    const fn new(code: ErrorCode, retryable: bool, message: &'static str) -> Self {
        Self {
            code,
            retryable,
            message,
        }
    }
}

impl From<DecodeError> for Failure {
    fn from(error: DecodeError) -> Self {
        Self::new(
            error.code(),
            false,
            "the request is not valid for this operation",
        )
    }
}

/// Serves desktop operations against the agent's state.
#[derive(Debug, Clone)]
pub struct DesktopHandler {
    pub writer: WriterHandle,
    pub trigger: CaptureTrigger,
    pub session_id: SessionId,
}

/// Encodes a response envelope.
#[must_use]
pub fn response_frame(request_id: Uuid, kind: &str, payload: Map<String, Value>) -> Vec<u8> {
    let envelope = json!({
        "protocol_version": PROTOCOL_VERSION,
        "request_id": request_id,
        "kind": kind,
        "payload": payload,
    });
    serde_json::to_vec(&envelope).unwrap_or_default()
}

/// Encodes an error envelope (`kind: "error"`, payload shaped like `ErrorResponse`).
#[must_use]
pub fn error_frame(request_id: Uuid, failure: &Failure) -> Vec<u8> {
    let payload = json!({
        "request_id": request_id,
        "code": failure.code,
        "retryable": failure.retryable,
        "message": failure.message,
    });
    let Value::Object(payload) = payload else {
        return Vec::new();
    };
    response_frame(request_id, "error", payload)
}

/// Best-effort request ID from a message that failed validation, so the
/// client can correlate the error; the nil UUID otherwise.
fn request_id_hint(bytes: &[u8]) -> Uuid {
    serde_json::from_slice::<Value>(bytes)
        .ok()
        .and_then(|value| value.get("request_id")?.as_str()?.parse().ok())
        .unwrap_or_else(Uuid::nil)
}

fn name<T: serde::Serialize>(value: &T) -> Value {
    serde_json::to_value(value).unwrap_or(Value::Null)
}

fn summary_json(summary: &CheckpointSummary) -> Value {
    json!({
        "id": summary.id.to_string(),
        "session_id": summary.session_id.to_string(),
        "captured_at": summary.captured_at.to_string(),
        "reason": name(&summary.reason),
        "completeness": name(&summary.completeness),
        "resource_count": summary.resource_count,
        "protected": summary.protected,
    })
}

/// The user-facing label of a resource. Sent to the local client only, never logged.
fn label(payload: &ResourcePayload) -> &str {
    match payload {
        ResourcePayload::ApplicationV1(app) => &app.app_id,
        ResourcePayload::ProjectV1(project) => &project.path,
        ResourcePayload::FolderV1(folder) => &folder.path,
        ResourcePayload::BrowserTabV1(tab) => &tab.url,
    }
}

fn detail_json(detail: &CheckpointDetail) -> Value {
    let sources: Vec<_> = detail
        .sources
        .iter()
        .map(|source| {
            json!({
                "source_key": source.source_key,
                "state": source.state.as_str(),
                "required": source.required,
                "captured_at": source.captured_at.map(|at| at.to_string()),
                "error": source.error.as_ref().map(name),
            })
        })
        .collect();
    let resources: Vec<_> = detail
        .resources
        .iter()
        .map(|resource| {
            json!({
                "ordinal": resource.ordinal,
                "kind": name(&resource.kind()),
                "adapter_id": resource.adapter.id,
                "label": label(&resource.payload),
            })
        })
        .collect();
    json!({ "summary": summary_json(&detail.summary), "sources": sources, "resources": resources })
}

impl DesktopHandler {
    /// Handles one request frame and returns the response frame.
    pub async fn handle(&self, bytes: &[u8]) -> Vec<u8> {
        let envelope = match decode_envelope(bytes) {
            Ok(envelope) => envelope,
            Err(error) => return error_frame(request_id_hint(bytes), &Failure::from(error)),
        };
        let request_id = envelope.request_id;
        let kind = envelope.kind.to_string();
        match self.dispatch(&kind, envelope.payload).await {
            Ok((definition, payload)) => match encode_desktop_payload(definition, &payload) {
                Ok(payload) => response_frame(request_id, &format!("{kind}.result"), payload),
                Err(_) => error_frame(
                    request_id,
                    &Failure::new(
                        ErrorCode::StorageUnavailable,
                        false,
                        "stored data could not be reported",
                    ),
                ),
            },
            Err(failure) => error_frame(request_id, &failure),
        }
    }

    async fn dispatch(
        &self,
        kind: &str,
        payload: Map<String, Value>,
    ) -> Result<(&'static str, Value), Failure> {
        match kind {
            "status.get" => {
                let _: wire::EmptyRequest = decode_desktop_payload("EmptyRequest", payload)?;
                let (latest, pending) = self
                    .store(|store| {
                        Ok((store.latest_checkpoint()?, store.pending_candidate_count()?))
                    })
                    .await?;
                Ok((
                    "StatusResult",
                    json!({
                        "agent_version": env!("CARGO_PKG_VERSION"),
                        "protocol_version": PROTOCOL_VERSION,
                        "session_id": self.session_id.to_string(),
                        "capture": "active",
                        "last_checkpoint": latest.as_ref().map(summary_json),
                        "pending_candidates": pending,
                    }),
                ))
            }
            "capture.request" => {
                let _: wire::EmptyRequest = decode_desktop_payload("EmptyRequest", payload)?;
                Ok((
                    "CaptureRequestResult",
                    json!({ "accepted": self.trigger.request() }),
                ))
            }
            "checkpoints.list" => {
                let request: wire::CheckpointsListRequest =
                    decode_desktop_payload("CheckpointsListRequest", payload)?;
                let limit = u32::try_from(request.limit.get()).unwrap_or(u32::MAX);
                let list = self
                    .store(move |store| store.list_checkpoints(limit))
                    .await?;
                let checkpoints: Vec<_> = list.iter().map(summary_json).collect();
                Ok((
                    "CheckpointsListResult",
                    json!({ "checkpoints": checkpoints }),
                ))
            }
            "checkpoints.get" => {
                let request: wire::CheckpointGetRequest =
                    decode_desktop_payload("CheckpointGetRequest", payload)?;
                let id = CheckpointId::from_uuid(request.id);
                let detail = self.store(move |store| store.checkpoint_detail(id)).await?;
                Ok(("CheckpointDetail", detail_json(&detail)))
            }
            _ => Err(Failure::new(
                ErrorCode::UnsupportedCapability,
                false,
                "this operation is not available",
            )),
        }
    }

    async fn store<T: Send + 'static>(
        &self,
        query: impl FnOnce(&mut Store) -> Result<T, StoreError> + Send + 'static,
    ) -> Result<T, Failure> {
        let busy = Failure::new(
            ErrorCode::StorageUnavailable,
            true,
            "storage is busy; try again",
        );
        let stopped = Failure::new(
            ErrorCode::StorageUnavailable,
            false,
            "storage is not available",
        );
        let reply = self.writer.submit(query).map_err(|error| match error {
            WriterError::Busy => busy.clone(),
            WriterError::Stopped | WriterError::Lost => stopped.clone(),
        })?;
        match reply.await {
            Ok(Ok(value)) => Ok(value),
            Ok(Err(StoreError::NotFound)) => Err(Failure::new(
                ErrorCode::NotFound,
                false,
                "no such checkpoint",
            )),
            Ok(Err(_)) | Err(_) => Err(stopped),
        }
    }
}
