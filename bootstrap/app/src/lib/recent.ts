// The browser's own recent list (docs/frontends/gui.md#home): every demo, browser
// project, folder, and server opened in this browser, newest first, in local
// storage under `jazyk.recent`. A folder's directory handle cannot be written as text, so it lives
// in IndexedDB under the entry's id; a stored project's files live in browser storage
// (lib/browser-store.ts). Storage that is absent or refuses is simply not remembered.
import { useSyncExternalStore } from 'react'
import { del as idbDel, get as idbGet, keys as idbKeys, set as idbSet } from 'idb-keyval'

export type FolderMode = 'read' | 'write'

export interface RecentEntry {
  // `demo:<id>`, `browser:<uuid>`, `folder:<uuid>`, or `server:<port>`.
  id: string
  kind: 'demo' | 'browser' | 'folder' | 'server'
  name: string
  // A server on this computer (docs/frontends/gui.md#connecting-to-a-local-jazyk):
  // its port, and the project root it served when last seen.
  port?: number
  root?: string
  // A folder's mode when it was last left.
  mode?: FolderMode
  // A demo whose edited copy is in browser storage.
  edited?: boolean
  // How many changes a folder has kept in browser storage, not yet on disk.
  kept?: number
  openedAt: number
}

// An entry whose data lives in browser storage: dropping it from the list would
// strand that data, so it is never dropped to make room.
export const holdsData = (r: RecentEntry) => r.kind === 'browser' || !!r.edited || (r.kept ?? 0) > 0

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

// Put an entry at the top, over the one with the same id. Past the limit the oldest
// entries that hold no stored data are dropped.
export function touchRecent(e: Omit<RecentEntry, 'openedAt'>) {
  const prior = cache.find((r) => r.id === e.id)
  const list = [{ ...prior, ...e, openedAt: Date.now() }, ...cache.filter((r) => r.id !== e.id)]
  const dropped: RecentEntry[] = []
  for (let i = list.length - 1; i > 0 && list.length > MAX; i--) {
    if (holdsData(list[i])) continue
    dropped.push(...list.splice(i, 1))
  }
  save(list)
  for (const d of dropped) if (d.kind === 'folder') void idbDel(d.id).catch(() => {})
}

// Change fields of an entry in place, keeping its position.
export function updateRecent(id: string, patch: Partial<Omit<RecentEntry, 'id' | 'openedAt'>>) {
  const at = cache.find((r) => r.id === id)
  if (!at || Object.entries(patch).every(([k, v]) => at[k as keyof RecentEntry] === v)) return
  save(cache.map((r) => (r.id === id ? { ...r, ...patch } : r)))
}

export function setRecentMode(id: string, mode: FolderMode) {
  updateRecent(id, { mode })
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

// The read-only choice beside Open folder, remembered. Writing unless changed.
export function readOnlyPref(): boolean {
  try {
    return localStorage.getItem(READ_ONLY_KEY) === '1'
  } catch {
    return false
  }
}

export function setReadOnlyPref(v: boolean) {
  try {
    localStorage.setItem(READ_ONLY_KEY, v ? '1' : '0')
  } catch {
    // not remembered
  }
}
