// The Board arrangement (docs/frontends/gui.md#workspace-experimental): the open
// items as windows on a table top, the graph drawn between them, the zoom levels
// down to groups, the remembered layout.
import { expect, test, type Page } from '@playwright/test'
import { offline, pageErrors } from './helpers'

const BACKEND = '/docs/docs/backend.md'
const FRONTEND = '/docs/docs/frontend.md'
const INVENTORY_DOC = '/docs/docs/subsystems/inventory.md'
const INVENTORY = '/model/entities/ent%3Ainventory-management'

const win = (page: Page, path: string) => page.locator(`.react-flow__node-window[data-id="w:${path}"]`)
const docRow = (page: Page, name: string) => page.locator('.wsn .ide-doc-name', { hasText: new RegExp(`^${name.replace('.', '\\.')}$`) })

test.beforeEach(async ({ page }) => {
  await offline(page)
  await page.addInitScript(() => localStorage.setItem('jazyk.workspace', 'board'))
})

async function openBoard(page: Page) {
  await page.goto('/#/')
  await page.locator('.home .demo', { hasText: 'The ERP' }).click()
  await page.waitForURL(/#\/docs/)
  await expect(page.locator('.board .react-flow')).toBeVisible()
}

// Opens a document as a kept item, the way a double click in the navigator does.
async function openDoc(page: Page, name: string, path: string) {
  await docRow(page, name).dblclick()
  await expect(win(page, path)).toBeAttached()
}

// The node's place on the board, from its transform.
async function position(page: Page, path: string): Promise<{ x: number; y: number }> {
  const t = (await win(page, path).evaluate((el) => (el as HTMLElement).style.transform)) ?? ''
  const m = /translate\((-?[\d.]+)px,\s*(-?[\d.]+)px\)/.exec(t)
  if (!m) throw new Error(`no position in ${t}`)
  return { x: Number(m[1]), y: Number(m[2]) }
}

async function zoomText(page: Page): Promise<string> {
  return (await page.locator('.board-zoom').textContent()) ?? ''
}

// Keys go to the board, not to a focused editor or button.
async function blur(page: Page) {
  await page.evaluate(() => (document.activeElement as HTMLElement | null)?.blur())
}

// Waits until the board stops moving (an animated zoom or pan has landed).
async function settle(page: Page) {
  const viewport = page.locator('.react-flow__viewport')
  let last = ''
  await expect
    .poll(async () => {
      const now = (await viewport.getAttribute('style')) ?? ''
      const same = now === last
      last = now
      return same
    }, { intervals: [150] })
    .toBe(true)
}

// Zooms to a window: a double click on its title bar.
async function zoomTo(page: Page, path: string) {
  await win(page, path).locator('.bw-title').dblclick()
  await expect(page.locator('.board-zoom')).toContainText('pages')
  await settle(page)
}

test('opening two documents puts two windows on the board, each its page', async ({ page }) => {
  const errors = pageErrors(page)
  await openBoard(page)
  await openDoc(page, 'backend.md', BACKEND)
  await openDoc(page, 'frontend.md', FRONTEND)
  await expect(win(page, BACKEND).locator('.bw-title')).toContainText('backend.md')
  await expect(win(page, FRONTEND).locator('.bw-title')).toContainText('frontend.md')
  // The second opened from the first: it went right of it.
  const a = await position(page, BACKEND)
  const b = await position(page, FRONTEND)
  expect(b.x).toBeGreaterThan(a.x)
  // The page in the focused window is live: the editor itself.
  await zoomTo(page, FRONTEND)
  await expect(win(page, FRONTEND).locator('.cm-content')).toBeVisible()
  // Closing a window closes its item.
  await win(page, FRONTEND).getByRole('button', { name: /^Close/ }).click()
  await expect(win(page, FRONTEND)).toHaveCount(0)
  expect(errors).toEqual([])
})

test('the graph layer draws the entities a document brings and the relationships between them', async ({ page }) => {
  await openBoard(page)
  await openDoc(page, 'backend.md', BACKEND)
  const chip = page.locator('.react-flow__node-chip .bc[data-entity="ent:inventory-management"]')
  await expect(chip).toContainText('Inventory Management')
  // A relationship edge from or to it, with its verb.
  const rel = page.locator('.react-flow__edge[data-id^="rel:"][data-id*="ent:inventory-management"]')
  await expect(rel.first()).toBeAttached()
  await expect(page.locator('.be-label-rel').first()).toBeAttached()
  // The document links to the entities it defines with a dashed edge.
  await expect(page.locator(`.react-flow__edge[data-id="def:${BACKEND}->ent:inventory-management"]`)).toBeAttached()
  // A click selects the chip for Connections; the toggle hides the layer.
  await chip.click()
  await expect(chip).toHaveClass(/sel/)
  await expect(page.locator('.wsc')).toContainText('Inventory Management')
  await page.locator('.board-tools').getByRole('button', { name: 'Graph' }).click()
  await expect(page.locator('.react-flow__node-chip')).toHaveCount(0)
  await page.locator('.board-tools').getByRole('button', { name: 'Graph' }).click()
  await expect(chip).toBeAttached()
})

test('zooming out below 15 percent shows the groups, and a double click on one zooms back in', async ({ page }) => {
  await openBoard(page)
  await openDoc(page, 'backend.md', BACKEND)
  await openDoc(page, 'inventory.md', INVENTORY_DOC)
  await blur(page)
  for (let i = 0; i < 5 && !(await zoomText(page)).includes('groups'); i++) {
    await page.keyboard.press('Escape')
    await page.waitForTimeout(450)
  }
  await expect(page.locator('.board-zoom')).toContainText('groups')
  const regions = page.locator('.react-flow__node-group .bg')
  await expect(regions.filter({ hasText: 'docs/subsystems/' })).toContainText('1 document')
  await expect(regions.filter({ hasText: 'Backend' })).toContainText('entities')
  await expect(page.locator('.react-flow__node-window')).toHaveCount(0)
  // Lifted edges run between the groups.
  await expect(page.locator('.react-flow__edge[data-id^="lift:"]').first()).toBeAttached()
  // The region may sit outside the view; the double click is dispatched to it.
  await regions.filter({ hasText: 'docs/subsystems/' }).dispatchEvent('dblclick')
  await expect(page.locator('.board-zoom')).not.toContainText('groups')
  await expect(win(page, INVENTORY_DOC)).toBeVisible()
})

test('a window stays where it was dragged across a reload', async ({ page }) => {
  await openBoard(page)
  await openDoc(page, 'backend.md', BACKEND)
  await zoomTo(page, BACKEND)
  const before = await position(page, BACKEND)
  const title = win(page, BACKEND).locator('.bw-name')
  const box = (await title.boundingBox())!
  const x = box.x + box.width / 2
  const y = box.y + box.height / 2
  await page.mouse.move(x, y)
  await page.mouse.down()
  await page.mouse.move(x + 60, y + 60, { steps: 6 })
  await page.mouse.move(x + 120, y + 120, { steps: 6 })
  await page.mouse.up()
  const moved = await position(page, BACKEND)
  expect(moved.x).not.toBe(before.x)
  expect(moved.y).not.toBe(before.y)
  await page.waitForTimeout(400)
  await page.reload()
  await expect(win(page, BACKEND)).toBeAttached()
  expect(await position(page, BACKEND)).toEqual(moved)
})

test('an entity opened from inside a document window opens beside it with an edge', async ({ page }) => {
  await openBoard(page)
  await openDoc(page, 'backend.md', BACKEND)
  await zoomTo(page, BACKEND)
  const doc = win(page, BACKEND)
  await expect(doc.locator('.cm-content')).toBeVisible()
  // The mention sits at the end of the document.
  await doc.locator('.cm-content .cm-line').last().click()
  await page.keyboard.press('Control+End')
  const mention = doc.locator('.cm-content .entity-mark', { hasText: 'Inventory Management' })
  await expect(mention).toBeVisible()
  await mention.click()
  const entity = win(page, INVENTORY)
  await expect(entity).toBeAttached()
  await expect(entity.locator('.bw-title')).toContainText('Inventory Management')
  const a = await position(page, BACKEND)
  const b = await position(page, INVENTORY)
  const width = await doc.evaluate((el) => (el as HTMLElement).offsetWidth)
  expect(b.x).toBeGreaterThan(a.x + width)
  expect(b.y).toBe(a.y)
  // The document defines the entity, so the dashed edge now ends at its window,
  // and the entity's chip gave way to it.
  await expect(page.locator(`.react-flow__edge[data-id="def:${BACKEND}->ent:inventory-management"]`)).toBeAttached()
  await expect(page.locator('.react-flow__node-chip .bc[data-entity="ent:inventory-management"]')).toHaveCount(0)
  // The mention selected the entity for Connections, and the address bar follows the new window.
  await expect(page.locator('.wsc')).toContainText('Inventory Management')
  await expect(page).toHaveURL(/#\/model\/entities\/ent%3Ainventory-management$/)
})

test('Escape steps out one level at a time and Z fits everything', async ({ page }) => {
  await openBoard(page)
  await openDoc(page, 'backend.md', BACKEND)
  await zoomTo(page, BACKEND)
  await blur(page)
  await page.keyboard.press('Escape')
  await expect(page.locator('.board-zoom')).toContainText('stills')
  await expect(win(page, BACKEND).locator('.bw-still')).toBeVisible()
  await page.keyboard.press('Escape')
  await expect(page.locator('.board-zoom')).toContainText('cards')
  await expect(win(page, BACKEND).locator('.bw-card-title')).toHaveText('backend.md')
  await page.keyboard.press('Escape')
  await expect(page.locator('.board-zoom')).toContainText('groups')
  await page.keyboard.press('z')
  await expect(page.locator('.board-zoom')).not.toContainText('groups')
})
