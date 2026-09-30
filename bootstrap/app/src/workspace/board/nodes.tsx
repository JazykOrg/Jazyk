// The board's nodes: a window per open item (live, still, or a card by zoom), a
// chip per entity the windows bring, a region per group on the overview.
import { Component, memo, Suspense, useCallback, useLayoutEffect, useRef, useState, type ReactNode } from 'react'
import { Handle, NodeResizeControl, Position, useStore, type Node, type NodeProps } from '@xyflow/react'
import { useQuery } from '@tanstack/react-query'
import Markdown from 'react-markdown'
import remarkGfm from 'remark-gfm'
import AppRoutes from '../../routes/AppRoutes'
import { get } from '../../lib/api'
import { useCoverage, useDocs, useGraph, useMatrix } from '../../lib/queries'
import { kindWord, shorten, statusWord } from '../../lib/present'
import { itemKind, itemTitle, useWorkspace } from '../items'
import ItemNavigation, { type ItemNavigate } from './ItemNavigation'
import { useBoardActions } from './actions'
import { canonical, docOf, nodeOf, pathOfNode, WIN_MIN_H, WIN_MIN_W, type Group, type Level } from './model'
import { registerBody, stillOf, useRuntime } from './runtime'

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

// Hidden handles: the edges attach to the nodes' borders (FloatingEdge), but
// xyflow draws an edge only between nodes that carry a handle.
function Handles() {
  return (
    <>
      <Handle type="target" position={Position.Left} className="board-handle" isConnectable={false} />
      <Handle type="source" position={Position.Right} className="board-handle" isConnectable={false} />
    </>
  )
}

export type WindowData = { path: string; focused: boolean }
export type WindowNodeType = Node<WindowData, 'window'>

// The title an item's window shows: an entity's or a requirement's own words where
// the graph has them, else the path's.
function useItemTitle(path: string): string {
  const { data: g } = useGraph()
  const kind = itemKind(path)
  if (g && (kind === 'entity' || kind === 'requirement' || kind === 'diagram')) {
    const id = canonical(g, nodeOf(path))
    const name = g.entities[id]?.name ?? g.requirements[id]?.statement ?? g.views[id]?.title
    if (name) return shorten(name, 80)
  }
  return itemTitle(path)
}

class PageBoundary extends Component<{ children: ReactNode }, { error: Error | null }> {
  state = { error: null as Error | null }
  static getDerivedStateFromError(error: Error) {
    return { error }
  }
  render() {
    if (this.state.error) return <p className="bw-empty">this page failed to render: {this.state.error.message}</p>
    return this.props.children
  }
}

// The page itself, navigating through the item: a new page opens as a window
// beside this one, a mention selects its node and opens its page beside, a change
// of this page's own query stays here.
function LiveBody({ path }: { path: string }) {
  const [loc, setLoc] = useState(path)
  const actions = useBoardActions()
  const ref = useRef<HTMLDivElement | null>(null)
  const setRef = useCallback(
    (el: HTMLDivElement | null) => {
      ref.current = el
      registerBody(path, el)
    },
    [path],
  )
  const onNavigate = useCallback<ItemNavigate>(
    (to) => {
      const cur = new URL(loc, 'http://board')
      const next = new URL(to, cur)
      if (next.pathname === cur.pathname) {
        const node = next.searchParams.get('node')
        if (node && node !== cur.searchParams.get('node')) {
          useWorkspace.getState().select(node)
          const page = pathOfNode(node)
          if (page) actions.open(page, { from: path })
        }
        next.searchParams.delete('node')
        next.searchParams.delete('entity')
        const qs = next.searchParams.toString()
        setLoc(`${next.pathname}${qs ? `?${qs}` : ''}`)
        return
      }
      actions.open(`${next.pathname}${next.search}`, { from: path })
    },
    [loc, path, actions],
  )
  return (
    <div className="bw-live" ref={setRef}>
      <PageBoundary>
        <ItemNavigation onNavigate={onNavigate}>
          <Suspense fallback={<p className="bw-empty">loading…</p>}>
            <AppRoutes location={loc} />
          </Suspense>
        </ItemNavigation>
      </PageBoundary>
    </div>
  )
}

interface DocContent {
  path: string
  text: string
  hash: string
}

// The same query the editor reads, so a still and a card share its cache.
function useDocText(doc: string) {
  return useQuery({
    queryKey: ['docs', 'content', doc],
    queryFn: () => get<DocContent>(`/api/docs/content?path=${encodeURIComponent(doc)}`),
    enabled: doc !== '',
    staleTime: 5_000,
  })
}

function EntitySummary({ id }: { id: string }) {
  const { data: g } = useGraph()
  const e = g?.entities[id]
  if (!e) return <p className="bw-empty">{id}</p>
  return (
    <div className="bw-read">
      <h1>{e.name}</h1>
      {e.stereotype && <p className="muted">{kindWord(e.stereotype)}</p>}
      {e.definition && <p>{e.definition}</p>}
      {(e.attributes?.length ?? 0) > 0 && (
        <ul>
          {e.attributes!.map((a) => (
            <li key={a.name}>
              <b>{a.name}</b>
              {a.value ? `: ${a.value}` : a.type ? `: ${a.type}` : ''}
            </li>
          ))}
        </ul>
      )}
    </div>
  )
}

// A still for a window never drawn live in this tab: a document in its Read form,
// an entity's summary, else the title.
function ReadStill({ path, title }: { path: string; title: string }) {
  const kind = itemKind(path)
  const doc = kind === 'doc' ? docOf(path) : ''
  const text = useDocText(doc)
  const { data: g } = useGraph()
  if (kind === 'doc') {
    return (
      <div className="bw-read md">
        {text.data ? <Markdown remarkPlugins={[remarkGfm]}>{text.data.text}</Markdown> : <p className="bw-empty">{title}</p>}
      </div>
    )
  }
  if (kind === 'entity' && g) return <EntitySummary id={canonical(g, nodeOf(path))} />
  return (
    <div className="bw-read">
      <h1>{title}</h1>
    </div>
  )
}

function StillBody({ path, title }: { path: string; title: string }) {
  const still = stillOf(path)
  const ref = useRef<HTMLDivElement>(null)
  useLayoutEffect(() => {
    const el = ref.current
    if (!el || !still) return
    el.setAttribute('inert', '')
    const all = el.querySelectorAll('*')
    for (const [i, top, left] of still.scroll) {
      const n = all[i]
      if (n) {
        n.scrollTop = top
        n.scrollLeft = left
      }
    }
  }, [still])
  if (still) return <div ref={ref} className="bw-still" dangerouslySetInnerHTML={{ __html: still.html }} />
  return (
    <div className="bw-still">
      <ReadStill path={path} title={title} />
    </div>
  )
}

// A card: the title, the kind, a few lines, a status (plans/workspace.md#zoom-levels).
function CardBody({ path, title }: { path: string; title: string }) {
  const kind = itemKind(path)
  const doc = kind === 'doc' ? docOf(path) : ''
  const text = useDocText(doc)
  const { data: g } = useGraph()
  const { data: cov } = useCoverage()
  const { data: docs } = useDocs()
  const { data: matrix } = useMatrix()
  let what: string = kind === 'doc' ? 'Document' : kind === 'entity' ? 'Entity' : kind === 'requirement' ? 'Requirement' : kind === 'diagram' ? 'Diagram' : itemTitle(path.split('/').slice(0, 2).join('/')) || 'Page'
  let lines: string[] = []
  let status = ''
  if (kind === 'doc') {
    lines = (text.data?.text ?? '')
      .split('\n')
      .map((l) => l.replace(/^[#>*\-\s]+|[*_`]/g, '').replace(/\[([^\]]*)\]\([^)]*\)/g, '$1').trim())
      .filter((l) => l.length > 0)
      .slice(1, 5)
    const rec = cov?.[doc]
    const sections = rec ? Object.keys(rec.sections).length : 0
    const covered = rec?.coverage ? Object.values(rec.coverage).filter((c) => c.state === 'covered').length : 0
    const problems = Object.values(docs?.docs.find((d) => d.path === doc)?.diagnostics ?? {}).reduce((a, b) => a + b, 0)
    status = [sections ? `${covered} of ${sections} sections covered` : 'not read yet', problems ? `${problems} problems` : ''].filter(Boolean).join(' · ')
  } else if (g && kind === 'entity') {
    const id = canonical(g, nodeOf(path))
    const e = g.entities[id]
    what = kindWord(e?.stereotype) || 'Entity'
    lines = e?.definition ? [e.definition] : []
    const reqs = Object.values(g.requirements).filter((r) => r.entities?.includes(id)).length
    status = `${reqs} requirements`
  } else if (g && kind === 'requirement') {
    const id = canonical(g, nodeOf(path))
    status = statusWord(matrix?.rows[id]?.status).word
  }
  return (
    <div className="bw-card">
      <div className="bw-card-kind">
        {GLYPH[kind]} {what}
      </div>
      <div className="bw-card-title">{title}</div>
      <div className="bw-card-lines">
        {lines.map((l, i) => (
          <p key={i}>{shorten(l, 140)}</p>
        ))}
      </div>
      {status && <div className="bw-card-status">{status}</div>}
    </div>
  )
}

export const WindowNode = memo(function WindowNode({ id, data, width, height, positionAbsoluteX, positionAbsoluteY }: NodeProps<WindowNodeType>) {
  const { path, focused } = data
  const level = useRuntime((s) => s.level)
  const live = useRuntime((s) => s.level === 'live' && s.live.includes(path))
  const actions = useBoardActions()
  const title = useItemTitle(path)
  const kind = itemKind(path)
  return (
    <div className={`bw lv-${level}${focused ? ' focused' : ''}${live ? ' is-live' : ''}`} data-path={path}>
      <Handles />
      {level !== 'card' && (
        <div className="bw-title" onDoubleClick={() => actions.zoomToNode(id)} title={path}>
          <span className="bw-glyph">{GLYPH[kind]}</span>
          <span className="bw-name">{title}</span>
          <button className="bw-close nodrag" aria-label={`Close ${title}`} onClick={() => actions.close(path)}>
            ×
          </button>
        </div>
      )}
      <div className={`bw-body${live ? ' nodrag nowheel nopan' : ''}`}>
        {level === 'card' ? <CardBody path={path} title={title} /> : live ? <LiveBody path={path} /> : <StillBody path={path} title={title} />}
      </div>
      {level !== 'card' && (
        <NodeResizeControl
          className="bw-resize"
          position="bottom-right"
          minWidth={WIN_MIN_W}
          minHeight={WIN_MIN_H}
          onResize={(_, p) => actions.resize(path, { x: p.x ?? positionAbsoluteX, y: p.y ?? positionAbsoluteY, w: p.width ?? width ?? 0, h: p.height ?? height ?? 0 })}
        />
      )}
    </div>
  )
})

export type ChipData = { entity: string; name: string; kind: string; scale: number; selected: boolean }
export type ChipNodeType = Node<ChipData, 'chip'>

export const ChipNode = memo(function ChipNode({ data }: NodeProps<ChipNodeType>) {
  return (
    <div className={`bc${data.selected ? ' sel' : ''}`} style={{ fontSize: 13 * data.scale }} title={data.kind ? `${data.name} · ${data.kind}` : data.name} data-entity={data.entity}>
      <Handles />
      <span className="bc-glyph">◆</span>
      <span className="bc-name">{data.name}</span>
      {data.kind && data.scale === 1 && <span className="bc-kind">{data.kind}</span>}
    </div>
  )
})

export type GroupData = { group: Group }
export type GroupNodeType = Node<GroupData, 'group'>

export const GroupNode = memo(function GroupNode({ data, width }: NodeProps<GroupNodeType>) {
  const zoom = useStore((s) => s.transform[2])
  const g = data.group
  const counts = [
    g.documents ? `${g.documents} ${g.documents === 1 ? 'document' : 'documents'}` : '',
    g.entities ? `${g.entities} ${g.entities === 1 ? 'entity' : 'entities'}` : '',
    g.pages ? `${g.pages} ${g.pages === 1 ? 'page' : 'pages'}` : '',
  ]
    .filter(Boolean)
    .join(' · ')
  // The label keeps its size on screen whatever the zoom, within its region.
  const s = Math.min(1 / Math.max(zoom, 0.01), (width ?? 1000) / 150)
  return (
    <div className={`bg bg-${g.kind}`}>
      <Handles />
      <div className="bg-label" style={{ fontSize: 15 * s, padding: `${6 * s}px ${10 * s}px` }}>
        <b>{g.label}</b>
        <span className="bg-counts">{counts}</span>
      </div>
    </div>
  )
})

export const nodeTypes = { window: WindowNode, chip: ChipNode, group: GroupNode }

export type BoardNode = WindowNodeType | ChipNodeType | GroupNodeType
export type { Level }
