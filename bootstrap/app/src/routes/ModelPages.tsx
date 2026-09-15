// The Model lens's center pages (docs/frontends/gui.md#model): a landing with the
// counts, an entity page (the card, the level diagram, the detail), a diagram
// page (the rendering), a requirement page.
import { Link, useParams } from 'react-router'
import { useGraph, useStatus, useView, useViews } from '../lib/queries'
import { useWords } from '../lib/words'
import ExploreCard from '../components/ExploreCard'
import DiagramSvg from '../components/DiagramSvg'
import { NodeDetail } from '../workbench/Inspector'
import '../shell/shell.css'

export function ModelHome() {
  const w = useWords()
  const { data: s } = useStatus()
  const views = useViews()
  const kinds = new Map<string, number>()
  for (const v of views.data?.views ?? []) kinds.set(v.kind, (kinds.get(v.kind) ?? 0) + 1)
  return (
    <div className="ws-scroll narrow">
      <div className="page-head">
        <h1>Model</h1>
        <span className="muted">what the documents describe, as one graph</span>
      </div>
      {s && (
        <div className="stat-strip">
          <span className="stat">
            <b>{s.counts.entities}</b> entities
          </span>
          <span className="stat">
            <b>{s.counts.requirements}</b> requirements
          </span>
          <span className="stat">
            <b>{s.counts.relationships}</b> {w.relations.toLowerCase()}
          </span>
          <Link to="/model/diagrams" className="stat">
            <b>{s.counts.views ?? views.data?.views.length ?? 0}</b> {w.diagrams.toLowerCase()}
          </Link>
        </div>
      )}
      {s && s.counts.entities === 0 && (
        <div className="empty-state">
          <b>Nothing extracted yet</b>
          Run a sync from the button in the top bar. The entities and requirements it reads out of the documents appear here.
        </div>
      )}
      <p className="muted">Pick an entity on the left to open its page, or a diagram to see the picture.</p>
      {kinds.size > 0 && (
        <>
          <h2>{w.diagrams}</h2>
          <ul>
            {[...kinds.entries()].map(([k, n]) => (
              <li key={k}>
                <Link to={`/model/diagrams?kind=${k}`}>
                  {n} {k}
                </Link>
              </li>
            ))}
          </ul>
        </>
      )}
    </div>
  )
}

export function EntityPage() {
  const { id = '' } = useParams()
  const eid = decodeURIComponent(id)
  const graph = useGraph()
  const ent = graph.data?.entities[eid]
  return (
    <div className="ws-scroll">
      {!ent && graph.data && <p className="empty">no entity {eid}</p>}
      {ent && (
        <>
          <div className="page-head">
            <h1>{ent.name}</h1>
            {ent.stereotype && <span className="chip sev-none">«{ent.stereotype}»</span>}
            <span className="mono muted">{eid}</span>
          </div>
          <ExploreCard id={eid} />
          <LevelDiagram id={eid} />
          <NodeDetail id={eid} />
        </>
      )}
    </div>
  )
}

function LevelDiagram({ id }: { id: string }) {
  const graph = useGraph()
  const slug = id.replace(/^ent:/, '')
  const views = useViews()
  const own = (views.data?.views ?? []).find((v) => v.id === `view:class/${slug}` || v.id === `view:component/${slug}`)
  const view = useView(own?.id ?? '')
  if (!own || !graph.data) return null
  const svg = view.data?.svg
  if (!svg) return null
  return (
    <div className="card">
      <p className="row" style={{ margin: 0 }}>
        <b>{own.title}</b>
        <Link to={`/model/diagrams/${encodeURIComponent(own.id)}`} className="muted">
          open the diagram →
        </Link>
      </p>
      <DiagramSvg svg={svg} />
    </div>
  )
}

export function DiagramGallery() {
  const w = useWords()
  const views = useViews()
  const list = views.data?.views ?? []
  const byKind = new Map<string, typeof list>()
  for (const v of list) {
    const g = byKind.get(v.kind)
    if (g) g.push(v)
    else byKind.set(v.kind, [v])
  }
  return (
    <div className="ws-scroll narrow">
      <h1>{w.diagrams}</h1>
      {list.length === 0 && views.data && (
        <div className="empty-state">
          <b>No diagrams yet</b>
          Every sync draws a class diagram per level, a component diagram per system, and a flow diagram per story it finds.
        </div>
      )}
      {[...byKind.entries()].map(([k, vs]) => (
        <div key={k}>
          <h2>{k}</h2>
          {vs.map((v) => (
            <p key={v.id} className="row" style={{ margin: '2px 0' }}>
              <Link to={`/model/diagrams/${encodeURIComponent(v.id)}`}>{v.title}</Link>
              <span className="muted">
                {v.members} members · {v.edges} edges{v.default ? ' · default' : ''}
              </span>
              {v.limits.some((l) => l.over) && <span className="chip sev-warning">over limit</span>}
            </p>
          ))}
        </div>
      ))}
    </div>
  )
}

export function DiagramPage() {
  const { id = '' } = useParams()
  const vid = decodeURIComponent(id)
  const view = useView(vid)
  const v = view.data
  return (
    <div className="ws-scroll">
      {view.error && <p className="error-inline">{view.error.message}</p>}
      {v && (
        <>
          <div className="page-head">
            <h1>{v.title}</h1>
            <span className="chip sev-none">{v.kind}</span>
            {v.default && <span className="chip sev-info">default</span>}
            <span className="mono muted">{vid}</span>
          </div>
          {v.svg ? <DiagramSvg svg={v.svg} /> : <p className="muted">{v.renderError ?? 'no rendering for this diagram yet'}</p>}
          <NodeDetail id={vid} />
        </>
      )}
    </div>
  )
}

export function RequirementPage() {
  const { id = '' } = useParams()
  const rid = decodeURIComponent(id)
  const graph = useGraph()
  const r = graph.data?.requirements[rid]
  return (
    <div className="ws-scroll">
      {!r && graph.data && <p className="empty">no requirement {rid}</p>}
      {r && (
        <>
          <div className="page-head">
            <h1 style={{ fontSize: 18 }}>{r.statement}</h1>
            <span className="mono muted">{rid}</span>
          </div>
          <NodeDetail id={rid} />
        </>
      )}
    </div>
  )
}
