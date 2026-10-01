// A folder on disk in its two modes (docs/frontends/wasm.md#a-folder-on-disk), and the
// changes it has not taken kept in browser storage
// (docs/frontends/wasm.md#when-a-folder-cannot-be-written). The directory picker
// cannot be driven by a test, so it hands back a folder in the browser's private file
// system, seeded with the sort example. `window.__failWrites` makes every file write
// fail, the way a lapsed permission or a moved folder does; `window.__denyWrite`
// makes the browser refuse a write grant.
import { expect, test, type Page } from '@playwright/test'
import { api, chooseMenuItem, closeMenu, createProject, offline, openFolders, openProjectMenu, projectMenuItems, storedKeys, storedText, toWrite, typeAtEnd, whereChangesGo } from './helpers'

// A file mid-write (the writable's close replaces it) reads as absent or unreadable
// for an instant; that reads as '' so an expect.poll around it retries.
async function readProjectFile(page: Page, path: string, root = 'proj'): Promise<string> {
  return page.evaluate(
    async ([p, r]) => {
      try {
        let d = await (await navigator.storage.getDirectory()).getDirectoryHandle(r)
        const segs = p.split('/')
        const name = segs.pop()!
        for (const s of segs) d = await d.getDirectoryHandle(s)
        return await (await (await d.getFileHandle(name)).getFile()).text()
      } catch (e) {
        if (e instanceof DOMException && (e.name === 'NotFoundError' || e.name === 'NotReadableError')) return ''
        throw e
      }
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
    const w = window as unknown as { showDirectoryPicker: unknown; __pickDir?: string; __failWrites?: boolean; __refused?: number; __denyWrite?: boolean }
    w.showDirectoryPicker = async () => (await navigator.storage.getDirectory()).getDirectoryHandle(w.__pickDir ?? 'proj', { create: true })
    // A write grant the browser refuses: both the query and the request say so.
    const perms = FileSystemHandle.prototype as unknown as Record<'queryPermission' | 'requestPermission', (o: { mode: string }) => Promise<string>>
    for (const name of ['queryPermission', 'requestPermission'] as const) {
      const real = perms[name]
      perms[name] = function (o: { mode: string }) {
        if (w.__denyWrite && o?.mode === 'readwrite') return Promise.resolve('denied')
        return real.call(this, o)
      }
    }
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
  // Writing is the default; read-only is the choice.
  await expect(page.locator('.home-readonly input')).not.toBeChecked()
  await page.locator('.home-readonly input').check()
  await page.getByRole('button', { name: 'Open folder…' }).click()
  await page.waitForURL(/#\/docs/)
  await page.goto('/#/docs/docs/main.js.md')
  expect(await whereChangesGo(page)).toBe('Saved to proj (read-only)')

  await typeAtEnd(page, '\nHeld line.')
  await page.keyboard.press('Control+s')
  await expect(page.locator('.ide-askwrite')).toContainText('read-only')
  expect(await projectMenuItems(page)).toContain('Write 1 change to the folder')
  expect(await readProjectFile(page, 'docs/main.js.md')).toBe(original)

  await page.locator('.ide-askwrite').getByRole('button', { name: 'write to disk' }).click()
  await expect.poll(() => whereChangesGo(page)).toBe('Saved to proj (writing)')
  await expect.poll(() => readProjectFile(page, 'docs/main.js.md')).toContain('Held line.')

  await typeAtEnd(page, '\nAuto line.')
  await expect.poll(() => readProjectFile(page, 'docs/main.js.md'), { timeout: 8000 }).toContain('Auto line.')
})

test('a folder whose writes fail keeps them in this browser, and Retry lands them', async ({ page }) => {
  await openFolder(page, 'write')
  await page.goto('/#/docs/docs/main.js.md')
  expect(await whereChangesGo(page)).toBe('Saved to proj (writing)')
  await failWrites(page, true)

  await typeAtEnd(page, '\nKept line.')
  // The one warning beside the name; clicking it opens the menu with the reason.
  const warning = page.locator('.ws-top .ws-unwritten')
  await expect(warning).toHaveText(/\d+ changes? not written/, { timeout: 8000 })
  expect(await readProjectFile(page, 'docs/main.js.md')).not.toContain('Kept line.')
  await expect.poll(() => storedKeys(page, 'kept:folder:')).toContainEqual(expect.stringMatching(/:docs\/main\.js\.md$/))
  await warning.click()
  const menu = page.locator('.ws-menu')
  await expect(menu.locator('.ws-menu-note.failed')).toContainText(/Could not write to proj: \d+ changes? kept in this browser/)

  // Still failing: Retry stops at the first change and keeps them all.
  const before = await refused(page)
  await menu.getByRole('menuitem', { name: 'Retry' }).click()
  await expect.poll(() => refused(page)).toBeGreaterThan(before)
  await expect(warning).toBeVisible()

  await failWrites(page, false)
  await chooseMenuItem(page, 'Retry')
  await expect(warning).toHaveCount(0)
  expect(await whereChangesGo(page)).toBe('Saved to proj (writing)')
  expect(await projectMenuItems(page)).not.toContain('Retry')
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
  await expect(page.locator('.ws-top .ws-unwritten')).toBeVisible({ timeout: 8000 })
  await expect.poll(() => storedKeys(page, 'kept:folder:')).not.toEqual([])

  // A reload is a new tab's worth of state: writes work again. The folder reopens
  // from the recent row.
  await page.reload()
  await page.goto('/#/')
  const row = page.locator('.proj.recent', { hasText: 'proj' })
  await expect(row.locator('.shape')).toContainText(/folder · writing · \d+ changes? kept in this browser/)
  await row.locator('.recent-open').click()
  await page.waitForURL(/#\/docs/)

  // The tab shows the work as it was left; the folder does not have it yet.
  expect((await api<{ text: string }>(page, 'GET', '/api/docs/content?path=docs/main.js.md')).text).toContain('Kept line.')
  expect(await readProjectFile(page, 'docs/main.js.md')).not.toContain('Kept line.')
  expect(await whereChangesGo(page)).toBe('Saved to proj (writing)')
  await chooseMenuItem(page, 'Write 1 change to the folder')
  await expect.poll(() => projectMenuItems(page)).not.toContain('Discard')
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
  await expect.poll(() => projectMenuItems(page)).toContain('Write 1 change to the folder')
  await expect.poll(() => storedKeys(page, 'kept:folder:')).toContainEqual(expect.stringMatching(/:docs\/main\.js\.md$/))

  await page.reload()
  await page.goto('/#/')
  const row = page.locator('.proj.recent', { hasText: 'proj' })
  await expect(row.locator('.shape')).toContainText(/folder · read-only · \d+ changes? kept in this browser/)
  await row.locator('.recent-open').click()
  await page.waitForURL(/#\/docs/)
  expect((await api<{ text: string }>(page, 'GET', '/api/docs/content?path=docs/main.js.md')).text).toContain('Held line.')
  expect(await whereChangesGo(page)).toBe('Saved to proj (read-only)')
  expect(await projectMenuItems(page)).toContain('Write 1 change to the folder')
  expect(await readProjectFile(page, 'docs/main.js.md')).toBe(original)

  await page.goto('/#/docs/docs/main.js.md')
  await toWrite(page)
  await expect(page.locator('.cm-content')).toContainText('Held line.')
  await chooseMenuItem(page, 'Discard')
  await expect.poll(() => whereChangesGo(page)).toBe('Saved to proj (read-only)')
  expect(await projectMenuItems(page)).not.toContain('Discard')
  await expect.poll(async () => (await api<{ text: string }>(page, 'GET', '/api/docs/content?path=docs/main.js.md')).text).toBe(original)
  await expect.poll(() => storedKeys(page, 'kept:folder:')).toEqual([])
  // The editor shows the folder as it is, and nothing is kept again.
  await expect(page.locator('.cm-content')).not.toContainText('Held line.')
  await page.waitForTimeout(1500)
  expect(await projectMenuItems(page)).not.toContain('Discard')
})

test('a browser project saved to a folder becomes that folder, in writing mode', async ({ page }) => {
  await createProject(page, 'Garden planner')

  await page.evaluate(() => {
    ;(window as unknown as { __pickDir: string }).__pickDir = 'saved'
  })
  await chooseMenuItem(page, 'Save to a folder on this computer…')
  await expect.poll(() => whereChangesGo(page)).toBe('Saved to saved (writing)')
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

test("'not now' on the write prompt keeps the folder read-only and holds the change", async ({ page }) => {
  const original = await readProjectFile(page, 'docs/main.js.md')
  await openFolder(page, 'read')
  await page.goto('/#/docs/docs/main.js.md')
  await typeAtEnd(page, '\nHeld line.')
  await page.keyboard.press('Control+s')
  const ask = page.locator('.ide-askwrite')
  await expect(ask).toContainText('read-only')
  await ask.getByRole('button', { name: 'not now' }).click()
  await expect(ask).toHaveCount(0)
  expect(await whereChangesGo(page)).toBe('Saved to proj (read-only)')
  expect(await projectMenuItems(page)).toContain('Write 1 change to the folder')
  expect(await readProjectFile(page, 'docs/main.js.md')).toBe(original)
  expect((await api<{ text: string }>(page, 'GET', '/api/docs/content?path=docs/main.js.md')).text).toContain('Held line.')
  await expect.poll(() => storedKeys(page, 'kept:folder:')).toContainEqual(expect.stringMatching(/:docs\/main\.js\.md$/))
})

test('switching a writing folder to read-only from the top bar keeps new edits in this browser', async ({ page }) => {
  await openFolder(page, 'write')
  await page.goto('/#/docs/docs/main.js.md')
  expect(await whereChangesGo(page)).toBe('Saved to proj (writing)')
  await chooseMenuItem(page, 'Switch to read-only')
  await expect.poll(() => whereChangesGo(page)).toBe('Saved to proj (read-only)')
  const original = await readProjectFile(page, 'docs/main.js.md')

  await typeAtEnd(page, '\nKept line.')
  await page.keyboard.press('Control+s')
  await expect.poll(() => projectMenuItems(page)).toContain('Write 1 change to the folder')
  await expect.poll(() => storedKeys(page, 'kept:folder:')).toContainEqual(expect.stringMatching(/:docs\/main\.js\.md$/))
  expect(await readProjectFile(page, 'docs/main.js.md')).toBe(original)
  // The recent list remembers the mode it was left in.
  await page.goto('/#/')
  await expect(page.locator('.proj.recent', { hasText: 'proj' }).locator('.shape')).toHaveText('folder · read-only · 1 change kept in this browser')
})

// Read is the default in a folder opened read-only, Write in a writing one
// (docs/frontends/gui.md#editor).
test('a read-only folder opens its documents in Read, a writing folder in Write', async ({ page }) => {
  await openFolder(page, 'read')
  await page.goto('/#/docs/docs/main.js.md')
  await expect(page.locator('.ide-mode').getByRole('button', { name: 'Read' })).toHaveAttribute('aria-pressed', 'true')
  await expect(page.locator('.md-read')).toBeVisible()
  await expect(page.locator('.cm-content')).toBeHidden()
  await chooseMenuItem(page, 'Close project')
  await openFolder(page, 'write')
  await page.goto('/#/docs/docs/main.js.md')
  await expect(page.locator('.ide-mode').getByRole('button', { name: 'Write' })).toHaveAttribute('aria-pressed', 'true')
  await expect(page.locator('.cm-content')).toBeVisible()
})

test('picking the same folder twice gives one Recent row', async ({ page }) => {
  await openFolder(page, 'read')
  await page.goto('/#/')
  await expect(page.locator('.proj.recent', { hasText: 'proj' })).toHaveCount(1)
  await page.getByRole('button', { name: 'Open folder…' }).click()
  await page.waitForURL(/#\/docs/)
  await page.goto('/#/')
  await expect(page.locator('.proj.recent')).toHaveCount(1)
  await expect(page.locator('.proj.recent').locator('.shape')).toHaveText('folder · read-only')
})

test('a save to a folder whose writes fail leaves the browser project intact', async ({ page }) => {
  await createProject(page, 'Garden planner')
  await page.evaluate(() => {
    const w = window as unknown as { __pickDir: string; __failWrites: boolean }
    w.__pickDir = 'saved'
    w.__failWrites = true
  })
  let alerted = ''
  page.once('dialog', (d) => {
    alerted = d.message()
    void d.accept()
  })
  await chooseMenuItem(page, 'Save to a folder on this computer…')
  await expect.poll(() => alerted).toContain('Could not save Garden planner to saved')
  expect(alerted).toContain('The project stays in this browser')

  // Still a browser project: its storage, its descriptor, its recent row.
  expect(await whereChangesGo(page)).toBe('Saved in this browser only')
  expect(await api(page, 'GET', '/api/capabilities')).toMatchObject({ persistence: 'browser' })
  expect((await storedKeys(page, 'file:browser:')).length).toBe(2)
  await failWrites(page, false)
  await page.goto('/#/docs/docs/README.md')
  await typeAtEnd(page, '\nStill kept here.')
  await expect.poll(() => storedText(page, 'file:browser:', 'docs/README.md')).toContain('Still kept here.')
  await page.goto('/#/')
  await expect(page.locator('.proj.recent', { hasText: 'Garden planner' }).locator('.shape')).toHaveText('browser project · only in this browser')
  await expect(page.locator('.proj.recent', { hasText: 'saved' })).toHaveCount(0)
})

test('a write grant the browser refuses leaves the folder read-only and says so', async ({ page }) => {
  const original = await readProjectFile(page, 'docs/main.js.md')
  await openFolder(page, 'read')
  await page.goto('/#/docs/docs/main.js.md')
  await typeAtEnd(page, '\nHeld line.')
  await page.keyboard.press('Control+s')
  await expect(page.locator('.ide-askwrite')).toBeVisible()
  await page.evaluate(() => {
    ;(window as unknown as { __denyWrite: boolean }).__denyWrite = true
  })
  await page.locator('.ide-askwrite').getByRole('button', { name: 'write to disk' }).click()
  // A refused grant is a problem: the warning beside the name, the reason in the menu.
  await expect(page.locator('.ws-top .ws-unwritten')).toHaveText('1 change not written')
  let menu = await openProjectMenu(page)
  await expect(menu.locator('.ws-menu-note.failed')).toContainText('the browser did not grant write access')
  await expect(menu.locator('.ws-menu-where')).toHaveText('Saved to proj (read-only)')
  await expect(menu.getByRole('menuitem', { name: 'Write 1 change to the folder' })).toBeVisible()
  await closeMenu(page)
  expect(await readProjectFile(page, 'docs/main.js.md')).toBe(original)
  await expect.poll(() => storedKeys(page, 'kept:folder:')).toContainEqual(expect.stringMatching(/:docs\/main\.js\.md$/))

  // Granted the next time: the held change lands and the message goes.
  await page.evaluate(() => {
    ;(window as unknown as { __denyWrite: boolean }).__denyWrite = false
  })
  await chooseMenuItem(page, 'Write 1 change to the folder')
  await expect.poll(() => whereChangesGo(page)).toBe('Saved to proj (writing)')
  await expect(page.locator('.ws-top .ws-unwritten')).toHaveCount(0)
  menu = await openProjectMenu(page)
  await expect(menu.locator('.ws-menu-note.failed')).toHaveCount(0)
  await closeMenu(page)
  await expect.poll(() => readProjectFile(page, 'docs/main.js.md')).toContain('Held line.')
})

// A folder with no jazyk.toml opens into the setup wizard, which writes the file
// (docs/frontends/gui.md#the-setup-wizard); the read-only choice is off by default.
test('a folder without jazyk.toml opens into the setup wizard, which writes it', async ({ page }) => {
  await page.evaluate(async () => {
    const root = await navigator.storage.getDirectory()
    await root.removeEntry('plain', { recursive: true }).catch(() => {})
    const plain = await root.getDirectoryHandle('plain', { create: true })
    const docs = await plain.getDirectoryHandle('docs', { create: true })
    const w = await (await docs.getFileHandle('a.md', { create: true })).createWritable()
    await w.write('# A\n\nThe thing shall work.\n')
    await w.close()
    ;(window as unknown as { __pickDir?: string }).__pickDir = 'plain'
  })
  await expect(page.locator('.home-readonly input')).not.toBeChecked()
  await page.getByRole('button', { name: 'Open folder…' }).click()
  await expect(page.locator('.wizard h2')).toHaveText('Set up this project')
  await expect(page.locator('.wizard')).toContainText('plain')
  await page.locator('.wizard textarea').first().fill('docs/**/*.md')
  await page.getByRole('button', { name: 'Write jazyk.toml' }).click()
  await expect(page).toHaveURL(/#\/docs/)
  await expect.poll(() => readProjectFile(page, 'jazyk.toml', 'plain')).toContain('docs/**/*.md')
  await openFolders(page, ['docs'])
  await expect(page.locator('.ws-nav')).toContainText('a.md')
  expect(await whereChangesGo(page)).toBe('Saved to plain (writing)')
})
