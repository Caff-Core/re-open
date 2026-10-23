// Placeholder for root commands whose implementation lands in a later task.
// Usage: node scripts/pending.mjs <command> <task>

import { pathToFileURL } from 'node:url';

/**
 * @param {string | undefined} command
 * @param {string | undefined} task
 * @returns {string}
 */
export function pendingMessage(command, task) {
  if (!command || !task) {
    return 'usage: node scripts/pending.mjs <command> <task>';
  }
  return `"pnpm ${command}" is not implemented yet; it arrives in task ${task} of docs/IMPLEMENTATION_PLAN.md.`;
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  console.error(pendingMessage(process.argv[2], process.argv[3]));
  process.exit(1);
}
