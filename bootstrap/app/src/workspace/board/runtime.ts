// The board's transient state: the zoom level the windows draw at and which
// windows are live (plans/workspace.md#cost). A window leaving live keeps a still:
// the page as it was last drawn, so the board shows the same page without its cost.
import { create } from 'zustand'
import type { Level } from './model'

interface Runtime {
  level: Level
  live: string[]
  // The one link whose requirements are expanded, by edge id.
  expanded: string | null
  // The entities the caret rested on, most recent first: the fan's order.
  recent: string[]
  setLevel: (level: Level) => void
  setLive: (live: string[]) => void
  toggleLink: (id: string) => void
  touchRecent: (entity: string) => void
}

export const useRuntime = create<Runtime>((set, get) => ({
  level: 'live',
  live: [],
  expanded: null,
  recent: [],
  // One linkage expanded at a time: expanding one folds the one before.
  toggleLink: (id) => set((s) => ({ expanded: s.expanded === id ? null : id })),
  touchRecent: (entity) =>
    set((s) => (s.recent[0] === entity ? s : { recent: [entity, ...s.recent.filter((e) => e !== entity)].slice(0, 24) })),
  setLevel: (level) => {
    if (level === get().level) return
    if (level !== 'live') captureAll()
    set({ level })
  },
  setLive: (live) => {
    const cur = get().live
    if (cur.length === live.length && cur.every((p, i) => p === live[i])) return
    for (const p of cur) if (!live.includes(p)) capture(p)
    set({ live })
  },
}))

export interface Still {
  html: string
  // The scroll offsets of the scrolled elements, in document order.
  scroll: [number, number, number][]
}

const bodies = new Map<string, HTMLElement>()
const stills = new Map<string, Still>()

// A live window's body registers itself so its still can be taken before it goes.
export function registerBody(path: string, el: HTMLElement | null) {
  if (el) bodies.set(path, el)
  else bodies.delete(path)
}

export function capture(path: string) {
  const el = bodies.get(path)
  if (!el || !el.isConnected) return
  const scroll: [number, number, number][] = []
  el.querySelectorAll('*').forEach((n, i) => {
    if (n.scrollTop > 0 || n.scrollLeft > 0) scroll.push([i, n.scrollTop, n.scrollLeft])
  })
  stills.set(path, { html: el.innerHTML, scroll })
}

function captureAll() {
  for (const p of bodies.keys()) capture(p)
}

export function stillOf(path: string): Still | undefined {
  return stills.get(path)
}
