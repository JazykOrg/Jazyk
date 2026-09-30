// The workspace's items: every open page, one path each, shared by the Tabs and
// Board arrangements (docs/frontends/gui.md#workspace-experimental, plans/workspace.md).
// An item renders anywhere through the page table (routes/AppRoutes.tsx) against its
// own path. The arrangement is a per-browser switch; the open items are remembered
// per project.
import { create } from 'zustand'

export type Arrangement = 'lenses' | 'tabs' | 'board'

export interface Item {
  // The page's path with its query, e.g. `/docs/docs/main.md` or
  // `/model/entities/ent%3Aquay`.
  path: string
  // A preview tab: replaced by the next single-click open, kept by an edit or a pin.
  preview?: boolean
  // The item it was opened from, so the board can place it beside that one and draw
  // an edge between them.
  from?: string
}

const ARRANGEMENT_KEY = 'jazyk.workspace'
const itemsKey = (project: string) => `jazyk.workspace.items:${project}`

function readArrangement(): Arrangement {
  try {
    const v = localStorage.getItem(ARRANGEMENT_KEY)
    return v === 'tabs' || v === 'board' ? v : 'lenses'
  } catch {
    return 'lenses'
  }
}

interface WorkspaceState {
  arrangement: Arrangement
  setArrangement: (a: Arrangement) => void
  // The project the items belong to (the open project's recent id or root).
  project: string | null
  items: Item[]
  // The item in front: the focused tab, the window last zoomed to.
  focused: string | null
  // The node the Connections panel shows (an entity, a requirement, a diagnostic).
  selected: string | null
  loadProject: (project: string | null) => void
  openItem: (path: string, opts?: { preview?: boolean; from?: string }) => void
  closeItem: (path: string) => void
  focusItem: (path: string) => void
  pinItem: (path: string) => void
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
  setArrangement: (arrangement) => {
    try {
      localStorage.setItem(ARRANGEMENT_KEY, arrangement)
    } catch {
      // not remembered
    }
    set({ arrangement })
  },
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
    set({ project, items, focused, selected: null })
  },
  // Opening an open item focuses it. A preview open replaces the current preview
  // item; any other open adds the item after the focused one.
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
    set({ items: next, focused: path })
    save(project, next, path)
  },
  closeItem: (path) => {
    const { items, focused, project } = get()
    const at = items.findIndex((i) => i.path === path)
    if (at < 0) return
    const next = items.filter((i) => i.path !== path)
    const nextFocus = focused === path ? (next[at] ?? next[at - 1] ?? null)?.path ?? null : focused
    set({ items: next, focused: nextFocus })
    save(project, next, nextFocus)
  },
  focusItem: (path) => {
    const { items, project } = get()
    if (!items.some((i) => i.path === path)) return
    set({ focused: path })
    save(project, items, path)
  },
  pinItem: (path) => {
    const { items, focused, project } = get()
    const next = items.map((i) => (i.path === path ? { ...i, preview: false } : i))
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

// The kind an item's path names, for its icon.
export function itemKind(path: string): 'doc' | 'product' | 'generated' | 'entity' | 'requirement' | 'diagram' | 'inbox' | 'history' | 'other' {
  const p = path.split('?')[0]
  if (p.startsWith('/docs/')) return 'doc'
  if (p.startsWith('/product/generated/')) return 'generated'
  if (p.startsWith('/product')) return 'product'
  if (p.startsWith('/model/entities/')) return 'entity'
  if (p.startsWith('/model/requirements/')) return 'requirement'
  if (p.startsWith('/model/diagrams/')) return 'diagram'
  if (p.startsWith('/inbox')) return 'inbox'
  if (p.startsWith('/history')) return 'history'
  return 'other'
}
