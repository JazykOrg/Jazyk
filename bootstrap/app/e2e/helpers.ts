import { expect, type Page } from '@playwright/test'

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

// The backend in the tab answers the same envelope the app reads.
export async function api<T = unknown>(page: Page, method: string, path: string, body?: unknown): Promise<T> {
  return page.evaluate(
    async ([m, p, b]) => (await (window as unknown as { jazyk: { request: (m: string, p: string, b?: unknown) => Promise<{ body: unknown }> } }).jazyk.request(m as string, p as string, b)).body,
    [method, path, body] as const,
  ) as Promise<T>
}

// Types at the end of the open document, the way a person appends a line.
export async function typeAtEnd(page: Page, text: string) {
  await page.locator('.cm-content').click()
  await page.keyboard.press('Control+End')
  await page.keyboard.type(text)
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
