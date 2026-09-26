// A folder on disk in its two modes (docs/frontends/wasm.md#a-folder-on-disk). The
// directory picker cannot be driven by a test, so it hands back a folder in the
// browser's private file system, seeded with the sort example.
import { expect, test, type Page } from '@playwright/test'
import { offline } from './helpers'

async function readProjectFile(page: Page, path: string): Promise<string> {
  return page.evaluate(async (p) => {
    let d = await (await navigator.storage.getDirectory()).getDirectoryHandle('proj')
    const segs = p.split('/')
    const name = segs.pop()!
    for (const s of segs) d = await d.getDirectoryHandle(s)
    return (await (await d.getFileHandle(name)).getFile()).text()
  }, path)
}

test.beforeEach(async ({ page, browserName }) => {
  test.skip(browserName !== 'chromium', 'the File System Access API is Chromium only')
  await offline(page)
  await page.addInitScript(() => {
    ;(window as unknown as { showDirectoryPicker: unknown }).showDirectoryPicker = async () =>
      (await navigator.storage.getDirectory()).getDirectoryHandle('proj', { create: true })
  })
  await page.goto('/#/')
  const files = await (await page.request.get('/demos/sort.json')).json()
  await page.evaluate(async (files: Record<string, string>) => {
    const root = await navigator.storage.getDirectory()
    await root.removeEntry('proj', { recursive: true }).catch(() => {})
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

test('read-only holds a save and asks; writing lands it and then autosaves', async ({ page }) => {
  const original = await readProjectFile(page, 'docs/main.js.md')
  await expect(page.locator('.home-readonly input')).toBeChecked()
  await page.getByRole('button', { name: 'Open folder…' }).click()
  await page.waitForURL(/#\/docs/)
  await page.goto('/#/docs/docs/main.js.md')
  await expect(page.locator('.ws-folder')).toContainText('read-only')

  await page.locator('.cm-content').click()
  await page.keyboard.press('Control+End')
  await page.keyboard.type('\nHeld line.')
  await page.keyboard.press('Control+s')
  await expect(page.locator('.ide-askwrite')).toContainText('read-only')
  await expect(page.locator('.ws-folder')).toContainText('unwritten')
  expect(await readProjectFile(page, 'docs/main.js.md')).toBe(original)

  await page.locator('.ide-askwrite').getByRole('button', { name: 'write to disk' }).click()
  await expect(page.locator('.ws-folder')).toContainText('writing')
  await expect.poll(() => readProjectFile(page, 'docs/main.js.md')).toContain('Held line.')

  await page.locator('.cm-content').click()
  await page.keyboard.press('Control+End')
  await page.keyboard.type('\nAuto line.')
  await expect.poll(() => readProjectFile(page, 'docs/main.js.md'), { timeout: 8000 }).toContain('Auto line.')
})
