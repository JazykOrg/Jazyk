// The Board arrangement (docs/frontends/gui.md#the-board, plans/workspace.md#board):
// the open items as windows on a dotted table top, the entities they bring as chips
// linked to them and to each other, a focused window filling 80 percent of the view
// with its links' portals in the margin, and what the board draws changing with the
// zoom down to titles. The shell (Workspace.tsx) loads it lazily when the Board
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
  chipId,
  chipScale,
  chipSpot,
  CHIP_H,
  CHIP_STEP,
  CHIP_W,
  docOf,
  focusZoom,
  levelOf,
  nearest,
  nodeOf,
  placeBeside,
  placeNear,
  placeRight,
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
import { nodeTypes, type BoardNode, type ChipData, type WindowData } from './nodes'
import { Links, type NodeName } from './links'
import { useRuntime } from './runtime'

const GLYPH: Record<ReturnType<typeof itemKind>, string> = {
  doc: '¶',
  product: '▣',
  generated: '⎘',
  entity: '◆',
  requirement: '✓',
  diagram: '◇',
  inbox: '✉',
  history: '↺',
  other: '▢',
}

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
  useEffect(() => useRuntime.getState().setLevel(level), [level])
  return null
}

function Toolbar({ graphOn, toggleGraph, arrange, fit }: { graphOn: boolean; toggleGraph: () => void; arrange: () => void; fit: () => void }) {
  const zoom = useStore((s) => s.transform[2])
  const level = levelOf(zoom)
  const words = { live: 'pages', still: 'stills', card: 'cards', title: 'titles' }
  return (
    <div className="board-tools">
      <button onClick={arrange} title="Lay the windows and the entities out by the graph">
        Arrange
      </button>
      <button onClick={fit} title="Fit everything (Z)">
        Fit
      </button>
      <button className={graphOn ? 'on' : ''} aria-pressed={graphOn} onClick={toggleGraph} title="The entities and the relationships between the windows">
        Graph
      </button>
      <span className="board-zoom mono" title="Escape zooms out one level">
        {Math.round(zoom * 100)}% · {words[level]}
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
  const caret = useApp((a) => a.caretEntity)
  const rf = useReactFlow<BoardNode, BoardEdgeType>()
  const storeApi = useStoreApi()
  const [saved, setSaved] = useState<Saved>(() => readSaved(project))
  const initialViewport = useRef(saved.viewport)

  // Remembered a beat after the last change: a drag writes once when it settles.
  useEffect(() => {
    const t = window.setTimeout(() => writeSaved(project, saved), 200)
    return () => window.clearTimeout(t)
  }, [project, saved])

  const layer = useMemo(() => buildLayer(items, g, saved.graph), [items, g, saved.graph])
  const scale = chipScale(level)

  // Where each new window goes, at its size (a page at 80 percent of the board, an
  // entity as a card): beside the window it was opened from, beside the chip it was
  // opened from, else right of the rightmost window. Windows are keyed by their
  // item's page (the path without its query).
  const hints = useRef(new Map<string, { from?: string; near?: Point }>())
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
      windows[key] = anchor ? placeBeside(anchor, taken, w, h) : hint?.near ? placeNear(hint.near, taken, w, h) : placeRight(taken, w, h)
    }
    setSaved((s) => ({ ...s, windows }))
  }, [items, saved.windows, storeApi])

  // Focus: the window fills 80 percent of the board, centered; its links show in the
  // margin (docs/frontends/gui.md#the-board).
  const focusRect = useCallback(
    (r: Rect, kind: string, duration = 450) => {
      const { width, height } = storeApi.getState()
      const zoom = focusZoom(r, { w: width, h: height }, kind)
      void rf.setCenter(r.x + r.w / 2, r.y + r.h / 2, { zoom, duration })
    },
    [rf, storeApi],
  )

  // Opening or showing an item from outside its window focuses it; a press inside a
  // window focuses its item without moving the board.
  const focusedKey = focused ? itemKey(focused) : null
  const seenFocus = useRef<string | null | undefined>(undefined)
  const selfFocus = useRef<string | null>(null)
  useEffect(() => {
    if (!focusedKey) return
    const r = saved.windows[focusedKey]
    if (!r || seenFocus.current === focusedKey) return
    const first = seenFocus.current === undefined
    seenFocus.current = focusedKey
    if ((first && initialViewport.current) || selfFocus.current === focusedKey) {
      selfFocus.current = null
      return
    }
    focusRect(r, itemKind(focusedKey))
  }, [focusedKey, saved.windows, focusRect])

  // The rect every window and chip sits in, at the chips' size on this level.
  const rects = useMemo(() => {
    const m = new Map<string, Rect>()
    for (const it of items) {
      const r = saved.windows[itemKey(it.path)]
      if (r) m.set(windowId(it.path), r)
    }
    for (const c of layer.chips) {
      const owner = saved.windows[c.owner]
      if (!owner) continue
      const p = saved.chips[c.entity] ?? chipSpot(owner, c.index, c.count, scale)
      m.set(chipId(c.entity), { x: p.x, y: p.y, w: CHIP_W * scale, h: CHIP_H * scale })
    }
    return m
  }, [items, saved.windows, saved.chips, layer, scale])

  // The caret on a mention in a document window: that entity's node and the link
  // between them, when there is one.
  const hot = useMemo(() => {
    if (!caret || !g) return { node: null as string | null, edge: null as string | null }
    const doc = items.find((i) => itemKind(i.path) === 'doc' && docOf(i.path) === caret.doc)
    const node = layer.entityNode.get(canonical(g, caret.id)) ?? null
    if (!doc || !node) return { node: null, edge: null }
    const w = windowId(doc.path)
    const edge = layer.edges.find((e) => (e.source === w && e.target === node) || (e.source === node && e.target === w))
    return { node, edge: edge?.id ?? null }
  }, [caret, g, items, layer])

  // Node data objects are kept while they are equal, so a drag re-renders only the
  // node being dragged and never a live page.
  const windowData = useRef(new Map<string, WindowData>())
  const chipData = useRef(new Map<string, ChipData>())
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
        // On the titles level a title reads over the chips around it.
        zIndex: level === 'title' ? 4 : isFocused ? 2 : 1,
      })
    }
    for (const c of layer.chips) {
      const owner = saved.windows[c.owner]
      if (!owner) continue
      // On the cards level an unmoved window's column keeps to the window's height.
      const pinned = saved.chips[c.entity]
      if (!pinned && level === 'card' && c.index >= Math.max(3, Math.floor(owner.h / (CHIP_STEP * scale)))) continue
      const e = g?.entities[c.entity]
      const r = rects.get(chipId(c.entity))!
      let data = chipData.current.get(c.entity)
      const next: ChipData = {
        entity: c.entity,
        name: e?.name ?? c.entity,
        kind: kindWord(e?.stereotype),
        scale,
        selected: selected === c.entity,
        hot: hot.node === chipId(c.entity),
      }
      if (!data || data.name !== next.name || data.kind !== next.kind || data.scale !== next.scale || data.selected !== next.selected || data.hot !== next.hot) {
        data = next
        chipData.current.set(c.entity, data)
      }
      out.push({
        id: chipId(c.entity),
        type: 'chip',
        position: { x: r.x, y: r.y },
        width: r.w,
        height: r.h,
        data,
        zIndex: 3,
      })
    }
    return out
  }, [items, saved.windows, saved.chips, layer, g, level, scale, focusedKey, selected, rects, hot.node])

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

  // What a portal names: a window's title, a chip's entity.
  const names = useMemo(() => {
    const m = new Map<string, NodeName>()
    for (const it of items) {
      const kind = itemKind(it.path)
      let name = itemTitle(it.path)
      if (g && (kind === 'entity' || kind === 'requirement' || kind === 'diagram')) {
        const id = canonical(g, nodeOf(it.path))
        name = g.entities[id]?.name ?? g.requirements[id]?.statement ?? g.views[id]?.title ?? name
      }
      m.set(windowId(it.path), { glyph: GLYPH[kind], name: shorten(name, 40) })
    }
    for (const c of layer.chips) m.set(chipId(c.entity), { glyph: '◆', name: shorten(g?.entities[c.entity]?.name ?? c.entity, 40) })
    return m
  }, [items, layer, g])

  const onNodesChange = useCallback((changes: NodeChange<BoardNode>[]) => {
    let moved = false
    const windows: Record<string, Rect> = {}
    const chips: Record<string, Point> = {}
    for (const c of changes) {
      if (c.type !== 'position' || !c.position) continue
      if (c.id.startsWith('w:')) {
        windows[c.id.slice(2)] = { x: c.position.x, y: c.position.y, w: 0, h: 0 }
        moved = true
      } else if (c.id.startsWith('c:')) {
        chips[c.id.slice(2)] = { x: c.position.x, y: c.position.y }
        moved = true
      }
    }
    if (!moved) return
    setSaved((s) => {
      const w = { ...s.windows }
      for (const [p, r] of Object.entries(windows)) if (w[p]) w[p] = { ...w[p], x: r.x, y: r.y }
      return { ...s, windows: w, chips: { ...s.chips, ...chips } }
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
    computeLive()
    const vp = rf.getViewport()
    setSaved((s) => ({ ...s, viewport: { x: vp.x, y: vp.y, zoom: vp.zoom } }))
  }, [computeLive, rf])

  // Refs so the actions stay one object for the board's life: a live page never
  // re-renders because the board did.
  const state = useRef({ saved, rects, layer, scale })
  state.current = { saved, rects, layer, scale }
  const actions = useMemo<BoardActions>(() => {
    const zoomToNode = (id: string) => {
      const r = state.current.rects.get(id)
      if (!r) return
      if (id.startsWith('w:')) focusRect(r, itemKind(id.slice(2)))
      else void rf.setCenter(r.x + r.w / 2, r.y + r.h / 2, { zoom: Math.max(rf.getZoom(), 0.9), duration: 450 })
    }
    return {
      zoomToNode,
      // A portal: focus the node at the far end of the link.
      go: (id) => {
        zoomToNode(id)
        if (id.startsWith('w:')) {
          const ws = useWorkspace.getState()
          const item = ws.items.find((i) => windowId(i.path) === id)
          if (item && itemKey(ws.focused ?? '') !== itemKey(item.path)) {
            selfFocus.current = itemKey(item.path)
            ws.focusItem(item.path)
          }
        } else if (id.startsWith('c:')) useWorkspace.getState().select(id.slice(2))
      },
      // Through the shell's moves, so an open page is focused (and retargeted)
      // rather than opened twice.
      open: (path, { from, near }) => {
        const key = itemKey(path)
        if (!useWorkspace.getState().items.some((i) => itemKey(i.path) === key)) {
          if (from) hints.current.set(key, { from })
          else if (near) {
            const r = state.current.rects.get(chipId(near))
            if (r) hints.current.set(key, { near: { x: r.x + r.w - CHIP_W, y: r.y } })
          }
        }
        showPath(path, from ? { from } : {})
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
      focus: (path) => {
        const ws = useWorkspace.getState()
        if (itemKey(ws.focused ?? '') === itemKey(path)) return
        selfFocus.current = itemKey(path)
        ws.focusItem(path)
      },
    }
  }, [focusRect, rf])

  // A requirement from a link's linkage opens beside the link's window end, else
  // beside its chip.
  const openRequirement = useCallback(
    (id: string, edge: LayerEdge) => {
      const path = `/model/requirements/${encodeURIComponent(id)}`
      const windowEnd = [edge.source, edge.target].find((n) => n.startsWith('w:'))
      const item = windowEnd ? useWorkspace.getState().items.find((i) => windowId(i.path) === windowEnd) : undefined
      if (item) actions.open(path, { from: item.path })
      else actions.open(path, { near: edge.source.slice(2) })
    },
    [actions],
  )

  const onNodeClick = useCallback<NodeMouseHandler<BoardNode>>(
    (e, node) => {
      const t = e.target as HTMLElement
      if (node.type === 'window') {
        // A press anywhere in a window already focused it (WindowNode); a click on
        // a still, a card, or a title zooms to it.
        if (t.closest('.bw-close, .bw-resize, button')) return
        if (useRuntime.getState().level !== 'live' && t.closest('.bw-body')) actions.zoomToNode(node.id)
      } else if (node.type === 'chip') {
        useWorkspace.getState().select((node.data as ChipData).entity)
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
      } else if (node.type === 'chip') {
        const entity = (node.data as ChipData).entity
        actions.open(`/model/entities/${encodeURIComponent(entity)}`, { near: entity })
      }
    },
    [actions],
  )

  const fit = useCallback(() => void rf.fitView({ padding: 0.08, duration: 400, maxZoom: 1 }), [rf])

  // `Escape` zooms out one level, `Z` fits everything; neither while typing or
  // inside a live page.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.defaultPrevented || e.ctrlKey || e.metaKey || e.altKey || editable(e.target)) return
      if (e.key === 'Escape') {
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
    // The chips take their size on the cards level, so no two overlap at any zoom.
    const big = chipScale('card')
    const nodesIn = [...rs.entries()].map(([id, r]) => (id.startsWith('c:') ? { id, w: CHIP_W * big, h: CHIP_H * big } : { id, w: r.w, h: r.h }))
    const edgesIn = l.edges.map((e) => ({ source: e.source, target: e.target }))
    const pos = layout({ nodes: nodesIn, edges: edgesIn })
    const windows = { ...s.windows }
    const chips = { ...s.chips }
    for (const [id, p] of pos) {
      if (id.startsWith('w:') && windows[id.slice(2)]) windows[id.slice(2)] = { ...windows[id.slice(2)], x: p.x, y: p.y }
      else if (id.startsWith('c:')) chips[id.slice(2)] = p
    }
    setSaved((x) => ({ ...x, windows, chips }))
    // The arranged board shows as cards at the least, never as titles.
    window.setTimeout(() => void rf.fitView({ padding: 0.08, duration: 400, maxZoom: 1, minZoom: 0.16 }), 60)
  }, [rf])

  const toggleGraph = useCallback(() => setSaved((s) => ({ ...s, graph: !s.graph })), [])

  const colorMode = theme === 'auto' ? 'system' : theme
  return (
    <BoardActionsContext.Provider value={actions}>
      <ReactFlow<BoardNode, BoardEdgeType>
        nodes={nodes}
        edges={edges}
        nodeTypes={nodeTypes}
        edgeTypes={edgeTypes}
        onNodesChange={onNodesChange}
        onNodeClick={onNodeClick}
        onNodeDoubleClick={onNodeDoubleClick}
        onMoveEnd={onMoveEnd}
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
        <MiniMap pannable zoomable position="bottom-right" className="board-minimap" style={{ width: 150, height: 100 }} nodeClassName={(n) => `mm-${n.type}`} />
        <Links edges={drawn} rects={rects} names={names} level={level} hotEdge={hot.edge} onGo={actions.go} onOpenRequirement={openRequirement} />
        <Panel position="top-left">
          <Toolbar graphOn={saved.graph} toggleGraph={toggleGraph} arrange={() => void arrange()} fit={fit} />
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
