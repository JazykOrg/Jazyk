// The workspace (docs/frontends/gui.md#workspace): Tabs by default with no lenses and
// no inspector, the Board one button away over the same items and history, closing
// the focused item as a back, preview tabs, splits that come back on a reload, and a
// Cmd-click on a mention opening its entity as a tab.
import { expect, test, type Page } from '@playwright/test'
import { offline, openDemo, pageErrors, typeAtEnd } from './helpers'

test.beforeEach(async ({ page }) => {
  await offline(page)
})

// The workspace opens in Tabs; the settings page is the first tab, pinned so it
// stays when a document takes the focus (a transient page closes once unheld).
async function toTabs(page: Page) {
  await page.goto('/#/settings')
  await expect(page.locator('.wst-dock')).toBeVisible()
  await page.locator('.wst-dock .wst-tab[data-path="/settings"] .wst-close.pin').click()
  await expect(page.locator('.wst-dock .wst-tab[data-path="/settings"]')).toHaveClass(/pinned/)
}

const tabs = (page: Page) => page.locator('.wst-dock .dv-tab')
const tab = (page: Page, path: string) => page.locator(`.wst-dock .wst-tab[data-path="${path}"]`)
const docRow = (page: Page, name: string) => page.locator('.wsn .ide-doc-name', { hasText: new RegExp(`^${name.replace('.', '\\.')}$`) })

test('Tabs is the layout: no lenses, no inspector, one navigator', async ({ page }) => {
  const errors = pageErrors(page)
  await openDemo(page, 'The ERP')
  await toTabs(page)
  const top = page.locator('.ws-top')
  for (const lens of ['Docs', 'Model', 'Product']) await expect(top.getByRole('link', { name: lens, exact: true })).toHaveCount(0)
  await expect(top.getByRole('link', { name: /^Inbox/ })).toBeVisible()
  await expect(top.getByRole('radio')).toHaveCount(0)
  await expect(page.locator('.wb-inspector')).toHaveCount(0)
  // One navigator: Documents and Product, no Model head.
  const nav = page.locator('.wsn')
  await expect(nav.getByRole('button', { name: /Documents/ })).toBeVisible()
  await expect(nav.getByRole('button', { name: /Product/ })).toBeVisible()
  await expect(nav.getByRole('button', { name: /^.?\s*Model/ })).toHaveCount(0)
  expect(errors).toEqual([])
})

test('a tab collapses into the Board and a window expands back into Tabs, over the same items and history', async ({ page }) => {
  await openDemo(page, 'The ERP')
  await toTabs(page)
  await docRow(page, 'main.md').dblclick()
  await docRow(page, 'backend.md').dblclick()
  await expect(page).toHaveURL(/#\/docs\/docs\/backend\.md$/)
  await page.locator('.wst-item[data-item="/docs/docs/backend.md"] .ws-arrange-btn').click()
  const win = (path: string) => page.locator(`.react-flow__node-window[data-id="w:${path}"]`)
  await expect(win('/docs/docs/backend.md').locator('.bw')).toHaveClass(/focused/)
  await expect(win('/docs/docs/main.md')).toBeAttached()
  await expect(page.locator('.wst-dock')).toHaveCount(0)
  // Back walks the same history on the Board.
  await page.goBack()
  await expect(page).toHaveURL(/#\/docs\/docs\/main\.md$/)
  await expect(win('/docs/docs/main.md').locator('.bw')).toHaveClass(/focused/)
  // The window expands back into Tabs, with that tab in front, and forward still works.
  await win('/docs/docs/main.md').locator('.ws-arrange-btn').click()
  await expect(page.locator('.wst-dock')).toBeVisible()
  await expect(page.locator('.wst-dock .dv-active-tab .wst-tab')).toHaveAttribute('data-path', '/docs/docs/main.md')
  await page.goForward()
  await expect(page).toHaveURL(/#\/docs\/docs\/backend\.md$/)
  await expect(page.locator('.wst-dock .dv-active-tab .wst-tab')).toHaveAttribute('data-path', '/docs/docs/backend.md')
})

test('a file stays; an entity is held by the focus, and a pin keeps it', async ({ page }) => {
  await openDemo(page, 'The ERP')
  await toTabs(page)
  await docRow(page, 'backend.md').dblclick()
  const A = '/docs/docs/backend.md'
  const B = '/model/entities/ent%3Abackend'
  const C = '/model/entities/ent%3Ainventory-management'
  // A file has a close; an entity a pin instead.
  await expect(tab(page, A).locator('.wst-close')).not.toHaveClass(/pin/)
  await page.goto(`/#${B}`)
  await expect(tab(page, B).locator('.wst-close.pin')).toBeVisible()
  await page.goto(`/#${C}`)
  // C came from B, so B is held while C has the focus.
  await expect(tab(page, B)).toBeVisible()
  await expect(tab(page, C)).toBeVisible()
  // Back on the file, neither is held.
  await tab(page, A).click()
  await expect(tab(page, B)).toHaveCount(0)
  await expect(tab(page, C)).toHaveCount(0)

  // Again, and pin C this time: C stays, B is let go.
  await page.goto(`/#${B}`)
  await page.goto(`/#${C}`)
  await expect(tab(page, B)).toBeVisible()
  await tab(page, C).locator('.wst-close.pin').click()
  await expect(tab(page, C)).toHaveClass(/pinned/)
  await expect(tab(page, B)).toHaveCount(0)
  await tab(page, A).click()
  await expect(tab(page, C)).toBeVisible()
})

test('an entity’s states link to its state diagram', async ({ page }) => {
  await openDemo(page, 'The novel')
  await toTabs(page)
  await page.goto('/#/model/entities/ent%3Areport')
  await page.locator('.wst-item:visible').getByRole('link', { name: 'see the states' }).click()
  await expect(page).toHaveURL(/#\/model\/diagrams\/view%3Astate%2Freport$/)
})

test('closing the focused tab goes back to the one shown before it', async ({ page }) => {
  await openDemo(page, 'The ERP')
  await toTabs(page)
  await docRow(page, 'main.md').dblclick()
  await docRow(page, 'backend.md').dblclick()
  await docRow(page, 'frontend.md').dblclick()
  // main.md shown again: the history is main, backend, frontend, main.
  await tab(page, '/docs/docs/main.md').click()
  await expect(page).toHaveURL(/#\/docs\/docs\/main\.md$/)
  await tab(page, '/docs/docs/main.md').locator('.wst-close').click()
  await expect(page).toHaveURL(/#\/docs\/docs\/frontend\.md$/)
  await expect(tab(page, '/docs/docs/main.md')).toHaveCount(0)
  await tab(page, '/docs/docs/frontend.md').locator('.wst-close').click()
  await expect(page).toHaveURL(/#\/docs\/docs\/backend\.md$/)
  await expect(page.locator('.wst-dock .dv-active-tab .wst-tab')).toHaveAttribute('data-path', '/docs/docs/backend.md')
})

test('a single click opens a preview tab the next one replaces; a double click keeps it', async ({ page }) => {
  await openDemo(page, 'The ERP')
  await toTabs(page)
  await expect(tabs(page)).toHaveCount(1)
  await docRow(page, 'main.md').click()
  await expect(tabs(page)).toHaveCount(2)
  await expect(tab(page, '/docs/docs/main.md')).toHaveClass(/preview/)
  await expect(page).toHaveURL(/#\/docs\/docs\/main\.md$/)

  await docRow(page, 'backend.md').click()
  await expect(tabs(page)).toHaveCount(2)
  await expect(tab(page, '/docs/docs/main.md')).toHaveCount(0)
  await expect(tab(page, '/docs/docs/backend.md')).toHaveClass(/preview/)

  await docRow(page, 'backend.md').dblclick()
  await expect(tab(page, '/docs/docs/backend.md')).not.toHaveClass(/preview/)
  await docRow(page, 'frontend.md').dblclick()
  await expect(tabs(page)).toHaveCount(3)
  await expect(page).toHaveURL(/#\/docs\/docs\/frontend\.md$/)

  // A tab brought to the front is the address bar's path.
  await tab(page, '/docs/docs/backend.md').click()
  await expect(page).toHaveURL(/#\/docs\/docs\/backend\.md$/)
  await expect(page.locator('.wst-item[data-item="/docs/docs/backend.md"] h1, .wst-item[data-item="/docs/docs/backend.md"] .cm-content').first()).toBeVisible()
})

test('split right shows two documents side by side, and a reload restores the tabs and the split', async ({ page }) => {
  await openDemo(page, 'The ERP')
  await toTabs(page)
  await docRow(page, 'main.md').dblclick()
  await docRow(page, 'backend.md').dblclick()
  await expect(tabs(page)).toHaveCount(3)

  await tab(page, '/docs/docs/backend.md').click({ button: 'right' })
  await page.getByText('Split right', { exact: true }).click()
  await expect(page.locator('.wst-dock .dv-groupview')).toHaveCount(2)
  const main = page.locator('.wst-item[data-item="/docs/docs/main.md"]')
  const backend = page.locator('.wst-item[data-item="/docs/docs/backend.md"]')
  await tab(page, '/docs/docs/main.md').click()
  await expect(main).toBeVisible()
  await expect(backend).toBeVisible()
  const a = (await main.boundingBox())!
  const b = (await backend.boundingBox())!
  expect(b.x).toBeGreaterThanOrEqual(a.x + a.width - 2)
  expect(Math.abs(a.y - b.y)).toBeLessThan(4)

  await page.reload()
  await expect(page.locator('.wst-dock .dv-groupview')).toHaveCount(2)
  await expect(tabs(page)).toHaveCount(3)
  await expect(tab(page, '/docs/docs/main.md')).toBeVisible()
  await expect(tab(page, '/docs/docs/backend.md')).toBeVisible()
  await expect(page).toHaveURL(/#\/docs\/docs\/main\.md$/)
})

test('a plain click on a mention places the cursor, and Cmd-click opens its entity as a tab', async ({ page }) => {
  await openDemo(page, 'The ERP')
  await toTabs(page)
  await page.goto('/#/docs/docs/backend.md')
  const item = page.locator('.wst-item[data-item="/docs/docs/backend.md"]')
  // `backend system` in the first paragraph names the Backend (a mention outside a link).
  const mention = item.locator('.cm-content .entity-mark', { hasText: 'backend system' }).first()
  await expect(mention).toBeVisible()
  const before = await tabs(page).count()
  await mention.click()
  await expect(page).toHaveURL(/#\/docs\/docs\/backend\.md$/)
  await expect(tabs(page)).toHaveCount(before)

  await mention.click({ modifiers: ['ControlOrMeta'] })
  await expect(tab(page, '/model/entities/ent%3Abackend')).toBeVisible()
  await expect(tabs(page)).toHaveCount(before + 1)
  await expect(page).toHaveURL(/#\/model\/entities\/ent%3Abackend$/)
  await expect(page.locator('.wst-item[data-item="/model/entities/ent%3Abackend"] h1')).toContainText('Backend')
  // There is no Connections panel.
  await expect(page.locator('.wsc')).toHaveCount(0)
})

test('an edit keeps a preview tab', async ({ page }) => {
  await openDemo(page, 'The ERP')
  await toTabs(page)
  await docRow(page, 'main.md').click()
  await expect(tab(page, '/docs/docs/main.md')).toHaveClass(/preview/)
  await typeAtEnd(page, ' kept')
  await expect(tab(page, '/docs/docs/main.md')).not.toHaveClass(/preview/)
  await docRow(page, 'backend.md').click()
  await expect(tabs(page)).toHaveCount(3)
})

test('dragging a tab to an edge splits without closing anything, and a closed tab stays closed after a reload', async ({ page }) => {
  await openDemo(page, 'The ERP')
  await toTabs(page)
  await docRow(page, 'main.md').dblclick()
  await docRow(page, 'backend.md').dblclick()
  await expect(tabs(page)).toHaveCount(3)
  const group = page.locator('.wst-dock .dv-groupview').first()
  const box = (await group.boundingBox())!
  await page.locator('.wst-dock .dv-tab', { has: page.locator('[data-path="/docs/docs/main.md"]') }).dragTo(group, {
    targetPosition: { x: box.width - 16, y: box.height / 2 },
    force: true,
  })
  await expect(page.locator('.wst-dock .dv-groupview')).toHaveCount(2)
  await expect(tabs(page)).toHaveCount(3)

  await tab(page, '/docs/docs/backend.md').locator('.wst-close').click()
  await expect(tabs(page)).toHaveCount(2)
  await expect(page).not.toHaveURL(/backend\.md/)
  await page.reload()
  await expect(tabs(page)).toHaveCount(2)
  await expect(tab(page, '/docs/docs/backend.md')).toHaveCount(0)
})

test('on a narrow screen one item shows and the switcher lays the open items out as a grid', async ({ page }) => {
  await openDemo(page, 'The ERP')
  await toTabs(page)
  await page.goto('/#/docs/docs/main.md')
  await page.goto('/#/docs/docs/backend.md')
  await page.setViewportSize({ width: 390, height: 800 })
  await expect(page.locator('.wst-dock')).toHaveCount(0)
  const switcher = page.getByRole('button', { name: 'open items' })
  await expect(switcher).toHaveText('3')
  await switcher.click()
  const grid = page.getByRole('dialog', { name: 'Open items' })
  await expect(grid.locator('.wst-card')).toHaveCount(3)
  await grid.locator('.wst-card', { hasText: '/docs/docs/main.md' }).locator('.wst-card-main').click()
  await expect(grid).toHaveCount(0)
  await expect(page).toHaveURL(/#\/docs\/docs\/main\.md$/)

})
