// The editor's browser tests (docs/frontends/wasm.md#building): the built page in
// dist-demo/, served by `vite preview` with the isolation headers.
// PW_CHANNEL=chrome runs them in the installed Chrome instead of Playwright's own.
import { defineConfig, devices } from '@playwright/test'

const PORT = 4174

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
  },
  projects: [
    { name: 'desktop', use: { viewport: { width: 1440, height: 1000 } }, testIgnore: /phone/ },
    { name: 'phone', use: { ...devices['Pixel 7'], channel: process.env.PW_CHANNEL || undefined }, testMatch: /phone/ },
  ],
  webServer: {
    command: `npx vite preview --outDir dist-demo --port ${PORT} --strictPort`,
    port: PORT,
    reuseExistingServer: !process.env.CI,
  },
})
