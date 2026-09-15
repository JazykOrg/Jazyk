import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import { resolve } from 'node:path'

// SharedArrayBuffer, which the model bridge blocks on, is only available to a
// cross-origin isolated page (docs/frontends/wasm.md#the-model).
const isolation = {
  'Cross-Origin-Opener-Policy': 'same-origin',
  'Cross-Origin-Embedder-Policy': 'require-corp',
}

// Two builds from one app. The default is the workbench the binary embeds, which
// talks to a server. `JAZYK_DEMO=1` builds the demo page instead, which carries the
// compiler as wasm and needs no server; it lands in its own directory so the
// embedded dist never grows a 7MB module (docs/frontends/wasm.md#building).
const demo = !!process.env.JAZYK_DEMO

export default defineConfig({
  plugins: [react()],
  server: {
    headers: isolation,
    proxy: {
      '/api': 'http://127.0.0.1:4680',
      '/lsp': { target: 'ws://127.0.0.1:4680', ws: true },
    },
  },
  preview: { headers: isolation },
  worker: { format: 'es' },
  build: {
    outDir: demo ? 'dist-demo' : 'dist',
    chunkSizeWarningLimit: 8000,
    rollupOptions: {
      input: demo
        ? { demo: resolve(__dirname, 'demo.html') }
        : { main: resolve(__dirname, 'index.html') },
    },
  },
})
