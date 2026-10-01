import { expect, type Locator, type Page } from '@playwright/test'

// Nothing leaves the machine: the page must work without the network, and the
// model must not download on arrival (docs/frontends/wasm.md#the-demo).
export async function offline(page: Page): Promise<string[]> {
  const external: string[] = []
  await page.route('**/*', (r) => {
    const u = r.request().url()
    if (u.startsWith('http://localhost')) return r.continue()
    external.push(u)
    return r.abort()
  })
  return external
}

export async function openDemo(page: Page, name: string) {
  await page.goto('/#/')
  await page.locator('.home .demo', { hasText: name }).click()
  await page.waitForURL(/#\/docs/)
  await expect(page.locator('.ws-lenses')).toBeVisible()
}

// The project menu (docs/frontends/gui.md#the-project-menu), opened from the
// project's name in the top bar.
export async function openProjectMenu(page: Page): Promise<Locator> {
  await page.locator('.ws-top .ws-project').click()
  const menu = page.locator('.ws-menu')
  await expect(menu).toBeVisible()
  return menu
}

export async function closeMenu(page: Page) {
  await page.keyboard.press('Escape')
  await expect(page.locator('.ws-menu')).toHaveCount(0)
}

// The menu's first line: where the project's changes go.
export async function whereChangesGo(page: Page): Promise<string> {
  const menu = await openProjectMenu(page)
  const text = (await menu.locator('.ws-menu-where').textContent()) ?? ''
  await closeMenu(page)
  return text
}

// The names of the menu's items, in order.
export async function projectMenuItems(page: Page): Promise<string[]> {
  const menu = await openProjectMenu(page)
  const names = await menu.getByRole('menuitem').allTextContents()
  await closeMenu(page)
  return names
}

// A submenu of the project menu (`Open recent`, `Open demo`), opened from the menu.
export async function openSubmenu(page: Page, label: string): Promise<Locator> {
  const menu = await openProjectMenu(page)
  await menu.getByRole('menuitem', { name: label, exact: true }).click()
  const sub = page.locator('.ws-menu-sub')
  await expect(sub).toBeVisible()
  return sub
}

export async function chooseMenuItem(page: Page, name: string) {
  const menu = await openProjectMenu(page)
  await menu.getByRole('menuitem', { name, exact: true }).click()
  await expect(page.locator('.ws-menu')).toHaveCount(0)
}

// Start a project in this browser, from Home's card (docs/frontends/gui.md#home).
export async function createProject(page: Page, name: string) {
  await page.getByLabel('Project name').fill(name)
  await page.getByRole('button', { name: 'Create' }).click()
  await page.waitForURL(/#\/docs/)
}

// The backend in the tab answers the same envelope the app reads.
export async function api<T = unknown>(page: Page, method: string, path: string, body?: unknown): Promise<T> {
  return page.evaluate(
    async ([m, p, b]) => (await (window as unknown as { jazyk: { request: (m: string, p: string, b?: unknown) => Promise<{ body: unknown }> } }).jazyk.request(m as string, p as string, b)).body,
    [method, path, body] as const,
  ) as Promise<T>
}

// The same request with its status, for a test that asserts a refusal.
export async function envelope(page: Page, method: string, path: string, body?: unknown): Promise<{ status: number; body: unknown }> {
  return page.evaluate(
    async ([m, p, b]) =>
      (window as unknown as { jazyk: { request: (m: string, p: string, b?: unknown) => Promise<{ status: number; body: unknown }> } }).jazyk.request(
        m as string,
        p as string,
        b,
      ),
    [method, path, body] as const,
  )
}

// Types at the end of the open document, the way a person appends a line. A
// document open in Read (the default in a read-only folder) is switched to Write
// first (docs/frontends/gui.md#editor).
// Switches the open document to Write. A hidden editor (Read is the default in a
// read-only folder) renders only the lines of its empty viewport, so an assertion
// on `.cm-content` needs Write first.
export async function toWrite(page: Page) {
  const write = page.locator('.ide-mode:visible').getByRole('button', { name: 'Write' })
  await expect(write).toBeVisible()
  if ((await write.getAttribute('aria-pressed')) !== 'true') await write.click()
}

// Clicks the last line rather than the middle of the text, the way a person appends.
export async function typeAtEnd(page: Page, text: string) {
  await toWrite(page)
  await page.locator('.cm-content:visible .cm-line').last().click()
  await page.keyboard.press('Control+End')
  await page.keyboard.type(text)
}

// Waits until browser storage holds every change the page made (the store is
// coalesced, docs/frontends/wasm.md#browser-storage), so a reload finds them.
export async function waitStored(page: Page) {
  await expect
    .poll(() => page.evaluate(() => (window as unknown as { jazykPending?: () => number }).jazykPending?.() ?? 0), { timeout: 10_000 })
    .toBe(0)
}

// Browser storage as the page keeps it (docs/frontends/wasm.md#browser-storage): the
// keys under `prefix`, and for each the stored text (a file's bytes, or a kept
// change's). A database that does not exist yet is created the way the page creates
// it, so the page never finds one without its object store.
async function stored(page: Page, prefix: string): Promise<Record<string, string>> {
  return page.evaluate(
    (prefix) =>
      new Promise<Record<string, string>>((resolve, reject) => {
        const open = indexedDB.open('jazyk-projects')
        open.onupgradeneeded = () => open.result.createObjectStore('files')
        open.onerror = () => reject(open.error)
        open.onsuccess = () => {
          const store = open.result.transaction('files').objectStore('files')
          const range = IDBKeyRange.bound(prefix, prefix + '￿')
          const keys = store.getAllKeys(range)
          const values = store.getAll(range)
          values.onerror = () => reject(values.error)
          values.onsuccess = () => {
            const out: Record<string, string> = {}
            const dec = new TextDecoder()
            keys.result.forEach((k, i) => {
              const v = values.result[i] as Uint8Array | { bytes?: Uint8Array; op: string }
              const bytes = v instanceof Uint8Array ? v : v.bytes
              out[String(k)] = bytes ? dec.decode(bytes) : `(${(v as { op: string }).op})`
            })
            open.result.close()
            resolve(out)
          }
        }
      }),
    prefix,
  )
}

export async function storedKeys(page: Page, prefix: string): Promise<string[]> {
  return Object.keys(await stored(page, prefix))
}

// The text of the one stored record whose key starts with `prefix` and ends with
// `path`, or '' when there is none.
export async function storedText(page: Page, prefix: string, path: string): Promise<string> {
  const all = await stored(page, prefix)
  const key = Object.keys(all).find((k) => k.endsWith(`:${path}`))
  return key ? all[key] : ''
}

// Collects page errors; a test asserts it stays empty.
export function pageErrors(page: Page): string[] {
  const errs: string[] = []
  page.on('pageerror', (e) => errs.push(String(e)))
  return errs
}
