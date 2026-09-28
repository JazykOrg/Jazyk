// Model tiers in a tab (docs/frontends/wasm.md#model-sources): the chooser lists
// the downloaded models strongest first with their tier and the ones in the cache
// marked, arrival loads the strongest downloaded one, and a model below the sync
// tier chats but cannot sync (docs/frontends/gui.md#the-action-button).
import { expect, test, type Page } from '@playwright/test'
import { api, offline, openDemo, pageErrors } from './helpers'

const SMALL = 'Qwen2.5-0.5B-Instruct-q4f16_1-MLC'
const MID = 'Hermes-3-Llama-3.2-3B-q4f16_1-MLC'

test.beforeEach(async ({ page }) => {
  await offline(page)
})

// The cache as a test sees it: the seam the page honours in place of WebLLM's
// hasModelInCache, installed before the page runs.
async function withDownloaded(page: Page, ids: string[], chosen?: string) {
  await page.addInitScript(
    ({ ids, chosen }: { ids: string[]; chosen?: string }) => {
      ;(window as unknown as { __jazykDownloaded: string[] }).__jazykDownloaded = ids
      if (chosen) localStorage.setItem('jazyk.modelSource', JSON.stringify({ kind: 'webllm', webllmModel: chosen }))
    },
    { ids, chosen },
  )
}

// Without WebGPU the downloaded source is not offered at all, so its chooser has
// nothing to show; the test says so instead of failing.
async function needsWebGpu(page: Page) {
  const gpu = await page.evaluate(() => 'gpu' in navigator)
  test.skip(!gpu, 'the downloaded model source needs WebGPU')
}

test('the chooser lists models strongest first with their tier, marks the downloaded ones, and arrival picks the strongest downloaded', async ({ page }) => {
  await withDownloaded(page, [SMALL, MID])
  await openDemo(page, 'The sort utility')
  await needsWebGpu(page)
  await page.goto('/#/settings')
  const select = page.locator('.settings-model .msrc').getByLabel('Downloaded model')
  const options = select.locator('option')
  await expect(options.first()).toHaveText(/^Gemma 2 9B · 6.3 GB · syncs and chat$/)
  await expect(options.last()).toHaveText(/^Qwen 2.5 0.5B · 0.9 GB · questions only · downloaded$/)
  await expect(options.filter({ hasText: 'Hermes 3 (Llama 3.2 3B)' })).toHaveText(/chat, syncs rough · downloaded$/)
  await expect(options.filter({ hasText: /^Llama 3.1 8B/ })).toHaveText(/syncs and chat$/)
  // The strongest downloaded model became the choice, without asking.
  await expect(select).toHaveValue(MID)
  const stored = await page.evaluate(() => JSON.parse(localStorage.getItem('jazyk.modelSource') ?? '{}'))
  expect(stored).toMatchObject({ kind: 'webllm', webllmModel: MID })
})

test('a remembered choice that is still downloaded is kept on arrival', async ({ page }) => {
  await withDownloaded(page, [SMALL, MID], SMALL)
  await openDemo(page, 'The sort utility')
  await needsWebGpu(page)
  await page.goto('/#/settings')
  await expect(page.locator('.settings-model .msrc').getByLabel('Downloaded model')).toHaveValue(SMALL)
})

test('with nothing downloaded the default is offered and nothing loads', async ({ page }) => {
  await withDownloaded(page, [])
  const external = await offline(page)
  await openDemo(page, 'The sort utility')
  await needsWebGpu(page)
  await page.waitForTimeout(1000)
  expect(external).toEqual([])
  expect(await api<{ model: string }>(page, 'GET', '/api/capabilities')).toMatchObject({ model: 'loadable', modelTier: null })
  await page.goto('/#/settings')
  const options = page.locator('.settings-model .msrc').getByLabel('Downloaded model').locator('option')
  await expect(options.filter({ hasText: 'downloaded' })).toHaveCount(0)
})

// The browser's built-in model stands in for any reader-tier model: a Prompt API the
// test installs answers, and records what it was asked.
test('a reader model chats over the read tools alone and cannot sync', async ({ page }) => {
  const errors = pageErrors(page)
  const REPLY = 'The sort utility reads lines and prints them sorted.'
  await page.addInitScript((reply: string) => {
    const prompts: string[] = []
    const systems: string[] = []
    ;(window as unknown as { __prompts: string[]; __systems: string[] }).__prompts = prompts
    ;(window as unknown as { __prompts: string[]; __systems: string[] }).__systems = systems
    ;(globalThis as unknown as { LanguageModel: unknown }).LanguageModel = {
      availability: async () => 'available',
      create: async (opts?: { initialPrompts?: { role: string; content: string }[] }) => {
        for (const p of opts?.initialPrompts ?? []) if (p.role === 'system') systems.push(p.content)
        return {
          prompt: async (input: string) => {
            prompts.push(input)
            return reply
          },
          destroy() {},
        }
      },
    }
  }, REPLY)
  await openDemo(page, 'The sort utility')
  const chat = page.locator('.wb-chat')
  await chat.locator('.msrc-option', { hasText: "The browser's built-in model" }).click()
  await chat.getByRole('button', { name: 'Load the model' }).click()
  await expect.poll(async () => (await api<{ model: string }>(page, 'GET', '/api/capabilities')).model).toBe('in-page')
  const caps = await api<{ jobs: string[]; modelTier: string }>(page, 'GET', '/api/capabilities')
  expect(caps).toMatchObject({ jobs: [], modelTier: 'reader' })

  // The action button reads Sync alone, and its sheet shows the chooser with the
  // tier a sync needs in place of Run.
  const main = page.locator('.ws-action > button.main')
  await expect(main).toHaveText(/^▸ Sync$/)
  await main.click()
  const sheet = page.locator('.ws-sheet')
  await expect(sheet.locator('.ws-tier')).toHaveText("A sync needs a model of the sync tier (7B and up) or an endpoint; the browser's built-in model answers questions.")
  await expect(sheet.locator('.msrc')).toBeVisible()
  await expect(sheet.locator('.msrc-status.v-ok')).toContainText("Ready: the browser's built-in model, reader tier (questions only).")
  await expect(sheet.getByRole('button', { name: 'Run' })).toHaveCount(0)
  await page.keyboard.press('Escape')
  await expect(sheet).toHaveCount(0)

  // Chat runs: the conversation opened as a reader, with the read tools alone.
  const box = chat.locator('.chat-input textarea')
  await box.fill('What does it do?')
  await box.press('Enter')
  await expect(chat.locator('.chat-row.agent')).toContainText(REPLY)
  const seen = await page.evaluate(() => {
    const w = window as unknown as { __prompts: string[]; __systems: string[] }
    return { prompts: w.__prompts, systems: w.__systems }
  })
  expect(seen.prompts[0]).toContain('can only read it')
  expect(seen.prompts[0]).not.toContain('CHAT SERVING')
  expect(seen.systems[0]).toContain('- search:')
  expect(seen.systems[0]).toContain('- get_entity:')
  expect(seen.systems[0]).not.toContain('- revise_requirement:')
  expect(seen.systems[0]).not.toContain('- report_feedback:')
  expect(seen.systems[0]).not.toContain('- goals:')
  expect(errors).toEqual([])
})
