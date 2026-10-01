// The board's nodes: a window per open item (live, still, a card, or its title by
// zoom; an entity always a card of its own).
import { Component, memo, Suspense, useCallback, useLayoutEffect, useMemo, useRef, useState, type ReactNode } from 'react'
import { Handle, NodeResizeControl, Position, useStore, type Node, type NodeProps } from '@xyflow/react'
import { useQuery } from '@tanstack/react-query'
import Markdown from 'react-markdown'
import remarkGfm from 'remark-gfm'
import AppRoutes from '../../routes/AppRoutes'
import { get } from '../../lib/api'
import { useCoverage, useDocs, useGraph, useMatrix } from '../../lib/queries'
import { kindWord, shorten, statusWord } from '../../lib/present'
import { useApp } from '../../lib/store'
import { ArrangeContext, CloseOrPin, ItemBar } from '../ItemControls'
import LinksBar from './linksbar'
import { isFileItem, itemKind, itemTitle, useWorkspace } from '../items'
import { itemKey, nodePagePath, splitSelection } from '../paths'
import { showPath } from '../actions'
import ItemNavigation, { type ItemNavigate } from './ItemNavigation'
import { useBoardActions } from './actions'
import { canonical, docOf, nodeOf, WIN_MIN_H, WIN_MIN_W } from './model'
import { KINDS } from '../kinds'
import { registerBody, stillOf, useRuntime } from './runtime'


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

export type WindowData = { path: string; focused: boolean; hot: boolean }
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

// The page itself, rendered against its item's path and navigating through the
// item: another page opens as a window beside this one, a `?node=` (a mention, a
// marker) selects the node and opens its page beside, and a change of this page's
// own query retargets the item in place (plans/workspace.md#board).
const LiveBody = memo(function LiveBody({ path }: { path: string }) {
  const actions = useBoardActions()
  const key = itemKey(path)
  const setRef = useCallback((el: HTMLDivElement | null) => registerBody(key, el), [key])
  const arrange = useMemo(() => ({ path, to: 'tabs' as const }), [path])
  const onNavigate = useCallback<ItemNavigate>(
    (to) => {
      const u = new URL(to, new URL(path, 'http://board'))
      const { path: target, node } = splitSelection(`${u.pathname}${u.search}${u.hash}`)
      if (node) {
        useWorkspace.getState().select(node)
        const page = nodePagePath(node)
        if (page && itemKey(page) !== key) actions.open(page, { from: path })
      }
      if (itemKey(target) !== key) actions.open(target, { from: path })
      else if (!node && target !== path) showPath(target)
    },
    [path, key, actions],
  )
  return (
    <div className="bw-live" ref={setRef}>
      <ArrangeContext.Provider value={arrange}>
      <PageBoundary>
        <ItemNavigation onNavigate={onNavigate}>
          <Suspense fallback={<p className="bw-empty">loading…</p>}>
            <AppRoutes location={path} />
          </Suspense>
        </ItemNavigation>
      </PageBoundary>
      </ArrangeContext.Provider>
    </div>
  )
})

interface DocContent {
  path: string
  text: string
  hash: string
}

// The same query the editor reads, so a still and a card share its cache.
export function useDocText(doc: string) {
  return useQuery({
    queryKey: ['docs', 'content', doc],
    queryFn: () => get<DocContent>(`/api/docs/content?path=${encodeURIComponent(doc)}`),
    enabled: doc !== '',
    staleTime: 5_000,
  })
}

// An entity's window: a card, not a page (docs/frontends/gui.md#the-board). Its
// name, kind, parent, definition, attributes, and counts; its connections are the
// board's links around it. The parent and the defining document open beside it.
const EntityCard = memo(function EntityCard({ path }: { path: string }) {
  const { data: g } = useGraph()
  const actions = useBoardActions()
  if (!g) return <p className="bw-empty">loading…</p>
  const id = canonical(g, nodeOf(path))
  const e = g.entities[id]
  if (!e) return <p className="bw-empty">{id}</p>
  const chain: string[] = []
  for (let cur = e.parent; cur && g.entities[cur] && chain.length < 4 && !chain.includes(cur); cur = g.entities[cur].parent) chain.unshift(cur)
  const parts = Object.entries(g.entities).filter(([, x]) => x.parent === id)
  // Every requirement naming it, the ones that concern it alone first: those show
  // on no link, only here.
  const reqs = Object.entries(g.requirements)
    .filter(([, r]) => r.entities?.some((x) => canonical(g, x) === id))
    .map(([rid, r]) => ({ rid, r, alone: new Set((r.entities ?? []).map((x) => canonical(g, x))).size === 1 }))
    .sort((a, b) => Number(b.alone) - Number(a.alone))
  const machine = Object.entries(g.stateMachines ?? {}).find(([, m]) => m.subject === id)
  const stateView = machine ? `view:state/${machine[0].replace(/^sm:/, '')}` : null
  const attrs = e.attributes ?? []
  const open = (to: string) => actions.open(to, { from: path })
  return (
    <div className="bw-entity">
      <div className="bwe-kind">
        {KINDS.entity.glyph} {kindWord(e.stereotype) || 'Entity'}
        {chain.length > 0 && (
          <span className="bwe-chain">
            {' '}in{' '}
            {chain.map((c, i) => (
              <span key={c}>
                {i > 0 && ' › '}
                <button className="bwe-link nodrag" onClick={() => open(`/model/entities/${encodeURIComponent(c)}`)}>
                  {g.entities[c].name}
                </button>
              </span>
            ))}
          </span>
        )}
      </div>
      <h2 className="bwe-name">{e.name}</h2>
      {e.definition && <p className="bwe-def">{e.definition}</p>}
      {attrs.length > 0 && (
        <ul className="bwe-attrs">
          {attrs.map((a) => (
            <li key={a.name}>
              <b>{a.name}</b>
              {a.value ? `: ${a.value}` : a.type ? `: ${a.type}` : ''}
            </li>
          ))}
        </ul>
      )}
      {parts.length > 0 && (
        <div className="bwe-sect">
          <h3>Parts</h3>
          <p className="bwe-chips">
            {parts.map(([pid, p]) => (
              <button key={pid} className="bwe-link nodrag" onClick={() => open(`/model/entities/${encodeURIComponent(pid)}`)}>
                {KINDS.entity.glyph} {p.name}
              </button>
            ))}
          </p>
        </div>
      )}
      {machine && (
        <div className="bwe-sect">
          <h3>States</h3>
          <p>
            {machine[1].states.join(' → ')}{' '}
            {stateView && g.views[stateView] && (
              <button className="bwe-link nodrag" onClick={() => open(`/model/diagrams/${encodeURIComponent(stateView)}`)}>
                see the states
              </button>
            )}
          </p>
        </div>
      )}
      {reqs.length > 0 && (
        <div className="bwe-sect">
          <h3>Requirements</h3>
          <ul className="lc-reqs open bwe-reqs">
            {reqs.map(({ rid, r, alone }) => (
              <li key={rid} data-req={rid} className={alone ? 'alone' : ''}>
                <button className="lc-req nodrag" onClick={() => actions.reveal?.(rid)} title={`${r.statement}\n(click: show it in its document)`}>
                  {r.statement}
                  {alone && <span className="bwe-alone"> · only this</span>}
                </button>
                <button className="lc-open nodrag" onClick={() => open(`/model/requirements/${encodeURIComponent(rid)}`)} title="Open the requirement as a window" aria-label="Open the requirement">
                  ↗
                </button>
              </li>
            ))}
          </ul>
        </div>
      )}
    </div>
  )
})

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
  return (
    <div className="bw-read">
      <h1>{title}</h1>
    </div>
  )
}

const StillBody = memo(function StillBody({ path, title }: { path: string; title: string }) {
  const still = stillOf(itemKey(path))
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
})

// A card: the title, the kind, a few lines, a status (plans/workspace.md#zoom-levels).
const CardBody = memo(function CardBody({ path, title }: { path: string; title: string }) {
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
        {KINDS[kind].glyph} {what}
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
})

// The farthest level: only the title, sized to read on screen, where the window is.
function TitleBody({ path, title }: { path: string; title: string }) {
  const zoom = useStore((st) => st.transform[2])
  const size = 14 / Math.max(zoom, 0.01)
  return (
    <div className="bw-titleonly" style={{ fontSize: size }}>
      <span className="bw-glyph">{KINDS[itemKind(path)].glyph}</span> {title}
    </div>
  )
}

export const WindowNode = memo(function WindowNode({ id, data, width, height, positionAbsoluteX, positionAbsoluteY }: NodeProps<WindowNodeType>) {
  const { path, focused, hot } = data
  const level = useRuntime((s) => s.level)
  const kind = itemKind(path)
  const entity = kind === 'entity'
  const live = useRuntime((s) => s.level === 'live' && (entity || s.live.includes(itemKey(path))))
  const actions = useBoardActions()
  const title = useItemTitle(path)
  const framed = level === 'live' || level === 'still'
  let body: ReactNode
  if (level === 'title') body = <TitleBody path={path} title={title} />
  else if (level === 'card') body = <CardBody path={path} title={title} />
  else if (entity) body = <EntityCard path={path} />
  else body = live ? <LiveBody path={path} /> : <StillBody path={path} title={title} />
  const press = useRef<{ x: number; y: number } | null>(null)
  // A transient item floats while unpinned and sits on the table once pinned.
  const transient = !isFileItem(path)
  const pinned = useWorkspace((w) => !!w.items.find((i) => i.path === path)?.pinned)
  const arrange = useMemo(() => ({ path, to: 'tabs' as const }), [path])
  return (
    <div
      className={`bw k-${kind} lv-${level}${transient ? (pinned ? ' pinned' : ' floating') : ''}${focused ? ' focused' : ''}${live ? ' is-live' : ''}${entity ? ' bw-ent' : ''}${hot ? ' hot' : ''}`}
      data-path={path}
      // A press focuses the item without moving the board, so a press and a drag
      // moves the window; a click (released where it started) focuses it fully.
      onPointerDownCapture={(e) => {
        press.current = { x: e.clientX, y: e.clientY }
        actions.focus(path, false)
      }}
      onClickCapture={(e) => {
        const p = press.current
        if (p && Math.hypot(e.clientX - p.x, e.clientY - p.y) > 4) return
        if ((e.target as HTMLElement).closest('.bw-close, .bw-resize')) return
        // The press may have opened another item (a Cmd-click on a mention), which
        // keeps the focus.
        if (itemKey(useWorkspace.getState().focused ?? '') !== itemKey(path)) return
        actions.focus(path, true)
      }}
    >
      <Handles />
      {framed && (
        <div className="bw-title" title={path}>
          <span className="bw-glyph">{KINDS[kind].glyph}</span>
          <span className="bw-name">{kind === 'doc' ? docOf(path) : title}</span>
          <CloseOrPin className="bw-close nodrag" path={path} title={title} onClose={() => actions.close(path)} />
        </div>
      )}
      {framed && kind !== 'doc' && (
        <ArrangeContext.Provider value={arrange}>
          <ItemBar path={path} />
        </ArrangeContext.Provider>
      )}
      <div className={`bw-body${live ? ' nodrag nowheel nopan' : ''}`}>{body}</div>
      {framed && <LinksBar path={path} />}
      {framed && (
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

export const nodeTypes = { window: WindowNode }

export type BoardNode = WindowNodeType
