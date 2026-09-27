// The chooser's table names models WebLLM's prebuilt config carries, with the memory
// it states (docs/frontends/wasm.md#model-sources).
import { describe, expect, it } from 'vitest'
import { DEFAULT_MODEL, MODEL_CHOICES, modelName } from './webllm'

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
})
