// The chat column in a tab (docs/frontends/wasm.md#chat): the embedded loop runs in
// the worker on the page's model, here an endpoint the test answers itself.
import { expect, test, type Route } from '@playwright/test'
import { api, offline, openDemo, pageErrors } from './helpers'

const ENDPOINT = 'http://localhost:4999/v1'
const ANSWER = 'The Customer is a person who holds an account and places Orders in the Shop.'

test.beforeEach(async ({ page }) => {
  await offline(page)
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

const CORS = {
  'Access-Control-Allow-Origin': '*',
  'Access-Control-Allow-Headers': '*',
  'Access-Control-Allow-Methods': 'POST, OPTIONS',
  // The page is cross-origin isolated (COEP require-corp).
  'Cross-Origin-Resource-Policy': 'cross-origin',
}

test('without a model the chat pane offers the model chooser, not a dead end', async ({ page }) => {
  const errors = pageErrors(page)
  await openDemo(page, 'The shop')
  const chat = page.locator('.wb-chat')
  await expect(chat.locator('.chat-model-head')).toContainText('Chat needs a model in this browser')
  await expect(chat.locator('.msrc-option', { hasText: 'Your endpoint' })).toBeVisible()
  await expect(chat).not.toContainText('no agent')
  expect(await api<{ agent: string }>(page, 'GET', '/api/capabilities')).toMatchObject({ agent: 'embedded' })
  expect(errors).toEqual([])
})

test('a chat on an endpoint source calls a read tool and answers in the pane', async ({ page }) => {
  const errors = pageErrors(page)
  const bodies: { messages: { role: string; content: string }[] }[] = []
  await page.route(`${ENDPOINT}/chat/completions`, async (route: Route) => {
    if (route.request().method() === 'OPTIONS') return route.fulfill({ status: 204, headers: CORS })
    bodies.push(route.request().postDataJSON())
    // The tab speaks the text codec: the first reply is a JSON action, the next the answer.
    const content = bodies.length === 1 ? '{"tool": "search", "args": {"query": "Customer"}}' : ANSWER
    return route.fulfill({ status: 200, headers: { ...CORS, 'Content-Type': 'application/json' }, body: completion(content) })
  })

  await openDemo(page, 'The shop')
  const chat = page.locator('.wb-chat')
  await chat.locator('.msrc-option', { hasText: 'Your endpoint' }).click()
  await chat.getByLabel('Base URL', { exact: true }).fill(ENDPOINT)
  await chat.getByLabel('Model', { exact: true }).fill('test-model')
  await chat.getByRole('button', { name: 'Save' }).click()

  // The model is ready: the chooser gives way to the conversation.
  await chat.getByRole('button', { name: 'New conversation' }).click()
  const box = chat.locator('.chat-input textarea')
  await box.fill('What is a Customer?')
  await box.press('Enter')

  await expect(chat.locator('.chat-row.user')).toContainText('What is a Customer?')
  await expect(chat.locator('.chat-row.tool')).toContainText('search')
  await expect(chat.locator('.chat-row.tool.ok')).toHaveCount(1)
  await expect(chat.locator('.chat-row.agent')).toContainText(ANSWER)
  // The turn ended as a conversation: the answer, not a nudge for a summary.
  await expect(chat.locator('.chat-row.marker').last()).toContainText('end_turn')
  expect(bodies.length).toBe(2)

  // The first call carried the chat serving's instructions and its read tools; the
  // second carried the search result back.
  const first = JSON.stringify(bodies[0].messages)
  expect(first).toContain('CHAT SERVING')
  expect(first).toContain('- search:')
  expect(first).not.toContain('- begin_generation:')
  const last = bodies[1].messages[bodies[1].messages.length - 1]
  expect(last.role).toBe('user')
  expect(last.content).toContain('RESULT:')

  // The session list and transcript answer the native shapes.
  const list = await api<{ sessions: { id: string; title: string; state: string }[]; commands: unknown[] }>(page, 'GET', '/api/chat/sessions')
  expect(list.sessions).toHaveLength(1)
  expect(list.sessions[0]).toMatchObject({ title: 'What is a Customer?', state: 'idle' })
  expect(list.commands).toEqual([])
  expect(errors).toEqual([])
})

test('an edit asked in chat moves the prose and the graph together', async ({ page }) => {
  const NEW = 'A Product has a name, a price, and a stock count.'
  let calls = 0
  await page.route(`${ENDPOINT}/chat/completions`, async (route: Route) => {
    if (route.request().method() === 'OPTIONS') return route.fulfill({ status: 204, headers: CORS })
    calls += 1
    const content =
      calls === 1
        ? JSON.stringify({ tool: 'revise_requirement', args: { id: 'req:catalog-2', new_text: NEW, statement: NEW } })
        : 'Done: the Product requirement now names the stock count.'
    return route.fulfill({ status: 200, headers: { ...CORS, 'Content-Type': 'application/json' }, body: completion(content) })
  })
  await openDemo(page, 'The shop')
  const before = await api<{ generation: number }>(page, 'GET', '/api/status')
  const chat = page.locator('.wb-chat')
  await chat.locator('.msrc-option', { hasText: 'Your endpoint' }).click()
  await chat.getByLabel('Base URL', { exact: true }).fill(ENDPOINT)
  await chat.getByLabel('Model', { exact: true }).fill('test-model')
  await chat.getByRole('button', { name: 'Save' }).click()
  await chat.getByRole('button', { name: 'New conversation' }).click()
  await chat.locator('.chat-input textarea').fill('A Product also has a stock count.')
  await chat.locator('.chat-input textarea').press('Enter')
  await expect(chat.locator('.chat-row.tool.ok')).toContainText('revise_requirement')
  await expect(chat.locator('.chat-row.agent')).toContainText('Done')

  const doc = await api<{ text: string }>(page, 'GET', '/api/docs/content?path=docs/catalog.md')
  expect(doc.text).toContain(NEW)
  const req = await api<{ statement: string; source: { quote: string } }>(page, 'GET', '/api/requirements/req%3Acatalog-2')
  expect(req.statement).toBe(NEW)
  expect(req.source.quote).toBe(NEW)
  const after = await api<{ generation: number }>(page, 'GET', '/api/status')
  expect(after.generation).toBeGreaterThan(before.generation)
})

test('a model error ends the turn with the error in the pane, never a hang', async ({ page }) => {
  await page.route(`${ENDPOINT}/chat/completions`, async (route: Route) => {
    if (route.request().method() === 'OPTIONS') return route.fulfill({ status: 204, headers: CORS })
    return route.fulfill({
      status: 400,
      headers: { ...CORS, 'Content-Type': 'application/json' },
      body: JSON.stringify({ error: { message: "This model's maximum context length is 4096 tokens." } }),
    })
  })
  await openDemo(page, 'The shop')
  const chat = page.locator('.wb-chat')
  await chat.locator('.msrc-option', { hasText: 'Your endpoint' }).click()
  await chat.getByLabel('Base URL', { exact: true }).fill(ENDPOINT)
  await chat.getByLabel('Model', { exact: true }).fill('small-model')
  await chat.getByRole('button', { name: 'Save' }).click()
  await chat.getByRole('button', { name: 'New conversation' }).click()
  await chat.locator('.chat-input textarea').fill('Tell me everything.')
  await chat.locator('.chat-input textarea').press('Enter')
  const end = chat.locator('.chat-row.marker').last()
  await expect(end).toContainText('turn ended')
  await expect(end).toContainText('maximum context length')
  await expect(end).toContainText('does not fit the model')
  // The box is back for the next prompt.
  await expect(chat.locator('.chat-input textarea')).toBeEnabled()
})

test('cancel ends a turn whose model call never answers', async ({ page }) => {
  let calls = 0
  await page.route(`${ENDPOINT}/chat/completions`, async (route: Route) => {
    if (route.request().method() === 'OPTIONS') return route.fulfill({ status: 204, headers: CORS })
    // A model that never replies: only the cancel can end the wait.
    calls += 1
  })
  await openDemo(page, 'The shop')
  const chat = page.locator('.wb-chat')
  await chat.locator('.msrc-option', { hasText: 'Your endpoint' }).click()
  await chat.getByLabel('Base URL', { exact: true }).fill(ENDPOINT)
  await chat.getByLabel('Model', { exact: true }).fill('slow-model')
  await chat.getByRole('button', { name: 'Save' }).click()
  await chat.getByRole('button', { name: 'New conversation' }).click()
  await chat.locator('.chat-input textarea').fill('Are you there?')
  await chat.locator('.chat-input textarea').press('Enter')
  await expect.poll(() => calls).toBe(1)
  await chat.getByRole('button', { name: 'cancel' }).click()
  await expect(chat.locator('.chat-row.marker').last()).toContainText('cancelled')
  await expect(chat.getByRole('button', { name: 'send' })).toBeVisible()
})
