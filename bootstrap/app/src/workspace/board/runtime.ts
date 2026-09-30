// The board's transient state: the zoom level the windows draw at and which
// windows are live (plans/workspace.md#cost). A window leaving live keeps a still:
// the page as it was last drawn, so the board shows the same page without its cost.
import { create } from 'zustand'
import type { Level } from './model'

interface Runtime {
  level: Level
  live: string[]
  setLevel: (level: Level) => void
  setLive: (live: string[]) => void
}

export const useRuntime = create<Runtime>((set, get) => ({
  level: 'live',
  live: [],
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
