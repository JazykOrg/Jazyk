// The editor's browser tests (docs/frontends/wasm.md#building): the built page in
// dist/, served by `vite preview` with the isolation headers.
// PW_CHANNEL=chrome runs them in the installed Chrome instead of Playwright's own;
// PW_PORT picks the preview's port, so two checkouts can run them side by side
// (an existing server on the port is reused).
import { defineConfig, devices } from '@playwright/test'

const PORT = Number(process.env.PW_PORT) || 4174

export default defineConfig({
  testDir: 'e2e',
  timeout: 60_000,
  retries: process.env.CI ? 1 : 0,
  workers: 1,
  reporter: process.env.CI ? [['list'], ['github']] : 'list',
  use: {
    baseURL: `http://localhost:${PORT}/`,
    channel: process.env.PW_CHANNEL || undefined,
    trace: 'retain-on-failure',
    // The service worker would answer requests the tests route and count; only the
    // offline test lets it register (docs/frontends/wasm.md#offline).
    serviceWorkers: 'block',
  },
  projects: [
    { name: 'desktop', use: { viewport: { width: 1440, height: 1000 } }, testIgnore: /phone/ },
    { name: 'phone', use: { ...devices['Pixel 7'], channel: process.env.PW_CHANNEL || undefined }, testMatch: /phone/ },
  ],
  webServer: {
    command: `npx vite preview --outDir dist --port ${PORT} --strictPort`,
    port: PORT,
    reuseExistingServer: !process.env.CI,
  },
})
