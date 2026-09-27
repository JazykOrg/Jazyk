// Browser storage (docs/frontends/wasm.md#browser-storage): a project started in the
// browser and an edited demo survive a reload and reopen from Recent with the edit;
// a demo resets to its original.
import { readFile } from 'node:fs/promises'
import { expect, test, type Page } from '@playwright/test'
import { unzipSync } from 'fflate'
import { api, chooseMenuItem, createProject, offline, openDemo, openProjectMenu, pageErrors, projectMenuItems, storedKeys, storedText, typeAtEnd, whereChangesGo } from './helpers'

test.beforeEach(async ({ page }) => {
  await offline(page)
})

async function docText(page: Page, path: string): Promise<string> {
  return (await api<{ text: string }>(page, 'GET', `/api/docs/content?path=${path}`)).text
}

test('a new project starts in the browser, keeps an edit across a reload, and reopens from Recent', async ({ page }) => {
  const errors = pageErrors(page)
  await page.goto('/#/')
  await createProject(page, 'Garden planner')

  // The minimal project: a jazyk.toml and a README saying what to write.
  const docs = await api<{ docs: { path: string }[] }>(page, 'GET', '/api/docs')
  expect(docs.docs.map((d) => d.path)).toEqual(['docs/README.md'])
  expect(await docText(page, 'docs/README.md')).toContain('# Garden planner')
  expect(await api(page, 'GET', '/api/capabilities')).toMatchObject({ backend: 'wasm', persistence: 'browser' })
  await expect(page.locator('.ws-project')).toContainText('Garden planner')
  expect(await whereChangesGo(page)).toBe('Saved in this browser only')

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

  // The project menu says where it lives and offers the ways out.
  expect(await whereChangesGo(page)).toBe('Saved in this browser only')
  const items = await projectMenuItems(page)
  expect(items).toContain('Download as .zip')
  expect(items).toContain('Save to a folder on this computer…')
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
  expect(await whereChangesGo(page)).toBe('Demo: edits are saved in this browser')
  expect(await projectMenuItems(page)).toContain('Reset to the original')

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

  // Reset to the original, from the project menu: asks, then reopens the fixture.
  page.once('dialog', (d) => d.accept())
  await chooseMenuItem(page, 'Reset to the original')
  await expect.poll(() => docText(page, 'docs/catalog.md')).not.toContain(LINE)
  await expect.poll(() => projectMenuItems(page)).not.toContain('Reset to the original')
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
  await expect(page.locator('.home h2')).toHaveText(['Try a demo', 'Start a project in this browser', 'Open a folder on this computer', 'Connect to jazyk on this computer'])
  await expect(page.locator('.home-nofolder')).toContainText('needs Chrome, Edge, or another Chromium browser')
  await page.getByLabel('Project name').fill('Notes')
  await page.getByLabel('Project name').press('Enter')
  await page.waitForURL(/#\/docs/)
  const items = await projectMenuItems(page)
  expect(items).not.toContain('Save to a folder on this computer…')
  expect(items).toContain('Download as .zip')
})

test('forgetting a browser project asks, then deletes it from storage', async ({ page }) => {
  await page.goto('/#/')
  await createProject(page, 'Scratch')
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
  await createProject(page, 'Full disk')
  await page.evaluate(() => {
    ;(window as unknown as { __quota: boolean }).__quota = true
  })
  await page.goto('/#/docs/docs/README.md')
  await typeAtEnd(page, '\nA line the disk had no room for.')
  // The warning beside the name; the menu says why and offers the ways out.
  const warning = page.locator('.ws-top .ws-unwritten')
  await expect(warning).toHaveText(/\d+ changes? not written/, { timeout: 8000 })
  await warning.click()
  const menu = page.locator('.ws-menu')
  const failed = menu.locator('.ws-menu-note.failed')
  await expect(failed).toContainText(/Could not save to this browser: \d+ changes? in this tab only/)
  await expect(failed).toHaveAttribute('title', "this browser's storage for the site is full")
  await expect(menu.getByRole('menuitem', { name: 'Download as .zip' })).toBeVisible()

  await page.evaluate(() => {
    ;(window as unknown as { __quota: boolean }).__quota = false
  })
  await menu.getByRole('menuitem', { name: 'Retry' }).click()
  await expect(warning).toHaveCount(0)
  expect(await whereChangesGo(page)).toBe('Saved in this browser only')
  expect(await projectMenuItems(page)).not.toContain('Retry')
  expect(await storedText(page, 'file:browser:', 'docs/README.md')).toContain('no room for')
})

// The download is the whole tree as one .zip named after the project, the layout a
// folder would hold (docs/frontends/wasm.md#a-project-in-the-browser).
test('Download .zip carries jazyk.toml and the edited README', async ({ page }) => {
  await page.goto('/#/')
  await createProject(page, 'Garden planner')
  await page.goto('/#/docs/docs/README.md')
  await typeAtEnd(page, '\nA bed of tulips along the fence.')
  await expect.poll(() => docText(page, 'docs/README.md')).toContain('A bed of tulips')

  const download = page.waitForEvent('download')
  const menu = await openProjectMenu(page)
  await menu.getByRole('menuitem', { name: 'Download as .zip' }).click()
  const file = await download
  expect(file.suggestedFilename()).toBe('garden-planner.zip')
  const entries = unzipSync(new Uint8Array(await readFile(await file.path())))
  const dec = new TextDecoder()
  expect(dec.decode(entries['garden-planner/jazyk.toml'])).toContain('docs/**/*.md')
  expect(dec.decode(entries['garden-planner/docs/README.md'])).toContain('# Garden planner')
  expect(dec.decode(entries['garden-planner/docs/README.md'])).toContain('A bed of tulips along the fence.')
})
