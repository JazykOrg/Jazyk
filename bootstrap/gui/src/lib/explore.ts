// The explorer: the walk over the graph the cards give a markdown reader, live
// (docs/frontends/gui.md#explore). The position is the URL: `?entity=` is the card
// in the inspector, `?view=` the level overlaid on the map, `?detail=` the zoom
// beneath the level's members. The history is a stack of positions kept beside the
// URL: every move pushes, back and forward walk it, and a browser back lands on the
// same stack entry, so the two never disagree.
import { useCallback, useEffect } from 'react'
import { useLocation, useNavigate, useSearchParams } from 'react-router'
import { create } from 'zustand'
import { contextViewOf } from './levels'
import { useTree } from './queries'

export interface Position {
  entity: string
  view: string
}

interface ExploreStack {
  stack: Position[]
  index: number
  // Reconcile the stack with the position the URL now holds: a step back or
  // forward moves the index, anything else pushes (and truncates the forward part).
  sync: (p: Position) => void
}

const same = (a: Position | undefined, b: Position) => !!a && a.entity === b.entity && a.view === b.view

export const useExploreStack = create<ExploreStack>((set, get) => ({
  stack: [],
  index: -1,
  sync: (p) => {
    const { stack, index } = get()
    if (same(stack[index], p)) return
    if (same(stack[index - 1], p)) return set({ index: index - 1 })
    if (same(stack[index + 1], p)) return set({ index: index + 1 })
    // An empty position (nothing explored) is not a step.
    if (!p.entity && !p.view) return
    const next = [...stack.slice(0, index + 1), p]
    set({ stack: next, index: next.length - 1 })
  },
}))

// Mounted once in the shell: keeps the stack in step with the URL.
export function ExploreHistory() {
  const loc = useLocation()
  const sync = useExploreStack((s) => s.sync)
  useEffect(() => {
    if (!loc.pathname.startsWith('/graph')) return
    const sp = new URLSearchParams(loc.search)
    sync({ entity: sp.get('entity') ?? '', view: sp.get('view') ?? '' })
  }, [loc.pathname, loc.search, sync])
  return null
}

export interface MoveOptions {
  // Keep the overlaid view as it is: the entity is drawn there already (a tap on
  // the map, a tree row). Without it the map moves to the entity's context view.
  keepView?: boolean
  // Overlay this view with the entity: the card's `In context` and `Inside`.
  view?: string
}

export interface Explorer {
  entity: string
  view: string
  detail: number
  // Move to an entity: its card in the inspector, the map on the level it sits in.
  goEntity: (id: string, opts?: MoveOptions) => void
  // Overlay a view: the level, a flow at the level, a neighbor from `Around`.
  goView: (id: string, entity?: string) => void
  setDetail: (n: number) => void
  back: () => void
  forward: () => void
  canBack: boolean
  canForward: boolean
}

export function useExplorer(): Explorer {
  const [sp, setSp] = useSearchParams()
  const loc = useLocation()
  const navigate = useNavigate()
  const { data: tree } = useTree()
  const stack = useExploreStack((s) => s.stack)
  const index = useExploreStack((s) => s.index)
  const entity = sp.get('entity') ?? ''
  const view = sp.get('view') ?? ''
  const detail = Math.max(0, parseInt(sp.get('detail') ?? '0', 10) || 0)
  const onGraph = loc.pathname.startsWith('/graph')

  // Every move lands on the graph center; from elsewhere it navigates there with
  // the position, from the graph it rewrites the params in place (pushing history).
  const apply = useCallback(
    (edit: (next: URLSearchParams) => void) => {
      if (onGraph) {
        setSp((p) => {
          const next = new URLSearchParams(p)
          edit(next)
          return next
        })
      } else {
        const next = new URLSearchParams()
        edit(next)
        navigate(`/graph?${next.toString()}`)
      }
    },
    [onGraph, setSp, navigate],
  )

  const goEntity = useCallback(
    (id: string, opts?: MoveOptions) => {
      apply((next) => {
        next.set('entity', id)
        next.set('node', id)
        next.set('focus', id)
        const current = next.get('view') ?? ''
        const target = opts?.view ?? (opts?.keepView ? current : (contextViewOf(tree, id) ?? current))
        if (target) next.set('view', target)
        else next.delete('view')
        if (target !== current) next.delete('detail')
      })
    },
    [apply, tree],
  )

  const goView = useCallback(
    (id: string, ent?: string) => {
      apply((next) => {
        if ((next.get('view') ?? '') !== id) next.delete('detail')
        next.set('view', id)
        if (ent) {
          next.set('entity', ent)
          next.set('node', ent)
          next.set('focus', ent)
        } else {
          next.set('node', id)
          next.delete('focus')
        }
      })
    },
    [apply],
  )

  const setDetail = useCallback(
    (n: number) => {
      setSp(
        (p) => {
          const next = new URLSearchParams(p)
          if (n > 0) next.set('detail', String(n))
          else next.delete('detail')
          return next
        },
        { replace: true },
      )
    },
    [setSp],
  )

  // Back and forward land on a stack position; the sync effect recognizes the
  // step and moves the index instead of pushing.
  const goTo = useCallback(
    (p: Position) => {
      apply((next) => {
        for (const k of ['entity', 'view', 'node', 'focus', 'detail']) next.delete(k)
        if (p.entity) {
          next.set('entity', p.entity)
          next.set('node', p.entity)
          next.set('focus', p.entity)
        } else if (p.view) next.set('node', p.view)
        if (p.view) next.set('view', p.view)
      })
    },
    [apply],
  )
  const canBack = index > 0
  const canForward = index >= 0 && index < stack.length - 1
  const back = useCallback(() => {
    if (canBack) goTo(stack[index - 1])
  }, [canBack, goTo, stack, index])
  const forward = useCallback(() => {
    if (canForward) goTo(stack[index + 1])
  }, [canForward, goTo, stack, index])

  return { entity, view, detail, goEntity, goView, setDetail, back, forward, canBack, canForward }
}
