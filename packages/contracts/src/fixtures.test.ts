// Runs the shared cases in fixtures/protocol (also run by crates/protocol).
import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import { decodeEnvelope, decodeErrorResponse, type DecodeResult } from './index';

interface FixtureCase {
  name: string;
  file?: string;
  generate?: { from: string; padPayloadToBytes: number };
  expect: string;
}

const fixturesDir = new URL('../../../fixtures/protocol/', import.meta.url);

function loadCases(group: string): { dir: URL; cases: FixtureCase[] } {
  const dir = new URL(`${group}/`, fixturesDir);
  const { cases } = JSON.parse(readFileSync(new URL('cases.json', dir), 'utf8')) as {
    cases: FixtureCase[];
  };
  return { dir, cases };
}

/** Pads payload.pad so the compact encoding is exactly `target` bytes (mirrors the Rust test). */
function padded(base: URL, target: number): string {
  const value = JSON.parse(readFileSync(base, 'utf8')) as { payload: Record<string, unknown> };
  value.payload['pad'] = '';
  const emptyLength = new TextEncoder().encode(JSON.stringify(value)).byteLength;
  value.payload['pad'] = 'x'.repeat(target - emptyLength);
  const text = JSON.stringify(value);
  expect(new TextEncoder().encode(text).byteLength).toBe(target);
  return text;
}

function input(dir: URL, testCase: FixtureCase): Uint8Array | string {
  if (testCase.file) return readFileSync(new URL(testCase.file, dir));
  if (!testCase.generate) throw new Error(`case ${testCase.name} has no input`);
  return padded(new URL(testCase.generate.from, dir), testCase.generate.padPayloadToBytes);
}

const outcome = (result: DecodeResult<unknown>) => (result.ok ? 'ok' : result.error.code);

describe.each([
  { group: 'envelope', decode: decodeEnvelope },
  { group: 'error', decode: decodeErrorResponse },
])('$group fixtures', ({ group, decode }) => {
  const { dir, cases } = loadCases(group);

  it('has cases', () => {
    expect(cases.length).toBeGreaterThan(0);
  });

  it.each(cases)('$name → $expect', (testCase) => {
    expect(outcome(decode(input(dir, testCase)))).toBe(testCase.expect);
  });
});

describe('redaction', () => {
  it('does not echo input values in error details', () => {
    const result = decodeEnvelope(
      '{"protocol_version":1,"request_id":"secret-https://example.com/path","kind":"k","payload":{}}',
    );
    expect(result.ok).toBe(false);
    if (!result.ok) expect(result.error.detail).not.toContain('example.com');
  });
});
