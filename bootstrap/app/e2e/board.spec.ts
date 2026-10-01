// The Board arrangement (docs/frontends/gui.md#the-board): the open items as windows
// on a table top with the links between them, the focused window's fan, focus and the
// links' portals, the zoom levels down to titles, the remembered layout.
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

// Zooms to a window: a click on its bar.
async function zoomTo(page: Page, path: string) {
  await win(page, path).locator('.bw-title .bw-name').click()
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

test('a document window has one bar, and a click on the empty table top fits everything', async ({ page }) => {
  await openBoard(page)
  await openDoc(page, 'backend.md', BACKEND)
  await zoomTo(page, BACKEND)
  const bar = win(page, BACKEND).locator('.bw-title')
  await expect(bar).toContainText('docs/backend.md')
  await expect(bar.locator('.ide-mode').getByRole('button', { name: 'Write' })).toHaveAttribute('aria-pressed', 'true')
  await expect(bar).toContainText('saved')
  await expect(win(page, BACKEND).locator('.ide-topbar')).toHaveCount(0)
  // The switch turns every document.
  await bar.locator('.ide-mode').getByRole('button', { name: 'Read' }).click()
  await expect(win(page, BACKEND).locator('.md-read')).toBeVisible()
  await bar.locator('.ide-mode').getByRole('button', { name: 'Write' }).click()
  await expect(win(page, BACKEND).locator('.cm-content')).toBeVisible()
  // Nothing shows below the document until the caret rests on something.
  await expect(page.locator('.board-refs')).toHaveCount(0)
  // A click on the empty table top fits everything: one window fits at 100 percent.
  await blur(page)
  await page.keyboard.press('Escape')
  await expect(page.locator('.board-zoom')).toContainText('stills')
  await settle(page)
  await page.keyboard.press('Escape')
  await expect(page.locator('.board-zoom')).toContainText('cards')
  await settle(page)
  const board = (await page.locator('.board .react-flow').boundingBox())!
  await page.mouse.click(board.x + 30, board.y + board.height - 30)
  await expect(page.locator('.board-zoom')).toContainText('pages')
})

test('the caret on a mention brings its entity below the document as a reference, its title opening and focusing it', async ({ page }) => {
  await openBoard(page)
  await openDoc(page, 'backend.md', BACKEND)
  await zoomTo(page, BACKEND)
  const doc = win(page, BACKEND)
  // `backend system` in the first paragraph names the Backend, inside a requirement's quote.
  await doc.locator('.cm-line', { hasText: 'REST API' }).locator('.entity-mark').first().click()
  const card = page.locator('.board-refs .br-card[data-entity="ent:backend"]')
  await expect(card).toBeVisible()
  await expect(card.locator('.lc-head')).toHaveClass(/hot/)
  await expect(card.locator('.lc-reqs li').first()).toBeVisible()
  // The caret is in a requirement's quote: it is first on the card, highlighted, and lightly in the text.
  await expect(card.locator('.lc-reqs li').first()).toHaveClass(/hot/)
  await expect(doc.locator('.cm-content .jz-req-caret').first()).toBeVisible()
  // Two lines folded, the rest behind the last line.
  expect(await card.locator('.lc-reqs li').count()).toBeLessThanOrEqual(2)
  await card.locator('.br-title').click()
  await expect(win(page, BACKEND_ENTITY)).toBeAttached()
  await expect(page).toHaveURL(/#\/model\/entities\/ent%3Abackend$/)
})

test('a link with one end out of view is one portal carrying its requirements; a line reveals its quote, its button opens it', async ({ page }) => {
  await openBoard(page)
  await openDoc(page, 'backend.md', BACKEND)
  await zoomTo(page, BACKEND)
  // The window fills about 80 percent of the board.
  const board = (await page.locator('.board .react-flow').boundingBox())!
  const box = (await win(page, BACKEND).boundingBox())!
  expect(box.width / board.width).toBeGreaterThan(0.7)
  expect(box.width / board.width).toBeLessThan(0.9)
  await win(page, BACKEND).locator('.cm-line', { hasText: 'REST API' }).locator('.entity-mark').first().click({ modifiers: ['ControlOrMeta'] })
  await expect(win(page, BACKEND_ENTITY)).toBeAttached()
  await settle(page)
  // The document sits out of view: its portal carries the link's requirements.
  const portal = page.locator(`.bl-portal.lc[data-node="w:${BACKEND}"]`)
  await expect(portal).toBeVisible()
  await expect(portal.locator('.lc-reqs li')).toHaveCount(2)
  await expect(page.locator(`.bl-label[data-edge="men:${BACKEND}->ent:backend"]`)).toHaveCount(0)
  // The last line expands it to all of them.
  await portal.locator('.lc-more').click()
  expect(await portal.locator('.lc-reqs li').count()).toBeGreaterThan(2)
  await portal.locator('.lc-more').click()
  await expect(portal.locator('.lc-reqs li')).toHaveCount(2)
  // A line reveals its quote in the document: the caret lands in it, lightly highlighted.
  await portal.locator('.lc-req').first().click()
  await expect(page).toHaveURL(/#\/docs\/docs\/backend\.md$/)
  await settle(page)
  await expect(win(page, BACKEND).locator('.cm-content .jz-req-caret').first()).toBeVisible()
  // The button at a line's right opens the requirement.
  await page.locator('.board-links .lc-open, .board-refs .lc-open').first().click()
  await expect(page).toHaveURL(/#\/model\/requirements\//)
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

test('a plain click on a mention places the cursor and shows its entity below; Cmd-click opens the entity as a card beside, with a portal back', async ({ page }) => {
  await openBoard(page)
  await openDoc(page, 'backend.md', BACKEND)
  await zoomTo(page, BACKEND)
  const doc = win(page, BACKEND)
  await expect(doc.locator('.cm-content')).toBeVisible()
  // `backend system` in the first paragraph names the Backend (a mention outside a link).
  const mention = doc.locator('.cm-content .entity-mark', { hasText: 'backend system' }).first()
  await expect(mention).toBeVisible()
  await mention.click()
  // Only the cursor moved: nothing opened, and the entity shows below as a reference.
  await expect(win(page, BACKEND_ENTITY)).toHaveCount(0)
  await expect(page.locator('.board-refs .br-card[data-entity="ent:backend"] .lc-head')).toHaveClass(/hot/)

  await mention.click({ modifiers: ['ControlOrMeta'] })
  const entity = win(page, BACKEND_ENTITY)
  await expect(entity).toBeAttached()
  await expect(entity.locator('.bw-entity .bwe-name')).toHaveText('Backend')
  const a = await position(page, BACKEND)
  const b = await position(page, BACKEND_ENTITY)
  const width = await doc.evaluate((el) => (el as HTMLElement).offsetWidth)
  expect(b.x).toBeGreaterThan(a.x + width)
  expect(b.y).toBe(a.y)
  // The document links to the entity's window now.
  await expect(page.locator(`.react-flow__edge[data-id="men:${BACKEND}->ent:backend"]`)).toBeAttached()
  await expect(page).toHaveURL(/#\/model\/entities\/ent%3Abackend$/)
  // Focused on the card, the document it came from is a portal on the link back.
  await settle(page)
  const back = page.locator(`.bl-portal[data-node="w:${BACKEND}"]`)
  await expect(back).toBeVisible()
  await back.locator('.bl-go').click()
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
