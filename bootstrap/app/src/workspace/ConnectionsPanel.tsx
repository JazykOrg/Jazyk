// Connections, in the inspector's place (docs/frontends/gui.md#workspace-experimental):
// a panel docked under the center showing the node last clicked, compactly. An entity
// is its connections picture, where a click on a neighbour shows the links and
// requirements between the two; a requirement its card; a diagnostic its Inbox card.
// `Open` opens the node's page as an item. The panel collapses to a strip and keeps
// its height and state per browser; on a narrow screen it is a bottom sheet.
import { useCallback, useEffect, useRef, useState } from 'react'
import { useNavigate } from 'react-router'
import Connections from '../components/Connections'
import { RequirementView } from '../components/NodeViews'
import InboxCard from '../components/InboxCard'
import { NodeDetail } from '../workbench/Inspector'
import { useGraph, useMatrix } from '../lib/queries'
import { useNodeName } from '../lib/present'
import { useResolveId } from '../components/NodeLink'
import { useWorkspace } from './items'
import { nodePagePath } from './paths'

const KEY = 'jazyk.workspace.connections'
const MIN = 120

function readPanel(): { open: boolean; height: number } {
  try {
    const v = JSON.parse(localStorage.getItem(KEY) ?? '{}') as { open?: boolean; height?: number }
    return { open: v.open !== false, height: typeof v.height === 'number' ? v.height : 280 }
  } catch {
    return { open: true, height: 280 }
  }
}

function kindWord(id: string): string {
  if (id.startsWith('ent:')) return 'entity'
  if (id.startsWith('req:')) return 'requirement'
  if (id.startsWith('diag:')) return 'problem'
  if (id.startsWith('crit:')) return 'example'
  if (id.startsWith('view:')) return 'diagram'
  return ''
}

function Body({ id }: { id: string }) {
  const graph = useGraph()
  const matrix = useMatrix()
  const g = graph.data
  if (graph.error) return <p className="error-inline">{graph.error.message}</p>
  if (!g) return <p className="muted">loading…</p>
  if (id.startsWith('ent:')) {
    if (!g.entities[id]) return <p className="muted">No such entity.</p>
    return <Connections key={id} id={id} graph={g} withChildren />
  }
  if (id.startsWith('req:') && g.requirements[id]) return <RequirementView id={id} graph={g} row={matrix.data?.rows?.[id]} />
  if (id.startsWith('diag:') && g.diagnostics[id]) return <InboxCard id={id} d={g.diagnostics[id]} />
  return <NodeDetail id={id} />
}

export default function ConnectionsPanel({ narrow }: { narrow: boolean }) {
  const selected = useWorkspace((w) => w.selected)
  const select = useWorkspace((w) => w.select)
  const id = useResolveId(selected ?? '')
  const name = useNodeName()
  const navigate = useNavigate()
  const [state, setState] = useState(readPanel)
  const remember = useCallback((next: { open: boolean; height: number }) => {
    setState(next)
    try {
      localStorage.setItem(KEY, JSON.stringify(next))
    } catch {
      // not remembered
    }
  }, [])
  // A new selection opens a collapsed panel: the click asked to see it.
  const last = useRef(selected)
  useEffect(() => {
    if (selected && selected !== last.current && !state.open) remember({ ...state, open: true })
    last.current = selected
  }, [selected, state, remember])

  // Dragging the top edge sets the height.
  const drag = useRef<{ y: number; h: number } | null>(null)
  const onDown = (e: React.PointerEvent) => {
    drag.current = { y: e.clientY, h: state.height }
    ;(e.target as HTMLElement).setPointerCapture(e.pointerId)
  }
  const onMove = (e: React.PointerEvent) => {
    if (!drag.current) return
    const max = Math.max(MIN, window.innerHeight - 200)
    const h = Math.min(max, Math.max(MIN, drag.current.h + drag.current.y - e.clientY))
    setState((s) => ({ ...s, height: h }))
  }
  const onUp = () => {
    if (!drag.current) return
    drag.current = null
    remember(state)
  }

  // On a phone the sheet leaves with a swipe down.
  const swipe = useRef<number | null>(null)

  const page = selected ? nodePagePath(id) : null
  const title = selected ? name(id) : ''
  const open = () => page && navigate(page)

  if (narrow) {
    if (!selected || !state.open) return null
    return (
      <div className="wsc-sheet" role="dialog" aria-label="Connections">
        <div
          className="wsc-grab"
          onTouchStart={(e) => (swipe.current = e.touches[0].clientY)}
          onTouchEnd={(e) => {
            const from = swipe.current
            swipe.current = null
            if (from !== null && e.changedTouches[0].clientY - from > 60) select(null)
          }}
        />
        <header className="wsc-head">
          <span className="wsc-kind">{kindWord(id)}</span>
          <b className="wsc-title">{title}</b>
          <span className="ws-spacer" />
          {page && (
            <button className="wsc-open" onClick={open}>
              Open
            </button>
          )}
          <button className="wsc-x" aria-label="close Connections" onClick={() => select(null)}>
            ✕
          </button>
        </header>
        <div className="wsc-body">
          <Body id={id} />
        </div>
      </div>
    )
  }

  return (
    <section className={`wsc${state.open ? ' open' : ''}`} style={state.open ? { height: state.height } : undefined} aria-label="Connections">
      {state.open && <div className="wsc-resize" onPointerDown={onDown} onPointerMove={onMove} onPointerUp={onUp} title="drag to resize" />}
      <header className="wsc-head">
        <button className="wsc-toggle" aria-expanded={state.open} onClick={() => remember({ ...state, open: !state.open })}>
          <span aria-hidden="true">{state.open ? '▾' : '▸'}</span> Connections
        </button>
        {selected && (
          <>
            <span className="wsc-kind">{kindWord(id)}</span>
            <b className="wsc-title" title={id}>
              {title}
            </b>
          </>
        )}
        <span className="ws-spacer" />
        {page && (
          <button className="wsc-open" onClick={open}>
            Open
          </button>
        )}
      </header>
      {state.open && (
        <div className="wsc-body">
          {selected ? <Body id={id} /> : <p className="muted wsc-empty">Click a name in a document, a marker, or a diagram box to see what it connects to.</p>}
        </div>
      )}
    </section>
  )
}
