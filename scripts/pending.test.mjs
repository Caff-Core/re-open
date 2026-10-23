import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';
import { pendingMessage } from './pending.mjs';

const script = fileURLToPath(new URL('./pending.mjs', import.meta.url));

describe('pendingMessage', () => {
  it('names the command and the implementing task', () => {
    expect(pendingMessage('dev:agent', '1.8')).toBe(
      '"pnpm dev:agent" is not implemented yet; it arrives in task 1.8 of docs/IMPLEMENTATION_PLAN.md.',
    );
  });

  it('returns usage when arguments are missing', () => {
    expect(pendingMessage(undefined, undefined)).toMatch(/^usage:/);
  });
});

describe('pending.mjs CLI', () => {
  it('exits non-zero and prints the message on stderr', () => {
    let status = 0;
    let stderr = '';
    try {
      execFileSync(process.execPath, [script, 'package:linux', '6.1'], { stdio: 'pipe' });
    } catch (error) {
      const failure = /** @type {{ status: number, stderr: Buffer }} */ (error);
      status = failure.status;
      stderr = failure.stderr.toString();
    }
    expect(status).toBe(1);
    expect(stderr).toContain('task 6.1');
  });
});
