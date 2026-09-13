// The page's half of the browser backend: it starts the worker, seeds it with the
// demo project, answers its model calls with the model the page loaded, and
// presents the whole thing as a Backend the API client can talk to.
// See docs/frontends/wasm.md#the-worker.

import type { Backend, Reply } from './backend'
import type { FolderMirror } from './folder'

// The shared control block the worker blocks on: [0] state, [1] reply length.
const CONTROL_INTS = 2
// One megabyte holds any completion this demo produces.
const REPLY_BYTES = 1 << 20

export interface ModelHost {
  // Answers one OpenAI-shaped request body with one response body.
  chat(body: string): Promise<string>
  name(): string
}

export interface WasmBackendOptions {
  // The project, one entry per file: text for the bundled demo, bytes for a folder
  // read off disk.
  files: Record<string, string | Uint8Array>
  model: ModelHost
  // Called with each trace event as the build emits it.
  onTrace?: (events: unknown[]) => void
  // When set, every mutation the backend makes is mirrored into this folder.
  mirror?: FolderMirror
}

export function crossOriginIsolated_(): boolean {
  return typeof SharedArrayBuffer !== 'undefined' && self.crossOriginIsolated === true
}

export async function startWasmBackend(opts: WasmBackendOptions): Promise<Backend> {
  const worker = new Worker(new URL('../wasm-backend/worker.ts', import.meta.url), {
    type: 'module',
  })

  let nextId = 1
  const pending = new Map<number, { resolve: (v: unknown) => void; reject: (e: Error) => void }>()

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
      if (msg.op === 'write') opts.mirror?.write(msg.path, msg.bytes)
      else if (msg.op === 'remove') opts.mirror?.remove(msg.path, !!msg.dir)
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

  function send<T>(payload: Record<string, unknown>, transfer?: unknown[]): Promise<T> {
    const id = nextId++
    return new Promise<T>((resolve, reject) => {
      pending.set(id, { resolve: resolve as (v: unknown) => void, reject })
      worker.postMessage({ ...payload, id }, (transfer ?? []) as Transferable[])
    })
  }

  if (control && replyBuf) {
    await send({ kind: 'install', control, reply: replyBuf })
  }
  await send({ kind: 'seed', files: opts.files, model: opts.model.name(), mirror: !!opts.mirror })

  return {
    kind: 'wasm',
    async request(method, path, body): Promise<Reply> {
      const out = await send<Reply>({ kind: 'request', method, path, body })
      return out
    },
  }
}
