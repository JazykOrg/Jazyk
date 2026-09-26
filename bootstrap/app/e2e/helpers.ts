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

// Collects page errors; a test asserts it stays empty.
export function pageErrors(page: Page): string[] {
  const errs: string[] = []
  page.on('pageerror', (e) => errs.push(String(e)))
  return errs
}
