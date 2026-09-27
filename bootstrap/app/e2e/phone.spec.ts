// A phone-width screen gives the editor the width (docs/frontends/gui.md#layout).
import { expect, test } from '@playwright/test'
import { offline, openDemo } from './helpers'

test('the editor takes the width and the navigator is a drawer', async ({ page }) => {
  await offline(page)
  await openDemo(page, 'The sort utility')
  await page.goto('/#/docs/docs/main.js.md')
  await expect(page.locator('.cm-content')).toBeVisible()
  const center = await page.locator('.ws-center').boundingBox()
  expect(center!.width).toBeGreaterThanOrEqual(page.viewportSize()!.width - 2)
  await expect(page.locator('.wb-chat')).toHaveCount(0)
  await page.locator('.ws-navtoggle').tap()
  await expect(page.locator('.ws')).toHaveClass(/nav-open/)
  await page.locator('.ws-nav a', { hasText: 'main.test.js.md' }).tap()
  await expect(page.locator('.ws')).not.toHaveClass(/nav-open/)
  await expect(page).toHaveURL(/main\.test\.js\.md/)
})

test('resizing across the breakpoint hides and shows the chat column', async ({ page }) => {
  await offline(page)
  await openDemo(page, 'The sort utility')
  await page.goto('/#/docs/docs/main.js.md')
  await expect(page.locator('.wb-chat')).toHaveCount(0)
  await expect(page.locator('.ws-navtoggle')).toBeVisible()

  await page.setViewportSize({ width: 900, height: 800 })
  await expect(page.locator('.wb-chat')).toBeVisible()
  await expect(page.locator('.ws-navtoggle')).toBeHidden()
  await expect(page.locator('.ws-nav')).toBeVisible()

  await page.setViewportSize({ width: 600, height: 800 })
  await expect(page.locator('.wb-chat')).toHaveCount(0)
  await expect(page.locator('.ws-navtoggle')).toBeVisible()
})

test('tapping the scrim closes the navigator drawer', async ({ page }) => {
  await offline(page)
  await openDemo(page, 'The sort utility')
  await page.goto('/#/docs/docs/main.js.md')
  await page.locator('.ws-navtoggle').tap()
  await expect(page.locator('.ws')).toHaveClass(/nav-open/)
  const scrim = page.locator('.ws-scrim')
  await expect(scrim).toBeVisible()
  // The drawer covers the left of the scrim; the tap lands beside it.
  const box = (await scrim.boundingBox())!
  await scrim.tap({ position: { x: box.width - 16, y: box.height / 2 } })
  await expect(page.locator('.ws')).not.toHaveClass(/nav-open/)
  await expect(scrim).toHaveCount(0)
  await expect(page).toHaveURL(/main\.js\.md/)
})
