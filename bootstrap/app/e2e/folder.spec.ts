// A folder on disk in its two modes (docs/frontends/wasm.md#a-folder-on-disk), and the
// changes it has not taken kept in browser storage
// (docs/frontends/wasm.md#when-a-folder-cannot-be-written). The directory picker
// cannot be driven by a test, so it hands back a folder in the browser's private file
// system, seeded with the sort example. `window.__failWrites` makes every file write
// fail, the way a lapsed permission or a moved folder does.
import { expect, test, type Page } from '@playwright/test'
import { api, offline, storedKeys, typeAtEnd } from './helpers'

async function readProjectFile(page: Page, path: string, root = 'proj'): Promise<string> {
  return page.evaluate(
    async ([p, r]) => {
      let d = await (await navigator.storage.getDirectory()).getDirectoryHandle(r)
      const segs = p.split('/')
      const name = segs.pop()!
      for (const s of segs) d = await d.getDirectoryHandle(s)
      return (await (await d.getFileHandle(name)).getFile()).text()
    },
    [path, root] as const,
  )
}

const failWrites = (page: Page, fail: boolean) =>
  page.evaluate((f) => {
    ;(window as unknown as { __failWrites: boolean }).__failWrites = f
  }, fail)

// Closing a tab that keeps changes asks first; the tests answer yes.
const acceptDialogs = (page: Page) => page.on('dialog', (d) => void d.accept())

// How many file writes the stub refused so far.
const refused = (page: Page) => page.evaluate(() => (window as unknown as { __refused?: number }).__refused ?? 0)

test.beforeEach(async ({ page, browserName }) => {
  test.skip(browserName !== 'chromium', 'the File System Access API is Chromium only')
  await offline(page)
  await page.addInitScript(() => {
    const w = window as unknown as { showDirectoryPicker: unknown; __pickDir?: string; __failWrites?: boolean; __refused?: number }
    w.showDirectoryPicker = async () => (await navigator.storage.getDirectory()).getDirectoryHandle(w.__pickDir ?? 'proj', { create: true })
    const real = FileSystemFileHandle.prototype.createWritable
    FileSystemFileHandle.prototype.createWritable = function (...args: Parameters<typeof real>) {
      if (w.__failWrites) {
        w.__refused = (w.__refused ?? 0) + 1
        return Promise.reject(new DOMException('The disk refused the write.', 'NoModificationAllowedError'))
      }
      return real.apply(this, args)
    }
    // Chrome closes the page when it reads a private-file-system handle back out of
    // IndexedDB, which a picked folder never is. The test stores the folder's name in
    // the handle's place and hands the handle back when it is read.
    const dirs = new Map<string, FileSystemDirectoryHandle>()
    void navigator.storage.getDirectory().then((root) => {
      for (const name of ['proj', 'saved']) void root.getDirectoryHandle(name).then((h) => dirs.set(name, h), () => {})
    })
    const put = IDBObjectStore.prototype.put
    IDBObjectStore.prototype.put = function (value: unknown, key?: IDBValidKey) {
      if (value instanceof FileSystemDirectoryHandle) {
        dirs.set(value.name, value)
        value = { __testDir: value.name }
      }
      return put.call(this, value, key)
    }
    const result = Object.getOwnPropertyDescriptor(IDBRequest.prototype, 'result')!
    Object.defineProperty(IDBRequest.prototype, 'result', {
      get() {
        const r = result.get!.call(this)
        return r && typeof r === 'object' && '__testDir' in r ? dirs.get((r as { __testDir: string }).__testDir) : r
      },
    })
  })
  await page.goto('/#/')
  const files = await (await page.request.get('/demos/sort.json')).json()
  await page.evaluate(async (files: Record<string, string>) => {
    const root = await navigator.storage.getDirectory()
    await root.removeEntry('proj', { recursive: true }).catch(() => {})
    await root.removeEntry('saved', { recursive: true }).catch(() => {})
    const proj = await root.getDirectoryHandle('proj', { create: true })
    for (const [path, text] of Object.entries(files)) {
      const segs = path.split('/')
      const name = segs.pop()!
      let d = proj
      for (const s of segs) d = await d.getDirectoryHandle(s, { create: true })
      const w = await (await d.getFileHandle(name, { create: true })).createWritable()
      await w.write(text)
      await w.close()
    }
  }, files)
})

async function openFolder(page: Page, mode: 'read' | 'write') {
  const box = page.locator('.home-readonly input')
  if (mode === 'write') await box.uncheck()
  else await box.check()
  await page.getByRole('button', { name: 'Open folder…' }).click()
  await page.waitForURL(/#\/docs/)
}

test('read-only holds a save and asks; writing lands it and then autosaves', async ({ page }) => {
  const original = await readProjectFile(page, 'docs/main.js.md')
  await expect(page.locator('.home-readonly input')).toBeChecked()
  await page.getByRole('button', { name: 'Open folder…' }).click()
  await page.waitForURL(/#\/docs/)
  await page.goto('/#/docs/docs/main.js.md')
  await expect(page.locator('.ws-folder')).toContainText('read-only')

  await typeAtEnd(page, '\nHeld line.')
  await page.keyboard.press('Control+s')
  await expect(page.locator('.ide-askwrite')).toContainText('read-only')
  await expect(page.locator('.ws-folder')).toContainText('unwritten')
  expect(await readProjectFile(page, 'docs/main.js.md')).toBe(original)

  await page.locator('.ide-askwrite').getByRole('button', { name: 'write to disk' }).click()
  await expect(page.locator('.ws-folder')).toContainText('writing')
  await expect.poll(() => readProjectFile(page, 'docs/main.js.md')).toContain('Held line.')

  await typeAtEnd(page, '\nAuto line.')
  await expect.poll(() => readProjectFile(page, 'docs/main.js.md'), { timeout: 8000 }).toContain('Auto line.')
})

test('a folder whose writes fail keeps them in this browser, and Retry lands them', async ({ page }) => {
  await openFolder(page, 'write')
  await page.goto('/#/docs/docs/main.js.md')
  await expect(page.locator('.ws-folder')).toContainText('writing')
  await failWrites(page, true)

  await typeAtEnd(page, '\nKept line.')
  const failed = page.locator('.ws-folder.failed')
  await expect(failed).toContainText(/Could not write to proj: \d+ changes? kept in this browser/, { timeout: 8000 })
  expect(await readProjectFile(page, 'docs/main.js.md')).not.toContain('Kept line.')
  await expect.poll(() => storedKeys(page, 'kept:folder:')).toContainEqual(expect.stringMatching(/:docs\/main\.js\.md$/))

  // Still failing: Retry stops at the first change and keeps them all.
  const before = await refused(page)
  await failed.getByRole('button', { name: 'Retry' }).click()
  await expect.poll(() => refused(page)).toBeGreaterThan(before)
  await expect(failed.getByRole('button', { name: 'Retry' })).toBeVisible()

  await failWrites(page, false)
  await failed.getByRole('button', { name: 'Retry' }).click()
  await expect(page.locator('.ws-folder')).toHaveText(/^writing/)
  await expect(page.locator('.ws-folder.failed')).toHaveCount(0)
  expect(await readProjectFile(page, 'docs/main.js.md')).toContain('Kept line.')
  await expect.poll(() => storedKeys(page, 'kept:folder:')).toEqual([])

  // Writing goes on as usual.
  await typeAtEnd(page, '\nAfter line.')
  await expect.poll(() => readProjectFile(page, 'docs/main.js.md'), { timeout: 8000 }).toContain('After line.')
})

test('reopening a folder with kept changes shows them and offers to write them', async ({ page }) => {
  acceptDialogs(page)
  await openFolder(page, 'write')
  await page.goto('/#/docs/docs/main.js.md')
  await failWrites(page, true)
  await typeAtEnd(page, '\nKept line.')
  await expect(page.locator('.ws-folder.failed')).toBeVisible({ timeout: 8000 })
  await expect.poll(() => storedKeys(page, 'kept:folder:')).not.toEqual([])

  // A reload is a new tab's worth of state: writes work again, nothing is open.
  await page.reload()
  await page.goto('/#/')
  const row = page.locator('.proj.recent', { hasText: 'proj' })
  await expect(row.locator('.shape')).toContainText(/folder · writing · \d+ changes? kept in this browser/)
  await row.locator('.recent-open').click()
  await page.waitForURL(/#\/docs/)

  // The tab shows the work as it was left; the folder does not have it yet.
  expect((await api<{ text: string }>(page, 'GET', '/api/docs/content?path=docs/main.js.md')).text).toContain('Kept line.')
  expect(await readProjectFile(page, 'docs/main.js.md')).not.toContain('Kept line.')
  const held = page.locator('.ws-folder.held')
  await expect(held).toContainText('kept in this browser')
  await held.getByRole('button', { name: 'Write to folder' }).click()
  await expect(page.locator('.ws-folder')).toHaveText(/^writing/)
  expect(await readProjectFile(page, 'docs/main.js.md')).toContain('Kept line.')
  await expect.poll(() => storedKeys(page, 'kept:folder:')).toEqual([])
})

test("read-only mode's held changes survive a reload, and discard reopens the folder as it is", async ({ page }) => {
  acceptDialogs(page)
  const original = await readProjectFile(page, 'docs/main.js.md')
  await openFolder(page, 'read')
  await page.goto('/#/docs/docs/main.js.md')
  await typeAtEnd(page, '\nHeld line.')
  await page.keyboard.press('Control+s')
  await expect(page.locator('.ws-folder')).toContainText('unwritten')
  await expect.poll(() => storedKeys(page, 'kept:folder:')).toContainEqual(expect.stringMatching(/:docs\/main\.js\.md$/))

  await page.reload()
  await page.goto('/#/')
  const row = page.locator('.proj.recent', { hasText: 'proj' })
  await expect(row.locator('.shape')).toContainText(/folder · read-only · \d+ changes? kept in this browser/)
  await row.locator('.recent-open').click()
  await page.waitForURL(/#\/docs/)
  expect((await api<{ text: string }>(page, 'GET', '/api/docs/content?path=docs/main.js.md')).text).toContain('Held line.')
  await expect(page.locator('.ws-folder')).toContainText(/read-only · \d+ unwritten/)
  expect(await readProjectFile(page, 'docs/main.js.md')).toBe(original)

  await page.goto('/#/docs/docs/main.js.md')
  await expect(page.locator('.cm-content')).toContainText('Held line.')
  await page.locator('.ws-folder').getByRole('button', { name: 'discard' }).click()
  await expect(page.locator('.ws-folder')).toHaveText(/^read-only/)
  await expect(page.locator('.ws-folder')).not.toContainText('unwritten')
  await expect.poll(async () => (await api<{ text: string }>(page, 'GET', '/api/docs/content?path=docs/main.js.md')).text).toBe(original)
  await expect.poll(() => storedKeys(page, 'kept:folder:')).toEqual([])
  // The editor shows the folder as it is, and nothing is kept again.
  await expect(page.locator('.cm-content')).not.toContainText('Held line.')
  await page.waitForTimeout(1500)
  await expect(page.locator('.ws-folder')).not.toContainText('unwritten')
})

test('a browser project saved to a folder becomes that folder, in writing mode', async ({ page }) => {
  await page.getByRole('button', { name: 'New project' }).click()
  await page.getByLabel('Project name').fill('Garden planner')
  await page.getByRole('button', { name: 'Create' }).click()
  await page.waitForURL(/#\/docs/)

  await page.evaluate(() => {
    ;(window as unknown as { __pickDir: string }).__pickDir = 'saved'
  })
  await page.locator('.ws-place-chip').click()
  await page.locator('.ws-place-menu').getByRole('button', { name: 'Save to a folder…' }).click()
  await expect(page.locator('.ws-folder')).toHaveText(/^writing/)
  expect(await readProjectFile(page, 'jazyk.toml', 'saved')).toContain('docs/**/*.md')
  expect(await readProjectFile(page, 'docs/README.md', 'saved')).toContain('# Garden planner')
  expect(await api(page, 'GET', '/api/capabilities')).toMatchObject({ persistence: 'folder' })
  // The browser copy is gone; the recent entry is the folder's.
  await expect.poll(() => storedKeys(page, 'file:browser:')).toEqual([])

  await page.goto('/#/docs/docs/README.md')
  await typeAtEnd(page, '\nWritten through to the folder.')
  await expect.poll(() => readProjectFile(page, 'docs/README.md', 'saved'), { timeout: 8000 }).toContain('Written through')

  await page.goto('/#/')
  await expect(page.locator('.proj.recent', { hasText: 'Garden planner' })).toHaveCount(0)
  await expect(page.locator('.proj.recent', { hasText: 'saved' }).locator('.shape')).toHaveText('folder · writing')
})
