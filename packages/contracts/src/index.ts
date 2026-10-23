// Wire types and validation shared by the desktop app and the browser extension.
// Generated types alone do not validate untrusted input; decode with these functions.
import type { Envelope } from './generated/envelope';
import type { ErrorCode, ErrorResponse } from './generated/error';
import {
  type StandaloneValidator,
  validateEnvelope,
  validateErrorResponse,
} from './generated/validators.js';

export type { Envelope, ErrorCode, ErrorResponse };

/** Protocol major version this build speaks. */
export const PROTOCOL_VERSION = 1;
/** Maximum encoded message size (docs/PROTOCOL.md, "Framing and limits"). */
export const MAX_MESSAGE_BYTES = 256 * 1024;

/** Why a message was rejected. `detail` never contains input values. */
export interface DecodeError {
  code: Extract<ErrorCode, 'LIMIT_EXCEEDED' | 'INVALID_PAYLOAD' | 'VERSION_UNSUPPORTED'>;
  detail: string;
}

export type DecodeResult<T> = { ok: true; value: T } | { ok: false; error: DecodeError };

const encoder = new TextEncoder();
const decoder = new TextDecoder('utf-8', { fatal: true });

const fail = (code: DecodeError['code'], detail: string): { ok: false; error: DecodeError } => ({
  ok: false,
  error: { code, detail },
});

function parse(input: string | Uint8Array): DecodeResult<unknown> {
  const bytes = typeof input === 'string' ? encoder.encode(input) : input;
  if (bytes.byteLength > MAX_MESSAGE_BYTES) {
    return fail(
      'LIMIT_EXCEEDED',
      `message of ${String(bytes.byteLength)} bytes exceeds ${String(MAX_MESSAGE_BYTES)} bytes`,
    );
  }
  try {
    const text = typeof input === 'string' ? input : decoder.decode(bytes);
    return { ok: true, value: JSON.parse(text) as unknown };
  } catch {
    return fail('INVALID_PAYLOAD', 'not valid JSON');
  }
}

function check<T>(validate: StandaloneValidator, value: unknown): DecodeResult<T> {
  if (validate(value)) return { ok: true, value: value as T };
  // The schema path comes from our own schema, so it is safe to report.
  const schemaPath = validate.errors?.[0]?.schemaPath ?? '#';
  return fail('INVALID_PAYLOAD', `schema violation at ${schemaPath}`);
}

function declaredVersion(value: unknown): number | undefined {
  if (typeof value !== 'object' || value === null) return undefined;
  const version = (value as Record<string, unknown>)['protocol_version'];
  return typeof version === 'number' && Number.isInteger(version) && version >= 0
    ? version
    : undefined;
}

/** Decodes and validates one envelope from untrusted input. */
export function decodeEnvelope(input: string | Uint8Array): DecodeResult<Envelope> {
  const parsed = parse(input);
  if (!parsed.ok) return parsed;
  // A future version may carry fields this schema rejects; report it as unsupported.
  const version = declaredVersion(parsed.value);
  if (version !== undefined && version !== PROTOCOL_VERSION) {
    return fail('VERSION_UNSUPPORTED', `protocol version ${String(version)} is not supported`);
  }
  return check<Envelope>(validateEnvelope, parsed.value);
}

/** Decodes and validates one error response from untrusted input. */
export function decodeErrorResponse(input: string | Uint8Array): DecodeResult<ErrorResponse> {
  const parsed = parse(input);
  if (!parsed.ok) return parsed;
  return check<ErrorResponse>(validateErrorResponse, parsed.value);
}
