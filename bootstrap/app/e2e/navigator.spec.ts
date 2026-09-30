// The Docs lens as a code editor has it (docs/frontends/gui.md#docs): the file tree
// with its actions on the rows, the outline that follows the editor, the one Read
// or Write toggle (#editor), the links that work in Read (#markdown-preview), and
// the entity mentions underlined in a tab.
import { expect as baseExpect, test } from '@playwright/test'
import { api, offline, openDemo } from './helpers'

// The backend in the tab answers from a worker; on a loaded machine a list or a
// document takes seconds, so the waits here are generous.
const expect = baseExpect.configure({ timeout: 20_000 })

test.beforeEach(async ({ page }, info) => {
  info.setTimeout(180_000)
  await offline(page)
})

const dirRow = (page: import('@playwright/test').Page, name: string) => page.locator('.ws-nav .ft-dir', { has: page.locator('.ft-dirname', { hasText: new RegExp(`^${name}$`) }) })

test('the tree has collapsible folders, remembered per project, folders first', async ({ page }) => {
  await openDemo(page, 'The ERP')
  const nav = page.locator('.ws-nav')
  // No button row above the tree: the first row is the docs folder itself.
  await expect(nav.locator('.wb-explorer-label').first()).toHaveText('documents')
  await expect(nav.locator('.ft .ft-row').first()).toHaveClass(/ft-dir/)
  // Under docs/: the folders (pages, subsystems) before the files, each by name.
  const names = await nav.locator('.ft .ft-row[aria-level="2"] .ft-name').allTextContents()
  expect(names).toEqual(['pages', 'subsystems', 'backend.md', 'frontend.md', 'main.md'])

  const inventory = nav.locator('.ide-doc-name', { hasText: 'inventory.md' })
  await expect(inventory).toBeVisible()
  await dirRow(page, 'subsystems').click()
  await expect(inventory).toHaveCount(0)
  await expect(dirRow(page, 'subsystems')).toHaveAttribute('aria-expanded', 'false')

  // Remembered across a reload, for this project.
  await page.reload()
  await expect(nav.locator('.ide-doc-name', { hasText: 'backend.md' })).toBeVisible()
  await expect(nav.locator('.ide-doc-name', { hasText: 'inventory.md' })).toHaveCount(0)
  await dirRow(page, 'subsystems').click()
  await expect(nav.locator('.ide-doc-name', { hasText: 'inventory.md' })).toBeVisible()

  // Opening a document under a closed folder opens the folder and highlights the row.
  await dirRow(page, 'pages').click()
  await page.goto('/#/docs/docs/pages/login.md')
  await expect(nav.locator('.ft-row.active .ft-name')).toHaveText('login.md')
  await expect(dirRow(page, 'pages')).toHaveAttribute('aria-expanded', 'true')
})

test('a hover action creates a document in place, F2 renames it, and a delete asks twice', async ({ page }) => {
  await openDemo(page, 'The ERP')
  const nav = page.locator('.ws-nav')
  const docs = dirRow(page, 'docs')
  await docs.hover()
  await docs.getByRole('button', { name: 'New document in docs' }).click()
  const input = nav.locator('.ft-input')
  await expect(input).toBeFocused()
  await input.fill('release notes')
  await input.press('Enter')
  await page.waitForURL(/#\/docs\/docs\/release%20notes\.md|#\/docs\/docs\/release notes\.md/)
  await expect(nav.locator('.ft-row.active .ft-name')).toHaveText('release notes.md')
  const created = await api<{ text: string }>(page, 'GET', `/api/docs/content?path=${encodeURIComponent('docs/release notes.md')}`)
  expect(created.text).toBe('# Release notes\n')

  // F2 on the focused row renames it in place, the name alone; the rename itself
  // is the documents API's, which the backend in a tab does not serve.
  const row = nav.locator('.ft-row.active')
  await row.focus()
  await page.keyboard.press('F2')
  const rename = nav.locator('.ft-input')
  await expect(rename).toHaveValue('release notes.md')
  await rename.press('Escape')
  await expect(nav.locator('.ft-input')).toHaveCount(0)
  await expect(nav.locator('.ide-doc-name', { hasText: /^release notes\.md$/ })).toBeVisible()

  // Delete asks for a second click on the row, never a dialog. The second click
  // goes to the documents API; the backend in a tab does not serve it and the
  // tree says so under the row, the way it reports any refusal.
  const notes = nav.locator('.ft-row.active')
  await notes.hover()
  await notes.getByRole('button', { name: 'Delete release notes.md' }).click()
  await expect(nav.locator('.ide-doc-name', { hasText: /^release notes\.md$/ })).toBeVisible()
  await expect(nav.locator('.ft-err')).toHaveCount(0)
  await notes.getByRole('button', { name: /click again to confirm/ }).click()
  await expect(nav.locator('.ft-err')).toContainText('does not serve DELETE')
  await expect(nav.locator('.ide-doc-name', { hasText: /^release notes\.md$/ })).toBeVisible()
})

test('the keyboard walks the tree', async ({ page }) => {
  await openDemo(page, 'The ERP')
  await page.goto('/#/docs/docs/main.md')
  const nav = page.locator('.ws-nav')
  await nav.locator('.ft-row.active').focus()
  await page.keyboard.press('ArrowUp')
  await expect(nav.locator('.ft-row.focused .ft-name')).toHaveText('frontend.md')
  await page.keyboard.press('Enter')
  await page.waitForURL(/#\/docs\/docs\/frontend\.md/)
  // The tree settles on the opened document (its ancestors open themselves) before
  // the walk goes on, or that opening lands after the close below.
  await expect(nav.locator('.ft-row.active .ft-name')).toHaveText('frontend.md')
  // Left from a file goes to its folder; Left on an open folder closes it.
  await page.keyboard.press('ArrowLeft')
  await expect(nav.locator('.ft-row.focused .ft-name')).toHaveText('docs')
  await page.keyboard.press('ArrowLeft')
  await expect(dirRow(page, 'docs')).toHaveAttribute('aria-expanded', 'false')
  await page.keyboard.press('ArrowRight')
  await expect(dirRow(page, 'docs')).toHaveAttribute('aria-expanded', 'true')
})

test('the outline follows the cursor in Write and a click scrolls the editor', async ({ page }) => {
  await openDemo(page, 'The ERP')
  await page.goto('/#/docs/docs/backend.md')
  const outline = page.locator('.outline')
  await expect(outline.locator('.outline-row')).toHaveCount(4)
  await page.locator('.cm-content').getByText('More to come soon.').click()
  await expect(outline.locator('.outline-row.active .outline-title')).toHaveText('Sub-systems')
  await page.locator('.cm-content').getByText('It is backed by a PostgreSQL').click()
  await expect(outline.locator('.outline-row.active .outline-title')).toHaveText('Backend system')

  // Clicking a section moves the editor there; the outline follows it back.
  await outline.locator('.outline-row', { hasText: 'Database' }).click()
  await expect(outline.locator('.outline-row.active .outline-title')).toHaveText('Database')
  await expect(page.locator('.cm-content').getByText('Database', { exact: true })).toBeInViewport()
})

test('the one toggle: Write is the editor, Read the page in its place, remembered per document', async ({ page }) => {
  await openDemo(page, 'The ERP')
  await page.goto('/#/docs/docs/backend.md')
  const toggle = page.locator('.ide-mode')
  await expect(toggle.getByRole('button', { name: 'Write' })).toHaveAttribute('aria-pressed', 'true')
  await expect(page.locator('.cm-content')).toBeVisible()
  await expect(page.locator('.md-read')).toHaveCount(0)
  // No third state: no preview beside the editor, no reader button.
  await expect(page.locator('.ide-topbar').getByRole('button', { name: /preview|reader/i })).toHaveCount(0)

  await toggle.getByRole('button', { name: 'Read' }).click()
  await expect(page.locator('.md-read')).toBeVisible()
  await expect(page.locator('.cm-content')).toBeHidden()
  await expect(page.locator('.md-read h1').first()).toHaveText('Backend system')
  // The read states and the requirement markers ride along.
  await expect(page.locator('.md-read > .md-cov-covered').first()).toBeVisible()
  await expect(page.locator('.md-read .md-req').first()).toBeVisible()

  // Another document opens in its own default; coming back finds Read again.
  await page.goto('/#/docs/docs/main.md')
  await expect(toggle.getByRole('button', { name: 'Write' })).toHaveAttribute('aria-pressed', 'true')
  await page.goto('/#/docs/docs/backend.md')
  await expect(toggle.getByRole('button', { name: 'Read' })).toHaveAttribute('aria-pressed', 'true')
  await expect(page.locator('.md-read')).toBeVisible()
})

test('links work in Read: a document opens in Read, a missing one says so, an anchor stays', async ({ page }) => {
  await openDemo(page, 'The ERP')
  await page.goto('/#/docs/docs/backend.md')
  await page.locator('.ide-mode').getByRole('button', { name: 'Read' }).click()
  const read = page.locator('.md-read')
  // The link the ERP writes to a document it does not hold.
  const dead = read.locator('.md-dead', { hasText: 'database documentation' })
  await expect(dead).toContainText('not in this project')
  await expect(dead.locator('a')).toHaveCount(0)

  // An anchor scrolls this page, nothing else.
  await read.getByRole('link', { name: 'API endpoints' }).click()
  await expect(page).toHaveURL(/#\/docs\/docs\/backend\.md$/)

  // A relative link opens the document, in Read.
  await read.getByRole('link', { name: 'frontend interface' }).click()
  await page.waitForURL(/#\/docs\/docs\/frontend\.md/)
  await expect(page.locator('.ide-mode').getByRole('button', { name: 'Read' })).toHaveAttribute('aria-pressed', 'true')
  await expect(page.locator('.md-read h1').first()).toHaveText('Warehouse interface')
})

test('the outline follows the top of the view in Read', async ({ page }) => {
  await openDemo(page, 'The ERP')
  await page.goto('/#/docs/docs/subsystems/user.md')
  await page.locator('.ide-mode').getByRole('button', { name: 'Read' }).click()
  const outline = page.locator('.outline')
  const titles = await outline.locator('.outline-title').allTextContents()
  expect(titles.length).toBeGreaterThan(2)
  await expect(outline.locator('.outline-row.active .outline-title')).toHaveText(titles[0])
  await outline.locator('.outline-row').last().click()
  await expect(outline.locator('.outline-row.active .outline-title')).toHaveText(titles[titles.length - 1])
})

test('entity mentions are underlined in Write and in Read; Cmd-click opens the entity in Write, a click in Read', async ({ page }) => {
  await openDemo(page, 'The ERP')
  await page.goto('/#/docs/docs/backend.md')
  // The editor draws the lines in view; the mention sits at the end of the document.
  await page.locator('.cm-content .cm-line').last().click()
  await page.keyboard.press('Control+End')
  const mention = page.locator('.cm-content .entity-mark', { hasText: 'Inventory Management' })
  await expect(mention).toBeVisible()
  await expect(mention).toHaveAttribute('data-entity', 'ent:inventory-management')
  // The alias too, and the longer name where two overlap.
  await expect(page.locator('.cm-content .entity-mark', { hasText: 'backend system' }).first()).toBeVisible()
  await expect(page.locator('.cm-content .entity-mark', { hasText: /^User Management$/ })).toBeVisible()
  // A plain click only places the cursor; Cmd-click opens the entity (a mention
  // outside a link: inside one, the link wins).
  const backend = page.locator('.cm-content .entity-mark', { hasText: 'backend system' }).first()
  await backend.click()
  await expect(page).not.toHaveURL(/node=/)
  await backend.click({ modifiers: ['ControlOrMeta'] })
  await expect(page).toHaveURL(/node=ent%3Abackend/)
  await expect(page.locator('.wb-inspector')).toContainText('Backend')

  await page.locator('.ide-mode').getByRole('button', { name: 'Read' }).click()
  const readMention = page.locator('.md-read .entity-mark', { hasText: 'Inventory Management' })
  await expect(readMention).toBeVisible()
  const plain = page.locator('.md-read p .entity-mark', { hasText: 'backend system' }).first()
  await expect(plain).toBeVisible()
  await page.goto('/#/docs/docs/backend.md')
  await plain.click()
  await expect(page).toHaveURL(/node=ent%3Abackend/)
})

test('the product files and the generated pages are trees of the same kind', async ({ page }) => {
  await openDemo(page, 'The ERP')
  await page.goto('/#/product/files')
  const nav = page.locator('.ws-nav')
  await expect(nav.locator('.ft .ft-dir').first()).toBeVisible()
  await expect(nav.locator('.ft .ft-file', { hasText: 'server.js' })).toBeVisible()
  await page.goto('/#/product/generated')
  await expect(nav.locator('.ft .ft-file', { hasText: 'backend.md' }).first()).toBeVisible()
  await dirRow(page, 'entities').click()
  await expect(dirRow(page, 'entities')).toHaveAttribute('aria-expanded', 'false')
})
