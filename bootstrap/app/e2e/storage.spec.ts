// Browser storage (docs/frontends/wasm.md#browser-storage): a project started in the
// browser and an edited demo survive a reload and reopen from Recent with the edit;
// a demo resets to its original.
import { expect, test, type Page } from '@playwright/test'
import { api, offline, openDemo, pageErrors, storedKeys, storedText, typeAtEnd } from './helpers'

test.beforeEach(async ({ page }) => {
  await offline(page)
})

async function docText(page: Page, path: string): Promise<string> {
  return (await api<{ text: string }>(page, 'GET', `/api/docs/content?path=${path}`)).text
}

test('a new project starts in the browser, keeps an edit across a reload, and reopens from Recent', async ({ page }) => {
  const errors = pageErrors(page)
  await page.goto('/#/')
  await page.getByRole('button', { name: 'New project' }).click()
  await page.getByLabel('Project name').fill('Garden planner')
  await page.getByRole('button', { name: 'Create' }).click()
  await page.waitForURL(/#\/docs/)

  // The minimal project: a jazyk.toml and a README saying what to write.
  const docs = await api<{ docs: { path: string }[] }>(page, 'GET', '/api/docs')
  expect(docs.docs.map((d) => d.path)).toEqual(['docs/README.md'])
  expect(await docText(page, 'docs/README.md')).toContain('# Garden planner')
  expect(await api(page, 'GET', '/api/capabilities')).toMatchObject({ backend: 'wasm', persistence: 'browser' })
  await expect(page.locator('.ws-project')).toContainText('Garden planner')
  await expect(page.locator('.ws-place-chip')).toContainText('in this browser')

  await page.goto('/#/docs/docs/README.md')
  await typeAtEnd(page, '\nA planner lays out the beds of a garden.')
  await expect(page.locator('.ide-topbar-right')).toContainText('saved', { timeout: 5000 })
  // The edit reached browser storage, one record per file.
  await expect.poll(() => storedText(page, 'file:browser:', 'docs/README.md')).toContain('lays out the beds')

  await page.reload()
  await page.goto('/#/')
  const row = page.locator('.proj.recent', { hasText: 'Garden planner' })
  await expect(row.locator('.shape')).toHaveText('browser project · only in this browser')
  await row.locator('.recent-open').click()
  await page.waitForURL(/#\/docs/)
  expect(await docText(page, 'docs/README.md')).toContain('A planner lays out the beds of a garden.')

  // The Home banner and the top bar menu say where it lives.
  await page.locator('.ws-place-chip').click()
  await expect(page.locator('.ws-place-menu')).toContainText('stored in this browser only')
  await expect(page.locator('.ws-place-menu').getByRole('button', { name: 'Download .zip' })).toBeVisible()
  await expect(page.locator('.ws-place-menu').getByRole('button', { name: 'Save to a folder…' })).toBeVisible()
  expect(errors).toEqual([])
})

test('an edited demo reopens with the edit after a reload, then resets to the original', async ({ page }) => {
  const LINE = 'The shop also sells gift cards.'
  await openDemo(page, 'The shop')
  // An unedited demo stores nothing.
  expect(await storedKeys(page, 'file:demo:shop:')).toEqual([])
  await page.goto('/#/')
  await expect(page.locator('.proj.recent', { hasText: 'The shop' }).locator('.shape')).toHaveText('demo')

  await page.locator('.proj.recent', { hasText: 'The shop' }).locator('.recent-open').click()
  await page.waitForURL(/#\/docs/)
  await page.goto('/#/docs/docs/catalog.md')
  await typeAtEnd(page, `\n${LINE}`)
  await expect(page.locator('.ide-topbar-right')).toContainText('saved', { timeout: 5000 })
  await expect.poll(() => storedText(page, 'file:demo:shop:', 'docs/catalog.md')).toContain(LINE)
  // The copy carries the whole fixture, not just the edited file.
  expect(await storedKeys(page, 'file:demo:shop:')).toContain('file:demo:shop:jazyk.toml')
  await expect(page.locator('.ws-place-chip')).toContainText('demo · your edits')

  await page.reload()
  await page.goto('/#/')
  const row = page.locator('.proj.recent', { hasText: 'The shop' })
  await expect(row.locator('.shape')).toHaveText('demo with your edits')
  await row.locator('.recent-open').click()
  await page.waitForURL(/#\/docs/)
  expect(await docText(page, 'docs/catalog.md')).toContain(LINE)

  // Picking the demo from the list opens the same edited copy.
  await openDemo(page, 'The shop')
  expect(await docText(page, 'docs/catalog.md')).toContain(LINE)

  // Reset to the original, from the top bar: asks, then reopens the fixture.
  page.once('dialog', (d) => d.accept())
  await page.locator('.ws-place-chip').click()
  await page.locator('.ws-place-menu').getByRole('button', { name: 'Reset to the original' }).click()
  await expect.poll(() => docText(page, 'docs/catalog.md')).not.toContain(LINE)
  await expect(page.locator('.ws-place-chip')).toHaveText(/^demo\b(?! ·)/)
  expect(await storedKeys(page, 'file:demo:shop:')).toEqual([])
  await page.goto('/#/')
  await expect(page.locator('.proj.recent', { hasText: 'The shop' }).locator('.shape')).toHaveText('demo')
})

test('an edited demo resets from its recent row', async ({ page }) => {
  await openDemo(page, 'The shop')
  await page.goto('/#/docs/docs/system.md')
  await typeAtEnd(page, '\nA row reset line.')
  await expect.poll(() => storedText(page, 'file:demo:shop:', 'docs/system.md')).toContain('A row reset line.')
  await page.goto('/#/')
  const row = page.locator('.proj.recent', { hasText: 'The shop' })
  page.once('dialog', (d) => d.accept())
  await row.getByRole('button', { name: 'Reset to the original' }).click()
  await expect(row.locator('.shape')).toHaveText('demo')
  await expect.poll(() => storedKeys(page, 'file:demo:shop:')).toEqual([])
  await row.locator('.recent-open').click()
  await page.waitForURL(/#\/docs/)
  expect(await docText(page, 'docs/system.md')).not.toContain('A row reset line.')
})

test('New project works in a browser without the folder API', async ({ page }) => {
  await page.addInitScript(() => {
    delete (window as unknown as { showDirectoryPicker?: unknown }).showDirectoryPicker
  })
  await page.goto('/#/')
  await expect(page.locator('.home h2')).toHaveText(['Local project', 'Demos'])
  await expect(page.locator('.home-nofolder')).toContainText('New projects and the demos work here')
  await page.getByRole('button', { name: 'New project' }).click()
  await page.getByLabel('Project name').fill('Notes')
  await page.getByLabel('Project name').press('Enter')
  await page.waitForURL(/#\/docs/)
  await page.locator('.ws-place-chip').click()
  await expect(page.locator('.ws-place-menu').getByRole('button', { name: 'Save to a folder…' })).toHaveCount(0)
  await expect(page.locator('.ws-place-menu').getByRole('button', { name: 'Download .zip' })).toBeVisible()
})

test('forgetting a browser project asks, then deletes it from storage', async ({ page }) => {
  await page.goto('/#/')
  await page.getByRole('button', { name: 'New project' }).click()
  await page.getByLabel('Project name').fill('Scratch')
  await page.getByRole('button', { name: 'Create' }).click()
  await page.waitForURL(/#\/docs/)
  expect((await storedKeys(page, 'file:browser:')).length).toBe(2)
  await page.goto('/#/')
  let asked = ''
  page.once('dialog', (d) => {
    asked = d.message()
    void d.accept()
  })
  await page.locator('.proj.recent', { hasText: 'Scratch' }).locator('.recent-forget').click()
  await expect(page.locator('.proj.recent', { hasText: 'Scratch' })).toHaveCount(0)
  expect(asked).toContain('stored only in this browser')
  await expect.poll(() => storedKeys(page, 'file:browser:')).toEqual([])
})

test('a store the browser refuses is reported with the way out, and Retry stores it', async ({ page }) => {
  await page.addInitScript(() => {
    const w = window as unknown as { __quota?: boolean }
    const put = IDBObjectStore.prototype.put
    IDBObjectStore.prototype.put = function (value: unknown, key?: IDBValidKey) {
      if (w.__quota && String(key).startsWith('file:')) throw new DOMException('The quota has been exceeded.', 'QuotaExceededError')
      return put.call(this, value, key)
    }
  })
  await page.goto('/#/')
  await page.getByRole('button', { name: 'New project' }).click()
  await page.getByLabel('Project name').fill('Full disk')
  await page.getByRole('button', { name: 'Create' }).click()
  await page.waitForURL(/#\/docs/)
  await page.evaluate(() => {
    ;(window as unknown as { __quota: boolean }).__quota = true
  })
  await page.goto('/#/docs/docs/README.md')
  await typeAtEnd(page, '\nA line the disk had no room for.')
  const failed = page.locator('.ws-folder.failed')
  await expect(failed).toContainText(/Could not save to this browser: \d+ changes? in this tab only/, { timeout: 8000 })
  await expect(failed).toHaveAttribute('title', "this browser's storage for the site is full")
  await expect(failed.getByRole('button', { name: 'Download' })).toBeVisible()

  await page.evaluate(() => {
    ;(window as unknown as { __quota: boolean }).__quota = false
  })
  await failed.getByRole('button', { name: 'Retry' }).click()
  await expect(page.locator('.ws-folder.failed')).toHaveCount(0)
  await expect(page.locator('.ws-place-chip')).toContainText('in this browser')
  expect(await storedText(page, 'file:browser:', 'docs/README.md')).toContain('no room for')
})
