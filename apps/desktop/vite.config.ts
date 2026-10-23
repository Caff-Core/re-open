import process from 'node:process';
import react from '@vitejs/plugin-react';
import { defineConfig } from 'vite';

// Set by `tauri dev` when developing against a remote device; unused on desktop.
const host = process.env['TAURI_DEV_HOST'];

export default defineConfig({
  plugins: [react()],
  // Keep Rust compiler errors visible in the terminal.
  clearScreen: false,
  server: {
    // tauri.conf.json devUrl expects this exact port.
    port: 1420,
    strictPort: true,
    host: host ?? false,
    ...(host ? { hmr: { protocol: 'ws', host, port: 1421 } } : {}),
    watch: { ignored: ['**/src-tauri/**'] },
  },
  build: {
    target: 'es2023',
  },
});
