// Offline (docs/frontends/wasm.md#offline): once the page has loaded and a demo has
// been opened, the service worker serves the app shell and the demo's fixture from
// its cache, so the page loads and the same demo opens with the network off. The
// worker is blocked for every other test (playwright.config.ts); this one lets it
// register.
import { expect, test } from '@playwright/test'
import { openDemo, pageErrors } from './helpers'

test.use({ serviceWorkers: 'allow' })

test('the page loads and opens a demo it has seen with the network off', async ({ page, context }) => {
  const errors = pageErrors(page)
  await page.goto('/#/')
  // The worker installs (the precache: the page, the scripts, the wasm module),
  // activates, and takes the page; only then does a fetch pass through it.
  await page.waitForFunction(async () => {
    const reg = await navigator.serviceWorker.ready
    return reg.active !== null && navigator.serviceWorker.controller !== null
  }, undefined, { timeout: 60_000 })
  await openDemo(page, 'The shop')
  await expect(page.locator('.ide-doc-name').first()).toBeVisible()
  // The fixture entered the runtime cache on its way through the worker.
  await page.waitForFunction(async () => {
    const cache = await caches.open('jazyk-demos')
    return (await cache.match(`${location.origin}/demos/shop.json`)) !== undefined
  })

  await context.setOffline(true)
  await page.goto('/#/')
  await expect(page.locator('.home h1')).toBeVisible()
  await expect(page.locator('.home .demo', { hasText: 'The shop' })).toBeVisible()
  // The page is still cross-origin isolated: the cached response kept its headers.
  expect(await page.evaluate(() => crossOriginIsolated)).toBe(true)
  await page.locator('.home .demo', { hasText: 'The shop' }).click()
  await page.waitForURL(/#\/docs/)
  await expect(page.locator('.ws-lenses')).toBeVisible()
  await expect(page.locator('.ide-doc-name').first()).toBeVisible()
  await page.locator('.ide-doc-name').first().click()
  await expect(page.locator('.cm-content')).toBeVisible()
  expect(errors).toEqual([])
})
