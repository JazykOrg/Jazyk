// The browser's own recent list (docs/frontends/gui.md#home): every demo and folder
// opened in this browser, newest first, in local storage under `jazyk.recent`. A
// folder's directory handle cannot be written as text, so it lives in IndexedDB
// under the entry's id. Storage that is absent or refuses is simply not remembered.
import { useSyncExternalStore } from 'react'
import { del as idbDel, get as idbGet, keys as idbKeys, set as idbSet } from 'idb-keyval'

export type FolderMode = 'read' | 'write'

export interface RecentEntry {
  id: string
  kind: 'demo' | 'folder'
  name: string
  // A folder's mode when it was last left.
  mode?: FolderMode
  openedAt: number
}

const KEY = 'jazyk.recent'
const READ_ONLY_KEY = 'jazyk.folder.readOnly'
const MAX = 12
const CHANGED = 'jazyk:recent-changed'

function load(): RecentEntry[] {
  try {
    const raw = localStorage.getItem(KEY)
    const list = raw ? (JSON.parse(raw) as RecentEntry[]) : []
    return Array.isArray(list) ? list : []
  } catch {
    return []
  }
}

// useSyncExternalStore wants the same value back until something changed.
let cache: RecentEntry[] = load()

function save(list: RecentEntry[]) {
  cache = list
  try {
    localStorage.setItem(KEY, JSON.stringify(list))
  } catch {
    // not remembered
  }
  window.dispatchEvent(new Event(CHANGED))
}

export function readRecent(): RecentEntry[] {
  return cache
}

// Put an entry at the top, replacing the one with the same id.
export function touchRecent(e: Omit<RecentEntry, 'openedAt'>) {
  const rest = cache.filter((r) => r.id !== e.id)
  const dropped = rest.slice(MAX - 1)
  save([{ ...e, openedAt: Date.now() }, ...rest.slice(0, MAX - 1)])
  for (const d of dropped) if (d.kind === 'folder') void idbDel(d.id).catch(() => {})
}

export function setRecentMode(id: string, mode: FolderMode) {
  save(cache.map((r) => (r.id === id ? { ...r, mode } : r)))
}

export function removeRecent(id: string) {
  save(cache.filter((r) => r.id !== id))
  void idbDel(id).catch(() => {})
}

export function useRecent(): RecentEntry[] {
  return useSyncExternalStore(
    (notify) => {
      const onStorage = (e: StorageEvent) => {
        if (e.key !== KEY) return
        cache = load()
        notify()
      }
      window.addEventListener(CHANGED, notify)
      window.addEventListener('storage', onStorage)
      return () => {
        window.removeEventListener(CHANGED, notify)
        window.removeEventListener('storage', onStorage)
      }
    },
    () => cache,
  )
}

// The id a folder is remembered under. Picking the same folder again finds its
// entry, so the list holds one row per folder.
export async function rememberFolder(handle: FileSystemDirectoryHandle): Promise<string> {
  try {
    for (const k of await idbKeys()) {
      if (typeof k !== 'string' || !k.startsWith('folder:')) continue
      const h = await idbGet<FileSystemDirectoryHandle>(k)
      if (h && (await h.isSameEntry(handle))) {
        await idbSet(k, handle)
        return k
      }
    }
    const id = `folder:${crypto.randomUUID()}`
    await idbSet(id, handle)
    return id
  } catch {
    // No IndexedDB: the folder opens, and is not remembered.
    return `folder:${crypto.randomUUID()}`
  }
}

export async function folderHandle(id: string): Promise<FileSystemDirectoryHandle | undefined> {
  try {
    return await idbGet<FileSystemDirectoryHandle>(id)
  } catch {
    return undefined
  }
}

// The read-only choice beside Open folder, remembered. Read-only unless changed.
export function readOnlyPref(): boolean {
  try {
    return localStorage.getItem(READ_ONLY_KEY) !== '0'
  } catch {
    return true
  }
}

export function setReadOnlyPref(v: boolean) {
  try {
    localStorage.setItem(READ_ONLY_KEY, v ? '1' : '0')
  } catch {
    // not remembered
  }
}
