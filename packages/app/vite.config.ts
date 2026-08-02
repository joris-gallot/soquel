import path from 'node:path'
import tailwindcss from '@tailwindcss/vite'
import vue from '@vitejs/plugin-vue'
import { defineConfig } from 'vite'

export default defineConfig({
  plugins: [vue(), tailwindcss()],
  build: {
    // The default 500 kB warns about network transfer; these assets ship inside
    // the binary. Kept just above the CodeMirror-bearing chunk so a genuinely
    // heavy import still trips it.
    chunkSizeWarningLimit: 700,
  },
  // Tauri: fixed port for devUrl, don't clear Rust output, ignore src-tauri.
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true,
    watch: {
      ignored: ['**/src-tauri/**'],
    },
  },
  resolve: {
    alias: {
      '@': path.resolve(new URL('./src', import.meta.url).pathname),
    },
  },
})
