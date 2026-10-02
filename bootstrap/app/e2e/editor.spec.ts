// The editor as a visitor drives it (docs/frontends/wasm.md#building).
import { expect, test } from '@playwright/test'
import { api, offline, openDemo, openFolders, pageErrors, typeAtEnd } from './helpers'

test.beforeEach(async ({ page }) => {
  await offline(page)
})

test('every route but Home goes home until a project is open', async ({ page }) => {
  await page.goto('/#/model/entities/ent%3Abackend')
  await page.waitForURL(/#\/$/)
  await expect(page.locator('.ws-lenses')).toHaveCount(0)
  await expect(page.locator('.home .demo')).toHaveCount(6)
})

test("nothing downloads the model on arrival; the action button's popover offers it", async ({ page }) => {
  const external = await offline(page)
  await openDemo(page, 'The sort utility')
  await page.waitForTimeout(1500)
  expect(external).toEqual([])
  const caps = await api<{ model: string }>(page, 'GET', '/api/capabilities')
  // A browser without WebGPU has no model to offer at all.
  if (caps.model === 'loadable') {
    await page.locator('.ws-action > button.main').click()
    await expect(page.locator('.ws-pop')).toContainText('Load the model')
    expect(external).toEqual([])
  }
})

test('a demo opened after another carries only its own documents', async ({ page }) => {
  await openDemo(page, 'The ERP')
  await openDemo(page, 'The novel')
  const docs = await api<{ docs: { path: string }[] }>(page, 'GET', '/api/docs')
  const paths = docs.docs.map((d) => d.path)
  expect(paths.some((p) => p.includes('backend'))).toBe(false)
  expect(paths.some((p) => p.includes('characters'))).toBe(true)
})

test('the product pages render for every demo', async ({ page }) => {
  const errs = pageErrors(page)
  for (const demo of ['The sort utility', 'The ERP']) {
    await openDemo(page, demo)
    for (const route of ['#/product', '#/product/trace', '#/inbox', '#/history', '#/model']) {
      await page.goto(`/${route}`)
      await expect(page.locator('body')).not.toContainText('failed to render')
    }
  }
  expect(errs).toEqual([])
})

// The product a demo carries reads in the tab (docs/frontends/wasm.md#the-product).
test('the product files list and a file opens with its text', async ({ page }) => {
  await openDemo(page, 'The sort utility')
  await page.goto('/#/product/files')
  const nav = page.locator('.ws-nav')
  // The product folder of the project tree, its files product files and tests.
  await openFolders(page, ['code', 'code/test'])
  await expect(nav.getByRole('link', { name: /^main\.js( ?\d+)?$/ })).toBeVisible()
  await expect(nav.getByRole('link', { name: /^main\.test\.js( ?\d+)?$/ })).toBeVisible()
  await nav.getByRole('link', { name: /^main\.js( ?\d+)?$/ }).click()
  await page.waitForURL(/#\/product\/files\/main\.js$/)
  await expect(page.locator('.ide-topbar')).toContainText('main.js')
  await expect(page.locator('.deliv-editor').first()).toContainText('readStdinLines')

  // A file the ledger binds shows its sites as lenses; a fixture without a ledger
  // binds none.
  const deliv = await api<{ files: { path: string; owners: { requirements: string[] } }[] }>(page, 'GET', '/api/deliverable')
  const bound = deliv.files.find((f) => f.owners.requirements.length > 0)
  if (bound) {
    const file = await api<{ sites: unknown[] }>(page, 'GET', `/api/deliverable/file?path=${encodeURIComponent(bound.path)}`)
    await page.goto(`/#/product/files/${bound.path}`)
    if (file.sites.length > 0) await expect(page.locator('.jz-lens').first()).toBeVisible()
  }
})

test('the trace matrix renders a row per requirement and exports from the tab', async ({ page }) => {
  await openDemo(page, 'The sort utility')
  const matrix = await api<{ rows: Record<string, unknown> }>(page, 'GET', '/api/verify/matrix')
  const n = Object.keys(matrix.rows).length
  expect(n).toBeGreaterThan(0)
  await page.goto('/#/product/trace')
  const center = page.locator('.ws-center')
  await expect(center.locator('.card > div .chip')).toHaveCount(n)
  // Tests do not run in a tab, so the matrix offers no run action.
  await expect(center.getByRole('button', { name: /^run$/ })).toHaveCount(0)
  const download = page.waitForEvent('download')
  await center.getByRole('link', { name: 'csv' }).click()
  const file = await download
  expect(file.suggestedFilename()).toBe('trace-matrix.csv')
  const text = await (await import('node:fs/promises')).readFile(await file.path(), 'utf8')
  expect(text.split('\n')[0]).toBe('requirement,entity,status,reason,repair,test,lastRun,evidence')
  expect(text.trim().split('\n')).toHaveLength(n + 1)
})

test('the generated pages list and one opens rendered', async ({ page }) => {
  await openDemo(page, 'The sort utility')
  // The generated pages sit in the build output folder of the project tree.
  await openFolders(page, ['.jazyk', '.jazyk/docsgen'])
  await page.locator('.ws-nav .ft-file[data-path=".jazyk/docsgen/index.md"] a').click()
  await expect(page.locator('.ws-center h1:visible').first()).toHaveText('Index')
})

test('an entity page reads by name, states its relations once, and explains a connection', async ({ page }) => {
  await openDemo(page, 'The ERP')
  await page.goto('/#/model/entities/ent%3Abackend')
  const center = page.locator('.ws-center')
  await expect(center.locator('h1')).toHaveText('Backend')
  // No compiler ids in the default mode, and no second list of relations.
  await expect(center).not.toContainText(/\b(ent|req|rel|view|g):[a-z0-9]/)
  await expect(center.getByRole('heading', { name: /relationships/i })).toHaveCount(0)
  const box = center.locator('.cn-n', { hasText: 'Frontend' })
  await box.click()
  await expect(center.locator('.cn-why')).toContainText('Backend is related to Frontend')
  await expect(center.locator('.cn-why .req-row')).not.toHaveCount(0)
})

test('a requirement keeps the Requirements list and shows where it comes from', async ({ page }) => {
  await openDemo(page, 'The ERP')
  await page.goto('/#/model')
  await page.locator('.wb-side-tabs a', { hasText: 'Requirements' }).click()
  await page.locator('.wb-list-row').first().click()
  await page.waitForURL(/#\/model\/requirements\//)
  await expect(page.locator('.wb-side-tabs a.active')).toHaveText('Requirements')
  await expect(page.locator('.ws-center .wst-item:visible')).toContainText('Where it comes from')
  await expect(page.locator('.ws-center .source-quote:visible blockquote')).toBeVisible()
})

test('a diagram shows its picture in the tab', async ({ page }) => {
  await openDemo(page, 'The ERP')
  await page.goto('/#/model/diagrams/view%3Acomponent%2Fwarehouse-erp')
  await expect(page.locator('.ws-center .wst-item:visible .view-svg svg').first()).toBeVisible()
})

test('a demo saves an edit on its own', async ({ page }) => {
  await openDemo(page, 'The ERP')
  await page.goto('/#/docs/docs/main.md')
  await typeAtEnd(page, '\nAutosaved line.')
  await expect(page.locator('.ide-topbar-right')).toContainText('saved', { timeout: 5000 })
  await expect
    .poll(async () => JSON.stringify(await api(page, 'GET', '/api/docs/content?path=docs/main.md')))
    .toContain('Autosaved line.')
})

test('the recent list remembers a demo, reopens it, and forgets it', async ({ page }) => {
  await openDemo(page, 'The shop')
  await page.goto('/#/')
  const row = page.locator('.proj.recent', { hasText: 'The shop' })
  await expect(row).toBeVisible()
  await row.locator('.recent-open').click()
  await page.waitForURL(/#\/docs/)
  await page.goto('/#/')
  await page.locator('.proj.recent', { hasText: 'The shop' }).locator('.recent-forget').click()
  await expect(page.locator('.proj.recent')).toHaveCount(0)
})

test('Home reads Open now, the four cards, then Recent', async ({ page }) => {
  await openDemo(page, 'The shop')
  await page.goto('/#/')
  await expect(page.locator('.home h2')).toHaveText([
    'Open now',
    'Try a demo',
    'Start a project in this browser',
    'Open a folder on this computer',
    'Connect to jazyk on this computer',
    'Recent',
  ])
  await expect(page.locator('.home').getByRole('button', { name: 'Open folder…' })).toBeVisible()
  // Going Home does not close the project.
  await expect(page.locator('.home-open')).toContainText('The shop')
  await expect(page.locator('.ws-lenses')).toBeVisible()
})

test('a browser without the folder API says so instead of offering the button', async ({ page }) => {
  await page.addInitScript(() => {
    delete (window as unknown as { showDirectoryPicker?: unknown }).showDirectoryPicker
  })
  await page.goto('/#/')
  await expect(page.locator('.home-nofolder')).toContainText('needs Chrome, Edge, or another Chromium browser')
  await expect(page.locator('.home').getByRole('button', { name: 'Open folder…' })).toHaveCount(0)
})

test('an endpoint model source is ready at once and is remembered in this browser only', async ({ page }) => {
  await openDemo(page, 'The sort utility')
  await page.goto('/#/settings')
  // The chat pane offers the same chooser until a model is ready; this one is Settings'.
  const chooser = page.locator('.settings-model .msrc')
  await chooser.locator('.msrc-option', { hasText: 'Your endpoint' }).click()
  await expect(chooser.locator('.msrc-privacy')).toHaveAttribute('aria-label', /Nothing is saved on a jazyk server/)
  await chooser.getByLabel('Base URL', { exact: true }).fill('http://localhost:4999/v1')
  await chooser.getByLabel('Model', { exact: true }).fill('test-model')
  await chooser.getByRole('button', { name: 'Save' }).click()
  await expect(chooser.locator('.msrc-status')).toContainText('Ready: test-model')
  expect(await api<{ model: string }>(page, 'GET', '/api/capabilities')).toMatchObject({ model: 'in-page' })
  const stored = await page.evaluate(() => JSON.parse(localStorage.getItem('jazyk.modelSource') ?? '{}'))
  expect(stored).toMatchObject({ kind: 'endpoint', baseUrl: 'http://localhost:4999/v1', model: 'test-model' })
  // A reload keeps the choice, and the open project with its route.
  await page.reload()
  await expect(chooser.locator('.msrc-status')).toContainText('Ready: test-model')
  await expect(page).toHaveURL(/#\/settings/)
  expect(await api<{ model: string }>(page, 'GET', '/api/capabilities')).toMatchObject({ model: 'in-page' })
})

// The browser's built-in model (docs/frontends/wasm.md#model-sources): a Prompt API
// the test installs stands in for Chrome's, and answers a chat.
test("the browser's built-in model source answers a chat", async ({ page }) => {
  const REPLY = 'The sort utility reads lines from standard input and prints them sorted.'
  await page.addInitScript((reply: string) => {
    const prompts: string[] = []
    ;(window as unknown as { __prompts: string[] }).__prompts = prompts
    ;(globalThis as unknown as { LanguageModel: unknown }).LanguageModel = {
      availability: async () => 'available',
      create: async () => ({
        prompt: async (input: string) => {
          prompts.push(input)
          return reply
        },
        destroy() {},
      }),
    }
  }, REPLY)
  await openDemo(page, 'The sort utility')
  const chat = page.locator('.wb-chat')
  const option = chat.locator('.msrc-option', { hasText: "Chrome's built-in model" })
  await expect(option).toContainText('Ready on this device')
  await option.click()
  await chat.getByRole('button', { name: 'Load the model' }).click()
  await expect.poll(async () => (await api<{ model: string }>(page, 'GET', '/api/capabilities')).model).toBe('in-page')

  const box = chat.locator('.chat-input textarea')
  await box.fill('What does it do?')
  await box.press('Enter')
  await expect(chat.locator('.chat-row.agent')).toContainText(REPLY)
  await expect(chat.locator('.chat-row.marker').last()).toContainText('end_turn')
  const prompts = await page.evaluate(() => (window as unknown as { __prompts: string[] }).__prompts)
  expect(prompts).toHaveLength(1)
  expect(prompts[0]).toContain('What does it do?')
  expect(await page.evaluate(() => JSON.parse(localStorage.getItem('jazyk.modelSource') ?? '{}'))).toMatchObject({ kind: 'chrome' })
})

// The requirement page under the presentation rules (docs/frontends/gui.md#model):
// criteria with their kind in words, a status in words, and ids only in Developer mode.
test('a requirement page shows the test behind each criterion with its result, and where it is implemented', async ({ page }) => {
  await openDemo(page, 'The ERP')
  await page.goto('/#/model/requirements/req%3Abackend-1')
  const item = page.locator('.ws-center .wst-item:visible')
  const test_ = item.locator('.crit-test').first()
  await expect(test_.locator('.crit-result')).toHaveText(/^(Passing|Failing|No result yet)$/)
  await expect(test_.getByRole('link', { name: 'crit_backend_1_b7db3c35.md' })).toBeVisible()
  // Implemented in: the anchored site opens the product file at its line.
  await expect(item.getByRole('heading', { name: 'Implemented in' })).toBeVisible()
  await item.getByRole('link', { name: 'app.js:11' }).click()
  await expect(page).toHaveURL(/#\/product\/files\/app\.js\?line=11$/)
})

test('a requirement page lists its criteria in words, and Developer mode shows the ids', async ({ page }) => {
  await openDemo(page, 'The sort utility')
  const graph = await api<{ criteria: Record<string, { requirement: string }> }>(page, 'GET', '/api/graph')
  const withCriteria = Object.values(graph.criteria).map((c) => c.requirement)
  const rid = withCriteria.sort()[0]
  expect(rid).toMatch(/^req:/)
  const n = withCriteria.filter((r) => r === rid).length

  await page.goto(`/#/model/requirements/${encodeURIComponent(rid)}`)
  const center = page.locator('.ws-center')
  await expect(center.locator('h1.statement')).toBeVisible()
  await expect(center.getByRole('heading', { name: 'Acceptance criteria' })).toBeVisible()
  const kinds = center.locator('.criterion .chip')
  await expect(kinds).toHaveCount(n)
  for (const word of await kinds.allTextContents()) expect(['Example', 'Failure case', 'Edge case', 'Measure']).toContain(word)
  await expect(center.locator('.status-line .status-word')).toHaveText(
    /^(Passing|Failing|Partly passing|No test yet|Not generated yet|Outdated: the requirement changed|Needs a rerun)$/,
  )
  await expect(center).not.toContainText(/\b(req|crit|ent):[a-z0-9]/)

  await page.locator('.ws-dev').click()
  await expect(center).toContainText(rid)
  await expect(center.locator('.criterion .dev-id')).toHaveCount(n)
  await expect(center.locator('.criterion .dev-id').first()).toContainText('crit:')
  await expect(center.locator('.status-line .status-word')).toContainText(/\((verified|failing|partial|missing|unimplemented|stale-[a-z]+)\)/)
})

// Every view of every demo has its picture in the tab (docs/frontends/wasm.md#diagrams):
// the fixtures ship rendered, and the read serves the .svg whose .puml matches.
test('every diagram of every demo has its picture', async ({ page }) => {
  // Six demos opened in turn: seventeen seconds alone, longer beside a build.
  test.setTimeout(180_000)
  let total = 0
  for (const demo of ['The shop', 'The sort utility', 'The ERP', 'The slide deck', 'The novel', 'The organization']) {
    await openDemo(page, demo)
    const graph = await api<{ views: Record<string, unknown> }>(page, 'GET', '/api/graph')
    const listed = await api<{ views: { id: string }[] }>(page, 'GET', '/api/views')
    const ids = Object.keys(graph.views)
    expect(listed.views.map((v) => v.id).sort(), demo).toEqual([...ids].sort())
    for (const id of ids) {
      const view = await api<{ svg: string | null; renderError: string | null }>(page, 'GET', `/api/views/${encodeURIComponent(id)}`)
      expect(view.renderError, `${demo}: ${id}`).toBeNull()
      expect(view.svg ?? '', `${demo}: ${id}`).toContain('<svg')
    }
    total += ids.length
  }
  // A fixture without a view (the sort utility) has nothing to draw; the rest do.
  expect(total).toBeGreaterThan(0)
})
