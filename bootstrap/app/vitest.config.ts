// The unit tests of the pure logic under src/lib (docs/frontends/wasm.md#building):
// `npm run test:unit`. jsdom gives them window, localStorage, and DOMException; a
// test file that needs IndexedDB imports fake-indexeddb itself.
import { defineConfig } from 'vitest/config'

export default defineConfig({
  test: {
    environment: 'jsdom',
    include: ['src/**/*.test.ts'],
  },
})
