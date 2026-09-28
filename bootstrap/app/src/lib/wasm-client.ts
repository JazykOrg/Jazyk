// The page's half of the browser backend: it starts the worker, seeds it with a
// project, answers its model calls with the model the page loaded, and presents
// the whole thing as the Backend the API client talks to. Until a project is open
// it answers the capability descriptor and the project list itself and refuses
// everything else with a 501, which is what Home renders the picker against.
// See docs/frontends/wasm.md#the-worker and gui.md#backends-and-capabilities.

import type { Job, TraceEvent } from './api'
import type { Backend, BackendReply } from './backend'
import type { Capabilities } from './capabilities'
import type { FsSink } from './browser-store'

// The shared control block the worker blocks on: [0] state, [1] reply length,
// [2] the chat cancel flag (docs/frontends/wasm.md#chat).
const CONTROL_INTS = 3
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
  // The loaded model's tier (docs/frontends/wasm.md#model-sources), null while none
  // is loaded. A host that says nothing counts as sync.
  tier?(): 'sync' | 'chat' | 'reader' | null
}

// Where the open project's changes go (docs/frontends/gui.md#backends-and-capabilities).
export type Persistence = 'folder' | 'browser' | 'memory'

export interface WasmProject {
  // The project, one entry per file: text for the bundled demo, bytes for a folder
  // read off disk or a project read from browser storage.
  files: Record<string, string | Uint8Array>
  name: string
  // A bundled demo, or a project of the person's own.
  demo: boolean
  // Where every mutation the backend makes is applied: the folder's mirror, or the
  // project's browser storage.
  sink: FsSink | null
  persistence: Persistence
}

export interface WasmBackendOptions {
  model: ModelHost
  // Called with each trace event as the build emits it, and the job it belongs to.
  onTrace?: (events: unknown[], jobId: number) => void
  // Called with each event the page or the worker emits, in the event stream's
  // shape: a chat turn's progress (docs/frontends/wasm.md#chat) and a job's
  // lifecycle with its trace (docs/frontends/wasm.md#the-demo, jobs in a tab).
  onEvent?: (event: Record<string, unknown>) => void
  // Called after every write request returns: the app refetches, since there is no
  // event stream to say what moved.
  onWrite?: () => void
  // Where changes would go before any project is open: browser storage, or the tab
  // alone when the browser has none.
  persistence?: () => Persistence
}

export function crossOriginIsolated_(): boolean {
  return typeof SharedArrayBuffer !== 'undefined' && self.crossOriginIsolated === true
}

export interface WasmBackend extends Backend {
  open(project: WasmProject): Promise<void>
  // The open project moves: a stored project saved to a folder.
  moveTo(place: { sink: FsSink | null; persistence: Persistence; demo: boolean; name: string }): void
  // Nothing is open anymore: the stored project was deleted.
  close(): void
  // Every file of the tree, by its path under the project root.
  dump(): Promise<Record<string, Uint8Array>>
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
  // Where the worker's mutations go right now. It follows `project` only once the
  // worker has seeded the new tree: mutations the old project's last request posted
  // arrive before the seed's reply and still belong to the old one.
  let sink: FsSink | null = null

  // The buffers exist only when the page is cross-origin isolated. Without them the
  // backend still serves every read; only a build needs the model.
  const isolated = crossOriginIsolated_()
  const control = isolated ? new SharedArrayBuffer(CONTROL_INTS * 4) : null
  const replyBuf = isolated ? new SharedArrayBuffer(REPLY_BYTES) : null
  const controlView = control ? new Int32Array(control) : null
  const replyView = replyBuf ? new Uint8Array(replyBuf) : null
  const encoder = new TextEncoder()

  // The model call the worker is blocked on, by number. A cancel answers it at once;
  // the model's own reply, arriving later, is then dropped rather than waking the
  // worker in the middle of another call.
  let callSeq = 0
  let inflight = 0
  function settle(call: number, payload: { body?: string; error?: string }) {
    if (inflight !== call) return
    inflight = 0
    answer(payload)
  }

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

  // Jobs in a tab (docs/frontends/wasm.md#the-demo): the worker runs a build
  // synchronously, so the page keeps the list and emits the lifecycle around the
  // call. Newest first, kept for the life of the tab; a project open empties it.
  let jobs: (Job & { events: { jobId: number; n: number; event: TraceEvent }[] })[] = []
  let nextJob = 1
  let runningJob: number | null = null
  let traceN = 0
  const emit = (event: Record<string, unknown>) => opts.onEvent?.(event)
  const jobRow = (j: Job & { events: unknown[] }): Job => {
    const { events: _events, ...row } = j
    return row
  }

  worker.onmessage = (e: MessageEvent) => {
    const msg = e.data
    if (msg?.kind === 'fs') {
      if (msg.op === 'write') sink?.write(msg.path, msg.bytes)
      else if (msg.op === 'remove') sink?.remove(msg.path, !!msg.dir)
      return
    }
    if (msg?.kind === 'log') {
      console.error('[jazyk backend]', msg.text)
      return
    }
    if (msg?.kind === 'trace') {
      // Emitted from inside a running build, one event at a time, numbered per job
      // and carried as job.trace, the event the server streams.
      const event = JSON.parse(msg.event) as TraceEvent
      opts.onTrace?.([event], runningJob ?? 0)
      if (runningJob === null) return
      const n = traceN++
      jobs.find((j) => j.id === runningJob)?.events.push({ jobId: runningJob, n, event })
      emit({ type: 'job.trace', jobId: runningJob, n, event })
      return
    }
    if (msg?.kind === 'event') {
      // A chat turn's progress, posted while the prompt request is still running.
      opts.onEvent?.(JSON.parse(msg.event))
      return
    }
    if (msg?.kind === 'llm') {
      // The worker is blocked until this answers, so every path must answer.
      const call = ++callSeq
      inflight = call
      opts.model
        .chat(msg.body)
        .then((body) => settle(call, { body }))
        .catch((err) => settle(call, { error: err instanceof Error ? err.message : String(err) }))
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
  // behind the app, and the project open decides the persistence.
  const capabilities = (): Capabilities => {
    const ready = opts.model.available()
    const modelTier = ready ? (opts.model.tier?.() ?? 'sync') : null
    return {
      backend: !project || project.demo ? 'demo' : 'wasm',
      projects: 'bundled',
      docs: 'write',
      events: 'pushed',
      lsp: false,
      // A sync needs a model of the sync tier (docs/frontends/wasm.md#model-sources);
      // chat runs on any.
      jobs: modelTier === 'sync' ? ['compile'] : [],
      // The chat loop runs in the worker; `model` says whether it has a model to call
      // (docs/frontends/wasm.md#chat).
      agent: 'embedded',
      model: ready ? 'in-page' : opts.model.loadable?.() ? 'loadable' : 'none',
      modelTier,
      rendering: 'none',
      persistence: project?.persistence ?? opts.persistence?.() ?? 'memory',
    }
  }

  // The prompt request whose turn the worker is running, until it answers.
  let turn: Promise<void> | null = null

  // One job, run to its end: queued and started before the call, finished after it
  // with the verdict. The worker answers nothing else while it runs.
  const runJob = async (body: Record<string, unknown>): Promise<BackendReply> => {
    const kind = typeof body.kind === 'string' ? body.kind : 'compile'
    // A build already running takes the tab: the submission is that job, as the
    // server answers a queued compile's id.
    const running = jobs.find((j) => j.state === 'running')
    if (running) return reply(200, { id: running.id })
    const id = nextJob++
    const job = {
      id,
      kind: {
        kind,
        ...(Array.isArray(body.entities) ? { entities: body.entities as string[] } : {}),
        ...(Array.isArray(body.targets) ? { targets: body.targets as string[] } : {}),
      },
      state: 'queued',
      queuedAt: new Date().toISOString(),
      startedAt: null as string | null,
      finishedAt: null as string | null,
      result: null as Record<string, unknown> | null,
      events: [] as { jobId: number; n: number; event: TraceEvent }[],
    }
    jobs = [job, ...jobs]
    emit({ type: 'job.queued', job: jobRow(job) })
    job.state = 'running'
    job.startedAt = new Date().toISOString()
    runningJob = id
    traceN = 0
    emit({ type: 'job.started', jobId: id })
    let status = 500
    let answered: unknown = null
    try {
      ;({ status, body: answered } = await send<{ status: number; body: unknown }>({ kind: 'request', method: 'POST', path: '/api/jobs', body }))
    } catch (e) {
      answered = { error: e instanceof Error ? e.message : String(e) }
    } finally {
      runningJob = null
    }
    const out = (answered && typeof answered === 'object' ? answered : {}) as Record<string, unknown>
    job.finishedAt = new Date().toISOString()
    job.state = status < 400 ? 'finished' : 'failed'
    job.result = status < 400 ? ((out.report as Record<string, unknown> | undefined) ?? out) : { error: out.error ?? 'the job failed' }
    emit({ type: 'job.finished', jobId: id, state: job.state, result: job.result })
    opts.onWrite?.()
    return reply(status, status < 400 ? { ...out, id } : out)
  }

  const request = async (method: string, path: string, body?: unknown): Promise<BackendReply> => {
    const bare = path.split('?')[0]
    if (bare === '/api/capabilities') return reply(200, capabilities())
    if (bare === '/api/projects') {
      return reply(200, {
        projects: project ? [{ root: '/project', name: project.name, current: true }] : [],
      })
    }
    if (!project) return reply(501, { error: 'no project is open in this tab yet' })
    // The job list is the page's (docs/frontends/wasm.md#the-demo).
    if (method === 'GET' && bare === '/api/jobs') return reply(200, { jobs: jobs.map(jobRow) })
    if (method === 'GET' && /^\/api\/jobs\/\d+$/.test(bare)) {
      const job = jobs.find((j) => j.id === Number(bare.slice('/api/jobs/'.length)))
      return job ? reply(200, { ...jobRow(job), events: job.events }) : reply(404, { error: 'no such job' })
    }
    if (method === 'POST' && bare === '/api/jobs') return runJob((body ?? {}) as Record<string, unknown>)
    // A job's cancel cannot reach the worker either: failing the model call in
    // flight ends the session at its next boundary.
    if (method === 'POST' && /^\/api\/jobs\/\d+\/cancel$/.test(bare)) {
      if (runningJob !== null && inflight) {
        inflight = 0
        answer({ error: 'the job was cancelled' })
      }
      return reply(202, { ok: true })
    }
    // A cancel cannot reach the worker: it is inside the turn. The flag stops the loop
    // at its next check, and failing the model call in flight ends the wait now.
    if (method === 'POST' && /^\/api\/chat\/sessions\/[^/]+\/cancel$/.test(bare)) {
      if (controlView) Atomics.store(controlView, 2, 1)
      if (inflight) {
        inflight = 0
        answer({ error: 'the turn was cancelled' })
      }
      return reply(202, { ok: true })
    }
    // The worker is inside a running turn and answers nothing until it ends, so a
    // second prompt is refused here, as the worker refuses one to a running session
    // (docs/frontends/wasm.md#chat).
    const prompt = method === 'POST' && /^\/api\/chat\/sessions\/[^/]+\/prompt$/.test(bare)
    if (prompt && turn) return reply(409, { error: 'a turn is already running; cancel it or wait for it to end' })
    const out = send<{ status: number; body: unknown }>({ kind: 'request', method, path, body })
    if (prompt) {
      const mine = out.then(
        () => undefined,
        () => undefined,
      )
      turn = mine
      void mine.then(() => {
        if (turn === mine) turn = null
      })
    }
    const { status, body: answered } = await out
    if (method !== 'GET') opts.onWrite?.()
    return reply(status, answered)
  }

  return {
    kind: 'wasm',
    request,
    capabilities,
    async open(p: WasmProject) {
      project = { ...p, files: {} }
      // The jobs belonged to the project the tab had open before.
      jobs = []
      await send({ kind: 'seed', files: p.files, model: opts.model.name() })
      sink = p.sink
    },
    moveTo(place) {
      if (project) project = { ...project, ...place }
      sink = place.sink
    },
    close() {
      project = null
      sink = null
    },
    dump() {
      return send<Record<string, Uint8Array>>({ kind: 'dump' })
    },
  }
}
