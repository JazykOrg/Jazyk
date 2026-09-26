// The editor as a visitor drives it (docs/frontends/wasm.md#building).
import { expect, test } from '@playwright/test'
import { api, offline, openDemo, pageErrors } from './helpers'

test.beforeEach(async ({ page }) => {
  await offline(page)
})

test('every route but Home goes home until a project is open', async ({ page }) => {
  await page.goto('/#/model/entities/ent%3Abackend')
  await page.waitForURL(/#\/$/)
  await expect(page.locator('.ws-lenses')).toHaveCount(0)
  await expect(page.locator('.home .demo')).toHaveCount(6)
})

test('nothing downloads the model on arrival; the Sync sheet offers it', async ({ page }) => {
  const external = await offline(page)
  await openDemo(page, 'The sort utility')
  await page.waitForTimeout(1500)
  expect(external).toEqual([])
  const caps = await api<{ model: string }>(page, 'GET', '/api/capabilities')
  // A browser without WebGPU has no model to offer at all.
  if (caps.model === 'loadable') {
    await page.locator('.ws-top').getByRole('button', { name: /Up to date|Sync/ }).click()
    await expect(page.locator('.ws-sheet')).toContainText('Load the model')
  }
})

test('a demo opened after another carries only its own documents', async ({ page }) => {
  await openDemo(page, 'The ERP')
  await openDemo(page, 'The novel')
  const docs = await api<{ docs: { path: string }[] }>(page, 'GET', '/api/docs')
  const paths = docs.docs.map((d) => d.path)
  expect(paths.some((p) => p.includes('backend'))).toBe(false)
  expect(paths.some((p) => p.includes('characters'))).toBe(true)
})

test('the product pages render for every demo', async ({ page }) => {
  const errs = pageErrors(page)
  for (const demo of ['The sort utility', 'The ERP']) {
    await openDemo(page, demo)
    for (const route of ['#/product', '#/product/trace', '#/inbox', '#/history', '#/model']) {
      await page.goto(`/${route}`)
      await expect(page.locator('body')).not.toContainText('failed to render')
    }
  }
  expect(errs).toEqual([])
})

test('an entity page reads by name, states its relations once, and explains a connection', async ({ page }) => {
  await openDemo(page, 'The ERP')
  await page.goto('/#/model/entities/ent%3Abackend')
  const center = page.locator('.ws-center')
  await expect(center.locator('h1')).toHaveText('Backend')
  // No compiler ids in the default mode, and no second list of relations.
  await expect(center).not.toContainText(/\b(ent|req|rel|view|g):[a-z0-9]/)
  await expect(center.getByRole('heading', { name: /relationships/i })).toHaveCount(0)
  const box = center.locator('.cn-n', { hasText: 'Frontend' })
  await box.click()
  await expect(center.locator('.cn-why')).toContainText('Backend is related to Frontend')
  await expect(center.locator('.cn-why .req-row')).not.toHaveCount(0)
})

test('a requirement keeps the Requirements list and shows where it comes from', async ({ page }) => {
  await openDemo(page, 'The ERP')
  await page.goto('/#/model')
  await page.locator('.wb-side-tabs a', { hasText: 'Requirements' }).click()
  await page.locator('.wb-list-row').first().click()
  await page.waitForURL(/#\/model\/requirements\//)
  await expect(page.locator('.wb-side-tabs a.active')).toHaveText('Requirements')
  await expect(page.locator('.ws-center')).toContainText('Where it comes from')
  await expect(page.locator('.ws-center .source-quote blockquote')).toBeVisible()
})

test('a diagram shows its picture in the tab', async ({ page }) => {
  await openDemo(page, 'The ERP')
  await page.goto('/#/model/diagrams/view%3Acomponent%2Fwarehouse-erp')
  await expect(page.locator('.ws-center svg').first()).toBeVisible()
})

test('a demo saves an edit on its own', async ({ page }) => {
  await openDemo(page, 'The ERP')
  await page.goto('/#/docs/docs/main.md')
  await page.locator('.cm-content').click()
  await page.keyboard.press('Control+End')
  await page.keyboard.type('\nAutosaved line.')
  await expect(page.locator('.ide-topbar-right')).toContainText('saved', { timeout: 5000 })
  await expect
    .poll(async () => JSON.stringify(await api(page, 'GET', '/api/docs/content?path=docs/main.md')))
    .toContain('Autosaved line.')
})

test('the recent list remembers a demo, reopens it, and forgets it', async ({ page }) => {
  await openDemo(page, 'The shop')
  await page.goto('/#/')
  const row = page.locator('.proj.recent', { hasText: 'The shop' })
  await expect(row).toBeVisible()
  await row.locator('.recent-open').click()
  await page.waitForURL(/#\/docs/)
  await page.goto('/#/')
  await page.locator('.proj.recent', { hasText: 'The shop' }).locator('.recent-forget').click()
  await expect(page.locator('.proj.recent')).toHaveCount(0)
})
