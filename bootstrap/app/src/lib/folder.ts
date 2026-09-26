// A folder on the computer as the project, through the File System Access API.
// The page reads the folder once into the backend's in-memory tree, and mirrors
// every write the backend makes back into the folder, or keeps it in read-only
// mode until asked to write. Reads never touch the disk again, so an edit made
// outside the tab is not seen until the folder is reopened.
// See docs/frontends/wasm.md#a-folder-on-disk.
import type { FolderMode } from './recent'
import { relPath, type Change, type FsSink, type StoreWriter } from './browser-store'

// The names the native doc walk skips too, plus what no project needs read.
const SKIP = new Set(['.git', 'node_modules', 'target', '.DS_Store'])

export type DirHandle = FileSystemDirectoryHandle & {
  entries(): AsyncIterableIterator<[string, FileSystemHandle]>
  removeEntry(name: string, opts?: { recursive?: boolean }): Promise<void>
}

export function folderPickerAvailable(): boolean {
  return typeof (window as unknown as { showDirectoryPicker?: unknown }).showDirectoryPicker === 'function'
}

const access = (mode: FolderMode) => (mode === 'write' ? 'readwrite' : 'read')

// Read-only asks the browser for read access only, so its prompt says so.
export async function pickFolder(mode: FolderMode): Promise<DirHandle> {
  const pick = (window as unknown as {
    showDirectoryPicker: (o: { mode: 'read' | 'readwrite'; id?: string }) => Promise<DirHandle>
  }).showDirectoryPicker
  return pick({ mode: access(mode), id: 'jazyk-project' })
}

// A grant does not outlive the tab: a remembered folder asks again, and switching
// to writing asks for write access. Answers whether the browser granted it.
export async function ensurePermission(handle: FileSystemDirectoryHandle, mode: FolderMode): Promise<boolean> {
  const h = handle as unknown as {
    queryPermission(o: { mode: string }): Promise<PermissionState>
    requestPermission(o: { mode: string }): Promise<PermissionState>
  }
  const o = { mode: access(mode) }
  if ((await h.queryPermission(o)) === 'granted') return true
  return (await h.requestPermission(o)) === 'granted'
}

// Every file under the folder, keyed by its slash path, as bytes.
export async function readFolder(root: DirHandle): Promise<Record<string, Uint8Array>> {
  const out: Record<string, Uint8Array> = {}
  async function walk(dir: DirHandle, prefix: string) {
    for await (const [name, handle] of dir.entries()) {
      if (SKIP.has(name)) continue
      const rel = prefix ? `${prefix}/${name}` : name
      if (handle.kind === 'directory') {
        await walk(handle as DirHandle, rel)
      } else {
        const file = await (handle as FileSystemFileHandle).getFile()
        out[rel] = new Uint8Array(await file.arrayBuffer())
      }
    }
  }
  await walk(root, '')
  return out
}

// Whether a folder holds anything a save would write among.
export async function folderIsEmpty(root: DirHandle): Promise<boolean> {
  for await (const [name] of root.entries()) if (!SKIP.has(name)) return false
  return true
}

const notFound = (e: unknown) => (e as { name?: string })?.name === 'NotFoundError'

// A failed write in words a person can act on.
export function describeFolderError(e: unknown): string {
  const name = (e as { name?: string })?.name
  if (name === 'NotAllowedError' || name === 'SecurityError') return 'the browser no longer grants write access'
  if (name === 'NotFoundError') return 'the folder is no longer where it was'
  if (name === 'NoModificationAllowedError' || name === 'InvalidStateError') return 'the file is locked or was changed underneath'
  if (e instanceof Error) return e.message || name || 'the write failed'
  return String(e)
}

async function dirAt(root: DirHandle, segments: string[], create: boolean): Promise<DirHandle> {
  let d: DirHandle = root
  for (const s of segments) d = (await d.getDirectoryHandle(s, { create })) as DirHandle
  return d
}

// One change, landed in the folder. Throws when it does not land; a remove of
// something already gone has landed.
async function land(root: DirHandle, rel: string, c: Change) {
  const segs = rel.split('/')
  const name = segs.pop()!
  if (c.op === 'write') {
    const d = await dirAt(root, segs, true)
    const w = await (await d.getFileHandle(name, { create: true })).createWritable()
    // A fresh ArrayBuffer: the bytes crossed from the worker as a plain copy, and the
    // writable's type wants one it can own.
    await w.write(c.bytes.slice().buffer as ArrayBuffer)
    await w.close()
    return
  }
  try {
    await (await dirAt(root, segs, false)).removeEntry(name, { recursive: c.dir })
  } catch (e) {
    if (!notFound(e)) throw e
  }
}

// A whole tree written into a folder, file by file: a project in the browser saved
// to disk (docs/frontends/wasm.md#a-project-in-the-browser). Stops at the first
// failure.
export async function writeTree(root: DirHandle, files: Record<string, Uint8Array>): Promise<void> {
  for (const [rel, bytes] of Object.entries(files)) await land(root, rel, { op: 'write', bytes })
}

// The mirror: applies the backend's mutations to the folder, one after another in
// the order they arrived, so a write and the remove that follows it never race.
//
// What the folder has not taken is kept: in read-only mode every mutation, and in
// writing mode a mutation that failed to land and every one after it, so the folder
// never takes a later change before an earlier one. Kept changes are the latest per
// path, in the order the paths were last touched, which replays a remove and a later
// write under the same directory in the order they happened. They go to browser
// storage as they are kept, so a reload does not lose them
// (docs/frontends/wasm.md#when-a-folder-cannot-be-written).
export class FolderMirror implements FsSink {
  private queue: Promise<void> = Promise.resolve()
  private kept = new Map<string, Change>()
  private seq: number
  private writing: Promise<boolean> | null = null
  // Why the last write failed, until the kept changes land.
  error: string | null = null
  // Called whenever the kept count, the mode, or an error changes.
  onChange?: () => void

  constructor(
    readonly root: DirHandle,
    private prefix: string,
    private mode: FolderMode,
    // Where kept changes are stored; none when the browser has no storage.
    private store: StoreWriter | null = null,
    kept: { changes: [string, Change][]; seq: number } = { changes: [], seq: 0 },
  ) {
    for (const [path, c] of kept.changes) this.kept.set(path, c)
    this.seq = kept.seq
    if (store) store.onState = () => this.onChange?.()
  }

  get keptCount(): number {
    return this.kept.size
  }

  get currentMode(): FolderMode {
    return this.mode
  }

  // Why browser storage refused to keep the changes, if it did.
  get storeError(): string | null {
    return this.store?.error ?? null
  }

  write(path: string, bytes: Uint8Array) {
    const rel = relPath(path, this.prefix)
    if (rel) this.route(rel, { op: 'write', bytes })
  }

  remove(path: string, dir: boolean) {
    const rel = relPath(path, this.prefix)
    if (rel) this.route(rel, { op: 'remove', dir })
  }

  private route(rel: string, c: Change) {
    if (this.mode === 'read' || this.kept.size > 0) return this.keep(rel, c)
    this.queue = this.queue.then(async () => {
      // An earlier one failed while this one waited: it queues behind it.
      if (this.kept.size > 0) return this.keep(rel, c)
      try {
        await land(this.root, rel, c)
      } catch (e) {
        console.error('[jazyk folder] a write failed, kept in this browser:', e)
        this.error = describeFolderError(e)
        this.keep(rel, c)
      }
    })
  }

  private keep(rel: string, c: Change) {
    this.kept.delete(rel)
    this.kept.set(rel, c)
    this.store?.put(rel, { ...c, seq: ++this.seq })
    this.onChange?.()
  }

  // Read-only from here on: new mutations are kept.
  readOnly() {
    this.mode = 'read'
    this.onChange?.()
  }

  // Writing from here on: the kept mutations land first, in order, then every new
  // one is applied as it arrives. The caller has the write grant already.
  startWriting(): Promise<boolean> {
    this.mode = 'write'
    this.onChange?.()
    return this.writeKept()
  }

  // Lands the kept changes in order and stops at the first that fails again, which
  // stays kept with everything after it. Answers whether all of them landed.
  writeKept(): Promise<boolean> {
    this.writing ??= this.landKept().finally(() => {
      this.writing = null
      this.onChange?.()
    })
    return this.writing
  }

  // Whether the kept changes are being written right now.
  get busy(): boolean {
    return this.writing !== null
  }

  private async landKept(): Promise<boolean> {
    await this.queue
    this.error = null
    this.onChange?.()
    while (this.kept.size > 0) {
      const [rel, c] = this.kept.entries().next().value as [string, Change]
      try {
        await land(this.root, rel, c)
      } catch (e) {
        console.error('[jazyk folder] a kept change failed again:', e)
        this.error = describeFolderError(e)
        this.onChange?.()
        return false
      }
      // A newer change to the same path, kept while this one landed, stays kept.
      if (this.kept.get(rel) === c) {
        this.kept.delete(rel)
        this.store?.del(rel)
      }
      this.onChange?.()
    }
    await this.store?.flush()
    return true
  }

  // Forget every kept change and store nothing more. The caller deletes what was
  // stored and reopens the folder as it is on disk.
  discard() {
    this.kept.clear()
    this.error = null
    this.store?.close()
    this.onChange?.()
  }

  // Stores what is kept now, before the page goes away.
  persist(): Promise<void> {
    return this.store?.flush() ?? Promise.resolve()
  }

  // Resolves once every mutation posted so far has landed or been kept.
  flush(): Promise<void> {
    return this.queue
  }
}
