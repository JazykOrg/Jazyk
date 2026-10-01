// The Board arrangement (docs/frontends/gui.md#the-board, plans/workspace.md#board):
// the open items as windows on a dotted table top with the links between them, a
// focused window filling 80 percent of the view with its links' portals in the
// margin and its unopened entities fanned out below it, and what the board draws
// changing with the zoom down to titles. The shell (Workspace.tsx) loads it lazily when the Board
// arrangement first shows.
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import {
  Background,
  BackgroundVariant,
  MarkerType,
  MiniMap,
  Panel,
  ReactFlow,
  ReactFlowProvider,
  useReactFlow,
  useStore,
  useStoreApi,
  type NodeChange,
  type NodeMouseHandler,
} from '@xyflow/react'
import '@xyflow/react/dist/style.css'
import './board.css'
import { useGraph, useProject } from '../../lib/queries'
import { useApp } from '../../lib/store'
import { kindWord, shorten } from '../../lib/present'
import { treeProjectKey } from '../../lib/tree'
import { itemKind, itemTitle, useWorkspace } from '../items'
import { itemKey } from '../paths'
import { closePath, showPath } from '../actions'
import { BoardActionsContext, type BoardActions } from './actions'
import { edgeTypes, type BoardEdgeType } from './edges'
import {
  buildLayer,
  canonical,
  docOf,
  caretEntities,
  flyFrame,
  focusZoom,
  FLY_MS,
  levelOf,
  nearest,
  nodeOf,
  placeBeside,
  placeRight,
  linkRequirements,
  readSaved,
  windowId,
  windowSize,
  writeSaved,
  zoomOut,
  type LayerEdge,
  type Point,
  type Rect,
  type Saved,
} from './model'
import { nodeTypes, type BoardNode, type WindowData } from './nodes'
import { Links, type NodeName } from './links'
import { References, type Reference } from './references'
import { LinkCards } from './linksbar'
import { KINDS } from '../kinds'
import { useRuntime } from './runtime'


// The project the items belong to: the shell loads it; a board mounted alone
// loads it the way the shell does.
function useEnsureProject() {
  const project = useWorkspace((s) => s.project)
  const { data: proj } = useProject()
  const key = treeProjectKey(proj?.root ?? '') || null
  useEffect(() => {
    if (!useWorkspace.getState().project && key) useWorkspace.getState().loadProject(key)
  }, [key])
  return project
}

export default function BoardArrangement() {
  const project = useEnsureProject()
  const level = useRuntime((s) => s.level)
  return (
    <div className={`board lv-${level}`}>
      <ReactFlowProvider key={project ?? ''}>
        <BoardCanvas project={project} />
      </ReactFlowProvider>
    </div>
  )
}

// The level follows the zoom; a window leaving live keeps its still.
function LevelWatcher() {
  const level = useStore((s) => levelOf(s.transform[2]))
  const flying = useRuntime((s) => s.flying)
  useEffect(() => {
    if (!flying) useRuntime.getState().setLevel(level)
  }, [level, flying])
  return null
}

const MAP_KEY = 'jazyk.board.map'

function readMap(): boolean {
  try {
    return localStorage.getItem(MAP_KEY) === '1'
  } catch {
    return false
  }
}

function Toolbar({ mapOn, toggleMap, arrange }: { mapOn: boolean; toggleMap: () => void; arrange: () => void }) {
  const zoom = useStore((s) => s.transform[2])
  return (
    <div className="board-tools">
      <button onClick={arrange} title="Lay the windows out by the links between them">
        Arrange
      </button>
      <button className={mapOn ? 'on' : ''} aria-pressed={mapOn} onClick={toggleMap} title="A minimap of the whole board">
        Map
      </button>
      <span className="board-zoom mono" data-level={levelOf(zoom)} title="Z fits everything; Escape zooms out one level">
        {Math.round(zoom * 100)}%
      </span>
    </div>
  )
}

function editable(t: EventTarget | null): boolean {
  const el = t as HTMLElement | null
  if (!el || !el.closest) return false
  return !!el.closest('input, textarea, select, [contenteditable="true"], .bw-body.nodrag, .board-tools')
}

function BoardCanvas({ project }: { project: string | null }) {
  const items = useWorkspace((s) => s.items)
  const focused = useWorkspace((s) => s.focused)
  const selected = useWorkspace((s) => s.selected)
  const { data: g } = useGraph()
  const level = useRuntime((s) => s.level)
  const theme = useApp((a) => a.theme)
  const caret = useApp((a) => a.caret)
  const rf = useReactFlow<BoardNode, BoardEdgeType>()
  const storeApi = useStoreApi()
  const [saved, setSaved] = useState<Saved>(() => readSaved(project))
  const initialViewport = useRef(saved.viewport)

  // Remembered a beat after the last change: a drag writes once when it settles.
  useEffect(() => {
    const t = window.setTimeout(() => writeSaved(project, saved), 200)
    return () => window.clearTimeout(t)
  }, [project, saved])

  const layer = useMemo(() => buildLayer(items, g), [items, g])

  // Where each new window goes, at its size (a page at 80 percent of the board, an
  // entity as a card): beside the window it was opened from, else right of the
  // rightmost window. Windows are keyed by their item's page (the path without its
  // query).
  const hints = useRef(new Map<string, { from?: string }>())
  useEffect(() => {
    const missing = items.filter((i) => !saved.windows[itemKey(i.path)])
    if (missing.length === 0) return
    const { width, height } = storeApi.getState()
    const board = width > 0 && height > 0 ? { w: width, h: height } : null
    const windows = { ...saved.windows }
    for (const it of missing) {
      const key = itemKey(it.path)
      const taken = items.filter((i) => windows[itemKey(i.path)]).map((i) => windows[itemKey(i.path)])
      const hint = hints.current.get(key)
      hints.current.delete(key)
      const from = hint?.from ?? it.from
      const anchor = from ? windows[itemKey(from)] : undefined
      const { w, h } = windowSize(itemKind(it.path), board)
      windows[key] = anchor ? placeBeside(anchor, taken, w, h) : placeRight(taken, w, h)
    }
    setSaved((s) => ({ ...s, windows }))
  }, [items, saved.windows, storeApi])

  // Focus: the window fills 80 percent of the board, centered; its links show in the
  // margin (docs/frontends/gui.md#the-board).
  // One short flight: the center moves while the zoom eases down (to 75 percent at
  // most, on a long move) and back up, in FLY_MS. A press on the table top or a wheel
  // stops it where it is.
  const flight = useRef(0)
  const flying = useRef(false)
  const landed = useRef<() => void>(() => {})
  const flyTo = useCallback(
    (cx: number, cy: number, z: number) => {
      const { width, height, transform } = storeApi.getState()
      const [tx, ty, z0] = transform
      const from = { cx: (width / 2 - tx) / z0, cy: (height / 2 - ty) / z0, z: z0 }
      const to = { cx, cy, z }
      const far = Math.hypot((to.cx - from.cx) * z0, (to.cy - from.cy) * z0) > width / 2
      const id = ++flight.current
      flying.current = true
      useRuntime.getState().setFlying(true)
      const start = performance.now()
      const step = (now: number) => {
        if (id !== flight.current) return
        const t = Math.min(1, (now - start) / FLY_MS)
        const c = flyFrame(from, to, t, far)
        void rf.setViewport({ x: width / 2 - c.cx * c.z, y: height / 2 - c.cy * c.z, zoom: c.z })
        if (t < 1) requestAnimationFrame(step)
        else {
          flying.current = false
          useRuntime.getState().setFlying(false)
          landed.current()
        }
      }
      requestAnimationFrame(step)
    },
    [rf, storeApi],
  )
  useEffect(() => {
    const stop = (e: Event) => {
      // A press on the table top or a wheel takes the view back; a press in a
      // window starts its own flight.
      const t = e.target as Element
      if (!flying.current || (e.type === 'pointerdown' && (!t.closest?.('.react-flow__pane') || t.closest('.react-flow__node')))) return
      flight.current++
      flying.current = false
      useRuntime.getState().setFlying(false)
      landed.current()
    }
    const el = document.querySelector('.board')
    el?.addEventListener('pointerdown', stop, true)
    el?.addEventListener('wheel', stop, true)
    return () => {
      el?.removeEventListener('pointerdown', stop, true)
      el?.removeEventListener('wheel', stop, true)
    }
  }, [])

  const focusRect = useCallback(
    (r: Rect, kind: string) => {
      const { width, height } = storeApi.getState()
      flyTo(r.x + r.w / 2, r.y + r.h / 2, focusZoom(r, { w: width, h: height }, kind))
    },
    [flyTo, storeApi],
  )

  // Opening or showing an item from outside its window focuses it, and so does
  // arriving from Tabs; a press inside a window focuses it (actions.focus).
  const focusedKey = focused ? itemKey(focused) : null
  const seenFocus = useRef<string | null | undefined>(undefined)
  const selfFocus = useRef<string | null>(null)
  useEffect(() => {
    if (!focusedKey) return
    const r = saved.windows[focusedKey]
    if (!r || seenFocus.current === focusedKey) return
    const first = seenFocus.current === undefined
    seenFocus.current = focusedKey
    const arrive = useWorkspace.getState().arrive
    if (arrive && itemKey(arrive) === focusedKey) {
      useWorkspace.getState().takeArrive()
      selfFocus.current = null
      focusRect(r, itemKind(focusedKey))
      return
    }
    if ((first && initialViewport.current) || selfFocus.current === focusedKey) {
      selfFocus.current = null
      return
    }
    focusRect(r, itemKind(focusedKey))
  }, [focusedKey, saved.windows, focusRect])

  // The rect every window sits in.
  const rects = useMemo(() => {
    const m = new Map<string, Rect>()
    for (const it of items) {
      const r = saved.windows[itemKey(it.path)]
      if (r) m.set(windowId(it.path), r)
    }
    return m
  }, [items, saved.windows])

  // What the caret rests on in a document window: the mention's entity (its window
  // and the link to it when open) and the requirements whose quote holds it, marked
  // on every linkage card.
  const caretEntity = caret?.entity && g ? canonical(g, caret.entity) : null
  const hotReqs = useMemo(() => new Set((caret?.reqs ?? []).map((r) => (g ? canonical(g, r) : r))), [caret, g])
  const hot = useMemo(() => {
    if (!caret || !g || !caretEntity) return { node: null as string | null, edge: null as string | null }
    const doc = items.find((i) => itemKind(i.path) === 'doc' && docOf(i.path) === caret.doc)
    const node = layer.entityNode.get(caretEntity) ?? null
    if (!doc || !node) return { node: null, edge: null }
    const w = windowId(doc.path)
    const edge = layer.edges.find((e) => (e.source === w && e.target === node) || (e.source === node && e.target === w))
    return { node, edge: edge?.id ?? null }
  }, [caret, caretEntity, g, items, layer])

  // References: the unopened entities the caret's text concerns, under the focused
  // document, each with the requirements between the document and it.
  const focusedItem = items.find((i) => itemKey(i.path) === focusedKey)
  const refs = useMemo<Reference[]>(() => {
    if (!g || !caret || !focusedItem || level !== 'live') return []
    if (itemKind(focusedItem.path) !== 'doc' || docOf(focusedItem.path) !== caret.doc) return []
    // Open or not: the caret's entities always show, so a name clicked on is seen
    // even when its window is out of view; a click on the card focuses that window.
    return caretEntities(g, caret.entity, caret.reqs, new Set()).map((entity) => ({
      entity,
      name: g.entities[entity]?.name ?? entity,
      kind: kindWord(g.entities[entity]?.stereotype),
      reqs: linkRequirements(g, { doc: caret.doc }, { entity }),
    }))
  }, [g, caret, focusedItem, level, layer])

  // Node data objects are kept while they are equal, so a drag re-renders only the
  // node being dragged and never a live page.
  const windowData = useRef(new Map<string, WindowData>())
  const nodes = useMemo<BoardNode[]>(() => {
    const out: BoardNode[] = []
    const framed = level === 'live' || level === 'still'
    for (const it of items) {
      const key = itemKey(it.path)
      const r = saved.windows[key]
      if (!r) continue
      const isFocused = focusedKey === key
      const isHot = hot.node === windowId(it.path)
      let data = windowData.current.get(key)
      if (!data || data.focused !== isFocused || data.path !== it.path || data.hot !== isHot) {
        data = { path: it.path, focused: isFocused, hot: isHot }
        windowData.current.set(key, data)
      }
      out.push({
        id: windowId(it.path),
        type: 'window',
        position: { x: r.x, y: r.y },
        width: r.w,
        height: r.h,
        data,
        dragHandle: framed ? '.bw-title' : undefined,
        zIndex: isFocused ? 2 : 1,
      })
    }
    return out
  }, [items, saved.windows, level, focusedKey, hot.node])

  // The links drawn: the ones whose two nodes are on the board.
  const shown = useMemo(() => new Set(nodes.map((n) => n.id)), [nodes])
  const drawn = useMemo(() => layer.edges.filter((e) => shown.has(e.source) && shown.has(e.target)), [layer, shown])

  const edges = useMemo<BoardEdgeType[]>(() => {
    const arrow = { type: MarkerType.ArrowClosed, width: 16, height: 16 }
    return drawn.map((e) => ({
      id: e.id,
      source: e.source,
      target: e.target,
      type: 'floating',
      data: { kind: e.kind, hot: e.id === hot.edge },
      markerEnd: e.kind === 'rel' || e.kind === 'from' ? arrow : undefined,
      markerStart: e.both ? arrow : undefined,
    }))
  }, [drawn, hot.edge])

  // What a portal names: a window's title.
  const names = useMemo(() => {
    const m = new Map<string, NodeName>()
    for (const it of items) {
      const kind = itemKind(it.path)
      let name = itemTitle(it.path)
      if (g && (kind === 'entity' || kind === 'requirement' || kind === 'diagram')) {
        const id = canonical(g, nodeOf(it.path))
        name = g.entities[id]?.name ?? g.requirements[id]?.statement ?? g.views[id]?.title ?? name
      }
      m.set(windowId(it.path), { glyph: KINDS[kind].glyph, name: shorten(name, 40) })
    }
    return m
  }, [items, g])

  const onNodesChange = useCallback((changes: NodeChange<BoardNode>[]) => {
    const windows: Record<string, Point> = {}
    for (const c of changes) {
      if (c.type === 'position' && c.position && c.id.startsWith('w:')) windows[c.id.slice(2)] = c.position
    }
    if (Object.keys(windows).length === 0) return
    setSaved((s) => {
      const w = { ...s.windows }
      for (const [p, r] of Object.entries(windows)) if (w[p]) w[p] = { ...w[p], x: r.x, y: r.y }
      return { ...s, windows: w }
    })
  }, [])

  // The live windows: at most six, the nearest the view's center among those in view.
  const computeLive = useCallback(() => {
    const { width, height, transform } = storeApi.getState()
    const [tx, ty, zoom] = transform
    const view = { x: -tx / zoom, y: -ty / zoom, w: width / zoom, h: height / zoom }
    const inView = items
      .map((i) => [itemKey(i.path), saved.windows[itemKey(i.path)]] as [string, Rect | undefined])
      .filter((e): e is [string, Rect] => !!e[1])
      .filter(([, r]) => r.x < view.x + view.w && view.x < r.x + r.w && r.y < view.y + view.h && view.y < r.y + r.h)
    useRuntime.getState().setLive(nearest(inView, { x: view.x + view.w / 2, y: view.y + view.h / 2 }))
  }, [items, saved.windows, storeApi])
  useEffect(() => {
    const t = window.setTimeout(computeLive, 60)
    return () => window.clearTimeout(t)
  }, [computeLive])

  const onMoveEnd = useCallback(() => {
    // A flight lands once, at its end.
    if (flying.current) return
    computeLive()
    const vp = rf.getViewport()
    setSaved((s) => ({ ...s, viewport: { x: vp.x, y: vp.y, zoom: vp.zoom } }))
  }, [computeLive, rf])

  landed.current = onMoveEnd

  // Refs so the actions stay one object for the board's life: a live page never
  // re-renders because the board did.
  const state = useRef({ saved, rects, layer })
  state.current = { saved, rects, layer }
  const actions = useMemo<BoardActions>(() => {
    const zoomToNode = (id: string) => {
      const r = state.current.rects.get(id)
      if (r) focusRect(r, itemKind(id.slice(2)))
    }
    return {
      zoomToNode,
      // A portal: focus the node at the far end of the link.
      go: (id) => {
        zoomToNode(id)
        const ws = useWorkspace.getState()
        const item = ws.items.find((i) => windowId(i.path) === id)
        if (item && itemKey(ws.focused ?? '') !== itemKey(item.path)) {
          selfFocus.current = itemKey(item.path)
          ws.focusItem(item.path)
        }
      },
      // Through the shell's moves, so an open page is focused (and retargeted)
      // rather than opened twice.
      open: (path, { from, background }) => {
        const key = itemKey(path)
        if (from && !useWorkspace.getState().items.some((i) => itemKey(i.path) === key)) hints.current.set(key, { from })
        showPath(path, { ...(from ? { from } : {}), ...(background ? { background } : {}) })
      },
      close: (path) => {
        closePath(path)
        setSaved((s) => {
          const w = { ...s.windows }
          delete w[itemKey(path)]
          return { ...s, windows: w }
        })
      },
      resize: (path, r) => setSaved((s) => ({ ...s, windows: { ...s.windows, [itemKey(path)]: { x: r.x, y: r.y, w: Math.round(r.w), h: Math.round(r.h) } } })),
      // A press in a window focuses its item without moving the board; the press
      // lands before the page's own click, so a page that opens another item on
      // that click leaves the new item focused.
      // A press in a window focuses it: the view moves to it unless it is the focused
      // window, wholly in view at its focus zoom (or `move` is false: a press on the
      // bar, whose click focuses unless it was a drag).
      focus: (path, move = true) => {
        const ws = useWorkspace.getState()
        const key = itemKey(path)
        const isFocused = itemKey(ws.focused ?? '') === key
        const r = state.current.rects.get(windowId(path))
        if (move && r) {
          const { width, height, transform } = storeApi.getState()
          const [tx, ty, z] = transform
          const sx = r.x * z + tx
          const sy = r.y * z + ty
          const whole = sx >= -2 && sy >= -2 && sx + r.w * z <= width + 2 && sy + r.h * z <= height + 2
          const near = z >= focusZoom(r, { w: width, h: height }, itemKind(key)) * 0.95
          if (!isFocused || !whole || !near) focusRect(r, itemKind(key))
        }
        if (isFocused) return
        selfFocus.current = key
        ws.focusItem(path)
      },
    }
  }, [focusRect, rf, storeApi])

  // A requirement from a link's linkage opens beside the link's source window.
  const openRequirement = useCallback(
    (id: string, edge: LayerEdge) => {
      const path = `/model/requirements/${encodeURIComponent(id)}`
      const windowEnd = [edge.source, edge.target].find((n) => n.startsWith('w:'))
      const item = windowEnd ? useWorkspace.getState().items.find((i) => windowId(i.path) === windowEnd) : undefined
      actions.open(path, item ? { from: item.path } : {})
    },
    [actions],
  )

  // A card under a window's links bar opens its item beside that window and focuses
  // it; the cards go.
  const shownGroup = useRuntime((s) => s.shown)
  const openFromCards = useCallback(
    (path: string) => {
      const from = useRuntime.getState().shown?.path
      useRuntime.getState().hideGroup()
      actions.open(path, from ? { from } : {})
    },
    [actions],
  )

  // A reference's title opens its entity beside the focused document and focuses
  // it; the entity stays on the board until it is closed.
  const openReference = useCallback(
    (entity: string) => {
      const from = useWorkspace.getState().focused
      actions.open(`/model/entities/${encodeURIComponent(entity)}`, from ? { from } : {})
    },
    [actions],
  )

  // A requirement's line reveals its quote in its document: in the window holding
  // that document (focused), else in the document opened beside the focused window.
  const graphRef = useRef(g)
  graphRef.current = g
  const reveal = useCallback(
    (rid: string) => {
      const r = graphRef.current?.requirements[rid]
      const doc = r?.source?.doc ?? r?.provenance?.quote?.doc
      const quote = r?.source?.quote ?? r?.provenance?.quote?.quote
      if (!doc || !quote) {
        actions.open(`/model/requirements/${encodeURIComponent(rid)}`, { from: useWorkspace.getState().focused ?? undefined })
        return
      }
      useApp.getState().setReveal(doc, quote)
      const win = useWorkspace.getState().items.find((i) => itemKind(i.path) === 'doc' && docOf(i.path) === doc)
      if (win) actions.go(windowId(win.path))
      else actions.open(`/docs/${doc}`, { from: useWorkspace.getState().focused ?? undefined })
    },
    [actions],
  )
  const windowActions = useMemo<BoardActions>(() => ({ ...actions, reveal }), [actions, reveal])

  const openRequirementFromRef = useCallback(
    (rid: string) => actions.open(`/model/requirements/${encodeURIComponent(rid)}`, { from: useWorkspace.getState().focused ?? undefined }),
    [actions],
  )

  const onNodeClick = useCallback<NodeMouseHandler<BoardNode>>(
    (e, node) => {
      const t = e.target as HTMLElement
      if (node.type === 'window') {
        // A press anywhere in a window already focused it (WindowNode).
        void t
      }
    },
    [actions],
  )

  const onNodeDoubleClick = useCallback<NodeMouseHandler<BoardNode>>(
    (e, node) => {
      const t = e.target as HTMLElement
      if (node.type === 'window') {
        if (t.closest('.bw-body.nodrag, .bw-close')) return
        actions.zoomToNode(node.id)
      }
    },
    [actions],
  )

  const fit = useCallback(() => {
    // A fit takes over from a flight under way.
    flight.current++
    flying.current = false
    useRuntime.getState().setFlying(false)
    void rf.fitView({ padding: 0.08, duration: 400, maxZoom: 1 })
  }, [rf])

  // A click on the empty table top fits everything; a pan that ends there does not.
  const pressAt = useRef<{ x: number; y: number } | null>(null)
  useEffect(() => {
    const onDown = (e: PointerEvent) => {
      pressAt.current = { x: e.clientX, y: e.clientY }
    }
    window.addEventListener('pointerdown', onDown, true)
    return () => window.removeEventListener('pointerdown', onDown, true)
  }, [])
  const onPaneClick = useCallback(
    (e: React.MouseEvent) => {
      const p = pressAt.current
      if (p && Math.hypot(e.clientX - p.x, e.clientY - p.y) > 4) return
      fit()
    },
    [fit],
  )

  // The browser's own page zoom is off while the Board shows: Ctrl and the wheel, a
  // trackpad pinch, and Cmd or Ctrl with + - 0 zoom the board instead
  // (docs/frontends/gui.md#the-board).
  useEffect(() => {
    const onWheel = (e: WheelEvent) => {
      if (e.ctrlKey) e.preventDefault()
    }
    const onKey = (e: KeyboardEvent) => {
      if (!(e.metaKey || e.ctrlKey) || e.altKey) return
      if (e.key === '+' || e.key === '=') {
        e.preventDefault()
        void rf.zoomIn({ duration: 200 })
      } else if (e.key === '-' || e.key === '_') {
        e.preventDefault()
        void rf.zoomOut({ duration: 200 })
      } else if (e.key === '0') {
        e.preventDefault()
        fit()
      }
    }
    const onGesture = (e: Event) => e.preventDefault()
    window.addEventListener('wheel', onWheel, { passive: false })
    window.addEventListener('keydown', onKey, true)
    document.addEventListener('gesturestart', onGesture)
    document.addEventListener('gesturechange', onGesture)
    return () => {
      window.removeEventListener('wheel', onWheel)
      window.removeEventListener('keydown', onKey, true)
      document.removeEventListener('gesturestart', onGesture)
      document.removeEventListener('gesturechange', onGesture)
    }
  }, [rf, fit])

  // `Escape` zooms out one level, `Z` fits everything; neither while typing or
  // inside a live page.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.defaultPrevented || e.ctrlKey || e.metaKey || e.altKey || editable(e.target)) return
      if (e.key === 'Escape') {
        flight.current++
        flying.current = false
        useRuntime.getState().setFlying(false)
        const z = zoomOut(levelOf(rf.getZoom()))
        if (z !== null) {
          e.preventDefault()
          void rf.zoomTo(z, { duration: 300 })
        }
      } else if (e.key === 'z' || e.key === 'Z') {
        e.preventDefault()
        fit()
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [rf, fit])

  const arrange = useCallback(async () => {
    const { arrange: layout } = await import('./arrange')
    const { saved: s, rects: rs, layer: l } = state.current
    const nodesIn = [...rs.entries()].map(([id, r]) => ({ id, w: r.w, h: r.h }))
    const edgesIn = l.edges.map((e) => ({ source: e.source, target: e.target }))
    const pos = layout({ nodes: nodesIn, edges: edgesIn })
    const windows = { ...s.windows }
    for (const [id, p] of pos) if (windows[id.slice(2)]) windows[id.slice(2)] = { ...windows[id.slice(2)], x: p.x, y: p.y }
    setSaved((x) => ({ ...x, windows }))
    // The arranged board shows as cards at the least, never as titles.
    window.setTimeout(() => void rf.fitView({ padding: 0.08, duration: 400, maxZoom: 1, minZoom: 0.16 }), 60)
  }, [rf])

  const [mapOn, setMapOn] = useState(readMap)
  const toggleMap = useCallback(
    () =>
      setMapOn((v) => {
        try {
          localStorage.setItem(MAP_KEY, v ? '0' : '1')
        } catch {
          // not remembered
        }
        return !v
      }),
    [],
  )

  const colorMode = theme === 'auto' ? 'system' : theme
  return (
    <BoardActionsContext.Provider value={windowActions}>
      <ReactFlow<BoardNode, BoardEdgeType>
        nodes={nodes}
        edges={edges}
        nodeTypes={nodeTypes}
        edgeTypes={edgeTypes}
        onNodesChange={onNodesChange}
        onNodeClick={onNodeClick}
        onNodeDoubleClick={onNodeDoubleClick}
        onMoveEnd={onMoveEnd}
        onPaneClick={onPaneClick}
        defaultViewport={initialViewport.current ?? { x: 40, y: 40, zoom: 1 }}
        fitView={!initialViewport.current && items.length > 0}
        fitViewOptions={{ padding: 0.08, maxZoom: 1 }}
        minZoom={0.03}
        maxZoom={2}
        colorMode={colorMode}
        elementsSelectable={false}
        nodesConnectable={false}
        zoomOnDoubleClick={false}
        panOnScroll
        zoomOnPinch
        deleteKeyCode={null}
        selectionKeyCode={null}
        multiSelectionKeyCode={null}
        proOptions={{ hideAttribution: true }}
      >
        <LevelWatcher />
        <Background variant={BackgroundVariant.Dots} gap={22} size={1.6} />
        {mapOn && <MiniMap pannable zoomable position="bottom-right" className="board-minimap" style={{ width: 150, height: 100 }} nodeClassName={(n) => `mm-${n.type}`} />}
        <Links edges={drawn} rects={rects} names={names} level={level} hotEdge={hot.edge} hotReqs={hotReqs} onGo={actions.go} onReveal={reveal} onOpenRequirement={openRequirement} />
        {shownGroup && saved.windows[itemKey(shownGroup.path)] && <LinkCards win={saved.windows[itemKey(shownGroup.path)]} onOpen={openFromCards} />}
        {!shownGroup && focusedItem && refs.length > 0 && saved.windows[itemKey(focusedItem.path)] && (
          <References
            key={itemKey(focusedItem.path)}
            win={saved.windows[itemKey(focusedItem.path)]}
            refs={refs}
            hotEntity={caretEntity}
            hotReqs={hotReqs}
            onOpen={openReference}
            onReveal={reveal}
            onOpenRequirement={openRequirementFromRef}
          />
        )}
        <Panel position="top-left">
          <Toolbar mapOn={mapOn} toggleMap={toggleMap} arrange={() => void arrange()} />
        </Panel>
        {items.length === 0 && (
          <Panel position="top-center" className="board-empty">
            Open a document or a page: it opens here as a window.
          </Panel>
        )}
      </ReactFlow>
    </BoardActionsContext.Provider>
  )
}
