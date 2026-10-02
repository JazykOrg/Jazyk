// The Board arrangement (docs/frontends/gui.md#the-board): the open items as windows
// on a table top with the links between them, the focused window's fan, focus and the
// links' portals, the zoom levels down to titles, the remembered layout.
import { expect, test, type Page } from '@playwright/test'
import { offline, openFolders, pageErrors } from './helpers'

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
  await openFolders(page, ['docs', 'docs/subsystems'])
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

// The zoom level the board draws at (live, still, card, title).
async function zoomText(page: Page): Promise<string> {
  return (await page.locator('.board').getAttribute('data-level')) ?? ''
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
  await expect(page.locator('.board')).toHaveAttribute('data-level', 'live')
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
  await win(page, FRONTEND).getByRole('button', { name: /^close /i }).click()
  await expect(win(page, FRONTEND)).toHaveCount(0)
  expect(errors).toEqual([])
})

test('a document window has two bars, and a click on the empty table top fits everything', async ({ page }) => {
  await openBoard(page)
  await openDoc(page, 'backend.md', BACKEND)
  await zoomTo(page, BACKEND)
  // The title bar: the path and the close; the editor's bar under it: the switch, the save state, the expand.
  const title = win(page, BACKEND).locator('.bw-title')
  await expect(title).toContainText('docs/backend.md')
  await expect(title.getByRole('button', { name: /^close /i })).toBeVisible()
  const bar = win(page, BACKEND).locator('.ide-topbar')
  await expect(bar.locator('.ide-mode').getByRole('button', { name: 'Write' })).toHaveAttribute('aria-pressed', 'true')
  await expect(bar).toContainText('saved')
  await expect(bar.getByRole('button', { name: 'Expand into Tabs' })).toBeVisible()
  // The switch turns every document.
  await bar.locator('.ide-mode').getByRole('button', { name: 'Read' }).click()
  await expect(win(page, BACKEND).locator('.md-read')).toBeVisible()
  await win(page, BACKEND).locator('.ide-topbar .ide-mode').getByRole('button', { name: 'Write' }).click()
  await expect(win(page, BACKEND).locator('.cm-content')).toBeVisible()
  // Nothing shows below the document until the caret rests on something.
  await expect(page.locator('.board-refs')).toHaveCount(0)
// A click on the empty table top fits everything, no closer than 60 percent: from
  // the cards level it steps in to stills.
  await blur(page)
  await page.keyboard.press('Escape')
  await expect(page.locator('.board')).toHaveAttribute('data-level', 'still')
  await settle(page)
  await page.keyboard.press('Escape')
  await expect(page.locator('.board')).toHaveAttribute('data-level', 'card')
  await settle(page)
  const board = (await page.locator('.board .react-flow').boundingBox())!
  await page.mouse.click(board.x + 30, board.y + board.height - 30)
  await expect(page.locator('.board')).toHaveAttribute('data-level', 'still')
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

test('closing the focused window goes back to the one shown before it, and a press in a half-seen window focuses it fully', async ({ page }) => {
  await openBoard(page)
  await openDoc(page, 'backend.md', BACKEND)
  await openDoc(page, 'frontend.md', FRONTEND)
  await zoomTo(page, FRONTEND)
  await expect(page).toHaveURL(/#\/docs\/docs\/frontend\.md$/)
  const board = (await page.locator('.board .react-flow').boundingBox())!
  await docRow(page, 'backend.md').dblclick()
  await expect(page).toHaveURL(/#\/docs\/docs\/backend\.md$/)
  await settle(page)
  // frontend.md shown again: the history is backend, frontend, backend, frontend.
  await docRow(page, 'frontend.md').dblclick()
  await settle(page)
  // backend.md sits left of frontend.md: pan until its right part shows at the left edge.
  await page.mouse.move(board.x + board.width / 2, board.y + 20)
  await page.mouse.wheel(-650, 0)
  await settle(page)
  const before = (await win(page, BACKEND).boundingBox())!
  expect(before.x).toBeLessThan(board.x)
  expect(before.x + before.width).toBeGreaterThan(board.x + 60)
  await page.mouse.click(before.x + before.width - 30, before.y + before.height / 2)
  await settle(page)
  const after = (await win(page, BACKEND).boundingBox())!
  expect(after.x).toBeGreaterThan(board.x)
  expect(after.x + after.width).toBeLessThan(board.x + board.width)
  // Closing the focused window is a back: frontend.md was shown before.
  await expect(page).toHaveURL(/#\/docs\/docs\/backend\.md$/)
  await win(page, BACKEND).getByRole('button', { name: /^close /i }).click()
  await expect(page).toHaveURL(/#\/docs\/docs\/frontend\.md$/)
  await expect(win(page, FRONTEND).locator('.bw')).toHaveClass(/focused/)
})

test('the links bar groups what a window links to; hovering shows them as cards and a card opens and focuses its item', async ({ page }) => {
  await openBoard(page)
  await openDoc(page, 'backend.md', BACKEND)
  await zoomTo(page, BACKEND)
  const bar = win(page, BACKEND).locator('.bw-links')
  // A document's other documents come first.
  await expect(bar.locator('.bw-link-group').first()).toHaveClass(/k-doc/)
  await bar.locator('.bw-link-group.k-entity').hover()
  const cards = page.locator('.board-cards .bl-mini[data-path^="/model/entities/"]')
  await expect(cards.first()).toBeVisible()
  const target = await cards.first().getAttribute('data-path')
  await cards.first().click()
  await expect(page).toHaveURL(new RegExp(`#${target!.replace(/[.%]/g, (c) => `\\${c}`)}$`))
  await expect(win(page, target!)).toBeAttached()
  await expect(page.locator('.board-cards')).toHaveCount(0)
})

test('a press and a drag on a zoomed-out window moves it without focusing in', async ({ page }) => {
  await openBoard(page)
  await openDoc(page, 'backend.md', BACKEND)
  await zoomTo(page, BACKEND)
  await blur(page)
  await page.keyboard.press('Escape')
  await expect(page.locator('.board')).toHaveAttribute('data-level', 'still')
  await settle(page)
  await page.keyboard.press('Escape')
  await expect(page.locator('.board')).toHaveAttribute('data-level', 'card')
  await settle(page)
  const before = await position(page, BACKEND)
  const box = (await win(page, BACKEND).boundingBox())!
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2)
  await page.mouse.down()
  await page.mouse.move(box.x + box.width / 2 + 80, box.y + box.height / 2 + 40, { steps: 8 })
  await page.mouse.up()
  await settle(page)
  // Still zoomed out, the window moved.
  await expect(page.locator('.board')).toHaveAttribute('data-level', 'card')
  expect(await position(page, BACKEND)).not.toEqual(before)
})

test('every window has a pin and a close; an unpinned window is held by the focus, a pin keeps it', async ({ page }) => {
  await openBoard(page)
  await openDoc(page, 'backend.md', BACKEND)
  const B = '/model/entities/ent%3Abackend'
  const C = '/model/entities/ent%3Ainventory-management'
  // A file opens pinned; an entity unpinned, floating.
  await expect(win(page, BACKEND).locator('.bw-title .pin')).toHaveClass(/on/)
  await expect(win(page, BACKEND).getByRole('button', { name: /^close /i })).toBeVisible()
  await page.goto(`/#${B}`)
  await expect(win(page, B).locator('.bw')).toHaveClass(/floating/)
  await page.goto(`/#${C}`)
  // C came from B, so B is held while C has the focus.
  await expect(win(page, B)).toBeAttached()
  // Back on the file, neither is held.
  await page.goto('/#/docs/docs/backend.md')
  await expect(win(page, B)).toHaveCount(0)
  await expect(win(page, C)).toHaveCount(0)
  // Again, and pin C this time: C stays, B is let go.
  await page.goto(`/#${B}`)
  await page.goto(`/#${C}`)
  await expect(win(page, B)).toBeAttached()
  await win(page, C).locator('.bw-title .pin').click()
  await expect(win(page, C).locator('.bw')).toHaveClass(/pinned/)
  await expect(win(page, B)).toHaveCount(0)
  await page.goto('/#/docs/docs/backend.md')
  await expect(win(page, C)).toBeAttached()
  // An unpinned file is held like anything else.
  await win(page, BACKEND).locator('.bw-title .pin').click()
  await expect(win(page, BACKEND).locator('.bw')).toHaveClass(/floating/)
})

test('a document comes back where it was left', async ({ page }) => {
  await openBoard(page)
  await openDoc(page, 'backend.md', BACKEND)
  await openDoc(page, 'frontend.md', FRONTEND)
  await docRow(page, 'backend.md').dblclick()
  await settle(page)
  const scroller = win(page, BACKEND).locator('.cm-scroller')
  await win(page, BACKEND).locator('.cm-line').last().click()
  await page.keyboard.press('Control+End')
  await scroller.evaluate((el) => el.scrollTo(0, el.scrollHeight))
  const top = await scroller.evaluate((el) => el.scrollTop)
  expect(top).toBeGreaterThan(0)
  // Away to frontend.md and back: the flight holds the live level, and an editor
  // that mounts again opens where it was.
  await docRow(page, 'frontend.md').dblclick()
  await settle(page)
  await docRow(page, 'backend.md').dblclick()
  await settle(page)
  await expect.poll(() => win(page, BACKEND).locator('.cm-scroller').evaluate((el) => el.scrollTop)).toBeGreaterThan(top - 30)
})

test('zooming out to titles keeps every window where it is and draws only its title', async ({ page }) => {
  await openBoard(page)
  await openDoc(page, 'backend.md', BACKEND)
  await openDoc(page, 'inventory.md', INVENTORY_DOC)
  const before = await position(page, INVENTORY_DOC)
  await blur(page)
  for (let i = 0; i < 5 && (await zoomText(page)) !== 'title'; i++) {
    await page.keyboard.press('Escape')
    await page.waitForTimeout(450)
  }
  await expect(page.locator('.board')).toHaveAttribute('data-level', 'title')
  await expect(win(page, INVENTORY_DOC).locator('.bw-titleonly')).toContainText('inventory.md')
  await expect(win(page, BACKEND).locator('.bw-titleonly')).toContainText('backend.md')
  expect(await position(page, INVENTORY_DOC)).toEqual(before)
  // A click on a title focuses its window again.
  await win(page, INVENTORY_DOC).locator('.bw-titleonly').click()
  await expect(page.locator('.board')).not.toHaveAttribute('data-level', 'title')
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
  await expect(page.locator('.board')).toHaveAttribute('data-level', 'still')
  await expect(win(page, BACKEND).locator('.bw-still')).toBeVisible()
  await page.keyboard.press('Escape')
  await expect(page.locator('.board')).toHaveAttribute('data-level', 'card')
  await expect(win(page, BACKEND).locator('.bw-card-title')).toHaveText('backend.md')
  await page.keyboard.press('Escape')
  await expect(page.locator('.board')).toHaveAttribute('data-level', 'title')
  await page.keyboard.press('z')
  await expect(page.locator('.board')).not.toHaveAttribute('data-level', 'title')
})
