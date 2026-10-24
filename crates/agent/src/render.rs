//! Plain-text rendering for the CLI.
//!
//! Responses are validated against the desktop schema before rendering, and
//! states are spelled out: a partial checkpoint, a stale source or an
//! unavailable provider is always named, never shown as complete.

use reopen_protocol::{DecodeError, decode_desktop_payload, generated as wire};
use serde_json::{Map, Value};

fn name<T: serde::Serialize>(value: &T) -> String {
    match serde_json::to_value(value) {
        Ok(Value::String(text)) => text,
        _ => "?".to_owned(),
    }
}

fn summary_line(summary: &wire::CheckpointSummary) -> String {
    let mut line = format!(
        "{}  {:<8}  {:<9}  {:>4} resources  {}",
        summary.captured_at.as_str(),
        name(&summary.completeness),
        name(&summary.reason),
        summary.resource_count,
        summary.id,
    );
    if summary.protected {
        line.push_str("  protected");
    }
    line
}

/// # Errors
///
/// [`DecodeError`] if the response does not match `StatusResult`.
pub fn status(payload: Map<String, Value>) -> Result<String, DecodeError> {
    let status: wire::StatusResult = decode_desktop_payload("StatusResult", payload)?;
    let last = status
        .last_checkpoint
        .as_ref()
        .map_or_else(|| "none yet".to_owned(), summary_line);
    Ok(format!(
        "Re-Open agent {} (protocol {}), capture {}\nSession: {}\nLast checkpoint: {last}\nPending recovery candidates: {}\n",
        status.agent_version.as_str(),
        status.protocol_version,
        name(&status.capture),
        status.session_id,
        status.pending_candidates,
    ))
}

/// # Errors
///
/// [`DecodeError`] if the response does not match `CaptureRequestResult`.
pub fn capture(payload: Map<String, Value>) -> Result<String, DecodeError> {
    let result: wire::CaptureRequestResult =
        decode_desktop_payload("CaptureRequestResult", payload)?;
    Ok(if result.accepted {
        "Capture requested.\n".to_owned()
    } else {
        "A capture is already pending; this request was merged into it.\n".to_owned()
    })
}

/// # Errors
///
/// [`DecodeError`] if the response does not match `CheckpointsListResult`.
pub fn history(payload: Map<String, Value>) -> Result<String, DecodeError> {
    let list: wire::CheckpointsListResult =
        decode_desktop_payload("CheckpointsListResult", payload)?;
    if list.checkpoints.is_empty() {
        return Ok("No checkpoints yet.\n".to_owned());
    }
    Ok(list
        .checkpoints
        .iter()
        .map(|summary| summary_line(summary) + "\n")
        .collect())
}

fn source_line(source: &wire::SourceSummary) -> String {
    let state = name(&source.state);
    let mut line = format!("  {:<24} {state}", source.source_key.as_str());
    match (&source.state, &source.captured_at) {
        (wire::SourceState::Stale, Some(at)) => {
            line.push_str(&format!(", data from {}", at.as_str()))
        }
        (wire::SourceState::Stale, None) => line.push_str(", capture time unknown"),
        _ => {}
    }
    if let Some(error) = &source.error {
        line.push_str(&format!(" ({})", error.as_str()));
    }
    if !source.required {
        line.push_str(", optional");
    }
    line
}

/// # Errors
///
/// [`DecodeError`] if the response does not match `CheckpointDetail`.
pub fn detail(payload: Map<String, Value>) -> Result<String, DecodeError> {
    let detail: wire::CheckpointDetail = decode_desktop_payload("CheckpointDetail", payload)?;
    let mut out = format!("{}\n", summary_line(&detail.summary));
    if detail.summary.completeness == wire::Completeness::Partial {
        out.push_str("Partial: not every required source was captured.\n");
    }
    out.push_str("Sources:\n");
    for source in &detail.sources {
        out.push_str(&source_line(source));
        out.push('\n');
    }
    out.push_str("Resources:\n");
    if detail.resources.is_empty() {
        out.push_str("  none\n");
    }
    for resource in &detail.resources {
        out.push_str(&format!(
            "  {:>3}  {:<12} {:<24} {}\n",
            resource.ordinal,
            name(&resource.kind),
            resource.adapter_id.as_str(),
            resource.label.as_str()
        ));
    }
    Ok(out)
}

/// # Errors
///
/// [`DecodeError`] if the response does not match `RestorePlan`.
pub fn plan(payload: Map<String, Value>) -> Result<String, DecodeError> {
    let plan: wire::RestorePlan = decode_desktop_payload("RestorePlan", payload)?;
    let mut out = format!(
        "Plan {} for checkpoint {} (valid for {} s)\n",
        plan.plan_id, plan.checkpoint_id, plan.expires_in_seconds
    );
    for warning in &plan.warnings {
        let text = match warning.as_str() {
            "duplicate_detection_limited" => {
                "only running apps are visible, so some may already be open"
            }
            "duplicate_detection_unavailable" => "could not check which apps are already open",
            other => other,
        };
        out.push_str(&format!("Warning: {text}\n"));
    }
    let mut launches = 0;
    for action in &plan.actions {
        let what = match (&action.disposition, &action.reason) {
            (wire::Disposition::Launch, _) if action.possible_duplicate => {
                "launch (may already be open)".to_owned()
            }
            (wire::Disposition::Launch, _) => "launch".to_owned(),
            (wire::Disposition::AlreadyOpen, _) => "already open".to_owned(),
            (wire::Disposition::Skip, Some(reason)) => format!("skip ({})", reason.as_str()),
            (wire::Disposition::Skip, None) => "skip".to_owned(),
        };
        if action.disposition == wire::Disposition::Launch {
            launches += 1;
        }
        out.push_str(&format!(
            "  {:>3}  {:<30} {}\n",
            action.ordinal,
            what,
            action.label.as_str()
        ));
    }
    out.push_str(&format!(
        "{launches} to launch. Start with: restore start {}\n",
        plan.plan_id
    ));
    Ok(out)
}

/// # Errors
///
/// [`DecodeError`] if the response does not match `RestoreRunRef`.
pub fn run_started(payload: Map<String, Value>) -> Result<String, DecodeError> {
    let run: wire::RestoreRunRef = decode_desktop_payload("RestoreRunRef", payload)?;
    Ok(format!(
        "Restore started: {}\nFollow with: restore status {}\n",
        run.run_id, run.run_id
    ))
}

/// # Errors
///
/// [`DecodeError`] if the response does not match `RestoreStatus`.
pub fn run_status(payload: Map<String, Value>) -> Result<String, DecodeError> {
    let status: wire::RestoreStatus = decode_desktop_payload("RestoreStatus", payload)?;
    let mut out = format!("Restore {}: {}\n", status.run_id, name(&status.state));
    for action in &status.actions {
        let result = action
            .result
            .as_ref()
            .map_or(String::new(), |result| format!(" ({})", result.as_str()));
        out.push_str(&format!(
            "  {:>3}  {:<13}{result}  {}\n",
            action.ordinal,
            name(&action.state),
            action.label.as_str()
        ));
    }
    Ok(out)
}

/// # Errors
///
/// [`DecodeError`] if the response does not match `RestoreCancelResult`.
pub fn run_cancelled(payload: Map<String, Value>) -> Result<String, DecodeError> {
    let result: wire::RestoreCancelResult = decode_desktop_payload("RestoreCancelResult", payload)?;
    Ok(if result.cancelled {
        "Cancelling: no further apps will be launched; ones already started stay open.\n".to_owned()
    } else {
        "That restore is not running.\n".to_owned()
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn object(value: &Value) -> Map<String, Value> {
        value.as_object().cloned().unwrap()
    }

    #[test]
    fn detail_names_partial_stale_and_unavailable_sources() {
        let payload = json!({
            "summary": {
                "id": "0b9c3f0e-5d6a-4c8e-9f1a-2b3c4d5e6f70",
                "session_id": "9ddac584-c8a5-49ca-af3f-4f93fcc82c8d",
                "captured_at": "2026-10-23T12:00:00Z",
                "reason": "scheduled",
                "completeness": "partial",
                "resource_count": 1,
                "protected": true
            },
            "sources": [
                { "source_key": "browser:a", "state": "stale", "required": true, "captured_at": "2026-10-23T11:00:00Z", "error": null },
                { "source_key": "browser:b", "state": "unavailable", "required": true, "captured_at": null, "error": "timeout" },
                { "source_key": "desktop.apps", "state": "complete", "required": true, "captured_at": "2026-10-23T12:00:00Z", "error": null }
            ],
            "resources": [
                { "ordinal": 0, "kind": "application", "adapter_id": "fake.apps", "label": "org.example.Editor" }
            ]
        });
        let text = detail(object(&payload)).unwrap();
        assert!(text.contains("partial"), "{text}");
        assert!(text.contains("Partial: not every required source was captured."));
        assert!(text.contains("stale, data from 2026-10-23T11:00:00Z"));
        assert!(text.contains("unavailable (timeout)"));
        assert!(text.contains("protected"));
        assert!(text.contains("org.example.Editor"));
    }

    #[test]
    fn invalid_responses_are_rejected_not_rendered() {
        let payload = json!({ "checkpoints": [{ "id": "x" }] });
        assert!(history(object(&payload)).is_err());
    }

    #[test]
    fn empty_history_and_merged_capture_are_explicit() {
        assert_eq!(
            history(object(&json!({ "checkpoints": [] }))).unwrap(),
            "No checkpoints yet.\n"
        );
        assert!(
            capture(object(&json!({ "accepted": false })))
                .unwrap()
                .contains("merged")
        );
    }
}
