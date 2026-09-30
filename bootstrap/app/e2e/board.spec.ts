// The Board arrangement (docs/frontends/gui.md#workspace-experimental): the open
// items as windows on a table top, the graph drawn between them, the zoom levels
// down to groups, the remembered layout. Driven through the lab page `/lab/board`.
import { expect, test, type Page } from '@playwright/test'
import { offline, openDemo, pageErrors } from './helpers'

const BACKEND = '/docs/docs/backend.md'
const MAIN = '/docs/docs/main.md'
const INVENTORY = '/model/entities/ent%3Ainventory-management'

const win = (page: Page, path: string) => page.locator(`.react-flow__node-window[data-id="w:${path}"]`)

async function openBoard(page: Page) {
  await offline(page)
  await openDemo(page, 'The ERP')
  await page.goto('/#/lab/board')
  await expect(page.locator('.board .react-flow')).toBeVisible()
}

async function openDoc(page: Page, doc: string) {
  await page.locator('.board-lab-docs button', { hasText: new RegExp(`^${doc}$`) }).click()
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

test('opening two documents puts two windows on the board, each its page', async ({ page }) => {
  const errors = pageErrors(page)
  await openBoard(page)
  await openDoc(page, 'docs/backend.md')
  await openDoc(page, 'docs/main.md')
  await expect(page.locator('.react-flow__node-window')).toHaveCount(2)
  await expect(win(page, BACKEND).locator('.bw-title')).toContainText('backend.md')
  await expect(win(page, MAIN).locator('.bw-title')).toContainText('main.md')
  // The second window went right of the first.
  const a = await position(page, BACKEND)
  const b = await position(page, MAIN)
  expect(b.x).toBeGreaterThan(a.x)
  // The page in the focused window is live: the editor itself.
  await expect(win(page, MAIN).locator('.cm-content')).toBeVisible()
  expect(errors).toEqual([])
})

test('the graph layer draws the entities a document brings and the relationships between them', async ({ page }) => {
  await openBoard(page)
  await openDoc(page, 'docs/backend.md')
  const chip = page.locator('.react-flow__node-chip .bc[data-entity="ent:inventory-management"]')
  await expect(chip).toContainText('Inventory Management')
  // A relationship edge from or to it, with its verb.
  const rel = page.locator('.react-flow__edge[data-id^="rel:"][data-id*="ent:inventory-management"]')
  await expect(rel.first()).toBeAttached()
  await expect(page.locator('.be-label-rel').first()).toBeVisible()
  // The document links to the entities it defines with a dashed edge.
  await expect(page.locator(`.react-flow__edge[data-id="def:${BACKEND}->ent:inventory-management"]`)).toBeAttached()
  // A click selects the chip; the toggle hides the layer.
  await chip.click()
  await expect(chip).toHaveClass(/sel/)
  await page.locator('.board-tools').getByRole('button', { name: 'Graph' }).click()
  await expect(page.locator('.react-flow__node-chip')).toHaveCount(0)
  await page.locator('.board-tools').getByRole('button', { name: 'Graph' }).click()
  await expect(chip).toBeVisible()
})

test('zooming out below 15 percent shows the groups, and a double click on one zooms back in', async ({ page }) => {
  await openBoard(page)
  await openDoc(page, 'docs/backend.md')
  await openDoc(page, 'docs/subsystems/inventory.md')
  await expect(page.locator('.react-flow__node-window')).toHaveCount(2)
  await page.locator('.board').click({ position: { x: 600, y: 900 } })
  for (let i = 0; i < 4 && !(await zoomText(page)).includes('groups'); i++) {
    await page.keyboard.press('Escape')
    await page.waitForTimeout(450)
  }
  await expect(page.locator('.board-zoom')).toContainText('groups')
  const regions = page.locator('.react-flow__node-group .bg')
  await expect(regions.filter({ hasText: 'docs/' }).first()).toBeVisible()
  await expect(regions.filter({ hasText: 'Backend' })).toContainText('entities')
  await expect(page.locator('.react-flow__node-window')).toHaveCount(0)
  // Lifted edges run between the groups.
  await expect(page.locator('.react-flow__edge[data-id^="lift:"]').first()).toBeAttached()
  await regions.filter({ hasText: 'docs/subsystems/' }).dblclick()
  await expect(page.locator('.board-zoom')).not.toContainText('groups')
  await expect(win(page, '/docs/docs/subsystems/inventory.md')).toBeVisible()
})

test('a window stays where it was dragged across a reload', async ({ page }) => {
  await openBoard(page)
  await openDoc(page, 'docs/main.md')
  await expect(win(page, MAIN)).toBeVisible()
  const before = await position(page, MAIN)
  const title = win(page, MAIN).locator('.bw-name')
  const box = (await title.boundingBox())!
  await page.mouse.move(box.x + 20, box.y + box.height / 2)
  await page.mouse.down()
  await page.mouse.move(box.x + 80, box.y + 60, { steps: 6 })
  await page.mouse.move(box.x + 140, box.y + 120, { steps: 6 })
  await page.mouse.up()
  const moved = await position(page, MAIN)
  expect(moved.x).not.toBe(before.x)
  expect(moved.y).not.toBe(before.y)
  await page.waitForTimeout(400)
  await page.reload()
  await expect(win(page, MAIN)).toBeVisible()
  expect(await position(page, MAIN)).toEqual(moved)
})

test('an entity opened from inside a document window opens beside it with an edge', async ({ page }) => {
  await openBoard(page)
  await openDoc(page, 'docs/backend.md')
  const doc = win(page, BACKEND)
  await expect(doc.locator('.cm-content')).toBeVisible()
  // The mention sits at the end of the document.
  await doc.locator('.cm-content .cm-line').last().click()
  await page.keyboard.press('Control+End')
  const mention = doc.locator('.cm-content .entity-mark', { hasText: 'Inventory Management' })
  await expect(mention).toBeVisible()
  await mention.click()
  // The address bar stays where it was: the window navigated, not the page.
  await expect(page).toHaveURL(/#\/lab\/board$/)
  const entity = win(page, INVENTORY)
  await expect(entity).toBeVisible()
  await expect(entity.locator('.bw-title')).toContainText('Inventory Management')
  const a = await position(page, BACKEND)
  const b = await position(page, INVENTORY)
  const width = await doc.evaluate((el) => (el as HTMLElement).offsetWidth)
  expect(b.x).toBeGreaterThan(a.x + width)
  expect(b.y).toBe(a.y)
  // The document defines the entity, so the dashed edge now ends at its window.
  await expect(page.locator(`.react-flow__edge[data-id="def:${BACKEND}->ent:inventory-management"]`)).toBeAttached()
  // The entity's chip gave way to its window.
  await expect(page.locator('.react-flow__node-chip .bc[data-entity="ent:inventory-management"]')).toHaveCount(0)
})

test('Z fits everything and Escape steps out one level at a time', async ({ page }) => {
  await openBoard(page)
  await openDoc(page, 'docs/main.md')
  await expect(page.locator('.board-zoom')).toContainText('pages')
  await page.locator('.board').click({ position: { x: 600, y: 900 } })
  await page.keyboard.press('Escape')
  await expect(page.locator('.board-zoom')).toContainText('stills')
  await expect(win(page, MAIN).locator('.bw-still')).toBeVisible()
  await page.keyboard.press('Escape')
  await expect(page.locator('.board-zoom')).toContainText('cards')
  await expect(win(page, MAIN).locator('.bw-card-title')).toHaveText('main.md')
  await page.keyboard.press('z')
  await expect(page.locator('.board-zoom')).toContainText('pages')
})
