// The worker the browser backend runs in. It owns the wasm module, its in-memory
// filesystem, and the synchronous build loop. Requests arrive as messages and are
// answered on the same channel; a model call goes the other way, and the worker
// blocks on Atomics.wait until the page answers.
// See docs/frontends/wasm.md#the-worker.

import init, {
  boot,
  drain_trace,
  list_files,
  request,
  seed_file,
  set_model,
} from './jazyk.js'
import { installBridge } from './bridge.js'

const ROOT = '/project'
const encoder = new TextEncoder()

type Req =
  | { id: number; kind: 'install'; control: SharedArrayBuffer; reply: SharedArrayBuffer }
  | { id: number; kind: 'seed'; files: Record<string, string>; model: string }
  | { id: number; kind: 'request'; method: string; path: string; body?: unknown }
  | { id: number; kind: 'trace' }
  | { id: number; kind: 'files' }

let ready: Promise<void> | null = null

function post(id: number, ok: unknown, error?: string) {
  self.postMessage({ kind: 'reply', id, ok, error })
}

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
        for (const [rel, text] of Object.entries(msg.files)) {
          seed_file(`${ROOT}/${rel}`, encoder.encode(text))
        }
        set_model(msg.model)
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
      case 'trace':
        post(msg.id, JSON.parse(drain_trace()))
        break
      case 'files':
        post(msg.id, JSON.parse(list_files()))
        break
    }
  } catch (err) {
    post(msg.id, null, err instanceof Error ? err.message : String(err))
  }
}
