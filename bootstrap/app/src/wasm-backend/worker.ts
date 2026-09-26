// The worker the browser backend runs in. It owns the wasm module, its in-memory
// filesystem, and the synchronous build loop. Requests arrive as messages and are
// answered on the same channel; a model call goes the other way, and the worker
// blocks on Atomics.wait until the page answers.
// See docs/frontends/wasm.md#the-worker.

import init, { boot, enable_fs_sink, list_files, read_file, request, reset, seed_file, set_model } from './jazyk.js'
import { installBridge } from './bridge.js'

const ROOT = '/project'
const encoder = new TextEncoder()

// A worker's console does not reach the page, and a panic in the wasm module writes
// its message there before it throws. Forward it, or every panic reads as
// "unreachable" with nothing behind it.
const realError = console.error.bind(console)
console.error = (...args: unknown[]) => {
  realError(...args)
  self.postMessage({ kind: 'log', level: 'error', text: args.map(String).join(' ') })
}

type Req =
  | { id: number; kind: 'install'; control: SharedArrayBuffer; reply: SharedArrayBuffer }
  // Files arrive as text (the bundled demo) or as bytes (a folder read off disk, a
  // stored project).
  | {
      id: number
      kind: 'seed'
      files: Record<string, string | Uint8Array>
      model: string
    }
  | { id: number; kind: 'request'; method: string; path: string; body?: unknown }
  | { id: number; kind: 'files' }
  // Every file of the tree under the root, keyed by its path relative to the root.
  | { id: number; kind: 'dump' }

let ready: Promise<void> | null = null

function post(id: number, ok: unknown, error?: string) {
  self.postMessage({ kind: 'reply', id, ok, error })
}

// Trace events reach the page through self.postMessage from inside the build, so
// they are already flowing by the time a request answers. Nothing to do here.

self.onmessage = async (e: MessageEvent<Req>) => {
  const msg = e.data
  try {
    if (msg.kind === 'install') {
      installBridge(msg.control, msg.reply)
      post(msg.id, true)
      return
    }

    if (!ready) ready = init().then(() => undefined)
    await ready

    switch (msg.kind) {
      case 'seed': {
        // A project opened over another starts from an empty tree.
        reset()
        for (const [rel, content] of Object.entries(msg.files)) {
          const bytes = typeof content === 'string' ? encoder.encode(content) : content
          seed_file(`${ROOT}/${rel}`, bytes)
        }
        set_model(msg.model)
        // Every mutation from here on reaches the page, which keeps it in the folder
        // or in browser storage (docs/frontends/wasm.md#browser-storage).
        enable_fs_sink()
        post(msg.id, JSON.parse(boot(ROOT)))
        break
      }
      case 'request': {
        // The backend answers the envelope the native server's status line and body
        // would have carried, so the page can rebuild a Response from it.
        const out = request(msg.method, msg.path, JSON.stringify(msg.body ?? null))
        post(msg.id, JSON.parse(out))
        break
      }
      case 'files':
        post(msg.id, JSON.parse(list_files()))
        break
      case 'dump': {
        const out: Record<string, Uint8Array> = {}
        for (const path of JSON.parse(list_files()) as string[]) {
          if (!path.startsWith(`${ROOT}/`)) continue
          const bytes = read_file(path)
          if (bytes) out[path.slice(ROOT.length + 1)] = bytes
        }
        post(msg.id, out)
        break
      }
    }
  } catch (err) {
    post(msg.id, null, err instanceof Error ? err.message : String(err))
  }
}
