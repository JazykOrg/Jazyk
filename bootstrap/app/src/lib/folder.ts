// A folder on the computer as the project, through the File System Access API.
// The page reads the folder once into the backend's in-memory tree, and mirrors
// every write the backend makes back into the folder, or holds it in read-only
// mode until asked to write. Reads never touch the disk again, so an edit made
// outside the tab is not seen until the folder is reopened.
// See docs/frontends/wasm.md#a-folder-on-disk.
import type { FolderMode } from './recent'

// The names the native doc walk skips too, plus what no project needs read.
const SKIP = new Set(['.git', 'node_modules', 'target', '.DS_Store'])

type DirHandle = FileSystemDirectoryHandle & {
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

type Held = { op: 'write'; bytes: Uint8Array } | { op: 'remove'; dir: boolean }

// The mirror: applies the backend's mutations to the folder, one after another in
// the order they arrived, so a write and the remove that follows it never race. In
// read-only mode it holds them instead: the latest per path, in the order the paths
// were last touched, which replays a remove and a later write under the same
// directory in the order they happened.
export class FolderMirror {
  private queue: Promise<void> = Promise.resolve()
  private failures = 0
  private held = new Map<string, Held>()
  // Called whenever the held count changes.
  onChange?: () => void

  constructor(
    readonly root: DirHandle,
    private prefix: string,
    private mode: FolderMode,
  ) {}

  get heldCount(): number {
    return this.held.size
  }

  get currentMode(): FolderMode {
    return this.mode
  }

  private hold(path: string, h: Held) {
    if (!this.rel(path)) return
    this.held.delete(path)
    this.held.set(path, h)
    this.onChange?.()
  }

  // Read-only from here on: new mutations are held.
  readOnly() {
    this.mode = 'read'
  }

  // Writing from here on: the held mutations land first, in order, then every new
  // one is applied as it arrives. The caller has the write grant already.
  async startWriting(): Promise<void> {
    this.mode = 'write'
    const held = [...this.held.entries()]
    this.held.clear()
    for (const [path, h] of held) {
      if (h.op === 'write') this.apply(path, h.bytes)
      else this.applyRemove(path, h.dir)
    }
    this.onChange?.()
    await this.flush()
  }

  // `path` is the backend's absolute path; only what lies under the project root
  // is the folder's business.
  private rel(path: string): string[] | null {
    if (!path.startsWith(this.prefix + '/')) return null
    return path.slice(this.prefix.length + 1).split('/').filter(Boolean)
  }

  private async dir(segments: string[], create: boolean): Promise<DirHandle | null> {
    let d: DirHandle = this.root
    for (const s of segments) {
      try {
        d = (await d.getDirectoryHandle(s, { create })) as DirHandle
      } catch {
        return null
      }
    }
    return d
  }

  write(path: string, bytes: Uint8Array) {
    if (this.mode === 'read') return this.hold(path, { op: 'write', bytes })
    this.apply(path, bytes)
  }

  remove(path: string, dir: boolean) {
    if (this.mode === 'read') return this.hold(path, { op: 'remove', dir })
    this.applyRemove(path, dir)
  }

  private apply(path: string, bytes: Uint8Array) {
    const segs = this.rel(path)
    if (!segs || segs.length === 0) return
    const name = segs.pop()!
    this.enqueue(async () => {
      const d = await this.dir(segs, true)
      if (!d) return
      const fh = await d.getFileHandle(name, { create: true })
      const w = await fh.createWritable()
      // A fresh ArrayBuffer: the bytes crossed from the worker as a plain copy, and
      // the writable's type wants one it can own.
      await w.write(bytes.slice().buffer as ArrayBuffer)
      await w.close()
    })
  }

  private applyRemove(path: string, dir: boolean) {
    const segs = this.rel(path)
    if (!segs || segs.length === 0) return
    const name = segs.pop()!
    this.enqueue(async () => {
      const d = await this.dir(segs, false)
      if (!d) return
      try {
        await d.removeEntry(name, { recursive: dir })
      } catch {
        // Already gone: the tree and the folder agree.
      }
    })
  }

  private enqueue(op: () => Promise<void>) {
    this.queue = this.queue.then(op).catch((e) => {
      this.failures += 1
      console.error('[jazyk folder] write failed:', e)
    })
  }

  // Resolves once every mutation posted so far has landed.
  flush(): Promise<void> {
    return this.queue
  }
}
