// The Board arrangement (docs/frontends/gui.md#the-board): the open items as windows
// on a table top, the graph drawn between them, focus and the links' portals, the
// zoom levels down to titles, the remembered layout.
import { expect, test, type Page } from '@playwright/test'
import { offline, pageErrors } from './helpers'

const BACKEND = '/docs/docs/backend.md'
const FRONTEND = '/docs/docs/frontend.md'
const INVENTORY_DOC = '/docs/docs/subsystems/inventory.md'
const BACKEND_ENTITY = '/model/entities/ent%3Abackend'

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
  // A relationship link from or to it.
  const rel = page.locator('.react-flow__edge[data-id^="rel:"][data-id*="ent:inventory-management"]')
  await expect(rel.first()).toBeAttached()
  // The document links to the entities it defines with a dashed link, to the ones it
  // mentions with a dotted one.
  await expect(page.locator(`.react-flow__edge[data-id="def:${BACKEND}->ent:inventory-management"]`)).toBeAttached()
  await expect(page.locator(`.react-flow__edge[data-id^="men:${BACKEND}->"]`).first()).toBeAttached()
  // A click selects the chip; the toggle hides the layer.
  await chip.click()
  await expect(chip).toHaveClass(/sel/)
  await page.locator('.board-tools').getByRole('button', { name: 'Graph' }).click()
  await expect(page.locator('.react-flow__node-chip')).toHaveCount(0)
  await page.locator('.board-tools').getByRole('button', { name: 'Graph' }).click()
  await expect(chip).toBeAttached()
})

test('a focused document fills most of the board, its entities show as portals in the margin, and the linkage expands', async ({ page }) => {
  await openBoard(page)
  await openDoc(page, 'backend.md', BACKEND)
  await zoomTo(page, BACKEND)
  // The window fills about 80 percent of the board.
  const board = (await page.locator('.board .react-flow').boundingBox())!
  const box = (await win(page, BACKEND).boundingBox())!
  expect(box.width / board.width).toBeGreaterThan(0.7)
  expect(box.width / board.width).toBeLessThan(0.9)
  // The chips sit out of view; their links carry portals at the board's edge.
  const portal = page.locator('.bl-portal[data-node="c:ent:inventory-management"]')
  await expect(portal).toBeVisible()
  const p = (await portal.boundingBox())!
  expect(p.x).toBeGreaterThan(box.x + box.width - 4)
  // The linkage on the link: the verb and the requirements behind it, collapsed.
  const label = page.locator(`.bl-label[data-edge="def:${BACKEND}->ent:inventory-management"]`)
  await expect(label.locator('.bl-head')).toHaveAttribute('title', /^defines: \d+ requirement/)
  await label.locator('.bl-head').click()
  await expect(label.locator('.bl-reqs li').first()).toBeVisible()
  // A click on the portal goes to the entity.
  await portal.click()
  await settle(page)
  await expect(page.locator('.react-flow__node-chip .bc[data-entity="ent:inventory-management"]')).toBeInViewport()
})

test('zooming out to titles keeps every window where it is and draws only its title', async ({ page }) => {
  await openBoard(page)
  await openDoc(page, 'backend.md', BACKEND)
  await openDoc(page, 'inventory.md', INVENTORY_DOC)
  const before = await position(page, INVENTORY_DOC)
  await blur(page)
  for (let i = 0; i < 5 && !(await zoomText(page)).includes('titles'); i++) {
    await page.keyboard.press('Escape')
    await page.waitForTimeout(450)
  }
  await expect(page.locator('.board-zoom')).toContainText('titles')
  await expect(win(page, INVENTORY_DOC).locator('.bw-titleonly')).toContainText('inventory.md')
  await expect(win(page, BACKEND).locator('.bw-titleonly')).toContainText('backend.md')
  expect(await position(page, INVENTORY_DOC)).toEqual(before)
  // A click on a title focuses its window again.
  await win(page, INVENTORY_DOC).locator('.bw-titleonly').click()
  await expect(page.locator('.board-zoom')).not.toContainText('titles')
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

test('a plain click on a mention places the cursor and highlights its link; Cmd-click opens the entity as a card beside, with a portal back', async ({ page }) => {
  await openBoard(page)
  await openDoc(page, 'backend.md', BACKEND)
  await zoomTo(page, BACKEND)
  const doc = win(page, BACKEND)
  await expect(doc.locator('.cm-content')).toBeVisible()
  // `backend system` in the first paragraph names the Backend (a mention outside a link).
  const mention = doc.locator('.cm-content .entity-mark', { hasText: 'backend system' }).first()
  await expect(mention).toBeVisible()
  await mention.click()
  // Only the cursor moved: nothing opened, and the link to the entity lights up.
  await expect(win(page, BACKEND_ENTITY)).toHaveCount(0)
  await expect(page.locator('.react-flow__node-chip .bc[data-entity="ent:backend"]')).toHaveClass(/hot/)
  await expect(page.locator('.bl-portal[data-node="c:ent:backend"]')).toHaveClass(/hot/)

  await mention.click({ modifiers: ['ControlOrMeta'] })
  const entity = win(page, BACKEND_ENTITY)
  await expect(entity).toBeAttached()
  await expect(entity.locator('.bw-entity .bwe-name')).toHaveText('Backend')
  const a = await position(page, BACKEND)
  const b = await position(page, BACKEND_ENTITY)
  const width = await doc.evaluate((el) => (el as HTMLElement).offsetWidth)
  expect(b.x).toBeGreaterThan(a.x + width)
  expect(b.y).toBe(a.y)
  // The document's link now ends at the entity's window, and its chip gave way to it.
  await expect(page.locator(`.react-flow__edge[data-id="men:${BACKEND}->ent:backend"]`)).toBeAttached()
  await expect(page.locator('.react-flow__node-chip .bc[data-entity="ent:backend"]')).toHaveCount(0)
  await expect(page).toHaveURL(/#\/model\/entities\/ent%3Abackend$/)
  // Focused on the card, the document it came from is a portal on the link back.
  await settle(page)
  const back = page.locator(`.bl-portal[data-node="w:${BACKEND}"]`)
  await expect(back).toBeVisible()
  await back.click()
  await expect(page).toHaveURL(/#\/docs\/docs\/backend\.md$/)
  await settle(page)
  await expect(doc.locator('.cm-content')).toBeInViewport()
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
  await expect(page.locator('.board-zoom')).toContainText('titles')
  await page.keyboard.press('z')
  await expect(page.locator('.board-zoom')).not.toContainText('titles')
})
