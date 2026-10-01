// The workspace's items: every open page, one path each, shared by the Tabs and
// Board arrangements (docs/frontends/gui.md#workspace, plans/workspace.md).
// An item renders anywhere through the page table (routes/AppRoutes.tsx) against its
// own path. The arrangement is a per-browser switch; the open items are remembered
// per project.
import { create } from 'zustand'

export type Arrangement = 'tabs' | 'board'

export interface Item {
  // The page's path with its query, e.g. `/docs/docs/main.md` or
  // `/model/entities/ent%3Aquay`.
  path: string
  // A preview tab: replaced by the next single-click open, kept by an edit or a pin.
  preview?: boolean
  // The item it was opened from, so the board can place it beside that one and draw
  // an edge between them; an unpinned item holds the one it came from.
  from?: string
  // Kept on the Board whether or not it is held; absent means the default (a file
  // pinned, anything else unpinned).
  pinned?: boolean
}

const FILE_KINDS = new Set(['doc', 'file', 'test', 'generated', 'project'])

// A file stays open until closed; any other item is transient.
export function isFileItem(path: string): boolean {
  return FILE_KINDS.has(itemKind(path))
}

// Whether an item is pinned: as set, else a file is and anything else is not.
export function isPinned(item: Item): boolean {
  return item.pinned ?? isFileItem(item.path)
}

// The items still open after the focus moves on the Board
// (docs/frontends/gui.md#workspace): every pinned item, and the items the focus
// holds. The focused item holds itself, and an unpinned item holds the item it was
// opened from, on back until a pinned one. With nothing focused, everything stays.
// Holding applies on the Board only; in Tabs nothing closes on its own.
function pruneOn(arrangement: Arrangement, items: Item[], focused: string | null): Item[] {
  return arrangement === 'board' ? prune(items, focused) : items
}

export function prune(items: Item[], focused: string | null): Item[] {
  if (!focused) return items
  const byKey = new Map(items.map((i) => [i.path.split(/[?#]/)[0], i]))
  const held = new Set<string>()
  let cur = byKey.get(focused.split(/[?#]/)[0])
  while (cur) {
    const key = cur.path.split(/[?#]/)[0]
    if (held.has(key)) break
    held.add(key)
    if (isPinned(cur) || !cur.from) break
    cur = byKey.get(cur.from.split(/[?#]/)[0])
  }
  const next = items.filter((i) => isPinned(i) || held.has(i.path.split(/[?#]/)[0]))
  return next.length === items.length ? items : next
}

const ARRANGEMENT_KEY = 'jazyk.workspace'
const itemsKey = (project: string) => `jazyk.workspace.items:${project}`

function readArrangement(): Arrangement {
  try {
    const v = localStorage.getItem(ARRANGEMENT_KEY)
    return v === 'board' ? 'board' : 'tabs'
  } catch {
    return 'tabs'
  }
}

interface WorkspaceState {
  arrangement: Arrangement
  // Switch the arrangement; `arrive` is the item the other arrangement shows in front
  // (the board focuses its window fully).
  setArrangement: (a: Arrangement, arrive?: string) => void
  arrive: string | null
  takeArrive: () => void
  // The project the items belong to (the open project's recent id or root).
  project: string | null
  items: Item[]
  // The item in front: the focused tab, the window last zoomed to.
  focused: string | null
  // The node the Connections panel shows (an entity, a requirement, a diagnostic).
  selected: string | null
  loadProject: (project: string | null) => void
  openItem: (path: string, opts?: { preview?: boolean; from?: string; background?: boolean }) => void
  // Close an item. `viaBack` leaves no focus behind: the history's back that follows
  // brings the item shown before it to the front (docs/frontends/gui.md#workspace).
  closeItem: (path: string, opts?: { viaBack?: boolean }) => void
  focusItem: (path: string) => void
  pinItem: (path: string) => void
  // Pin or unpin a transient item; an unpinned one no longer held closes.
  togglePin: (path: string) => void
  select: (node: string | null) => void
}

function save(project: string | null, items: Item[], focused: string | null) {
  if (!project) return
  try {
    localStorage.setItem(itemsKey(project), JSON.stringify({ items, focused }))
  } catch {
    // not remembered
  }
}

export const useWorkspace = create<WorkspaceState>((set, get) => ({
  arrangement: readArrangement(),
  setArrangement: (arrangement, arrive) => {
    try {
      localStorage.setItem(ARRANGEMENT_KEY, arrangement)
    } catch {
      // not remembered
    }
    const items = get().items
    const focus = arrive && items.some((i) => i.path === arrive) ? arrive : get().focused
    set({ arrangement, arrive: arrive ?? null, focused: focus })
    save(get().project, items, focus)
  },
  arrive: null,
  takeArrive: () => set({ arrive: null }),
  project: null,
  items: [],
  focused: null,
  selected: null,
  loadProject: (project) => {
    if (project === get().project) return
    let items: Item[] = []
    let focused: string | null = null
    if (project) {
      try {
        const raw = localStorage.getItem(itemsKey(project))
        if (raw) {
          const v = JSON.parse(raw) as { items?: Item[]; focused?: string | null }
          items = Array.isArray(v.items) ? v.items.filter((i) => typeof i?.path === 'string') : []
          focused = typeof v.focused === 'string' ? v.focused : null
        }
      } catch {
        // a corrupt entry reads as none
      }
    }
    set({ project, items: pruneOn(readArrangement(), items, focused), focused, selected: null })
  },
  // Opening an open item focuses it. A preview open replaces the current preview
  // item; any other open adds the item after the focused one. A background open
  // (the board's fan) adds it and leaves the focus where it is.
  openItem: (path, opts = {}) => {
    const { items, focused, project } = get()
    let next = items
    if (!items.some((i) => i.path === path)) {
      const item: Item = { path, preview: opts.preview, from: opts.from }
      const previewAt = opts.preview ? items.findIndex((i) => i.preview) : -1
      if (previewAt >= 0) {
        next = items.slice()
        next[previewAt] = item
      } else {
        const at = focused ? items.findIndex((i) => i.path === focused) : -1
        next = at >= 0 ? [...items.slice(0, at + 1), item, ...items.slice(at + 1)] : [...items, item]
      }
    } else if (!opts.preview) {
      next = items.map((i) => (i.path === path ? { ...i, preview: false } : i))
    }
    const nextFocus = opts.background ? focused : path
    next = pruneOn(get().arrangement, next, nextFocus)
    set({ items: next, focused: nextFocus })
    save(project, next, nextFocus)
  },
  closeItem: (path, opts = {}) => {
    const { items, focused, project } = get()
    const at = items.findIndex((i) => i.path === path)
    if (at < 0) return
    const next = items.filter((i) => i.path !== path)
    const nextFocus = focused !== path ? focused : opts.viaBack ? null : ((next[at] ?? next[at - 1] ?? null)?.path ?? null)
    set({ items: next, focused: nextFocus })
    save(project, next, nextFocus)
  },
  focusItem: (path) => {
    const { items, project } = get()
    if (!items.some((i) => i.path === path)) return
    const next = pruneOn(get().arrangement, items, path)
    set({ items: next, focused: path })
    save(project, next, path)
  },
  pinItem: (path) => {
    const { items, focused, project } = get()
    const next = items.map((i) => (i.path === path ? { ...i, preview: false, pinned: isFileItem(i.path) ? i.pinned : true } : i))
    set({ items: next })
    save(project, next, focused)
  },
  togglePin: (path) => {
    const { items, focused, project } = get()
    const next = pruneOn(
      get().arrangement,
      items.map((i) => (i.path === path ? { ...i, pinned: !isPinned(i) } : i)),
      focused,
    )
    set({ items: next })
    save(project, next, focused)
  },
  select: (selected) => set({ selected }),
}))

// A short title for an item's path, before the page itself names it.
export function itemTitle(path: string): string {
  const [p] = path.split('?')
  const segs = p.split('/').filter(Boolean)
  const last = decodeURIComponent(segs[segs.length - 1] ?? '')
  if (segs[0] === 'model' && segs.length >= 3) return last.replace(/^(ent|req|crit|view|diag):/, '')
  if (segs[0] === 'product' && segs.length === 1) return 'Product'
  if (segs[0] === 'inbox') return 'Inbox'
  if (segs[0] === 'history') return segs.length > 1 ? `Build #${last}` : 'History'
  return last || 'Home'
}

// The kind an item's path names, for its glyph, its color, and whether it is a file
// (kinds.ts). A product file under a tests folder, or named `.test` or `.spec`, is a
// test.
export type ItemKind = 'doc' | 'file' | 'test' | 'generated' | 'project' | 'product' | 'entity' | 'requirement' | 'diagram' | 'inbox' | 'history' | 'other'

export function itemKind(path: string): ItemKind {
  const p = path.split('?')[0]
  if (p.startsWith('/docs/')) return 'doc'
  if (p.startsWith('/product/generated/')) return 'generated'
  if (p.startsWith('/product/files/')) {
    const f = decodeURIComponent(p.slice('/product/files/'.length))
    return /(^|\/)(tests?|spec|__tests__)\//.test(f) || /\.(test|spec)\.[a-z0-9]+$/i.test(f) ? 'test' : 'file'
  }
  if (p.startsWith('/product')) return 'product'
  if (p.startsWith('/project/')) return 'project'
  if (p.startsWith('/model/entities/')) return 'entity'
  if (p.startsWith('/model/requirements/')) return 'requirement'
  if (p.startsWith('/model/diagrams/')) return 'diagram'
  if (p.startsWith('/inbox')) return 'inbox'
  if (p.startsWith('/history')) return 'history'
  return 'other'
}
