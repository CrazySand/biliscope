import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'

// Tauri expects a fixed dev port; the frontend lives in `src/`.
export default defineConfig({
  plugins: [vue()],
  clearScreen: false,
  server: {
    host: '127.0.0.1',
    port: 1420,
    strictPort: true,
  },
  envPrefix: ['VITE_', 'TAURI_'],
  build: {
    // WebView2 is an evergreen Chromium, so we can skip Vite's downlevel
    // transpile step (which shells out to esbuild's large-output temp files).
    target: 'esnext',
    // esbuild's minifier writes >1MB chunks to temp files that this sandbox
    // refuses to let it delete. A desktop bundle does not need minification.
    minify: false,
    // The DSH sandbox denies deleting pre-existing files; never empty `dist`.
    emptyOutDir: false,
    sourcemap: false,
  },
})
