// Connecting to a jazyk server on this computer
// (docs/frontends/gui.md#connecting-to-a-local-jazyk): the connect route reaches a
// mocked local server over its own origin, the token is stored under the port and
// dropped from the address, the documents open, and the server is listed in
// Recent. A stale token surfaces as the token prompt; a port nothing answers on
// says how to start a server.
import { expect, test, type Page, type Route } from '@playwright/test'
import { offline, openDemo, openFolders, pageErrors } from './helpers'

const PORT = 4690
const TOKEN = 'c0ffee0123'
const ROOT = '/home/me/garden'

const cors = (route: Route) => ({
  'access-control-allow-origin': route.request().headers()['origin'] ?? '*',
  'access-control-allow-headers': 'authorization, content-type',
  'access-control-allow-methods': 'GET, POST, PUT, DELETE, OPTIONS',
  'access-control-allow-private-network': 'true',
  'cross-origin-resource-policy': 'cross-origin',
})

// The server, mocked: every route the page reads to show the documents, guarded by
// the token exactly as the real one is (the ping alone answers without it).
async function mockServer(page: Page, token = TOKEN) {
  const calls: string[] = []
  await page.route(`http://127.0.0.1:${PORT}/**`, async (route) => {
    const req = route.request()
    const url = new URL(req.url())
    const path = url.pathname
    calls.push(`${req.method()} ${path}`)
    const json = (body: unknown, status = 200) =>
      route.fulfill({ status, headers: { ...cors(route), 'content-type': 'application/json' }, body: JSON.stringify(body) })
    if (req.method() === 'OPTIONS') return route.fulfill({ status: 204, headers: cors(route) })
    if (path === '/api/ping') return json({ app: 'jazyk-gui', version: '0.2.0', root: ROOT })
    const bearer = req.headers()['authorization']
    if (bearer !== `Bearer ${token}` && url.searchParams.get('token') !== token) return json({ error: 'missing or invalid session token' }, 401)
    switch (path) {
      case '/api/capabilities':
        return json({
          backend: 'server',
          projects: 'list',
          docs: 'write',
          events: 'stream',
          lsp: false,
          jobs: ['compile', 'gen', 'verify', 'audit', 'decompile', 'benchmark'],
          agent: 'none',
          model: 'configured',
          rendering: 'server',
          persistence: 'disk',
        })
      case '/api/status':
        return json({
          generation: 3,
          verdict: { state: 'converged' },
          spent: { sessions: 0, rounds: 0, tokens: 0 },
          parked: [],
          counts: { entities: 2, requirements: 3, relationships: 1 },
          coverage: { covered: 3, total: 3 },
          diagnostics: {},
        })
      case '/api/project':
        return json({ root: ROOT, out: `${ROOT}/jazyk-out`, docsGlob: ['docs/**/*.md'], roots: ['docs/plan.md'], deliverable: '', llm: { model: 'm', baseUrl: '' }, version: '0.2.0' })
      case '/api/projects':
        return json({ projects: [{ root: ROOT, name: 'garden', shape: 'docs-only', current: true, verdict: { state: 'converged' } }] })
      case '/api/settings':
        return json({ exists: true, hash: 'h', settings: { docsGlob: ['docs/**/*.md'], roots: ['docs/plan.md'], deliverable: null, llm: { baseUrl: null, model: 'm', apiKeyEnv: null, apiKeySet: false } } })
      case '/api/docs':
        return json({ docs: [{ path: 'docs/plan.md', contentHash: 'a', graphHash: 'a', stale: false }] })
      case '/api/docs/content':
        return json({ path: 'docs/plan.md', text: '# Garden plan\n\nA bed of beans by the fence.\n', hash: 'a' })
      case '/api/jobs':
        return req.method() === 'POST' ? json({ id: 1 }) : json({ jobs: [] })
      case '/api/release':
        return json({ released: true })
      case '/api/chat/sessions':
        return json({ sessions: [] })
      case '/api/watch':
        return json({ mode: 'queue' })
      case '/api/inbox':
        return json({ generation: 3, questions: 0, proposals: 0, problems: {}, pending: { docs: [], entities: 0 }, drafts: 0, modes: { compile: 'queue', generate: 'manual' }, total: 0 })
      case '/api/events':
        return route.fulfill({ status: 200, headers: { ...cors(route), 'content-type': 'text/event-stream' }, body: ': connected\n\n' })
      default:
        return json({ error: `not mocked: ${path}` }, 404)
    }
  })
  return calls
}

test.beforeEach(async ({ page }) => {
  await offline(page)
})

test('the connect route reaches the server, stores the token under the port, opens the documents, and lists the server in Recent', async ({ page }) => {
  const errors = pageErrors(page)
  const calls = await mockServer(page)
  await page.goto(`/#/connect?port=${PORT}&token=${TOKEN}`)
  await page.waitForURL(/#\/docs/)
  await expect(page.locator('.ws-lenses')).toBeVisible()
  // The token left the address and lives under the port.
  expect(page.url()).not.toContain(TOKEN)
  expect(await page.evaluate((k) => localStorage.getItem(k), `jazyk.server.${PORT}`)).toBe(TOKEN)
  // Every request went to the server on this computer, with the token.
  expect(calls).toContain('GET /api/ping')
  expect(calls).toContain('GET /api/docs')
  await expect(page.locator('.ws-project')).toContainText('garden')
  await openFolders(page, ['docs'])
  await expect(page.locator('.ide-doc-name', { hasText: 'plan.md' })).toBeVisible()
  await page.locator('.ide-doc-name', { hasText: 'plan.md' }).click()
  await expect(page.locator('.cm-content')).toContainText('A bed of beans by the fence.')

  // Recent lists the server in words, with its port.
  await page.goto('/#/')
  const row = page.locator('.proj.recent', { hasText: 'garden' })
  await expect(row.locator('.shape')).toHaveText(`jazyk on this computer · port ${PORT}`)
  expect(errors).toEqual([])
})

// The action button on a server (docs/frontends/gui.md#the-action-button): the
// arrow lists the actions the backend runs with their state and no buttons, the
// choice becomes the main part, and the main part records the release and starts
// the job at once.
test('on a server the arrow lists the actions, the choice becomes the main part, and the main part runs at once', async ({ page }) => {
  const errors = pageErrors(page)
  const calls = await mockServer(page)
  await page.goto(`/#/connect?port=${PORT}&token=${TOKEN}`)
  await page.waitForURL(/#\/docs/)
  const main = page.locator('.ws-action > button.main')
  await expect(main).toHaveText(/Up to date/)

  await page.locator('.ws-top').getByRole('button', { name: 'choose an action' }).click()
  const pop = page.locator('.ws-pop')
  const items = pop.getByRole('option')
  await expect(items).toHaveText([/^Build\s*Up to date/, /^Generate\s*Nothing to generate/, /^Review\s*No tests yet/])
  await expect(pop.locator('.msrc')).toHaveCount(0)
  await expect(pop.getByRole('button')).toHaveCount(0)
  await expect(pop.getByRole('link')).toHaveCount(0)
  await items.nth(1).click()
  await expect(pop).toHaveCount(0)
  await expect(main).toHaveText(/Generate · Nothing to generate/)

  await page.locator('.ws-top').getByRole('button', { name: 'choose an action' }).click()
  await items.first().click()
  await expect(main).toHaveText(/Up to date/)
  await main.click()
  await expect.poll(() => calls.filter((c) => c.startsWith('POST'))).toEqual(['POST /api/release', 'POST /api/jobs'])
  await expect(pop).toHaveCount(0)
  expect(errors).toEqual([])
})

test('a token the server refuses surfaces as the token prompt, and the pasted address resumes', async ({ page }) => {
  await mockServer(page)
  // A stored token from an earlier run, stale now that the server restarted.
  await page.goto('/#/')
  await page.evaluate((k) => localStorage.setItem(k, 'stale00'), `jazyk.server.${PORT}`)
  await page.goto(`/#/connect?port=${PORT}`)
  await expect(page.locator('.token-prompt')).toBeVisible()
  await page.locator('.token-prompt input').fill(`https://jazyk.dev/#/connect?port=${PORT}&token=${TOKEN}`)
  await page.locator('.token-prompt button[type=submit]').click()
  await expect(page.locator('.token-prompt')).toHaveCount(0)
  await openFolders(page, ['docs'])
  await expect(page.locator('.ide-doc-name', { hasText: 'plan.md' })).toBeVisible()
  expect(await page.evaluate((k) => localStorage.getItem(k), `jazyk.server.${PORT}`)).toBe(TOKEN)
})

test('a port nothing answers on says to run jazyk gui, from the route and from Home', async ({ page }) => {
  await page.goto(`/#/connect?port=4691&token=abc`)
  await expect(page.locator('.connect-route')).toContainText('Nothing answered on port 4691')
  await expect(page.locator('.connect-route')).toContainText('jazyk gui')
  await page.goto('/#/')
  await page.getByLabel('Port').fill('4692')
  await page.getByRole('button', { name: 'Connect' }).click()
  await expect(page.locator('.home-connect [role=status]')).toContainText('Nothing answered on port 4692')
})

test('opening a demo after a connection leaves the server for the backend in the tab', async ({ page }) => {
  const calls = await mockServer(page)
  // A demo seen before sits in Recent beside the server.
  await openDemo(page, 'The shop')
  await page.goto(`/#/connect?port=${PORT}&token=${TOKEN}`)
  await page.waitForURL(/#\/docs/)
  await openFolders(page, ['docs'])
  await expect(page.locator('.ide-doc-name', { hasText: 'plan.md' })).toBeVisible()
  await page.goto('/#/')
  await expect(page.locator('.proj.recent').first()).toContainText('garden')
  await page.locator('.proj.recent', { hasText: 'The shop' }).locator('.recent-open').click()
  await page.waitForURL(/#\/docs/)
  await expect(page.locator('.ws-project')).toContainText('The shop')
  const before = calls.length
  await openFolders(page, ['docs'])
  await page.locator('.ide-doc-name', { hasText: 'catalog.md' }).click()
  await expect(page.locator('.cm-content')).toBeVisible()
  // Nothing more went to the server once the demo was open.
  expect(calls.slice(before).filter((c) => !c.includes('/api/events'))).toEqual([])
  // The server keeps running: it stays among the open projects in the menu.
  await page.locator('.ws-project').click()
  await expect(page.locator('.ws-menu .ws-menu-tab', { hasText: 'garden' })).toContainText('running')
  await page.keyboard.press('Escape')
})
