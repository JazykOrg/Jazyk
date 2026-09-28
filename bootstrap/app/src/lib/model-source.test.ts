import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { builtInAvailability, builtInModel, builtInPresent, endpointModel, endpointReady } from './model-source'

// The module reads `jazyk.modelSource` once at import, so a test of that reads it
// fresh after arranging local storage.
async function fresh() {
  vi.resetModules()
  return import('./model-source')
}

const reply = (body: string, init: { ok?: boolean; status?: number } = {}) =>
  ({ ok: init.ok ?? true, status: init.status ?? 200, text: async () => body }) as Response

afterEach(() => {
  vi.restoreAllMocks()
  vi.unstubAllGlobals()
})

describe('endpointModel', () => {
  const request = JSON.stringify({
    model: 'whatever-the-backend-said',
    stream: true,
    stream_options: { include_usage: true },
    messages: [{ role: 'user', content: 'hi' }],
    temperature: 0.2,
  })

  function sent(fetch: ReturnType<typeof vi.fn>) {
    const [url, init] = fetch.mock.calls[0] as [string, RequestInit]
    return { url, headers: init.headers as Record<string, string>, body: JSON.parse(init.body as string) }
  }

  it('posts to /chat/completions under the base URL without its trailing slash', async () => {
    const fetch = vi.fn(async () => reply('{"ok":true}'))
    vi.stubGlobal('fetch', fetch)
    const host = endpointModel({ kind: 'endpoint', baseUrl: ' https://llm.example/v1/// ', model: ' gemma ' })
    expect(host.name()).toBe('gemma')
    expect(host.available()).toBe(true)
    expect(await host.chat(request)).toBe('{"ok":true}')
    expect(sent(fetch).url).toBe('https://llm.example/v1/chat/completions')
  })

  it('forces the chosen model and no streaming, and drops stream_options', async () => {
    const fetch = vi.fn(async () => reply('{}'))
    vi.stubGlobal('fetch', fetch)
    await endpointModel({ kind: 'endpoint', baseUrl: 'http://x', model: 'gemma' }).chat(request)
    const { body } = sent(fetch)
    expect(body).toEqual({ model: 'gemma', stream: false, messages: [{ role: 'user', content: 'hi' }], temperature: 0.2 })
    expect(body).not.toHaveProperty('stream_options')
  })

  it('sends Authorization only when a key is set', async () => {
    const fetch = vi.fn(async () => reply('{}'))
    vi.stubGlobal('fetch', fetch)
    await endpointModel({ kind: 'endpoint', baseUrl: 'http://x', model: 'm' }).chat(request)
    await endpointModel({ kind: 'endpoint', baseUrl: 'http://x', model: 'm', apiKey: '  ' }).chat(request)
    await endpointModel({ kind: 'endpoint', baseUrl: 'http://x', model: 'm', apiKey: ' sk-1 ' }).chat(request)
    const headers = fetch.mock.calls.map((c) => (c as unknown as [string, RequestInit])[1].headers as Record<string, string>)
    expect(headers[0]).toEqual({ 'Content-Type': 'application/json' })
    expect(headers[1]).toEqual({ 'Content-Type': 'application/json' })
    expect(headers[2]).toEqual({ 'Content-Type': 'application/json', Authorization: 'Bearer sk-1' })
  })

  it('reports a failed status with the start of the answer', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => reply('x'.repeat(400), { ok: false, status: 401 })))
    const host = endpointModel({ kind: 'endpoint', baseUrl: 'http://x', model: 'm' })
    await expect(host.chat(request)).rejects.toThrow(`http://x answered 401: ${'x'.repeat(300)}`)
  })

  it('names the URL and CORS when the call never reaches the endpoint', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => { throw new TypeError('Failed to fetch') }))
    const host = endpointModel({ kind: 'endpoint', baseUrl: 'http://x', model: 'm' })
    await expect(host.chat(request)).rejects.toThrow('http://x could not be reached from this page; check the URL, and that the endpoint allows this origin (CORS): Failed to fetch')
  })
})

describe('endpointReady', () => {
  it('wants the endpoint kind with a base URL and a model', () => {
    expect(endpointReady({ kind: 'endpoint', baseUrl: 'http://x', model: 'm' })).toBe(true)
    expect(endpointReady({ kind: 'endpoint', baseUrl: ' ', model: 'm' })).toBe(false)
    expect(endpointReady({ kind: 'endpoint', baseUrl: 'http://x' })).toBe(false)
    expect(endpointReady({ kind: 'webllm', baseUrl: 'http://x', model: 'm' })).toBe(false)
  })
})

describe('builtInModel', () => {
  type Prompts = { role: string; content: string }[]
  let created: { initialPrompts?: Prompts }[]
  let prompted: string[]

  beforeEach(() => {
    created = []
    prompted = []
    vi.stubGlobal('LanguageModel', {
      availability: async () => 'available',
      create: async (opts: { initialPrompts?: Prompts; monitor?: (m: EventTarget) => void }) => {
        created.push(opts)
        if (opts.monitor) {
          const m = new EventTarget()
          opts.monitor(m)
          m.dispatchEvent(Object.assign(new Event('downloadprogress'), { loaded: 0.5 }))
        }
        return {
          prompt: async (input: string) => {
            prompted.push(input)
            return 'the answer'
          },
          destroy() {},
        }
      },
    })
  })

  it('is present and available when the browser has the Prompt API', async () => {
    expect(builtInPresent()).toBe(true)
    expect(await builtInAvailability()).toBe('available')
  })

  it('is absent without it', async () => {
    vi.stubGlobal('LanguageModel', undefined)
    expect(builtInPresent()).toBe(false)
    expect(await builtInAvailability()).toBe('absent')
    await expect(builtInModel(() => {})).rejects.toThrow('this browser has no built-in model')
  })

  it('probes once, reporting download progress', async () => {
    const progress: string[] = []
    const host = await builtInModel((t) => progress.push(t))
    expect(host.name()).toBe('built-in')
    expect(created).toHaveLength(1)
    expect(progress).toEqual(['Chrome is downloading its built-in model for the first time (a few gigabytes, kept by the browser): 50%'])
  })

  it('primes a session with the merged system prompt first, then prompts the last message', async () => {
    const host = await builtInModel(() => {})
    const out = await host.chat(
      JSON.stringify({
        messages: [
          { role: 'user', content: 'first' },
          { role: 'system', content: 'S1' },
          { role: 'assistant', content: [{ type: 'text', text: 'A' }, { type: 'text', text: 'B' }] },
          { role: 'tool', content: 'tool output' },
          { role: 'system', content: 'S2' },
          { role: 'user', content: [{ type: 'text', text: 'last' }] },
        ],
      }),
    )
    expect(created[1].initialPrompts).toEqual([
      { role: 'system', content: 'S1\n\nS2' },
      { role: 'user', content: 'first' },
      { role: 'assistant', content: 'AB' },
      { role: 'user', content: 'tool output' },
    ])
    expect(prompted).toEqual(['last'])
    const parsed = JSON.parse(out)
    expect(parsed.model).toBe('built-in')
    expect(parsed.choices[0].message).toEqual({ role: 'assistant', content: 'the answer' })
    expect(parsed.choices[0].finish_reason).toBe('stop')
  })

  it('primes with no system prompt when there is none', async () => {
    const host = await builtInModel(() => {})
    await host.chat(JSON.stringify({ messages: [{ role: 'user', content: 'a' }, { role: 'user', content: 'b' }] }))
    expect(created[1].initialPrompts).toEqual([{ role: 'user', content: 'a' }])
    expect(prompted).toEqual(['b'])
  })
})

describe('the stored source', () => {
  beforeEach(() => {
    localStorage.clear()
  })

  it('falls back to webllm when nothing is stored', async () => {
    expect((await fresh()).readSource()).toEqual({ kind: 'webllm' })
  })

  it('falls back to webllm on an unknown kind or a corrupt value', async () => {
    localStorage.setItem('jazyk.modelSource', JSON.stringify({ kind: 'bogus', model: 'm' }))
    expect((await fresh()).readSource()).toEqual({ kind: 'webllm' })
    localStorage.setItem('jazyk.modelSource', '{nope')
    expect((await fresh()).readSource()).toEqual({ kind: 'webllm' })
  })

  it('reads a known kind back', async () => {
    localStorage.setItem('jazyk.modelSource', JSON.stringify({ kind: 'endpoint', baseUrl: 'http://x', model: 'm' }))
    expect((await fresh()).readSource()).toEqual({ kind: 'endpoint', baseUrl: 'http://x', model: 'm' })
  })

  it('writeSource stores the choice and tells the page', async () => {
    const { writeSource, readSource } = await fresh()
    const changed = vi.fn()
    window.addEventListener('jazyk:model-source-changed', changed)
    writeSource({ kind: 'chrome' })
    window.removeEventListener('jazyk:model-source-changed', changed)
    expect(readSource()).toEqual({ kind: 'chrome' })
    expect(JSON.parse(localStorage.getItem('jazyk.modelSource')!)).toEqual({ kind: 'chrome' })
    expect(changed).toHaveBeenCalledOnce()
  })
})

describe('the remembered load', () => {
  it('reloads the source that was loaded, and not another choice', async () => {
    localStorage.clear()
    const { rememberLoaded, wasLoaded } = await import('./model-source')
    expect(wasLoaded({ kind: 'webllm' })).toBe(false)
    rememberLoaded({ kind: 'chrome' })
    expect(wasLoaded({ kind: 'chrome' })).toBe(true)
    expect(wasLoaded({ kind: 'webllm' })).toBe(false)
    // The downloaded source remembers which model, so a new choice loads on request.
    rememberLoaded({ kind: 'webllm', webllmModel: 'Qwen2.5-3B-Instruct-q4f16_1-MLC' })
    expect(wasLoaded({ kind: 'webllm', webllmModel: 'Qwen2.5-3B-Instruct-q4f16_1-MLC' })).toBe(true)
    expect(wasLoaded({ kind: 'webllm' })).toBe(false)
  })
})

describe('the built-in model name', () => {
  it('follows the browser', async () => {
    const { builtInName } = await import('./model-source')
    expect(builtInName('Mozilla/5.0 Chrome/130.0 Safari/537.36 Edg/130.0')).toBe("Edge's built-in model (Phi-4-mini)")
    expect(builtInName('Mozilla/5.0 Chrome/130.0 Safari/537.36')).toBe("Chrome's built-in model (Gemini Nano)")
  })
})
