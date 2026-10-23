//! Runs the shared cases in `fixtures/protocol` (also run by packages/contracts).
#![allow(clippy::unwrap_used)] // Test harness: a panic is the failure report.

use std::fs;
use std::path::{Path, PathBuf};

use reopen_protocol::{DecodeError, ErrorCode, decode_envelope, decode_error_response};
use serde_json::Value;

fn fixtures_dir(group: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/protocol")
        .join(group)
}

/// Builds a message from a fixture, padding `payload.pad` so the compact
/// encoding is exactly `target` bytes.
fn padded(base: &Path, target: usize) -> Vec<u8> {
    let mut value: Value = serde_json::from_slice(&fs::read(base).unwrap()).unwrap();
    value["payload"]["pad"] = Value::String(String::new());
    let empty_len = serde_json::to_vec(&value).unwrap().len();
    value["payload"]["pad"] = Value::String("x".repeat(target - empty_len));
    let bytes = serde_json::to_vec(&value).unwrap();
    assert_eq!(bytes.len(), target);
    bytes
}

fn run_cases(group: &str, decode: impl Fn(&[u8]) -> Result<(), DecodeError>) {
    let dir = fixtures_dir(group);
    let cases: Value = serde_json::from_slice(&fs::read(dir.join("cases.json")).unwrap()).unwrap();
    let cases = cases["cases"].as_array().unwrap();
    assert!(!cases.is_empty());

    for case in cases {
        let name = case["name"].as_str().unwrap();
        let bytes = if let Some(file) = case["file"].as_str() {
            fs::read(dir.join(file)).unwrap()
        } else {
            let generate = &case["generate"];
            let target = usize::try_from(generate["padPayloadToBytes"].as_u64().unwrap()).unwrap();
            padded(&dir.join(generate["from"].as_str().unwrap()), target)
        };
        let actual = match decode(&bytes) {
            Ok(()) => "ok".to_owned(),
            Err(error) => code_name(error.code()),
        };
        assert_eq!(actual, case["expect"].as_str().unwrap(), "case {name:?}");
    }
}

fn code_name(code: ErrorCode) -> String {
    serde_json::to_value(code)
        .unwrap()
        .as_str()
        .unwrap()
        .to_owned()
}

#[test]
fn envelope_cases() {
    run_cases("envelope", |bytes| decode_envelope(bytes).map(drop));
}

#[test]
fn error_cases() {
    run_cases("error", |bytes| decode_error_response(bytes).map(drop));
}
