// Answering in the editor and the Inbox in a tab (docs/frontends/gui.md#questions,
// docs/frontends/gui.md#editor, docs/frontends/wasm.md#the-inbox,
// docs/frontends/wasm.md#workflow-modes-in-a-tab): an option's edit lands in the open
// editor as an edit, the scroll and the cursor kept, marked "changed by your answer"
// until the mark fades, in Write and in Read; it leaves nothing pending; a freeform
// answer is queued build work with Send and Build now; the workflow modes are served
// in the tab, and Build on save starts a build on a recorded answer.
import { expect as baseExpect, test, type Page, type Route } from '@playwright/test'
import { api, offline, openDemo, toWrite } from './helpers'

// The backend in the tab answers from a worker, and the novel's graph is large.
const expect = baseExpect.configure({ timeout: 30_000 })

const MIDPOINT = 'Make the midpoint match the Theo arc table (rival)'
const MARK = 'changed by your answer'

interface Rollup {
  handling: number
  pending: { docs: string[]; entities: number }
}

interface Board {
  goals: { id: string; kind: string }[]
}

// The goals that reread a document: what a dual write must never open.
const rereads = async (page: Page) =>
  (await api<Board>(page, 'GET', '/api/board')).goals
    .filter((g) => g.kind === 'reconcile-section' || g.kind === 'place-anchors')
    .map((g) => g.id)
    .sort()

test.beforeEach(async ({ page }, info) => {
  info.setTimeout(240_000)
  await offline(page)
  await openDemo(page, 'The novel')
})

test('an option chosen in the editor lands in place: the scroll and the cursor stay, the line is marked, nothing is pending', async ({ page }) => {
  const pendingBefore = (await api<Rollup>(page, 'GET', '/api/inbox')).pending.docs
  const rereadBefore = await rereads(page)

  // Opened at the midpoint: the editor scrolls there, the band in view.
  await page.goto(`/#/docs/docs/arcs.md?section=${encodeURIComponent('/arcs/the-midpoint')}`)
  await toWrite(page)
  const scroller = page.locator('.ide-editor .cm-scroller').first()
  const option = page.locator('.jz-band').getByRole('button', { name: new RegExp(MIDPOINT.replace(/[()]/g, '\\$&')) })
  await expect(option).toBeVisible()
  // The cursor at the start of the section's first line, above the sentence.
  const anchorLine = page.locator('.cm-line', { hasText: 'The midpoint is chapter 4' }).first()
  await expect(anchorLine).toBeVisible()
  await anchorLine.click({ position: { x: 5, y: 5 } })
  await page.keyboard.press('Home')
  // Scrolled a little further, with both still in view.
  await scroller.evaluate((el) => el.scrollBy(0, 12))
  await expect(option).toBeVisible()
  const top = await scroller.evaluate((el) => el.scrollTop)
  expect(top).toBeGreaterThan(0)

  await option.click()
  const changed = page.locator('.cm-line.jz-flash', { hasText: 'Theo moves from acquaintance to rival.' })
  await expect(changed).toHaveCount(1)
  await expect(changed).toHaveAttribute('title', MARK)
  await expect(page.locator('.cm-content')).not.toContainText('Theo moves from acquaintance to ally.')
  // The editor never reloaded: the view did not move.
  expect(Math.abs((await scroller.evaluate((el) => el.scrollTop)) - top)).toBeLessThanOrEqual(2)

  // The cursor is where it was: a key typed now lands at the end of that line.
  await page.locator('.cm-content').focus()
  await page.keyboard.type('Z')
  await expect(page.locator('.cm-line', { hasText: 'ZThe midpoint is chapter 4' })).toHaveCount(1)
  await page.keyboard.press('Backspace')

  // The mark fades.
  await expect(page.locator('.cm-line.jz-flash')).toHaveCount(0, { timeout: 15_000 })

  // One commit moved the prose and the graph: nothing waits to be reread, and the
  // change is in History.
  expect((await api<Rollup>(page, 'GET', '/api/inbox')).pending.docs).toEqual(pendingBefore)
  expect(await rereads(page)).toEqual(rereadBefore)
  const journal = await api<{ entries: { kind: string }[] }>(page, 'GET', '/api/journal?limit=5')
  expect(journal.entries.map((e) => e.kind)).toContain('answer')
})

test('an option chosen in the Inbox is marked in Read when the document opens', async ({ page }) => {
  await page.goto('/#/inbox/questions')
  const card = page.locator('.inbox-item[data-diag="diag:contradiction-2"]')
  await card.getByRole('button', { name: new RegExp(MIDPOINT.replace(/[()]/g, '\\$&')) }).click()
  await expect(card).toHaveCount(0)
  await page.goto('/#/docs/docs/arcs.md')
  await page.locator('.ide-mode').getByRole('button', { name: 'Read' }).click()
  const marked = page.locator(`.md-read [data-flash="${MARK}"]`)
  await expect(marked.first()).toContainText('Theo moves from acquaintance to rival.', { timeout: 8_000 })
})

test('a freeform answer is queued build work: Send, Queued for the next build, and Build now', async ({ page }) => {
  await page.goto('/#/inbox/questions')
  const card = page.locator('.inbox-item[data-diag="diag:contradiction-3"]')
  const box = card.getByRole('textbox', { name: 'answer in your own words' })
  const send = card.getByRole('button', { name: 'Send', exact: true })
  await expect(send).toBeVisible()
  await expect(send).toBeDisabled()
  await box.fill('Theo is 34.')
  await expect(send).toBeEnabled()
  await send.click()
  const state = card.locator('.answer-state[data-state="handling"]')
  await expect(state).toContainText('Queued for the next build')
  // Without a model the button says what it needs instead of failing.
  const buildNow = state.getByRole('button', { name: 'Build now' })
  await expect(buildNow).toBeVisible()
  await expect(buildNow).toBeDisabled()
  await expect(buildNow).toHaveAttribute('title', /choose a model/)
  expect((await api<Rollup>(page, 'GET', '/api/inbox')).handling).toBeGreaterThan(0)
})

// One OpenAI chat completion carrying the text the model wrote.
function completion(content: string) {
  return JSON.stringify({
    id: 'mock',
    object: 'chat.completion',
    model: 'test-model',
    choices: [{ index: 0, message: { role: 'assistant', content }, finish_reason: 'stop' }],
    usage: { prompt_tokens: 10, completion_tokens: 10, total_tokens: 20 },
  })
}

const ENDPOINT = 'http://localhost:4999/v1'
const CORS = {
  'Access-Control-Allow-Origin': '*',
  'Access-Control-Allow-Headers': '*',
  'Access-Control-Allow-Methods': 'POST, OPTIONS',
  'Cross-Origin-Resource-Policy': 'cross-origin',
}

test('Build on save in a tab: the mode is served and kept, and a recorded answer starts a build', async ({ page }) => {
  await page.route(`${ENDPOINT}/chat/completions`, async (route: Route) => {
    if (route.request().method() === 'OPTIONS') return route.fulfill({ status: 204, headers: CORS })
    return route.fulfill({ status: 200, headers: { ...CORS, 'Content-Type': 'application/json' }, body: completion('Nothing to change.') })
  })
  // A model that builds, from the endpoint the test answers.
  const chat = page.locator('.wb-chat')
  await chat.locator('.msrc-option', { hasText: 'Your endpoint' }).click()
  await chat.getByLabel('Base URL', { exact: true }).fill(ENDPOINT)
  await chat.getByLabel('Model', { exact: true }).fill('test-model')
  await chat.getByRole('button', { name: 'Save' }).click()
  await expect(chat.locator('.chat-empty')).toContainText('Ask about the project')

  expect(await api<{ compile: string }>(page, 'GET', '/api/watch')).toMatchObject({ compile: 'manual' })
  await page.goto('/#/settings')
  const mode = page.locator('.settings-workflow select')
  await expect(mode).toHaveCount(1)
  await mode.selectOption('watch')
  await expect(mode).toHaveValue('watch')
  expect(await api<{ compile: string }>(page, 'GET', '/api/watch')).toMatchObject({ compile: 'auto', mode: 'watch' })
  expect((await api<{ jobs: unknown[] }>(page, 'GET', '/api/jobs')).jobs).toHaveLength(0)

  await page.goto('/#/inbox/questions')
  const card = page.locator('.inbox-item[data-diag="diag:contradiction-3"]')
  await card.getByRole('textbox', { name: 'answer in your own words' }).fill('Theo is 34.')
  await card.getByRole('textbox', { name: 'answer in your own words' }).press('Enter')
  await expect(card.locator('.answer-state[data-state="handling"]')).toContainText('Queued for the next build', { timeout: 15_000 })
  // The build starts on its own a moment later.
  await expect.poll(async () => (await api<{ jobs: { kind: { kind: string } }[] }>(page, 'GET', '/api/jobs')).jobs.map((j) => j.kind.kind), { timeout: 20_000 }).toEqual(['compile'])
  await expect(page.locator('.ws-action .main')).toContainText(/Building/)
})
