import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import { VitePWA } from 'vite-plugin-pwa'

// SharedArrayBuffer, which the model bridge blocks on, is only available to a
// cross-origin isolated page (docs/frontends/wasm.md#serving).
const isolation = {
  'Cross-Origin-Opener-Policy': 'same-origin',
  'Cross-Origin-Embedder-Policy': 'require-corp',
}

// One page, jazyk.dev (docs/frontends/wasm.md#building): the app with the compiler
// as wasm, every backend in it (a demo, the browser, a folder, a server on this
// computer). The binary embeds nothing; `jazyk gui` opens this page.
export default defineConfig({
  plugins: [
    react(),
    // The service worker (docs/frontends/wasm.md#offline): the app shell is
    // precached, the wasm module included (hence the raised size limit); a demo's
    // fixture is cached when first fetched and refreshed in the background. A
    // response served from the cache keeps the headers it was cached with, so the
    // isolation headers hold offline too.
    VitePWA({
      registerType: 'autoUpdate',
      manifest: {
        name: 'Jazyk',
        short_name: 'Jazyk',
        description: 'Documents that compile: the Jazyk editor.',
        start_url: '/',
        display: 'standalone',
        // From src/tokens.css: the accent and the light background.
        theme_color: '#0e7a6d',
        background_color: '#f7f8f7',
        icons: [
          { src: 'favicon.svg', sizes: 'any', type: 'image/svg+xml' },
          { src: 'icon-192.png', sizes: '192x192', type: 'image/png' },
          { src: 'icon-512.png', sizes: '512x512', type: 'image/png', purpose: 'any maskable' },
        ],
      },
      workbox: {
        globPatterns: ['**/*.{js,css,html,wasm,woff,woff2,svg,png,webmanifest}'],
        // The fixtures are runtime-cached instead: a demo opened once opens offline,
        // one never opened is not downloaded on arrival. The model's code (WebLLM)
        // is fetched with the model, not with the page (docs/frontends/wasm.md#the-demo).
        globIgnores: ['demos/**', '**/webllm-*.js'],
        // The wasm module is about 7MB; Workbox's default limit is 2MB.
        maximumFileSizeToCacheInBytes: 16 * 1024 * 1024,
        navigateFallback: 'index.html',
        runtimeCaching: [
          {
            urlPattern: ({ url, sameOrigin }) => sameOrigin && /\/demos\/[^/]+\.json$/.test(url.pathname),
            handler: 'StaleWhileRevalidate',
            options: { cacheName: 'jazyk-demos' },
          },
        ],
      },
    }),
  ],
  server: { headers: isolation },
  preview: { headers: isolation },
  worker: { format: 'es' },
  build: {
    outDir: 'dist',
    chunkSizeWarningLimit: 8000,
    rollupOptions: {
      // The model's code in a chunk of its own, named so the precache can skip it.
      output: { manualChunks: { webllm: ['@mlc-ai/web-llm'] } },
    },
  },
})
