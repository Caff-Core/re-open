//! Re-Open wire types and validation.
//!
//! `generated` mirrors `packages/contracts/schema` and is produced by
//! `reopen-protocol-codegen`. Generated types alone do not validate untrusted
//! input: decode messages with the functions in this module, which enforce
//! size limits, the protocol version and the canonical schema first.

use std::fmt;
use std::sync::LazyLock;

use jsonschema::Validator;
use serde::de::DeserializeOwned;
use serde_json::Value;

#[allow(clippy::all, missing_debug_implementations)]
#[rustfmt::skip]
pub mod generated;

pub use generated::{Envelope, ErrorCode, ErrorResponse};

/// Protocol major version this build speaks.
pub const PROTOCOL_VERSION: u64 = 1;
/// Maximum encoded message size (docs/PROTOCOL.md, "Framing and limits").
pub const MAX_MESSAGE_BYTES: usize = 256 * 1024;

const ENVELOPE_SCHEMA: &str =
    include_str!("../../../packages/contracts/schema/envelope.schema.json");
const ERROR_SCHEMA: &str = include_str!("../../../packages/contracts/schema/error.schema.json");

#[allow(clippy::expect_used)] // Embedded constant schemas; compilation is covered by tests.
fn compile(schema: &str) -> Validator {
    let schema: Value = serde_json::from_str(schema).expect("embedded schema is valid JSON");
    jsonschema::validator_for(&schema).expect("embedded schema compiles")
}

static ENVELOPE_VALIDATOR: LazyLock<Validator> = LazyLock::new(|| compile(ENVELOPE_SCHEMA));
static ERROR_VALIDATOR: LazyLock<Validator> = LazyLock::new(|| compile(ERROR_SCHEMA));

/// Why a message was rejected. Details never contain input values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodeError {
    /// The encoded message is larger than [`MAX_MESSAGE_BYTES`].
    LimitExceeded { len: usize },
    /// The message is not valid JSON or violates the schema.
    InvalidPayload { detail: String },
    /// The message declares a protocol version this build does not speak.
    VersionUnsupported { version: u64 },
}

impl DecodeError {
    /// Stable wire error code for this rejection.
    #[must_use]
    pub const fn code(&self) -> ErrorCode {
        match self {
            Self::LimitExceeded { .. } => ErrorCode::LimitExceeded,
            Self::InvalidPayload { .. } => ErrorCode::InvalidPayload,
            Self::VersionUnsupported { .. } => ErrorCode::VersionUnsupported,
        }
    }
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LimitExceeded { len } => {
                write!(
                    f,
                    "message of {len} bytes exceeds {MAX_MESSAGE_BYTES} bytes"
                )
            }
            Self::InvalidPayload { detail } => write!(f, "invalid payload: {detail}"),
            Self::VersionUnsupported { version } => {
                write!(f, "protocol version {version} is not supported")
            }
        }
    }
}

impl std::error::Error for DecodeError {}

fn parse(bytes: &[u8]) -> Result<Value, DecodeError> {
    if bytes.len() > MAX_MESSAGE_BYTES {
        return Err(DecodeError::LimitExceeded { len: bytes.len() });
    }
    serde_json::from_slice(bytes).map_err(|_| DecodeError::InvalidPayload {
        detail: "not valid JSON".to_owned(),
    })
}

fn validate<T: DeserializeOwned>(validator: &Validator, value: Value) -> Result<T, DecodeError> {
    if let Some(error) = validator.iter_errors(&value).next() {
        // The schema path comes from our own schema, so it is safe to report.
        return Err(DecodeError::InvalidPayload {
            detail: format!("schema violation at {}", error.schema_path()),
        });
    }
    serde_json::from_value(value).map_err(|_| DecodeError::InvalidPayload {
        detail: "does not match the generated type".to_owned(),
    })
}

/// Decodes and validates one envelope from untrusted bytes.
///
/// # Errors
///
/// Returns [`DecodeError`] for oversized, malformed, schema-invalid or
/// unsupported-version messages.
pub fn decode_envelope(bytes: &[u8]) -> Result<Envelope, DecodeError> {
    let value = parse(bytes)?;
    // Check the version first: a future version may legitimately carry fields
    // this schema rejects, and must fail as unsupported rather than invalid.
    if let Some(version) = value.get("protocol_version").and_then(Value::as_u64)
        && version != PROTOCOL_VERSION
    {
        return Err(DecodeError::VersionUnsupported { version });
    }
    validate(&ENVELOPE_VALIDATOR, value)
}

/// Decodes and validates one error response from untrusted bytes.
///
/// # Errors
///
/// Returns [`DecodeError`] for oversized, malformed or schema-invalid messages.
pub fn decode_error_response(bytes: &[u8]) -> Result<ErrorResponse, DecodeError> {
    validate(&ERROR_VALIDATOR, parse(bytes)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_schemas_compile() {
        LazyLock::force(&ENVELOPE_VALIDATOR);
        LazyLock::force(&ERROR_VALIDATOR);
    }

    #[test]
    fn oversize_is_rejected_before_parsing() {
        let bytes = vec![b'{'; MAX_MESSAGE_BYTES + 1];
        assert_eq!(
            decode_envelope(&bytes),
            Err(DecodeError::LimitExceeded {
                len: MAX_MESSAGE_BYTES + 1
            })
        );
    }

    #[test]
    fn errors_do_not_echo_input_values() {
        let input = br#"{"protocol_version":1,"request_id":"secret-https://example.com/path","kind":"k","payload":{}}"#;
        let error = decode_envelope(input).unwrap_err();
        assert_eq!(error.code(), ErrorCode::InvalidPayload);
        assert!(!error.to_string().contains("example.com"), "{error}");
    }
}
