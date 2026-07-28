// Map: the entity graph on a cytoscape canvas. Nodes are entities, edges the
// derived relationships styled by type rank; filters cover scope, edge type,
// and neighborhood focus (docs/frontends/gui.md, "What it shows").
import { useEffect, useMemo, useRef, useState } from 'react'
import { useSearchParams } from 'react-router'
import cytoscape from 'cytoscape'
import fcose from 'cytoscape-fcose'
import { useGraph } from '../lib/queries'
import type { Entity, Graph } from '../lib/api'
import NodeLink from '../components/NodeLink'
import SectionLink from '../components/SectionLink'
import './map.css'

cytoscape.use(fcose)

// Relationship types, strongest first.
const EDGE_TYPES = [
  'generalization',
  'realization',
  'composition',
  'aggregation',
  'association',
  'dependency',
  'reference',
] as const
type EdgeType = (typeof EDGE_TYPES)[number]

// UML notation per type. members[0] is the source `a`, members[1] the target `b`:
// the triangle points at the general entity (target), the diamond sits at the
// owning end (source).
interface EdgeSpec {
  line: 'solid' | 'dashed' | 'dotted'
  width: number
  target?: { shape: 'triangle' | 'vee'; fill: cytoscape.Css.ArrowFill }
  source?: { shape: 'diamond'; fill: cytoscape.Css.ArrowFill }
}
const EDGE_STYLE: Record<EdgeType, EdgeSpec> = {
  generalization: { line: 'solid', width: 2, target: { shape: 'triangle', fill: 'hollow' } },
  realization: { line: 'dashed', width: 2, target: { shape: 'triangle', fill: 'hollow' } },
  composition: { line: 'solid', width: 2, source: { shape: 'diamond', fill: 'filled' } },
  aggregation: { line: 'solid', width: 2, source: { shape: 'diamond', fill: 'hollow' } },
  association: { line: 'solid', width: 1.5 },
  dependency: { line: 'dashed', width: 1.5, target: { shape: 'vee', fill: 'filled' } },
  reference: { line: 'dotted', width: 1.5 },
}

const nonPublic = (ent: Entity): boolean => !!ent.scope && ent.scope !== 'public'

// The fcose options we use (the plugin ships no types).
interface FcoseLayoutOptions {
  name: 'fcose'
  animate?: boolean
  randomize?: boolean
  fit?: boolean
  quality?: 'draft' | 'default' | 'proof'
  fixedNodeConstraint?: { nodeId: string; position: cytoscape.Position }[]
}

interface Palette {
  ink: string
  muted: string
  line: string
  accent: string
  panel: string
}

function readPalette(): Palette {
  const s = getComputedStyle(document.documentElement)
  const v = (name: string) => s.getPropertyValue(name).trim()
  return { ink: v('--ink'), muted: v('--muted'), line: v('--line'), accent: v('--accent'), panel: v('--panel') }
}

function buildStyle(p: Palette): cytoscape.StylesheetStyle[] {
  return [
    {
      selector: 'node',
      style: {
        'background-color': p.panel,
        'border-color': p.muted,
        'border-width': 1.5,
        width: 'data(size)',
        height: 'data(size)',
        label: 'data(label)',
        color: p.ink,
        'font-size': 10,
        'text-valign': 'bottom',
        'text-margin-y': 5,
        'text-wrap': 'wrap',
        'text-max-width': '120px',
      },
    },
    { selector: 'node[nonpublic = 1]', style: { 'border-style': 'dashed' } },
    { selector: 'node:selected', style: { 'border-color': p.accent, 'border-width': 3, color: p.accent } },
    {
      selector: 'edge',
      style: {
        'curve-style': 'bezier',
        'line-color': p.muted,
        opacity: 0.55,
        'source-arrow-color': p.muted,
        'target-arrow-color': p.muted,
        'arrow-scale': 1.2,
      },
    },
    ...EDGE_TYPES.map((t): cytoscape.StylesheetStyle => {
      const s = EDGE_STYLE[t]
      const style: cytoscape.Css.Edge = {
        width: s.width,
        'line-style': s.line,
        'target-arrow-shape': s.target?.shape ?? 'none',
        'source-arrow-shape': s.source?.shape ?? 'none',
      }
      if (s.target) style['target-arrow-fill'] = s.target.fill
      if (s.source) style['source-arrow-fill'] = s.source.fill
      return { selector: `edge[type = "${t}"]`, style }
    }),
    {
      selector: 'edge:selected',
      style: {
        'line-color': p.accent,
        'source-arrow-color': p.accent,
        'target-arrow-color': p.accent,
        opacity: 1,
        'z-index': 10,
      },
    },
    { selector: '.dim', style: { opacity: 0.12 } },
    { selector: '.hidden', style: { display: 'none' } },
  ]
}

// Positions survive filter toggles, refetches, and route changes within the session.
const posCache = new Map<string, cytoscape.Position>()

// Follow redirects until an id lands on a live entity.
function resolveEntity(graph: Graph, id: string): string | null {
  let cur = id
  const seen = new Set<string>()
  while (!graph.entities[cur] && graph.redirects[cur] && !seen.has(cur)) {
    seen.add(cur)
    cur = graph.redirects[cur]
  }
  return graph.entities[cur] ? cur : null
}

type Sel = { kind: 'node' | 'edge'; id: string } | null
type ScopeFilter = 'all' | 'public' | 'non-public'
type FocusHops = 'off' | '1' | '2'

export default function MapView() {
  const { data: graph, error } = useGraph()
  const [params, setParams] = useSearchParams()
  const [query, setQuery] = useState('')
  const [scope, setScope] = useState<ScopeFilter>('all')
  const [types, setTypes] = useState<Record<EdgeType, boolean>>(
    () => Object.fromEntries(EDGE_TYPES.map((t) => [t, t !== 'reference'])) as Record<EdgeType, boolean>,
  )
  const [focusHops, setFocusHops] = useState<FocusHops>('off')
  const [sel, setSel] = useState<Sel>(null)
  const boxRef = useRef<HTMLDivElement>(null)
  const cyRef = useRef<cytoscape.Core | null>(null)
  const prevSelRef = useRef<Sel>(null)
  const focusParam = params.get('focus')

  // Create the canvas once; restyle on theme change; destroy on unmount.
  useEffect(() => {
    if (!boxRef.current) return
    const cy = cytoscape({
      container: boxRef.current,
      style: buildStyle(readPalette()),
      minZoom: 0.1,
      maxZoom: 4,
    })
    cyRef.current = cy
    cy.on('tap', 'node', (e) => setSel({ kind: 'node', id: (e.target as cytoscape.NodeSingular).id() }))
    cy.on('tap', 'edge', (e) => setSel({ kind: 'edge', id: (e.target as cytoscape.EdgeSingular).id() }))
    cy.on('tap', (e) => {
      if (e.target === cy) setSel(null)
    })
    cy.on('dbltap', 'node', (e) => {
      const n = e.target as cytoscape.NodeSingular
      setSel({ kind: 'node', id: n.id() })
      setFocusHops('1')
      cy.center(n)
    })
    cy.on('dragfree layoutstop', () => {
      cy.nodes().forEach((n) => {
        posCache.set(n.id(), { ...n.position() })
      })
    })
    const restyle = () => cy.style(buildStyle(readPalette()))
    const mo = new MutationObserver(restyle)
    mo.observe(document.documentElement, { attributes: true, attributeFilter: ['data-theme'] })
    const mq = window.matchMedia('(prefers-color-scheme: dark)')
    mq.addEventListener('change', restyle)
    return () => {
      mo.disconnect()
      mq.removeEventListener('change', restyle)
      cy.destroy()
      cyRef.current = null
    }
  }, [])

  // Diff elements in place so cached positions hold across filter toggles and
  // generation refetches; layout only what is new (full re-layout when >30% is new).
  useEffect(() => {
    const cy = cyRef.current
    if (!cy || !graph) return

    const degree = new Map<string, number>()
    for (const rel of Object.values(graph.relationships)) {
      for (const m of rel.members) {
        const id = resolveEntity(graph, m)
        if (id) degree.set(id, (degree.get(id) ?? 0) + 1)
      }
    }

    const wantNodes = new Map<string, { id: string; label: string; size: number; nonpublic: number }>()
    for (const [id, ent] of Object.entries(graph.entities)) {
      if (scope === 'public' && nonPublic(ent)) continue
      if (scope === 'non-public' && !nonPublic(ent)) continue
      const d = degree.get(id) ?? 0
      wantNodes.set(id, {
        id,
        label: ent.name,
        size: Math.round(16 + Math.sqrt(d) * 7),
        nonpublic: nonPublic(ent) ? 1 : 0,
      })
    }

    const wantEdges = new Map<string, { id: string; source: string; target: string; type: string }>()
    for (const [rid, rel] of Object.entries(graph.relationships)) {
      if (!types[rel.type as EdgeType]) continue
      if (rel.members.length < 2) continue
      const a = resolveEntity(graph, rel.members[0])
      const b = resolveEntity(graph, rel.members[1])
      if (!a || !b || !wantNodes.has(a) || !wantNodes.has(b)) continue
      wantEdges.set(rid, { id: rid, source: a, target: b, type: rel.type })
    }

    const fresh: string[] = []
    cy.batch(() => {
      cy.edges().forEach((e) => {
        if (!wantEdges.has(e.id())) e.remove()
      })
      cy.nodes().forEach((n) => {
        if (!wantNodes.has(n.id())) n.remove()
      })
      for (const [id, data] of wantNodes) {
        const ex = cy.getElementById(id)
        if (ex.nonempty()) {
          ex.data(data)
        } else {
          const pos = posCache.get(id)
          cy.add({ group: 'nodes', data, position: pos ? { ...pos } : undefined })
          if (!pos) fresh.push(id)
        }
      }
      for (const [id, data] of wantEdges) {
        const ex = cy.getElementById(id)
        if (ex.nonempty()) ex.data(data)
        else cy.add({ group: 'edges', data })
      }
    })

    if (fresh.length > 0) {
      const total = cy.nodes().length
      const randomize = fresh.length > total * 0.3
      const freshSet = new Set(fresh)
      const fixed: { nodeId: string; position: cytoscape.Position }[] = []
      if (!randomize) {
        cy.nodes().forEach((n) => {
          if (!freshSet.has(n.id())) fixed.push({ nodeId: n.id(), position: { ...n.position() } })
        })
      }
      const opts: FcoseLayoutOptions = {
        name: 'fcose',
        animate: false,
        randomize,
        fit: randomize,
        quality: 'default',
        fixedNodeConstraint: fixed.length > 0 ? fixed : undefined,
      }
      cy.layout(opts as unknown as cytoscape.LayoutOptions).run()
    }
  }, [graph, scope, types])

  // Text search dims non-matching nodes (and edges with both ends dimmed).
  useEffect(() => {
    const cy = cyRef.current
    if (!cy) return
    const q = query.trim().toLowerCase()
    cy.batch(() => {
      if (!q) {
        cy.elements().removeClass('dim')
        return
      }
      cy.nodes().forEach((n) => {
        const hit =
          (n.data('label') as string).toLowerCase().includes(q) || n.id().toLowerCase().includes(q)
        n.toggleClass('dim', !hit)
      })
      cy.edges().forEach((e) => {
        e.toggleClass('dim', e.source().hasClass('dim') && e.target().hasClass('dim'))
      })
    })
  }, [query, graph, scope, types])

  // Focus mode hides everything outside the selected node's neighborhood.
  useEffect(() => {
    const cy = cyRef.current
    if (!cy) return
    cy.batch(() => {
      cy.elements().removeClass('hidden')
      if (focusHops === 'off' || sel?.kind !== 'node') return
      const root = cy.getElementById(sel.id)
      if (root.empty()) return
      let hood = root.closedNeighborhood()
      if (focusHops === '2') hood = hood.union(hood.nodes().closedNeighborhood())
      cy.elements().not(hood).addClass('hidden')
    })
  }, [sel, focusHops, graph, scope, types])

  // ?focus=ent:x deep link: select and center once the node exists.
  useEffect(() => {
    const cy = cyRef.current
    if (!cy || !focusParam || prevSelRef.current?.id === focusParam) return
    const n = cy.getElementById(focusParam)
    if (n.nonempty()) {
      setSel({ kind: 'node', id: focusParam })
      cy.center(n)
    }
  }, [focusParam, graph])

  // Selection state drives cy selection and the URL.
  useEffect(() => {
    const before = prevSelRef.current
    prevSelRef.current = sel
    const cy = cyRef.current
    if (cy) {
      cy.$(':selected').unselect()
      if (sel) cy.getElementById(sel.id).select()
    }
    if (!sel && !before) return // initial mount: leave a deep-link param alone
    setParams(
      (p) => {
        const next = new URLSearchParams(p)
        if (sel?.kind === 'node') {
          if (next.get('focus') === sel.id) return p
          next.set('focus', sel.id)
        } else {
          if (!next.has('focus')) return p
          next.delete('focus')
        }
        return next
      },
      { replace: true },
    )
  }, [sel, graph, scope, types])

  const selDegree = useMemo(() => {
    if (!graph || sel?.kind !== 'node') return 0
    return Object.values(graph.relationships).filter((r) =>
      r.members.some((m) => resolveEntity(graph, m) === sel.id),
    ).length
  }, [graph, sel])

  const empty = graph && Object.keys(graph.entities).length === 0

  return (
    <div className="map-root">
      <div className="map-toolbar">
        <input
          type="search"
          placeholder="search entities"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
        />
        <select value={scope} onChange={(e) => setScope(e.target.value as ScopeFilter)}>
          <option value="all">all scopes</option>
          <option value="public">public</option>
          <option value="non-public">non-public</option>
        </select>
        <div className="map-types">
          {EDGE_TYPES.map((t) => (
            <label key={t} className="mono">
              <input
                type="checkbox"
                checked={types[t]}
                onChange={() => setTypes({ ...types, [t]: !types[t] })}
              />
              {t}
            </label>
          ))}
        </div>
        <select
          value={focusHops}
          disabled={sel?.kind !== 'node'}
          onChange={(e) => setFocusHops(e.target.value as FocusHops)}
          title="neighborhood focus (needs a selected node)"
        >
          <option value="off">focus: off</option>
          <option value="1">focus: 1 hop</option>
          <option value="2">focus: 2 hops</option>
        </select>
      </div>
      <div className="map-body">
        <div className="map-canvas">
          <div className="map-cy" ref={boxRef} />
          {error ? (
            <p className="error-inline map-empty">{String(error)}</p>
          ) : empty ? (
            <p className="empty map-empty">No entities in the graph yet. Run a compile to populate it.</p>
          ) : null}
        </div>
        <aside className="map-detail">
          {sel && graph ? <Detail graph={graph} sel={sel} degree={selDegree} /> : <Legend />}
        </aside>
      </div>
    </div>
  )
}

function Detail({ graph, sel, degree }: { graph: Graph; sel: NonNullable<Sel>; degree: number }) {
  if (sel.kind === 'node') {
    const ent = graph.entities[sel.id]
    if (!ent) return <p className="empty">selection is no longer in the graph</p>
    return (
      <>
        <h3>{ent.name}</h3>
        <div className="map-field">
          <NodeLink id={sel.id} />
        </div>
        <div className="map-field muted">
          scope: {ent.scope ?? 'public'} · degree: {degree}
        </div>
        {ent.definition && <p className="map-def">{ent.definition}</p>}
        <div className="map-field">
          <NodeLink id={sel.id}>open →</NodeLink>
        </div>
      </>
    )
  }
  const rel = graph.relationships[sel.id]
  if (!rel) return <p className="empty">selection is no longer in the graph</p>
  return (
    <>
      <h3>{rel.type}</h3>
      {rel.members.map((m) => {
        const id = resolveEntity(graph, m)
        return (
          <div className="map-field" key={m}>
            {id ? (
              <>
                {graph.entities[id].name} <NodeLink id={id} />
              </>
            ) : (
              <span className="muted mono">{m}</span>
            )}
          </div>
        )
      })}
      <h3 className="map-reqs-head">requirements</h3>
      {rel.requirements.map((rid) => {
        const rq = graph.requirements[rid]
        if (!rq) return null
        return (
          <div className="map-req" key={rid}>
            <div>{rq.ears}</div>
            <SectionLink doc={rq.source.doc} section={rq.source.section} quote={rq.source.quote} />
          </div>
        )
      })}
    </>
  )
}

// One legend sample per type, drawn with the same UML markers as the canvas:
// diamond at the source (left) end, triangle or vee at the target (right) end.
function EdgeGlyph({ type }: { type: EdgeType }) {
  const s = EDGE_STYLE[type]
  const w = 56
  const y = 7
  const dash = s.line === 'dashed' ? '6 4' : s.line === 'dotted' ? '1.5 3' : undefined
  const x1 = s.source ? 14 : 1
  const x2 = s.target ? w - 10 : w - 1
  return (
    <svg width={w} height={14} className="legend-glyph" aria-hidden="true">
      <line className="l" x1={x1} y1={y} x2={x2} y2={y} strokeWidth={s.width} strokeDasharray={dash} />
      {s.source && (
        <polygon
          className={s.source.fill}
          points={`1,${y} 8,${y - 4.5} 15,${y} 8,${y + 4.5}`}
          strokeWidth={1.2}
        />
      )}
      {s.target?.shape === 'triangle' && (
        <polygon
          className={s.target.fill}
          points={`${w - 1},${y} ${w - 11},${y - 5} ${w - 11},${y + 5}`}
          strokeWidth={1.2}
        />
      )}
      {s.target?.shape === 'vee' && (
        <polyline
          className="l"
          points={`${w - 9},${y - 5} ${w - 1},${y} ${w - 9},${y + 5}`}
          strokeWidth={1.5}
        />
      )}
    </svg>
  )
}

function Legend() {
  return (
    <>
      <h3>edge types</h3>
      {EDGE_TYPES.map((t) => (
        <div className="legend-row" key={t}>
          <EdgeGlyph type={t} />
          <span className="mono">{t}</span>
        </div>
      ))}
      <p className="muted map-hint">
        UML notation: the diamond sits at the owning end, the triangle points at the general entity.
      </p>
      <p className="muted map-hint">
        <code>reference</code> edges are off by default: the weakest type would hairball the view.
      </p>
      <p className="muted map-hint">
        Tap a node or edge to inspect it. Double-tap a node to focus on its 1-hop neighborhood.
      </p>
    </>
  )
}
