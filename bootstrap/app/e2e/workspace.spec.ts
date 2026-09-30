// The workspace behind the switch (docs/frontends/gui.md#workspace-experimental): Tabs
// replace the lenses and the inspector, the navigator opens preview tabs, tabs split
// and come back on a reload, and Connections shows what a mention names.
import { expect, test, type Page } from '@playwright/test'
import { offline, openDemo, pageErrors, typeAtEnd } from './helpers'

test.beforeEach(async ({ page }) => {
  await offline(page)
})

async function toTabs(page: Page) {
  await page.goto('/#/settings')
  await page.locator('.settings-workspace select').selectOption('tabs')
  await expect(page.locator('.wst-dock')).toBeVisible()
}

const tabs = (page: Page) => page.locator('.wst-dock .dv-tab')
const tab = (page: Page, path: string) => page.locator(`.wst-dock .wst-tab[data-path="${path}"]`)
const docRow = (page: Page, name: string) => page.locator('.wsn .ide-doc-name', { hasText: new RegExp(`^${name.replace('.', '\\.')}$`) })

test('switching to Tabs shows no lenses and no inspector, with the switch in the top bar', async ({ page }) => {
  const errors = pageErrors(page)
  await openDemo(page, 'The ERP')
  await toTabs(page)
  const top = page.locator('.ws-top')
  for (const lens of ['Docs', 'Model', 'Product']) await expect(top.getByRole('link', { name: lens, exact: true })).toHaveCount(0)
  await expect(top.getByRole('link', { name: /^Inbox/ })).toBeVisible()
  await expect(top.getByRole('radio', { name: 'Tabs' })).toHaveAttribute('aria-checked', 'true')
  await expect(page.locator('.wb-inspector')).toHaveCount(0)
  // One navigator: Documents and Product, no Model head.
  const nav = page.locator('.wsn')
  await expect(nav.getByRole('button', { name: /Documents/ })).toBeVisible()
  await expect(nav.getByRole('button', { name: /Product/ })).toBeVisible()
  await expect(nav.getByRole('button', { name: /^.?\s*Model/ })).toHaveCount(0)
  await expect(page.locator('.wsc')).toBeVisible()
  // Back to the lenses from the top bar.
  await top.getByRole('radio', { name: 'Lenses' }).click()
  await expect(page.locator('.ws-top').getByRole('link', { name: 'Model', exact: true })).toBeVisible()
  await expect(page.locator('.wst-dock')).toHaveCount(0)
  expect(errors).toEqual([])
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

test('a mention shows the entity in Connections, a neighbour the links between them, and Open opens its page as a tab', async ({ page }) => {
  await openDemo(page, 'The ERP')
  await toTabs(page)
  await page.goto('/#/docs/docs/backend.md')
  const item = page.locator('.wst-item[data-item="/docs/docs/backend.md"]')
  await item.locator('.cm-content .cm-line').last().click()
  await page.keyboard.press('Control+End')
  const mention = item.locator('.cm-content .entity-mark', { hasText: 'Inventory Management' })
  await expect(mention).toBeVisible()
  await mention.click()
  await expect(page).toHaveURL(/node=ent%3Ainventory-management/)
  const panel = page.locator('.wsc')
  await expect(panel.locator('.wsc-title')).toHaveText('Inventory Management')
  await expect(panel.locator('.connections svg')).toBeVisible()
  // Selecting does not open anything.
  const before = await tabs(page).count()
  await panel.locator('.cn-n').first().click()
  await expect(panel.locator('.cn-why')).toBeVisible()

  await panel.getByRole('button', { name: 'Open', exact: true }).click()
  await expect(tab(page, '/model/entities/ent%3Ainventory-management')).toBeVisible()
  await expect(tabs(page)).toHaveCount(before + 1)
  await expect(page).toHaveURL(/#\/model\/entities\/ent%3Ainventory-management$/)
  await expect(page.locator('.wst-item[data-item="/model/entities/ent%3Ainventory-management"] h1')).toContainText('Inventory Management')
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

  // Connections is a bottom sheet here, for a selection only.
  await expect(page.getByRole('dialog', { name: 'Connections' })).toHaveCount(0)
  await page.goto('/#/docs/docs/main.md?node=ent%3Ainventory-management')
  const sheet = page.getByRole('dialog', { name: 'Connections' })
  await expect(sheet.locator('.wsc-title')).toHaveText('Inventory Management')
  await sheet.getByRole('button', { name: 'close Connections' }).click()
  await expect(sheet).toHaveCount(0)
})
