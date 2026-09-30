// The Board arrangement (docs/frontends/gui.md#workspace-experimental,
// plans/workspace.md#board): the open items as windows on a dotted table top, the
// entities they bring as chips with the relationships drawn between them, and what
// the board draws changing with the zoom down to groups. The shell loads it lazily.
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
import { kindWord } from '../../lib/present'
import { useWorkspace } from '../items'
import { BoardActionsContext, type BoardActions } from './actions'
import { edgeTypes, type BoardEdgeType } from './edges'
import {
  buildGroups,
  buildLayer,
  chipId,
  chipScale,
  chipSpot,
  CHIP_H,
  CHIP_STEP,
  CHIP_W,
  groupId,
  levelOf,
  nearest,
  placeBeside,
  placeNear,
  placeRight,
  readSaved,
  windowId,
  writeSaved,
  zoomOut,
  type Group,
  type Point,
  type Rect,
  type Saved,
} from './model'
import { nodeTypes, type BoardNode, type ChipData, type WindowData } from './nodes'
import { useRuntime } from './runtime'

// The project the items belong to: the shell loads it; a board mounted alone
// loads the open project's id itself.
function useEnsureProject() {
  const project = useWorkspace((s) => s.project)
  const local = useApp((a) => a.stored?.id ?? a.folder?.id ?? null)
  const { data: proj } = useProject()
  const key = local ?? proj?.root ?? null
  useEffect(() => {
    if (!useWorkspace.getState().project && key) useWorkspace.getState().loadProject(key)
  }, [key])
  return project
}

export default function BoardArrangement() {
  const project = useEnsureProject()
  return (
    <div className="board">
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
  const words = { live: 'pages', still: 'stills', card: 'cards', group: 'groups' }
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

  // Where each new window goes: beside the window it was opened from, beside the
  // chip it was opened from, else right of the rightmost window.
  const hints = useRef(new Map<string, { from?: string; near?: Point }>())
  useEffect(() => {
    const missing = items.filter((i) => !saved.windows[i.path])
    if (missing.length === 0) return
    const windows = { ...saved.windows }
    for (const it of missing) {
      const taken = items.filter((i) => windows[i.path]).map((i) => windows[i.path])
      const hint = hints.current.get(it.path)
      hints.current.delete(it.path)
      const from = hint?.from ?? it.from
      const anchor = from ? windows[from] : undefined
      windows[it.path] = anchor ? placeBeside(anchor, taken) : hint?.near ? placeNear(hint.near, taken) : placeRight(taken)
    }
    setSaved((s) => ({ ...s, windows }))
  }, [items, saved.windows])

  // Opening pans to the new window; a click on a window focuses it without moving.
  const seenFocus = useRef<string | null | undefined>(undefined)
  const selfFocus = useRef<string | null>(null)
  useEffect(() => {
    if (!focused) return
    const r = saved.windows[focused]
    if (!r || seenFocus.current === focused) return
    const first = seenFocus.current === undefined
    seenFocus.current = focused
    if (first || selfFocus.current === focused) {
      selfFocus.current = null
      return
    }
    rf.setCenter(r.x + r.w / 2, r.y + r.h / 2, { zoom: rf.getZoom(), duration: 400 })
  }, [focused, saved.windows, rf])

  // The rect every window and chip sits in (chips at their unscaled size).
  const rects = useMemo(() => {
    const m = new Map<string, Rect>()
    for (const it of items) {
      const r = saved.windows[it.path]
      if (r) m.set(windowId(it.path), r)
    }
    for (const c of layer.chips) {
      const owner = saved.windows[c.owner]
      if (!owner) continue
      const p = saved.chips[c.entity] ?? chipSpot(owner, c.index, 1)
      m.set(chipId(c.entity), { x: p.x, y: p.y, w: CHIP_W, h: CHIP_H })
    }
    return m
  }, [items, saved.windows, saved.chips, layer])

  const overview = useMemo(() => (level === 'group' ? buildGroups(items, layer, g, rects) : { groups: [] as Group[], edges: [] }), [level, items, layer, g, rects])

  // Node data objects are kept while they are equal, so a drag re-renders only the
  // node being dragged and never a live page.
  const windowData = useRef(new Map<string, WindowData>())
  const chipData = useRef(new Map<string, ChipData>())
  const nodes = useMemo<BoardNode[]>(() => {
    const out: BoardNode[] = []
    const group = level === 'group'
    for (const it of items) {
      const r = saved.windows[it.path]
      if (!r) continue
      let data = windowData.current.get(it.path)
      if (!data || data.focused !== (focused === it.path)) {
        data = { path: it.path, focused: focused === it.path }
        windowData.current.set(it.path, data)
      }
      out.push({
        id: windowId(it.path),
        type: 'window',
        position: { x: r.x, y: r.y },
        width: r.w,
        height: r.h,
        data,
        dragHandle: level === 'card' ? undefined : '.bw-title',
        hidden: group,
        zIndex: focused === it.path ? 2 : 1,
      })
    }
    for (const c of layer.chips) {
      const owner = saved.windows[c.owner]
      if (!owner) continue
      // On the cards level an unmoved window's column keeps to the window's height.
      const pinned = saved.chips[c.entity]
      if (!pinned && level === 'card' && c.index >= Math.max(3, Math.floor(owner.h / (CHIP_STEP * scale)))) continue
      const e = g?.entities[c.entity]
      const p = pinned ?? chipSpot(owner, c.index, scale)
      let data = chipData.current.get(c.entity)
      const next: ChipData = { entity: c.entity, name: e?.name ?? c.entity, kind: kindWord(e?.stereotype), scale, selected: selected === c.entity }
      if (!data || data.name !== next.name || data.kind !== next.kind || data.scale !== next.scale || data.selected !== next.selected) {
        data = next
        chipData.current.set(c.entity, data)
      }
      out.push({
        id: chipId(c.entity),
        type: 'chip',
        position: p,
        width: CHIP_W * scale,
        height: CHIP_H * scale,
        data,
        hidden: group,
        zIndex: 3,
      })
    }
    for (const grp of overview.groups) {
      out.push({
        id: groupId(grp.key),
        type: 'group',
        position: { x: grp.rect.x, y: grp.rect.y },
        width: grp.rect.w,
        height: grp.rect.h,
        data: { group: grp },
        draggable: false,
        zIndex: 0,
      })
    }
    return out
  }, [items, saved.windows, saved.chips, layer, g, level, scale, focused, selected, overview])

  const edges = useMemo<BoardEdgeType[]>(() => {
    const arrow = { type: MarkerType.ArrowClosed, width: 16, height: 16 }
    if (level === 'group') {
      return overview.edges.map((e) => ({ id: e.id, source: e.source, target: e.target, type: 'floating', data: { kind: 'lift', label: e.label, scale: 1 }, markerEnd: arrow }))
    }
    return layer.edges.map((e) => ({
      id: e.id,
      source: e.source,
      target: e.target,
      type: 'floating',
      data: { kind: e.kind, label: e.label, scale },
      markerEnd: e.kind === 'rel' || e.kind === 'from' ? arrow : undefined,
    }))
  }, [level, overview, layer, scale])

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
      .map((i) => [i.path, saved.windows[i.path]] as [string, Rect | undefined])
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

  const zoomToRect = useCallback(
    (r: Rect, minZoom: number) => {
      const { width, height } = storeApi.getState()
      const z = Math.min(1, Math.max(minZoom, Math.min(width / (r.w + 80), height / (r.h + 80))))
      void rf.setCenter(r.x + r.w / 2, r.y + r.h / 2, { zoom: z, duration: 450 })
    },
    [rf, storeApi],
  )

  // Refs so the actions stay one object for the board's life: a live page never
  // re-renders because the board did.
  const state = useRef({ saved, rects, overview, layer, scale })
  state.current = { saved, rects, overview, layer, scale }
  const actions = useMemo<BoardActions>(
    () => ({
      zoomToNode: (id) => {
        if (id.startsWith('g:')) {
          const grp = state.current.overview.groups.find((x) => groupId(x.key) === id)
          if (grp) zoomToRect(grp.bounds, 0.16)
          return
        }
        const r = state.current.rects.get(id)
        if (r) zoomToRect(r, 0.2)
      },
      open: (path, { from, near }) => {
        const ws = useWorkspace.getState()
        if (!ws.items.some((i) => i.path === path)) {
          if (from) hints.current.set(path, { from })
          else if (near) {
            const r = state.current.rects.get(chipId(near))
            if (r) hints.current.set(path, { near: { x: r.x + (state.current.scale - 1) * CHIP_W, y: r.y } })
          }
        }
        ws.openItem(path, from ? { from } : undefined)
      },
      close: (path) => {
        useWorkspace.getState().closeItem(path)
        setSaved((s) => {
          const w = { ...s.windows }
          delete w[path]
          return { ...s, windows: w }
        })
      },
      resize: (path, r) => setSaved((s) => ({ ...s, windows: { ...s.windows, [path]: { x: r.x, y: r.y, w: Math.round(r.w), h: Math.round(r.h) } } })),
      chipAt: (entity) => state.current.rects.get(chipId(entity)),
    }),
    [zoomToRect],
  )

  const onNodeClick = useCallback<NodeMouseHandler<BoardNode>>(
    (e, node) => {
      const t = e.target as HTMLElement
      if (node.type === 'window') {
        if (t.closest('.bw-close, .bw-resize')) return
        const lv = useRuntime.getState().level
        const path = (node.data as WindowData).path
        if (lv !== 'live' && t.closest('.bw-body')) {
          actions.zoomToNode(node.id)
        } else if (useWorkspace.getState().focused !== path) {
          selfFocus.current = path
          useWorkspace.getState().focusItem(path)
        }
      } else if (node.type === 'chip') {
        useWorkspace.getState().select((node.data as ChipData).entity)
      } else if (node.type === 'group') {
        actions.zoomToNode(node.id)
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
      } else {
        actions.zoomToNode(node.id)
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
    // A chip with no edge to its window still ranks beside it.
    const linked = new Set(l.edges.map((e) => `${e.source}|${e.target}`))
    for (const c of l.chips) {
      const w = windowId(c.owner)
      if (!linked.has(`${w}|${chipId(c.entity)}`)) edgesIn.push({ source: w, target: chipId(c.entity) })
    }
    const pos = layout({ nodes: nodesIn, edges: edgesIn })
    const windows = { ...s.windows }
    const chips = { ...s.chips }
    for (const [id, p] of pos) {
      if (id.startsWith('w:') && windows[id.slice(2)]) windows[id.slice(2)] = { ...windows[id.slice(2)], x: p.x, y: p.y }
      else if (id.startsWith('c:')) chips[id.slice(2)] = p
    }
    setSaved((x) => ({ ...x, windows, chips }))
    window.setTimeout(fit, 60)
  }, [fit])

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
        <MiniMap pannable zoomable position="bottom-right" className="board-minimap" nodeClassName={(n) => `mm-${n.type}`} />
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
