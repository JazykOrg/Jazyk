// The chooser's table names models WebLLM's prebuilt config carries, with the memory
// it states, and carries a tier per model from its parameter count
// (docs/frontends/wasm.md#model-sources).
import { afterEach, describe, expect, it, vi } from 'vitest'
import { DEFAULT_MODEL, MODEL_CHOICES, arrivalModel, downloadedModels, modelName, tier, tierOf } from './webllm'

afterEach(() => {
  vi.doUnmock('@mlc-ai/web-llm')
  vi.resetModules()
  delete (globalThis as { __jazykDownloaded?: unknown }).__jazykDownloaded
})

describe('the model choices', () => {
  it('are prebuilt WebLLM models with their stated memory', async () => {
    const { prebuiltAppConfig } = await import('@mlc-ai/web-llm')
    const known = new Map(prebuiltAppConfig.model_list.map((r) => [r.model_id, r]))
    for (const m of MODEL_CHOICES) {
      const r = known.get(m.id)
      expect(r, m.id).toBeDefined()
      expect(Math.round(((r?.vram_required_MB ?? 0) / 1024) * 10) / 10, m.id).toBe(m.gb)
    }
    expect(MODEL_CHOICES.map((m) => m.id)).toContain(DEFAULT_MODEL)
    expect(modelName(DEFAULT_MODEL)).toBe('Hermes 3 (Llama 3.2 3B)')
    expect(modelName('x')).toBe('x')
  })

  it('are sorted strongest first: parameters descending, then the name', () => {
    for (let i = 1; i < MODEL_CHOICES.length; i++) {
      const a = MODEL_CHOICES[i - 1]
      const b = MODEL_CHOICES[i]
      expect(a.params > b.params || (a.params === b.params && a.name.localeCompare(b.name) <= 0), `${a.name} before ${b.name}`).toBe(true)
    }
    expect(MODEL_CHOICES[0].name).toBe('Gemma 2 9B')
    expect(MODEL_CHOICES[MODEL_CHOICES.length - 1].name).toBe('Qwen 2.5 0.5B')
  })
})

describe('tier', () => {
  it('reads the tier from the parameter count', () => {
    expect(tier(9.2)).toBe('sync')
    expect(tier(7)).toBe('sync')
    expect(tier(6.9)).toBe('chat')
    expect(tier(3)).toBe('chat')
    expect(tier(2.9)).toBe('reader')
    expect(tier(0.5)).toBe('reader')
  })

  it('names the tier of a choice by id, and reader for an unknown id', () => {
    expect(tierOf('gemma-2-9b-it-q4f16_1-MLC')).toBe('sync')
    expect(tierOf(DEFAULT_MODEL)).toBe('chat')
    expect(tierOf('Qwen2.5-0.5B-Instruct-q4f16_1-MLC')).toBe('reader')
    expect(tierOf('nobody')).toBe('reader')
  })
})

describe('downloadedModels', () => {
  it("asks WebLLM's cache for each choice and answers the ones it holds", async () => {
    const asked: string[] = []
    vi.doMock('@mlc-ai/web-llm', () => ({
      hasModelInCache: async (id: string) => {
        asked.push(id)
        return id === DEFAULT_MODEL || id === 'Qwen2.5-7B-Instruct-q4f16_1-MLC'
      },
    }))
    vi.stubGlobal('caches', { keys: async () => ['webllm/model', 'webllm/config'] })
    const { downloadedModels: fresh, MODEL_CHOICES: choices } = await import('./webllm')
    const have = await fresh()
    expect([...have].sort()).toEqual([DEFAULT_MODEL, 'Qwen2.5-7B-Instruct-q4f16_1-MLC'].sort())
    expect(asked.sort()).toEqual(choices.map((m) => m.id).sort())
    vi.unstubAllGlobals()
  })

  it('answers nothing where the browser has no cache, without loading the library', async () => {
    let imported = false
    vi.doMock('@mlc-ai/web-llm', () => {
      imported = true
      return { hasModelInCache: async () => true }
    })
    const { downloadedModels: fresh } = await import('./webllm')
    expect(typeof caches).toBe('undefined')
    expect(await fresh()).toEqual(new Set())
    expect(imported).toBe(false)
  })

  it('answers nothing while the browser holds no WebLLM cache, without loading the library', async () => {
    let imported = false
    vi.doMock('@mlc-ai/web-llm', () => {
      imported = true
      return { hasModelInCache: async () => true }
    })
    vi.stubGlobal('caches', { keys: async () => ['jazyk-demos'] })
    const { downloadedModels: fresh } = await import('./webllm')
    expect(await fresh()).toEqual(new Set())
    expect(imported).toBe(false)
    vi.unstubAllGlobals()
  })

  it('honours the test seam in place of the cache', async () => {
    ;(globalThis as { __jazykDownloaded?: unknown }).__jazykDownloaded = ['a', 7, 'b']
    expect(await downloadedModels()).toEqual(new Set(['a', 'b']))
  })
})

describe('arrivalModel', () => {
  const strongest = 'gemma-2-9b-it-q4f16_1-MLC'
  const small = 'Qwen2.5-0.5B-Instruct-q4f16_1-MLC'

  it('picks the strongest downloaded model', () => {
    expect(arrivalModel(undefined, new Set([small, DEFAULT_MODEL, strongest]))).toBe(strongest)
    expect(arrivalModel(undefined, new Set([small]))).toBe(small)
  })

  it('keeps the chosen model when it is still downloaded', () => {
    expect(arrivalModel(DEFAULT_MODEL, new Set([DEFAULT_MODEL, strongest]))).toBe(DEFAULT_MODEL)
  })

  it('moves off a chosen model that is not downloaded to the strongest that is', () => {
    expect(arrivalModel(DEFAULT_MODEL, new Set([small, strongest]))).toBe(strongest)
  })

  it('answers null with nothing downloaded, so the page waits to be asked', () => {
    expect(arrivalModel(undefined, new Set())).toBeNull()
    expect(arrivalModel(DEFAULT_MODEL, new Set())).toBeNull()
  })
})
