// Where the page's model comes from (docs/frontends/wasm.md#model-sources): a model
// downloaded from Hugging Face (WebLLM), the browser's built-in model (Chrome's
// Prompt API), or an OpenAI-compatible endpoint. The choice lives in this browser's
// local storage and nowhere else.
import { useSyncExternalStore } from 'react'
import type { ModelHost } from './wasm-client'

export type SourceKind = 'webllm' | 'chrome' | 'endpoint'

export interface ModelSource {
  kind: SourceKind
  // The endpoint's base URL (the part before /chat/completions), model, and key.
  baseUrl?: string
  model?: string
  apiKey?: string
}

const KEY = 'jazyk.modelSource'
const CHANGED = 'jazyk:model-source-changed'

function load(): ModelSource {
  try {
    const raw = localStorage.getItem(KEY)
    const s = raw ? (JSON.parse(raw) as ModelSource) : null
    if (s && ['webllm', 'chrome', 'endpoint'].includes(s.kind)) return s
  } catch {
    // not remembered
  }
  return { kind: 'webllm' }
}

let cache: ModelSource = load()

export function readSource(): ModelSource {
  return cache
}

export function writeSource(s: ModelSource) {
  cache = s
  try {
    localStorage.setItem(KEY, JSON.stringify(s))
  } catch {
    // not remembered past this page
  }
  window.dispatchEvent(new Event(CHANGED))
}

export function useModelSource(): ModelSource {
  return useSyncExternalStore(
    (notify) => {
      window.addEventListener(CHANGED, notify)
      return () => window.removeEventListener(CHANGED, notify)
    },
    () => cache,
  )
}

// The source that was loaded in this browser, so the next visit loads it again
// without asking (docs/frontends/wasm.md#the-demo). A different choice since means
// no load until asked.
const LOADED_KEY = 'jazyk.modelLoaded'

export function rememberLoaded(kind: SourceKind) {
  try {
    localStorage.setItem(LOADED_KEY, kind)
  } catch {
    // not remembered
  }
}

export function wasLoaded(kind: SourceKind): boolean {
  try {
    return localStorage.getItem(LOADED_KEY) === kind
  } catch {
    return false
  }
}

export function endpointReady(s: ModelSource): boolean {
  return s.kind === 'endpoint' && !!s.baseUrl?.trim() && !!s.model?.trim()
}

// ---- the browser's built-in model (Chrome's Prompt API) ----

type Availability = 'available' | 'downloadable' | 'downloading' | 'unavailable'

interface PromptSession {
  prompt(input: string): Promise<string>
  destroy(): void
}

interface LanguageModelApi {
  availability(): Promise<Availability>
  create(opts: {
    initialPrompts?: { role: 'system' | 'user' | 'assistant'; content: string }[]
    monitor?: (m: EventTarget) => void
  }): Promise<PromptSession>
}

function languageModel(): LanguageModelApi | null {
  const lm = (globalThis as unknown as { LanguageModel?: LanguageModelApi }).LanguageModel
  return lm && typeof lm.availability === 'function' ? lm : null
}

// What the browser says about its built-in model; `absent` when it has no Prompt API.
export async function builtInAvailability(): Promise<Availability | 'absent'> {
  const lm = languageModel()
  if (!lm) return 'absent'
  try {
    return await lm.availability()
  } catch {
    return 'unavailable'
  }
}

export function builtInPresent(): boolean {
  return languageModel() !== null
}

interface ChatMessage {
  role: string
  content: string | { type: string; text?: string }[] | null
}

function text(m: ChatMessage): string {
  if (typeof m.content === 'string') return m.content
  if (Array.isArray(m.content)) return m.content.map((p) => p.text ?? '').join('')
  return ''
}

// One OpenAI-shaped reply carrying the text the model wrote.
function completion(model: string, content: string): string {
  return JSON.stringify({
    id: `local-${Date.now()}`,
    object: 'chat.completion',
    model,
    choices: [{ index: 0, message: { role: 'assistant', content }, finish_reason: 'stop' }],
    usage: { prompt_tokens: 0, completion_tokens: 0, total_tokens: 0 },
  })
}

// The built-in model's context is small: a prompt it cannot hold fails with its own
// words, and the remedy rides along, since a retry cannot help
// (docs/frontends/wasm.md#model-sources).
function tooLarge(e: unknown): Error {
  const msg = e instanceof Error ? e.message : String(e)
  if (/too large|too long|quota|context/i.test(msg)) {
    return new Error(`${msg} (the built-in model's context cannot hold this prompt; choose Download a model or Your endpoint)`)
  }
  return e instanceof Error ? e : new Error(msg)
}

// The built-in model as a host. The Prompt API is session-shaped, so each call opens
// a session primed with the history and prompts the last message: the backend sends
// the whole conversation every turn, as it does to any endpoint.
export async function builtInModel(onProgress: (text: string) => void): Promise<ModelHost> {
  const lm = languageModel()
  if (!lm) throw new Error('this browser has no built-in model')
  // Creating the first session makes the browser download the model if it has to.
  const probe = await lm.create({
    monitor(m) {
      m.addEventListener('downloadprogress', (e) => {
        const loaded = (e as unknown as { loaded: number }).loaded
        onProgress(`Chrome is downloading its built-in model for the first time (a few gigabytes, kept by the browser): ${Math.round(loaded * 100)}%`)
      })
    },
  })
  probe.destroy()
  const name = 'built-in'
  return {
    name: () => name,
    available: () => true,
    async chat(body: string): Promise<string> {
      const req = JSON.parse(body) as { messages?: ChatMessage[] }
      const msgs = req.messages ?? []
      const last = msgs[msgs.length - 1]
      const history = msgs.slice(0, -1).map((m) => ({
        role: (m.role === 'system' || m.role === 'assistant' ? m.role : 'user') as 'system' | 'user' | 'assistant',
        content: text(m),
      }))
      // The Prompt API takes a system prompt only first.
      const system = history.filter((m) => m.role === 'system')
      const rest = history.filter((m) => m.role !== 'system')
      const initial = system.length > 0 ? [{ role: 'system' as const, content: system.map((m) => m.content).join('\n\n') }, ...rest] : rest
      let session: PromptSession
      try {
        session = await lm.create({ initialPrompts: initial })
      } catch (e) {
        throw tooLarge(e)
      }
      try {
        return completion(name, await session.prompt(last ? text(last) : ''))
      } catch (e) {
        throw tooLarge(e)
      } finally {
        session.destroy()
      }
    },
  }
}

// ---- an OpenAI-compatible endpoint ----

// The endpoint as a host: the backend's request goes out as it is, with the chosen
// model and no streaming, and the reply comes back as it is.
export function endpointModel(s: ModelSource): ModelHost {
  const base = (s.baseUrl ?? '').trim().replace(/\/+$/, '')
  const model = (s.model ?? '').trim()
  return {
    name: () => model,
    available: () => true,
    async chat(body: string): Promise<string> {
      const req = JSON.parse(body) as Record<string, unknown>
      delete req.stream_options
      const headers: Record<string, string> = { 'Content-Type': 'application/json' }
      if (s.apiKey?.trim()) headers.Authorization = `Bearer ${s.apiKey.trim()}`
      let res: Response
      try {
        res = await fetch(`${base}/chat/completions`, {
          method: 'POST',
          headers,
          body: JSON.stringify({ ...req, model, stream: false }),
        })
      } catch (e) {
        // A refused cross-origin call surfaces as a bare network error.
        throw new Error(`${base} could not be reached from this page; check the URL, and that the endpoint allows this origin (CORS): ${e instanceof Error ? e.message : String(e)}`)
      }
      const out = await res.text()
      if (!res.ok) throw new Error(`${base} answered ${res.status}: ${out.slice(0, 300)}`)
      return out
    },
  }
}
