import { defineConfig } from 'vitest/config';

export default defineConfig({
  test: {
    projects: [
      { test: { name: 'root', include: ['scripts/**/*.test.mjs'], environment: 'node' } },
      'apps/*/vitest.config.{ts,mjs}',
      'packages/*/vitest.config.{ts,mjs}',
    ],
    passWithNoTests: true,
  },
});
