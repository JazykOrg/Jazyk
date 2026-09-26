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

// The product a demo carries reads in the tab (docs/frontends/wasm.md#the-product).
test('the product files list and a file opens with its text', async ({ page }) => {
  await openDemo(page, 'The sort utility')
  await page.goto('/#/product/files')
  const nav = page.locator('.ws-nav')
  await expect(nav.getByRole('link', { name: /^main\.js(\d+)?$/ })).toBeVisible()
  await expect(nav.getByRole('link', { name: /^main\.test\.js(\d+)?$/ })).toBeVisible()
  await nav.getByRole('link', { name: /^main\.js(\d+)?$/ }).click()
  await page.waitForURL(/#\/product\/files\/main\.js$/)
  await expect(page.locator('.ide-topbar')).toContainText('main.js')
  await expect(page.locator('.deliv-editor').first()).toContainText('readStdinLines')

  // A file the ledger binds shows its sites as lenses; a fixture without a ledger
  // binds none.
  const deliv = await api<{ files: { path: string; owners: { requirements: string[] } }[] }>(page, 'GET', '/api/deliverable')
  const bound = deliv.files.find((f) => f.owners.requirements.length > 0)
  if (bound) {
    const file = await api<{ sites: unknown[] }>(page, 'GET', `/api/deliverable/file?path=${encodeURIComponent(bound.path)}`)
    await page.goto(`/#/product/files/${bound.path}`)
    if (file.sites.length > 0) await expect(page.locator('.jz-lens').first()).toBeVisible()
  }
})

test('the trace matrix renders a row per requirement and exports from the tab', async ({ page }) => {
  await openDemo(page, 'The sort utility')
  const matrix = await api<{ rows: Record<string, unknown> }>(page, 'GET', '/api/verify/matrix')
  const n = Object.keys(matrix.rows).length
  expect(n).toBeGreaterThan(0)
  await page.goto('/#/product/trace')
  const center = page.locator('.ws-center')
  await expect(center.locator('.card > div .chip')).toHaveCount(n)
  // Tests do not run in a tab, so the matrix offers no run action.
  await expect(center.getByRole('button', { name: /^run$/ })).toHaveCount(0)
  const download = page.waitForEvent('download')
  await center.getByRole('link', { name: 'csv' }).click()
  const file = await download
  expect(file.suggestedFilename()).toBe('trace-matrix.csv')
  const text = await (await import('node:fs/promises')).readFile(await file.path(), 'utf8')
  expect(text.split('\n')[0]).toBe('requirement,entity,status,reason,repair,test,lastRun,evidence')
  expect(text.trim().split('\n')).toHaveLength(n + 1)
})

test('the generated pages list and one opens rendered', async ({ page }) => {
  await openDemo(page, 'The sort utility')
  await page.goto('/#/product/generated')
  await page.locator('.ws-nav').getByRole('link', { name: 'index.md', exact: true }).click()
  await expect(page.locator('.ws-center h1').first()).toHaveText('Index')
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

test('Home reads Recent, then Local project, then the demos', async ({ page }) => {
  await openDemo(page, 'The shop')
  await page.goto('/#/')
  await expect(page.locator('.home h2')).toHaveText(['Recent', 'Local project', 'Demos'])
  await expect(page.locator('.home').getByRole('button', { name: 'Open folder…' })).toBeVisible()
})

test('a browser without the folder API says so instead of offering the button', async ({ page }) => {
  await page.addInitScript(() => {
    delete (window as unknown as { showDirectoryPicker?: unknown }).showDirectoryPicker
  })
  await page.goto('/#/')
  await expect(page.locator('.home-nofolder')).toContainText('cannot open a folder')
  await expect(page.locator('.home').getByRole('button', { name: 'Open folder…' })).toHaveCount(0)
})

test('an endpoint model source is ready at once and is remembered in this browser only', async ({ page }) => {
  await openDemo(page, 'The sort utility')
  await page.goto('/#/settings')
  // The chat pane offers the same chooser until a model is ready; this one is Settings'.
  const chooser = page.locator('.settings-model .msrc')
  await expect(chooser.locator('.msrc-privacy')).toContainText('Nothing is saved on a jazyk server')
  await chooser.locator('.msrc-option', { hasText: 'Your endpoint' }).click()
  await chooser.getByLabel('Base URL', { exact: true }).fill('http://localhost:4999/v1')
  await chooser.getByLabel('Model', { exact: true }).fill('test-model')
  await chooser.getByRole('button', { name: 'Save' }).click()
  await expect(chooser.locator('.msrc-status')).toContainText('Ready: test-model')
  expect(await api<{ model: string }>(page, 'GET', '/api/capabilities')).toMatchObject({ model: 'in-page' })
  const stored = await page.evaluate(() => JSON.parse(localStorage.getItem('jazyk.modelSource') ?? '{}'))
  expect(stored).toMatchObject({ kind: 'endpoint', baseUrl: 'http://localhost:4999/v1', model: 'test-model' })
  // A reload keeps the choice; the demo reopens from the recent list.
  await page.reload()
  await page.locator('.proj.recent .recent-open').first().click()
  await page.waitForURL(/#\/docs/)
  expect(await api<{ model: string }>(page, 'GET', '/api/capabilities')).toMatchObject({ model: 'in-page' })
})
