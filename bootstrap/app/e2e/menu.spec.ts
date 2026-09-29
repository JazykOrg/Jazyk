// The project menu as a File menu (docs/frontends/gui.md#the-project-menu): the
// projects open in this browser's tabs, `Open recent ›` and `Open demo ›` as
// submenus, `Open folder…` with its read-only choice, new and connect, then the
// project in front. With no project open it holds the ways to open one.
import { expect, test, type Page } from '@playwright/test'
import { closeMenu, createProject, offline, openDemo, openProjectMenu, openSubmenu, projectMenuItems, storedKeys } from './helpers'

test.beforeEach(async ({ page }) => {
  await offline(page)
})

const recentRows = (page: Page) => page.locator('.home .proj.recent')

test('with no project open the menu reads jazyk and holds the ways to open one', async ({ page }) => {
  await page.goto('/#/')
  await expect(page.locator('.ws-top .ws-project')).toHaveText(/jazyk/)
  expect(await projectMenuItems(page)).toEqual(['Open recent', 'Open folder…', 'Open demo', 'New project in this browser…', 'Connect to jazyk on this computer…'])
  const menu = await openProjectMenu(page)
  // Nothing opened yet: Open recent has nothing to show.
  await expect(menu.getByRole('menuitem', { name: 'Open recent' })).toHaveAttribute('aria-disabled', 'true')
  await expect(menu.getByRole('menuitemcheckbox', { name: 'read-only' })).toHaveAttribute('aria-checked', 'false')
  await expect(menu.locator('.ws-menu-where')).toHaveCount(0)
  await closeMenu(page)
})

test('opening a demo from Open demo switches the project', async ({ page }) => {
  await page.goto('/#/')
  let sub = await openSubmenu(page, 'Open demo')
  await expect(sub.getByRole('menuitem')).toHaveCount(6)
  await sub.getByRole('menuitem').filter({ hasText: 'The sort utility' }).click()
  await page.waitForURL(/#\/docs/)
  await expect(page.locator('.ws-project')).toContainText('The sort utility')

  sub = await openSubmenu(page, 'Open demo')
  await sub.getByRole('menuitem').filter({ hasText: 'The ERP' }).click()
  await expect(page.locator('.ws-project')).toContainText('The ERP')
  const menu = await openProjectMenu(page)
  const mine = menu.getByRole('menuitemcheckbox', { checked: true }).first()
  await expect(mine).toContainText('The ERP')
  await expect(mine).toContainText('demo')
  await closeMenu(page)
})

test('Open recent lists the recent entries in a submenu, newest first, with Clear recent last', async ({ page }) => {
  await openDemo(page, 'The shop')
  let sub = await openSubmenu(page, 'Open demo')
  await sub.getByRole('menuitem').filter({ hasText: 'The ERP' }).click()
  await expect(page.locator('.ws-project')).toContainText('The ERP')

  sub = await openSubmenu(page, 'Open recent')
  const rows = sub.getByRole('menuitem')
  await expect(rows).toHaveCount(3)
  await expect(rows.nth(0)).toContainText('The ERP')
  await expect(rows.nth(0).locator('.state')).toHaveText('demo')
  await expect(rows.nth(1)).toContainText('The shop')
  await expect(rows.nth(2)).toHaveText('Clear recent…')

  // A row opens its entry, as its Recent row on Home does.
  await rows.nth(1).click()
  await expect(page.locator('.ws-project')).toContainText('The shop')
})

test("another tab's project shows under the open projects, and choosing it opens it here", async ({ page, context }) => {
  await openDemo(page, 'The shop')

  const erp = await context.newPage()
  await offline(erp)
  await openDemo(erp, 'The ERP')
  const sort = await context.newPage()
  await offline(sort)
  await openDemo(sort, 'The sort utility')

  await page.bringToFront()
  let menu = await openProjectMenu(page)
  const tabs = menu.getByRole('menuitemcheckbox').filter({ hasText: 'in another tab' })
  await expect(tabs).toHaveCount(2)
  await expect(tabs.filter({ hasText: 'The ERP' })).toHaveAttribute('aria-checked', 'false')
  await expect(tabs.filter({ hasText: 'The sort utility' })).toBeVisible()
  await closeMenu(page)

  // A tab that closes leaves the list.
  await sort.close()
  menu = await openProjectMenu(page)
  await expect(tabs).toHaveCount(1)
  await expect(tabs).toContainText('The ERP')

  await tabs.click()
  await expect(page.locator('.ws-project')).toContainText('The ERP')
  menu = await openProjectMenu(page)
  // Both tabs have The ERP now: it is this tab's, checked, and listed once.
  await expect(menu.getByRole('menuitemcheckbox').filter({ hasText: 'The ERP' })).toHaveCount(1)
  await expect(menu.getByRole('menuitemcheckbox', { checked: true }).first()).toContainText('The ERP')
  await closeMenu(page)
  await erp.close()
})

test('the read-only choice under Open folder is remembered', async ({ page }) => {
  await page.goto('/#/')
  let menu = await openProjectMenu(page)
  const readOnly = menu.getByRole('menuitemcheckbox', { name: 'read-only' })
  await expect(readOnly).toHaveAttribute('aria-checked', 'false')
  await readOnly.click()
  // The choice is made in place: the menu stays open.
  await expect(readOnly).toHaveAttribute('aria-checked', 'true')
  await closeMenu(page)
  expect(await page.evaluate(() => localStorage.getItem('jazyk.folder.readOnly'))).toBe('1')
  // Home's card shows the same choice.
  await expect(page.locator('.home-readonly input')).toBeChecked()

  await page.reload()
  menu = await openProjectMenu(page)
  await expect(menu.getByRole('menuitemcheckbox', { name: 'read-only' })).toHaveAttribute('aria-checked', 'true')
  await closeMenu(page)
})

test('Clear recent empties the list, asking first when an entry holds stored data', async ({ page }) => {
  const asked: string[] = []
  page.on('dialog', (d) => {
    asked.push(d.message())
    void d.accept()
  })

  // Plain entries just drop.
  await openDemo(page, 'The shop')
  let sub = await openSubmenu(page, 'Open demo')
  await sub.getByRole('menuitem').filter({ hasText: 'The ERP' }).click()
  await expect(page.locator('.ws-project')).toContainText('The ERP')
  const menu = await openProjectMenu(page)
  await menu.getByRole('menuitem', { name: 'Close project', exact: true }).click()
  await expect(recentRows(page)).toHaveCount(2)
  sub = await openSubmenu(page, 'Open recent')
  await sub.getByRole('menuitem', { name: 'Clear recent…' }).click()
  await expect(recentRows(page)).toHaveCount(0)
  expect(asked).toEqual([])

  // A browser project holds its files: Clear recent asks, then deletes them.
  await createProject(page, 'Scratch')
  await expect.poll(() => storedKeys(page, 'file:browser:')).not.toEqual([])
  await (await openProjectMenu(page)).getByRole('menuitem', { name: 'Close project', exact: true }).click()
  await expect(recentRows(page)).toHaveCount(1)
  sub = await openSubmenu(page, 'Open recent')
  await sub.getByRole('menuitem', { name: 'Clear recent…' }).click()
  await expect(recentRows(page)).toHaveCount(0)
  expect(asked).toHaveLength(1)
  expect(asked[0]).toContain('Scratch')
  await expect.poll(() => storedKeys(page, 'file:browser:')).toEqual([])
  const after = await openProjectMenu(page)
  await expect(after.getByRole('menuitem', { name: 'Open recent' })).toHaveAttribute('aria-disabled', 'true')
  await closeMenu(page)
})

test('New project and Connect ask in a small dialog', async ({ page }) => {
  await page.goto('/#/')
  let menu = await openProjectMenu(page)
  await menu.getByRole('menuitem', { name: 'Connect to jazyk on this computer…' }).click()
  const connect = page.getByRole('dialog', { name: 'Connect to jazyk on this computer' })
  await expect(connect.getByLabel('Port')).toHaveValue('4680')
  await connect.getByRole('button', { name: 'Cancel' }).click()
  await expect(connect).toHaveCount(0)

  menu = await openProjectMenu(page)
  await menu.getByRole('menuitem', { name: 'New project in this browser…' }).click()
  const ask = page.getByRole('dialog', { name: 'New project in this browser' })
  await expect(ask.getByLabel('Project name')).toBeFocused()
  await ask.getByLabel('Project name').fill('Garden planner')
  await ask.getByRole('button', { name: 'Create' }).click()
  await page.waitForURL(/#\/docs/)
  await expect(page.locator('.ws-project')).toContainText('Garden planner')
  await expect(ask).toHaveCount(0)
})
