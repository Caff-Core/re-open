import { defineProject } from 'vitest/config';

export default defineProject({
  test: {
    name: 'desktop',
    environment: 'happy-dom',
    include: ['src/**/*.test.{ts,tsx}'],
  },
});
