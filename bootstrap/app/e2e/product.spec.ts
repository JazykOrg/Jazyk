// The Product lens's overview (docs/frontends/gui.md#overview): every demo shows its
// product itself. The novel reads as one document with its print action, the deck
// renders its slides from the `.pptx` the fixture carries and downloads it whole,
// the sort utility shows a README and its test summary, and a project with no
// product says so.
import { expect, test } from '@playwright/test'
import { readFileSync, statSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { offline, openDemo, pageErrors } from './helpers'

const demos = join(dirname(fileURLToPath(import.meta.url)), '..', 'public', 'demos')
const DECK = 'deliverable/dist/jazyk-slides.pptx'

test('the novel reads as one document, first chapter first, with its print action', async ({ page }) => {
  const errors = pageErrors(page)
  await offline(page)
  await openDemo(page, 'The novel')
  await page.goto('/#/product')
  const first = page.locator('.product-doc .product-part').first()
  await expect(first).toHaveAttribute('data-path', '02-the-survey.md')
  await expect(first.locator('h1')).toHaveText('The survey')
  expect(await page.locator('.product-doc .product-part').count()).toBeGreaterThan(5)
  // The print copy exists while the browser prints, and holds the document alone.
  await page.evaluate(() => {
    const w = window as unknown as { printed?: string; print: () => void }
    w.print = () => {
      w.printed = document.querySelector('body > .print-root h1')?.textContent ?? ''
    }
  })
  await page.getByRole('button', { name: 'Print / Save as PDF' }).click()
  expect(await page.evaluate(() => (window as unknown as { printed?: string }).printed)).toBe('The survey')
  expect(errors).toEqual([])
})

test('the deck renders its slides from the .pptx and downloads it whole', async ({ page }) => {
  const errors = pageErrors(page)
  const external = await offline(page)
  await openDemo(page, 'The slide deck')
  await page.goto('/#/product')
  await expect(page.locator('.product-artifact .mono', { hasText: 'dist/jazyk-slides.pptx' })).toBeVisible()
  const slides = page.locator('.product-slides')
  await expect(slides).toContainText('About Jazyk', { timeout: 30_000 })
  expect(Number(await slides.getAttribute('data-rendered'))).toBeGreaterThanOrEqual(1)
  await expect(page.locator('.product-deck')).toContainText('2 slides')

  const fixture = JSON.parse(readFileSync(join(demos, 'slides.json'), 'utf8')) as Record<string, string | { base64: string }>
  const packed = fixture[DECK] as { base64: string }
  const size = Buffer.from(packed.base64, 'base64').length
  const [download] = await Promise.all([page.waitForEvent('download'), page.getByRole('button', { name: 'Download' }).click()])
  expect(download.suggestedFilename()).toBe('jazyk-slides.pptx')
  const saved = await download.path()
  expect(statSync(saved).size).toBe(size)
  expect(readFileSync(saved).equals(Buffer.from(packed.base64, 'base64'))).toBe(true)
  // The renderer is part of the page: nothing is fetched from elsewhere.
  expect(external).toEqual([])
  expect(errors).toEqual([])
})

test('the sort utility shows a README, how to run it, and its test summary', async ({ page }) => {
  await openDemo(page, 'The sort utility')
  await page.goto('/#/product')
  await expect(page.getByRole('heading', { name: 'How to run it' })).toBeVisible()
  await expect(page.locator('.card', { hasText: 'node --test' }).first()).toBeVisible()
  // The deliverable has no README, so the root document stands in and says so.
  await expect(page.locator('.product-readme h1')).toHaveText('Main.ts')
  await expect(page.getByText('The product has no README')).toBeVisible()
  const tests = page.locator('.product-tests')
  await expect(tests).toContainText(/\d+ verified/)
  await tests.getByRole('link', { name: /open the/ }).click()
  await expect(page).toHaveURL(/#\/product\/trace/)
})

test('a program without a README and a project without a product each say what they show', async ({ page }) => {
  await openDemo(page, 'The ERP')
  await page.goto('/#/product')
  await expect(page.getByRole('heading', { name: 'How to run it' })).toBeVisible()
  await expect(page.locator('.product-readme')).toBeVisible()
  await openDemo(page, 'The shop')
  await page.goto('/#/product')
  await expect(page.locator('.empty-state', { hasText: 'No product yet' })).toBeVisible()
  await expect(page.locator('.empty-state').getByRole('link', { name: 'Generate' })).toBeVisible()
})
