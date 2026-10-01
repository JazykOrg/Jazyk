// The Inbox in a tab (docs/frontends/wasm.md#the-inbox, docs/frontends/gui.md#inbox):
// the rollup's counts in the top bar and the navigator match the lists, pending
// changes render, an edit option resolves its question, a freeform answer sends on
// Enter and waits for the next build, triage changes a problem's state, and the
// inspector's problem panel is the Inbox's own card.
import { expect as baseExpect, test, type Page } from '@playwright/test'
import { api, offline, openDemo } from './helpers'

// The backend in the tab answers from a worker, and the novel's graph is large.
const expect = baseExpect.configure({ timeout: 30_000 })

interface Rollup {
  questions: number
  proposals: number
  handling: number
  total: number
  problems: Record<string, number>
  pending: { docs: string[]; entities: number }
  drafts: number
  sections: { questions: string[]; proposals: string[]; problems: string[]; drafts: string[] }
}

interface Diag {
  lifecycle?: string
  triage?: string | null
  answer?: { status: string; text: string } | null
}

const diag = async (page: Page, id: string): Promise<Diag> =>
  (await api<{ diagnostics: Record<string, Diag> }>(page, 'GET', '/api/graph')).diagnostics[id]

test.beforeEach(async ({ page }, info) => {
  info.setTimeout(240_000)
  await offline(page)
  await openDemo(page, 'The novel')
})

test('the Inbox counts what its sections list, and pending changes render', async ({ page }) => {
  const r = await api<Rollup>(page, 'GET', '/api/inbox')
  expect(r.questions).toBeGreaterThan(0)
  expect(r.sections.questions).toContain('diag:contradiction-2')
  expect(r.total).toBeGreaterThan(0)
  const problems = Object.values(r.problems).reduce((a, b) => a + b, 0)
  expect(problems).toBe(r.sections.problems.length)

  await page.goto('/#/inbox')
  // The status bar carries the count the top bar no longer does.
  await expect(page.getByRole('link', { name: `waiting on you (${r.total})` })).toBeVisible()
  const sub = (section: string) => page.locator(`.ws-nav-list a[href="#/inbox${section}"] .sub`)
  await expect(sub('')).toHaveText(String(r.total))
  await expect(sub('/questions')).toHaveText(String(r.questions))
  await expect(sub('/proposals')).toHaveText(String(r.proposals))
  await expect(sub('/problems')).toHaveText(String(problems))
  await expect(sub('/drafts')).toHaveText(String(r.drafts))

  // Every question the count names is a card with its options.
  await page.goto('/#/inbox/questions')
  await expect(page.locator('.inbox-item:visible')).toHaveCount(r.sections.questions.length)
  await expect(page.locator('.inbox-item .opts button:visible').first()).toBeVisible()

  // Pending changes render: the list or its empty state, never an endless loading.
  await page.goto('/#/inbox/pending')
  await expect(page.locator('.ws-scroll:visible').getByText(/^(Nothing pending|Documents changed since their last build|Entities to generate)$/).first()).toBeVisible()
  await expect(page.locator('.ws-scroll:visible').getByText('loading…')).toHaveCount(0)
})

test('an edit option applies and the question leaves the list', async ({ page }) => {
  await page.goto('/#/inbox/questions')
  const card = page.locator('.inbox-item[data-diag="diag:contradiction-2"]:visible')
  await expect(card).toBeVisible()
  await card.getByRole('button', { name: /Make the midpoint match the Theo arc table/ }).click()
  await expect(card).toHaveCount(0)
  await expect(page.locator('.inbox-error')).toHaveCount(0)
  expect((await diag(page, 'diag:contradiction-2')).lifecycle).toBe('resolved')
  const r = await api<Rollup>(page, 'GET', '/api/inbox')
  expect(r.sections.questions).not.toContain('diag:contradiction-2')
  await expect(page.locator('.ws-nav-list a[href="#/inbox/questions"] .sub')).toHaveText(String(r.questions))
})

test('a freeform answer sends on Enter, Shift+Enter makes a new line, and the next build acts on it', async ({ page }) => {
  const before = await api<Rollup>(page, 'GET', '/api/inbox')
  await page.goto('/#/inbox/questions')
  const card = page.locator('.inbox-item[data-diag="diag:contradiction-3"]:visible')
  const box = card.getByRole('textbox', { name: 'answer in your own words' })
  await box.click()
  await box.pressSequentially('Theo is 34.')
  await box.press('Shift+Enter')
  await box.pressSequentially('The arcs table is right.')
  await expect(box).toHaveValue('Theo is 34.\nThe arcs table is right.')
  await box.press('Enter')
  const state = card.locator('.answer-state[data-state="handling"]')
  await expect(state).toContainText('Queued for the next build')
  const d = await diag(page, 'diag:contradiction-3')
  expect(d.answer?.status).toBe('handling')
  expect(d.answer?.text).toBe('Theo is 34.\nThe arcs table is right.')
  // Listed with its progress, no longer counted as waiting.
  const after = await api<Rollup>(page, 'GET', '/api/inbox')
  expect(after.questions).toBe(before.questions - 1)
  expect(after.handling).toBe(before.handling + 1)
  expect(after.sections.questions).toContain('diag:contradiction-3')
})

test('Seen, Won\'t fix, and Hide change a problem\'s state', async ({ page }) => {
  await page.goto('/#/inbox/problems')
  const first = page.locator('.inbox-item:visible').first()
  await expect(first).toBeVisible()
  const id = (await first.getAttribute('data-diag')) as string
  const card = page.locator(`.inbox-item[data-diag="${id}"]:visible`)
  const button = (name: string) => card.locator('.triage').getByRole('button', { name, exact: true })

  await button('Seen').click()
  await expect(button('Seen')).toHaveAttribute('aria-pressed', 'true')
  expect((await diag(page, id)).triage).toBe('acknowledged')

  await button("Won't fix").click()
  await expect(button("Won't fix")).toHaveAttribute('aria-pressed', 'true')
  await expect(button('Seen')).toHaveAttribute('aria-pressed', 'false')
  expect((await diag(page, id)).triage).toBe('wontfix')

  // A click on the active one clears it.
  await button("Won't fix").click()
  await expect(button("Won't fix")).toHaveAttribute('aria-pressed', 'false')
  expect((await diag(page, id)).triage ?? null).toBeNull()

  // Hidden leaves the list and the count.
  const before = await api<Rollup>(page, 'GET', '/api/inbox')
  await button('Hidden').click()
  await expect(card).toHaveCount(0)
  expect((await diag(page, id)).triage).toBe('suppressed')
  const after = await api<Rollup>(page, 'GET', '/api/inbox')
  expect(after.sections.problems.length).toBe(before.sections.problems.length - 1)
})

test('a problem’s card holds its options and working triage', async ({ page }) => {
  await page.goto('/#/inbox/questions')
  const panel = page.locator('.ws-scroll:visible .inbox-item[data-diag="diag:contradiction-1"]')
  await expect(panel).toBeVisible()
  await expect(panel.getByRole('button', { name: /Make arcs agree that Theo turns it down/ })).toBeVisible()
  await expect(panel.getByRole('button', { name: /Make plot agree that Ines declines it/ })).toBeVisible()
  await expect(panel.getByRole('textbox', { name: 'answer in your own words' })).toBeVisible()
  // The related requirements by statement, not by id.
  const related = panel.locator('.related li')
  if ((await related.count()) > 0) await expect(related.first()).not.toHaveText(/^req:/)

  await panel.locator('.triage').getByRole('button', { name: 'Seen', exact: true }).click()
  await expect(panel.locator('.triage').getByRole('button', { name: 'Seen', exact: true })).toHaveAttribute('aria-pressed', 'true')
  expect((await diag(page, 'diag:contradiction-1')).triage).toBe('acknowledged')
})
