import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

// The dev server proxies to a running `jazyk gui --no-token` backend; the production
// build is served (and embedded) by the jazyk binary itself.
export default defineConfig({
  plugins: [react()],
  server: {
    proxy: {
      '/api': 'http://127.0.0.1:4680',
      '/lsp': { target: 'ws://127.0.0.1:4680', ws: true },
    },
  },
  build: {
    chunkSizeWarningLimit: 4000,
  },
})
