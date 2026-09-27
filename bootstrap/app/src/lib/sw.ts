// The service worker (docs/frontends/wasm.md#offline): Workbox through
// vite-plugin-pwa precaches the app shell (the page, the scripts and styles, the
// wasm module and its bindings, the fonts, the icons, the manifest) and keeps a
// demo's fixture once fetched, so the page loads and a demo it has seen opens with
// the network off. `autoUpdate`: a new build installs in the background and the
// next load runs it. A response the worker serves from its cache keeps the headers
// it was cached with (Workbox stores the whole response), so the isolation headers
// (COOP/COEP) an offline load needs are there too.
import { registerSW } from 'virtual:pwa-register'

export function registerServiceWorker() {
  if (!('serviceWorker' in navigator)) return
  registerSW({ immediate: true })
}
