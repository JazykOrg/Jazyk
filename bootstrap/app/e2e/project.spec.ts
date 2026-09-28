// The open project and the top bar around it: the project survives a reload at its
// route (docs/frontends/gui.md#the-open-project), the project menu says where
// changes go and offers the ways out (#the-project-menu), the action button is a
// split button whose list holds what the backend runs (#the-action-button), and a
// build in a tab shows its progress and opens its build thread
// (docs/frontends/wasm.md#the-demo, jobs in a tab).
import { readFile } from 'node:fs/promises'
import { expect, test, type Route } from '@playwright/test'
import { unzipSync } from 'fflate'
import { api, chooseMenuItem, createProject, offline, openDemo, openProjectMenu, pageErrors, projectMenuItems, typeAtEnd, whereChangesGo, waitStored } from './helpers'

test.beforeEach(async ({ page }) => {
  await offline(page)
})

const openId = (page: import('@playwright/test').Page) => page.evaluate(() => localStorage.getItem('jazyk.open'))

test('a reload lands in the open demo at the same route, and Close project forgets it', async ({ page }) => {
  const errors = pageErrors(page)
  await openDemo(page, 'The shop')
  await page.goto('/#/docs/docs/catalog.md')
  await expect(page.locator('.cm-content')).toBeVisible()
  expect(await openId(page)).toBe('demo:shop')

  await waitStored(page)

  await page.reload()
  await expect(page).toHaveURL(/#\/docs\/docs\/catalog\.md/)
  await expect(page.locator('.cm-content')).toBeVisible()
  await expect(page.locator('.ws-lenses')).toBeVisible()
  await expect(page.locator('.ws-project')).toContainText('The shop')

  // Home keeps the project open and offers the way back.
  await chooseMenuItem(page, 'Home')
  await expect(page).toHaveURL(/#\/$/)
  const open = page.locator('.home-open')
  await expect(open).toContainText('The shop')
  await expect(open).toContainText('Demo: edits are saved in this browser')
  await expect(page.locator('.ws-lenses')).toBeVisible()
  await open.getByRole('link', { name: 'Back to the project' }).click()
  await expect(page).toHaveURL(/#\/docs/)

  await chooseMenuItem(page, 'Close project')
  await expect(page).toHaveURL(/#\/$/)
  await expect(page.locator('.ws-lenses')).toHaveCount(0)
  await expect(page.locator('.home-open')).toHaveCount(0)
  expect(await openId(page)).toBeNull()
  await waitStored(page)
  await page.reload()
  await expect(page.locator('.home-cards')).toBeVisible()
  await expect(page.locator('.ws-lenses')).toHaveCount(0)
  expect(errors).toEqual([])
})

test('a reload lands in the open browser project at the same route', async ({ page }) => {
  await page.goto('/#/')
  await createProject(page, 'Garden planner')
  await page.goto('/#/docs/docs/README.md')
  await typeAtEnd(page, '\nA planner lays out the beds of a garden.')
  await expect(page.locator('.ide-topbar-right')).toContainText('saved', { timeout: 5000 })
  expect(await openId(page)).toMatch(/^browser:/)

  await waitStored(page)

  await page.reload()
  await expect(page).toHaveURL(/#\/docs\/docs\/README\.md/)
  await expect(page.locator('.cm-content')).toContainText('lays out the beds')
  await expect(page.locator('.ws-project')).toContainText('Garden planner')
  expect(await api(page, 'GET', '/api/capabilities')).toMatchObject({ backend: 'wasm', persistence: 'browser' })
  expect(await whereChangesGo(page)).toBe('Saved in this browser only')
})

test('the project menu says where changes go and downloads the project', async ({ page }) => {
  await openDemo(page, 'The shop')
  expect(await whereChangesGo(page)).toBe('Demo: edits are saved in this browser')
  const items = await projectMenuItems(page)
  expect(items).toEqual(['Save to a folder on this computer…', 'Download as .zip', 'Home', 'Close project'])

  const download = page.waitForEvent('download')
  const menu = await openProjectMenu(page)
  await menu.getByRole('menuitem', { name: 'Download as .zip' }).click()
  const file = await download
  expect(file.suggestedFilename()).toBe('the-shop.zip')
  const entries = unzipSync(new Uint8Array(await readFile(await file.path())))
  expect(Object.keys(entries)).toContain('the-shop/jazyk.toml')
  expect(Object.keys(entries)).toContain('the-shop/docs/catalog.md')
})

// No Run, no Cancel, no Settings: the popover has no buttons of its own.
async function expectNoPopoverButtons(pop: import('@playwright/test').Locator) {
  await expect(pop.getByRole('button', { name: /^(Run|Cancel|Settings)/ })).toHaveCount(0)
  await expect(pop.getByRole('link', { name: /Settings/ })).toHaveCount(0)
}

// A click on the empty stretch of the top bar, left of the search box.
async function clickElsewhere(page: import('@playwright/test').Page) {
  const box = await page.locator('.ws-top .ws-palette').boundingBox()
  if (!box) throw new Error('no search box in the top bar')
  await page.mouse.click(box.x - 30, box.y + box.height / 2)
}

test("in a tab the arrow's popover holds the model chooser alone; Developer mode adds the actions with reasons", async ({ page }) => {
  await openDemo(page, 'The shop')
  const top = page.locator('.ws-top')
  await top.getByRole('button', { name: 'choose a model' }).click()
  const pop = page.locator('.ws-pop')
  await expect(pop.locator('.msrc')).toBeVisible()
  await expect(pop.getByRole('listbox', { name: 'actions' })).toHaveCount(0)
  await expectNoPopoverButtons(pop)
  // A click anywhere else closes it.
  await clickElsewhere(page)
  await expect(pop).toHaveCount(0)

  await page.locator('.ws-dev').click()
  await top.getByRole('button', { name: 'choose a model' }).click()
  const items = pop.getByRole('listbox', { name: 'actions' }).getByRole('option')
  await expect(items).toHaveText([/^Build/, /^Generate/, /^Review/])
  await expect(items.nth(1)).toContainText('This backend cannot run a generate run.')
  await expect(items.nth(1)).toHaveAttribute('aria-disabled', 'true')
  await expect(items.nth(2)).toContainText('This backend cannot run a review.')
  await expect(pop).toContainText('Document')
  await expect(pop).toContainText('Preview prompt')
  await expect(pop.locator('.ws-pop-modes')).toBeVisible()
  await expectNoPopoverButtons(pop)
  await page.keyboard.press('Escape')
  await expect(pop).toHaveCount(0)
})

// With no model ready nothing could run: the main part opens the chooser, and no
// job starts (docs/frontends/gui.md#the-action-button).
test('with no model ready the main part opens the popover with the chooser and runs nothing', async ({ page }) => {
  await openDemo(page, 'The shop')
  const main = page.locator('.ws-action > button.main')
  await expect(main).toHaveText(/^▸ Build$/)
  await main.click()
  const pop = page.locator('.ws-pop')
  await expect(pop.locator('.msrc')).toBeVisible()
  await expect(pop.locator('.msrc-option', { hasText: 'Your endpoint' })).toBeVisible()
  await expectNoPopoverButtons(pop)
  expect((await api<{ jobs: unknown[] }>(page, 'GET', '/api/jobs')).jobs).toEqual([])
  await clickElsewhere(page)
  await expect(pop).toHaveCount(0)
})


// A build in a tab (docs/frontends/wasm.md#the-demo): the page keeps the job, the
// action button shows the run's progress, and the chat column opens the build
// thread. The endpoint answers every model call with an error, so each session
// fails fast and the build ends on its own.
test('a build in a tab shows its progress on the action button and opens its build thread', async ({ page }) => {
  const errors = pageErrors(page)
  const ENDPOINT = 'http://localhost:4999/v1'
  const CORS = {
    'Access-Control-Allow-Origin': '*',
    'Access-Control-Allow-Headers': '*',
    'Access-Control-Allow-Methods': 'POST, OPTIONS',
    'Cross-Origin-Resource-Policy': 'cross-origin',
  }
  let calls = 0
  // The model calls wait until the test has watched the run, so the build is
  // certainly still running while the main part is clicked again.
  let release = () => {}
  const released = new Promise<void>((r) => (release = r))
  await page.route(`${ENDPOINT}/chat/completions`, async (route: Route) => {
    if (route.request().method() === 'OPTIONS') return route.fulfill({ status: 204, headers: CORS })
    calls += 1
    await released
    await new Promise((r) => setTimeout(r, 600))
    return route.fulfill({
      status: 400,
      headers: { ...CORS, 'Content-Type': 'application/json' },
      body: JSON.stringify({ error: { message: 'the test model answers nothing' } }),
    })
  })

  await openDemo(page, 'The shop')
  const chat = page.locator('.wb-chat')
  await chat.locator('.msrc-option', { hasText: 'Your endpoint' }).click()
  await chat.getByLabel('Base URL', { exact: true }).fill(ENDPOINT)
  await chat.getByLabel('Model', { exact: true }).fill('test-model')
  await chat.getByRole('button', { name: 'Save' }).click()
  await expect.poll(async () => (await api<{ jobs: string[] }>(page, 'GET', '/api/capabilities')).jobs).toEqual(['compile'])

  // A changed document: the main part reads Build with the count.
  await page.goto('/#/docs/docs/catalog.md')
  await typeAtEnd(page, '\nThe shop also sells gift cards.')
  await expect(page.locator('.ide-topbar-right')).toContainText('saved', { timeout: 5000 })
  const main = page.locator('.ws-action > button.main')
  await expect(main).toHaveText(/Build · 1 document changed/)

  // The column is collapsed; the job opens it.
  await chat.locator('.chat-toggle', { hasText: '›' }).click()
  await expect(page.locator('.wb-chat-closed')).toBeVisible()

  // The main part runs at once: no popover, the build starts.
  await main.click()
  await expect(main).toHaveText(/Building/)
  await expect(main).toHaveClass(/running/)
  await expect(page.locator('.ws-pop')).toHaveCount(0)
  const open = page.locator('.wb-chat')
  await expect(open.locator('.chat-sess.active')).toContainText('Build #1')
  await expect(open.locator('.chat-sess.working')).toHaveCount(1)
  await expect(open.locator('.chat-row.working')).toBeVisible()
  await expect.poll(() => calls).toBeGreaterThan(0)

  // While it runs a click opens its build thread again, and starts nothing.
  await open.locator('.chat-toggle', { hasText: '›' }).click()
  await expect(page.locator('.wb-chat-closed')).toBeVisible()
  await main.click()
  await expect(page.locator('.wb-chat .chat-sess.active')).toContainText('Build #1')
  await expect(page.locator('.ws-pop')).toHaveCount(0)
  release()

  // The run ends on its own: the button reads the state again, the thread keeps
  // the run with its verdict.
  await expect(main).not.toHaveText(/Building/, { timeout: 45_000 })
  await expect(open.locator('.chat-row.working')).toHaveCount(0)
  await expect(open.locator('.chat-row.marker').last()).toContainText(/Build incomplete|Up to date|■|✗/)
  const jobs = await api<{ jobs: { id: number; state: string; kind: { kind: string } }[] }>(page, 'GET', '/api/jobs')
  expect(jobs.jobs).toHaveLength(1)
  expect(jobs.jobs[0]).toMatchObject({ id: 1, kind: { kind: 'compile' } })
  expect(['finished', 'failed']).toContain(jobs.jobs[0].state)
  const job = await api<{ events: { n: number }[] }>(page, 'GET', '/api/jobs/1')
  expect(job.events.length).toBeGreaterThan(0)
  expect(job.events[0].n).toBe(0)
  expect(errors).toEqual([])
})
