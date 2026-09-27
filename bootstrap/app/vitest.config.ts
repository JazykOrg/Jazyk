// The unit tests of the pure logic under src/lib and of the demo fixture scripts under
// scripts (docs/frontends/wasm.md#building): `npm run test:unit`. jsdom gives the
// src tests window, localStorage, and DOMException; a test file that needs IndexedDB
// imports fake-indexeddb itself. A script test runs in node (a
// `@vitest-environment node` comment at its top).
import { defineConfig } from 'vitest/config'

export default defineConfig({
  test: {
    environment: 'jsdom',
    include: ['src/**/*.test.ts', 'scripts/**/*.test.*'],
  },
})
