// Browser storage for projects (docs/frontends/wasm.md#browser-storage): the files of
// a project started in the browser or of an edited demo, and the changes a folder has
// not taken yet. IndexedDB, database `jazyk-projects`, object store `files`, one
// record per file, so a write stores one file and never the whole project:
//
//   file:<project>:<path>  the bytes of one file of a stored project
//   kept:<folder>:<path>   one change a folder has not taken, { seq, op, bytes, dir }
//
// Writes are coalesced: a burst of mutations (a build's shards and journal) lands as
// one transaction a moment after the last one. A write the browser refuses is kept
// queued and reported, never swallowed.
import { createStore, promisifyRequest, type UseStore } from 'idb-keyval'

// A third of a second after the last mutation, and never more than two seconds after
// the first one still pending, so a long build still lands as it goes.
const DELAY_MS = 300
const MAX_WAIT_MS = 2000

let db: UseStore | null = null
function files(): UseStore {
  return (db ??= createStore('jazyk-projects', 'files'))
}

// Everything under one key prefix. `￿` sorts after every character a path holds.
const under = (prefix: string) => IDBKeyRange.bound(prefix, prefix + '￿')

const encoder = new TextEncoder()
export const toBytes = (c: string | Uint8Array): Uint8Array => (typeof c === 'string' ? encoder.encode(c) : c)

// Whether this browser can store projects at all, probed once at boot. Some private
// windows have no IndexedDB, or one that refuses every open.
let available = false
export async function probeStorage(): Promise<boolean> {
  try {
    if (typeof indexedDB === 'undefined') return (available = false)
    await files()('readonly', (s) => promisifyRequest(s.count(under('file:'))))
    return (available = true)
  } catch {
    return (available = false)
  }
}
export function storageAvailable(): boolean {
  return available
}

// A refusal in words a person can act on.
export function describeStoreError(e: unknown): string {
  const name = (e as { name?: string })?.name
  if (name === 'QuotaExceededError') return "this browser's storage for the site is full"
  if (e instanceof Error) return e.message || name || 'the browser refused the write'
  return String(e)
}

// Asked once, when the first project is stored: persistent storage is not evicted
// under storage pressure. A refusal changes nothing; the project is stored anyway.
let persistAsked = false
export function requestPersistence() {
  if (persistAsked) return
  persistAsked = true
  void navigator.storage?.persist?.().catch(() => false)
}

const fileKey = (project: string) => `file:${project}:`
const keptKey = (folder: string) => `kept:${folder}:`

// Every file of a stored project, keyed by its path relative to the project root.
export async function readFiles(project: string): Promise<Record<string, Uint8Array>> {
  const prefix = fileKey(project)
  const [keys, values] = await files()('readonly', (s) =>
    Promise.all([promisifyRequest(s.getAllKeys(under(prefix))), promisifyRequest(s.getAll(under(prefix)))]),
  )
  const out: Record<string, Uint8Array> = {}
  keys.forEach((k, i) => {
    out[String(k).slice(prefix.length)] = values[i] as Uint8Array
  })
  return out
}

export async function hasFiles(project: string): Promise<boolean> {
  if (!available) return false
  try {
    return (await files()('readonly', (s) => promisifyRequest(s.count(under(fileKey(project)))))) > 0
  } catch {
    return false
  }
}

// A whole project in one transaction: a new project's first files.
export async function writeFiles(project: string, contents: Record<string, string | Uint8Array>): Promise<void> {
  const prefix = fileKey(project)
  await files()('readwrite', (s) => {
    for (const [path, c] of Object.entries(contents)) s.put(toBytes(c), prefix + path)
    return promisifyRequest(s.transaction)
  })
}

export async function deleteFiles(project: string): Promise<void> {
  await files()('readwrite', (s) => {
    s.delete(under(fileKey(project)))
    return promisifyRequest(s.transaction)
  })
}

// One change a folder has not taken yet (docs/frontends/wasm.md#when-a-folder-cannot-be-written).
export type Change = { op: 'write'; bytes: Uint8Array } | { op: 'remove'; dir: boolean }
export type KeptRecord = Change & { seq: number }

// A folder's kept changes, oldest first.
export async function readKept(folder: string): Promise<{ changes: [string, Change][]; seq: number }> {
  if (!available) return { changes: [], seq: 0 }
  const prefix = keptKey(folder)
  const [keys, values] = await files()('readonly', (s) =>
    Promise.all([promisifyRequest(s.getAllKeys(under(prefix))), promisifyRequest(s.getAll(under(prefix)))]),
  )
  const rows = keys.map((k, i) => [String(k).slice(prefix.length), values[i] as KeptRecord] as const)
  rows.sort((a, b) => a[1].seq - b[1].seq)
  const changes = rows.map(([path, r]): [string, Change] => [path, r.op === 'write' ? { op: 'write', bytes: r.bytes } : { op: 'remove', dir: r.dir }])
  return { changes, seq: rows.length ? rows[rows.length - 1][1].seq : 0 }
}

// The writer a folder's mirror keeps its changes through.
export function keptWriter(folder: string): StoreWriter | null {
  return available ? new StoreWriter(keptKey(folder)) : null
}

export async function deleteKept(folder: string): Promise<void> {
  if (!available) return
  await files()('readwrite', (s) => {
    s.delete(under(keptKey(folder)))
    return promisifyRequest(s.transaction)
  })
}

type Op = { put: unknown } | { del: 'file' | 'tree' }

// The coalescing writer under one key prefix. Pending operations are the latest per
// path in the order the paths were last touched, which replays a removed directory
// and a later write under it in the order they happened.
export class StoreWriter {
  private pending = new Map<string, Op>()
  private timer: number | null = null
  private firstAt = 0
  private running: Promise<void> = Promise.resolve()
  private closed = false
  // The last refusal, until a later store succeeds.
  error: string | null = null
  // Called when the error or the pending count after a failure changes.
  onState?: () => void

  constructor(private prefix: string) {}

  get pendingCount(): number {
    return this.pending.size
  }

  put(path: string, value: unknown) {
    this.touch(path, { put: value })
  }

  del(path: string) {
    this.touch(path, { del: 'file' })
  }

  // The path and everything under it.
  delTree(path: string) {
    this.touch(path, { del: 'tree' })
  }

  // Nothing more is stored: the project was deleted or left for a folder.
  close() {
    this.closed = true
    this.pending.clear()
    if (this.timer !== null) window.clearTimeout(this.timer)
    this.timer = null
  }

  private touch(path: string, op: Op) {
    if (this.closed || !available) return
    this.pending.delete(path)
    this.pending.set(path, op)
    const now = Date.now()
    if (this.timer === null) this.firstAt = now
    else window.clearTimeout(this.timer)
    const wait = Math.max(0, Math.min(DELAY_MS, this.firstAt + MAX_WAIT_MS - now))
    this.timer = window.setTimeout(() => {
      this.timer = null
      void this.flush()
    }, wait)
  }

  // Stores what is pending now. Resolves when it landed or was refused.
  flush(): Promise<void> {
    if (this.timer !== null) window.clearTimeout(this.timer)
    this.timer = null
    this.running = this.running.then(() => this.store())
    return this.running
  }

  private async store() {
    if (this.pending.size === 0 || this.closed) return
    const batch = [...this.pending]
    this.pending.clear()
    try {
      await files()('readwrite', (s) => {
        for (const [path, op] of batch) {
          const key = this.prefix + path
          if ('put' in op) {
            s.put(op.put, key)
          } else {
            s.delete(key)
            if (op.del === 'tree') s.delete(under(key + '/'))
          }
        }
        return promisifyRequest(s.transaction)
      })
      if (this.error !== null) {
        this.error = null
        this.onState?.()
      }
    } catch (e) {
      // Kept, in order, ahead of anything that arrived while the write ran; the
      // next store tries them again.
      const newer = this.pending
      this.pending = new Map(batch.filter(([p]) => !newer.has(p)))
      for (const [p, op] of newer) this.pending.set(p, op)
      this.error = describeStoreError(e)
      console.error('[jazyk storage] a write was refused:', e)
      this.onState?.()
    }
  }
}

// What the worker posts, applied somewhere: a folder, browser storage, or a buffer.
export interface FsSink {
  write(path: string, bytes: Uint8Array): void
  remove(path: string, dir: boolean): void
}

// The path under the project root the backend uses, or null for anything outside it.
export function relPath(path: string, root = '/project'): string | null {
  if (!path.startsWith(root + '/')) return null
  const rel = path.slice(root.length + 1).split('/').filter(Boolean).join('/')
  return rel || null
}

// A project stored in this browser: a browser project, or a demo's copy. A demo that
// is not edited yet carries its fixture as the baseline, stored with the first
// mutation, so an unedited demo stores nothing.
export class StoredProject implements FsSink {
  readonly writer: StoreWriter
  // Called once, on the first mutation of a project that had a baseline.
  onFirstChange?: () => void

  constructor(
    readonly id: string,
    private baseline: Record<string, string | Uint8Array> | null = null,
  ) {
    this.writer = new StoreWriter(fileKey(id))
  }

  get edited(): boolean {
    return this.baseline === null
  }

  private begin() {
    if (!this.baseline) return
    const b = this.baseline
    this.baseline = null
    for (const [path, c] of Object.entries(b)) this.writer.put(path, toBytes(c))
    requestPersistence()
    this.onFirstChange?.()
  }

  write(path: string, bytes: Uint8Array) {
    const rel = relPath(path)
    if (!rel) return
    this.begin()
    this.writer.put(rel, bytes)
  }

  remove(path: string, dir: boolean) {
    const rel = relPath(path)
    if (!rel) return
    this.begin()
    if (dir) this.writer.delTree(rel)
    else this.writer.del(rel)
  }
}

// Holds mutations while the project moves from one place to another, then replays
// them into the new place in order.
export class BufferSink implements FsSink {
  private ops: ({ path: string } & ({ op: 'write'; bytes: Uint8Array } | { op: 'remove'; dir: boolean }))[] = []
  write(path: string, bytes: Uint8Array) {
    this.ops.push({ path, op: 'write', bytes })
  }
  remove(path: string, dir: boolean) {
    this.ops.push({ path, op: 'remove', dir })
  }
  replay(into: FsSink) {
    for (const o of this.ops) {
      if (o.op === 'write') into.write(o.path, o.bytes)
      else into.remove(o.path, o.dir)
    }
    this.ops = []
  }
}

// Lays a folder's kept changes over what was read from it, in order.
export function overlay(base: Record<string, Uint8Array>, changes: [string, Change][]): Record<string, Uint8Array> {
  const out = { ...base }
  for (const [path, c] of changes) {
    if (c.op === 'write') {
      out[path] = c.bytes
      continue
    }
    delete out[path]
    if (c.dir) for (const k of Object.keys(out)) if (k.startsWith(path + '/')) delete out[k]
  }
  return out
}
