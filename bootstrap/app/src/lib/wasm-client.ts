// The page's half of the browser backend: it starts the worker, seeds it with a
// project, answers its model calls with the model the page loaded, and presents
// the whole thing as the Backend the API client talks to. Until a project is open
// it answers the capability descriptor and the project list itself and refuses
// everything else with a 501, which is what Home renders the picker against.
// See docs/frontends/wasm.md#the-worker and gui.md#backends-and-capabilities.

import type { Backend, BackendReply } from './backend'
import type { Capabilities } from './capabilities'
import type { FolderMirror } from './folder'

// The shared control block the worker blocks on: [0] state, [1] reply length.
const CONTROL_INTS = 2
// One megabyte holds any completion this demo produces.
const REPLY_BYTES = 1 << 20

export interface ModelHost {
  // Answers one OpenAI-shaped request body with one response body.
  chat(body: string): Promise<string>
  name(): string
  // Whether a call can be answered at all right now.
  available(): boolean
  // Whether the page can load a model on request (docs/frontends/wasm.md#the-demo).
  loadable?(): boolean
}

export interface WasmProject {
  // The project, one entry per file: text for the bundled demo, bytes for a folder
  // read off disk.
  files: Record<string, string | Uint8Array>
  name: string
  // When set, every mutation the backend makes is mirrored into this folder.
  mirror?: FolderMirror
}

export interface WasmBackendOptions {
  model: ModelHost
  // Called with each trace event as the build emits it.
  onTrace?: (events: unknown[]) => void
  // Called after every write request returns: the app refetches, since there is no
  // event stream to say what moved.
  onWrite?: () => void
}

export function crossOriginIsolated_(): boolean {
  return typeof SharedArrayBuffer !== 'undefined' && self.crossOriginIsolated === true
}

export interface WasmBackend extends Backend {
  open(project: WasmProject): Promise<void>
  capabilities(): Capabilities
}

function reply(status: number, body: unknown): BackendReply {
  return { status, body, text: typeof body === 'string' ? body : JSON.stringify(body) }
}

export async function startWasmBackend(opts: WasmBackendOptions): Promise<WasmBackend> {
  const worker = new Worker(new URL('../wasm-backend/worker.ts', import.meta.url), {
    type: 'module',
  })

  let nextId = 1
  const pending = new Map<number, { resolve: (v: unknown) => void; reject: (e: Error) => void }>()
  let project: WasmProject | null = null

  // The buffers exist only when the page is cross-origin isolated. Without them the
  // backend still serves every read; only a build needs the model.
  const isolated = crossOriginIsolated_()
  const control = isolated ? new SharedArrayBuffer(CONTROL_INTS * 4) : null
  const replyBuf = isolated ? new SharedArrayBuffer(REPLY_BYTES) : null
  const controlView = control ? new Int32Array(control) : null
  const replyView = replyBuf ? new Uint8Array(replyBuf) : null
  const encoder = new TextEncoder()

  // Write one reply into the shared buffer and wake the worker.
  function answer(payload: { body?: string; error?: string }) {
    if (!controlView || !replyView) return
    const bytes = encoder.encode(JSON.stringify(payload))
    const n = Math.min(bytes.length, replyView.length)
    replyView.set(bytes.subarray(0, n))
    Atomics.store(controlView, 1, n)
    Atomics.store(controlView, 0, 1)
    Atomics.notify(controlView, 0)
  }

  worker.onmessage = (e: MessageEvent) => {
    const msg = e.data
    if (msg?.kind === 'fs') {
      if (msg.op === 'write') project?.mirror?.write(msg.path, msg.bytes)
      else if (msg.op === 'remove') project?.mirror?.remove(msg.path, !!msg.dir)
      return
    }
    if (msg?.kind === 'log') {
      console.error('[jazyk backend]', msg.text)
      return
    }
    if (msg?.kind === 'trace') {
      // Emitted from inside a running build, one event at a time.
      opts.onTrace?.([JSON.parse(msg.event)])
      return
    }
    if (msg?.kind === 'llm') {
      // The worker is blocked until this answers, so every path must answer.
      opts.model
        .chat(msg.body)
        .then((body) => answer({ body }))
        .catch((err) => answer({ error: err instanceof Error ? err.message : String(err) }))
      return
    }
    if (msg?.kind === 'reply') {
      const p = pending.get(msg.id)
      if (!p) return
      pending.delete(msg.id)
      if (msg.error) p.reject(new Error(msg.error))
      else p.resolve(msg.ok)
    }
  }

  function send<T>(payload: Record<string, unknown>): Promise<T> {
    const id = nextId++
    return new Promise<T>((resolve, reject) => {
      pending.set(id, { resolve: resolve as (v: unknown) => void, reject })
      worker.postMessage({ ...payload, id })
    })
  }

  if (control && replyBuf) {
    await send({ kind: 'install', control, reply: replyBuf })
  }

  // What this page can do, read fresh on every ask: the model finishes loading
  // behind the app, and opening a folder changes the persistence.
  const capabilities = (): Capabilities => ({
    backend: project?.mirror ? 'wasm' : 'demo',
    projects: 'bundled',
    docs: 'write',
    events: 'pushed',
    lsp: false,
    jobs: opts.model.available() ? ['compile'] : [],
    agent: 'none',
    model: opts.model.available() ? 'in-page' : opts.model.loadable?.() ? 'loadable' : 'none',
    rendering: 'none',
    persistence: project?.mirror ? 'browser' : 'memory',
  })

  const request = async (method: string, path: string, body?: unknown): Promise<BackendReply> => {
    const bare = path.split('?')[0]
    if (bare === '/api/capabilities') return reply(200, capabilities())
    if (bare === '/api/projects') {
      return reply(200, {
        projects: project ? [{ root: '/project', name: project.name, current: true }] : [],
      })
    }
    if (!project) return reply(501, { error: 'no project is open in this tab yet' })
    const out = await send<{ status: number; body: unknown }>({ kind: 'request', method, path, body })
    if (method !== 'GET') opts.onWrite?.()
    return reply(out.status, out.body)
  }

  return {
    kind: 'wasm',
    request,
    capabilities,
    async open(p: WasmProject) {
      project = p
      await send({ kind: 'seed', files: p.files, model: opts.model.name(), mirror: !!p.mirror })
    },
  }
}
