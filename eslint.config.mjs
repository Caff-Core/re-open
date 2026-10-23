import js from '@eslint/js';
import { defineConfig } from 'eslint/config';
import prettier from 'eslint-config-prettier';
import { createTypeScriptImportResolver } from 'eslint-import-resolver-typescript';
import { importX } from 'eslint-plugin-import-x';
import reactHooks from 'eslint-plugin-react-hooks';
import globals from 'globals';
import { configs as tseslintConfigs } from 'typescript-eslint';

export default defineConfig(
  {
    ignores: [
      '**/dist/**',
      '**/target/**',
      '**/node_modules/**',
      '**/coverage/**',
      '.claude/worktrees/**',
      '**/src/generated/**',
      'apps/desktop/src-tauri/gen/**',
    ],
  },
  js.configs.recommended,
  tseslintConfigs.strict,
  importX.flatConfigs.recommended,
  importX.flatConfigs.typescript,
  {
    settings: {
      'import-x/resolver-next': [createTypeScriptImportResolver({ alwaysTryTypes: true })],
    },
    rules: {
      'import-x/no-cycle': 'error',
      'import-x/no-self-import': 'error',
      'import-x/order': [
        'error',
        { 'newlines-between': 'never', alphabetize: { order: 'asc', caseInsensitive: true } },
      ],
    },
  },
  {
    // Installed verbatim by the shadcn CLI; keep them diff-free against the registry.
    files: ['packages/ui/src/components/**'],
    rules: { 'import-x/order': 'off' },
  },
  {
    files: ['**/*.{tsx,jsx}'],
    ...reactHooks.configs.flat['recommended-latest'],
  },
  {
    files: ['scripts/**', '*.config.{mjs,ts}'],
    languageOptions: { globals: globals.node },
  },
  prettier,
);
