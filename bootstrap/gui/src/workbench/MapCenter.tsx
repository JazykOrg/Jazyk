// The map: the graph drawn with typed nodes, entities, documents, requirements,
// and deliverable files (docs/frontends/gui.md#graph). The overview shows
// entities and documents; focusing a node pulls in every adjacent node of every
// type, chips notwithstanding. Selection opens the inspector, never a new page.
// ?view= overlays one view's membership: structural kinds draw as the map, flow
// kinds as ordered steps with lanes, a state view as the derived machine.
import { useEffect, useMemo, useRef, useState } from 'react'
import { useSearchParams } from 'react-router'
import cytoscape from 'cytoscape'
import fcose from 'cytoscape-fcose'
import { useDeliverable, useDocs, useGraph, useView } from '../lib/queries'
import type { Entity, Graph, ViewDetail } from '../lib/api'
import NodeLink from '../components/NodeLink'
import '../routes/map.css'

cytoscape.use(fcose)

// Relationship types, strongest first; instantiation stands outside the ranking.
const EDGE_TYPES = [
  'generalization',
  'realization',
  'composition',
  'aggregation',
  'association',
  'dependency',
  'instantiation',
  'reference',
] as const
type EdgeType = (typeof EDGE_TYPES)[number]

// UML notation per type. members[0] is the source `a`, members[1] the target `b`:
// the triangle points at the general entity (target), the diamond sits at the
// owning end (source). Instantiation is a dashed open arrow labeled «instantiate».
interface EdgeSpec {
  line: 'solid' | 'dashed' | 'dotted'
  width: number
  target?: { shape: 'triangle' | 'vee'; fill: cytoscape.Css.ArrowFill }
  source?: { shape: 'diamond'; fill: cytoscape.Css.ArrowFill }
  label?: string
}
const EDGE_STYLE: Record<EdgeType, EdgeSpec> = {
  generalization: { line: 'solid', width: 2, target: { shape: 'triangle', fill: 'hollow' } },
  realization: { line: 'dashed', width: 2, target: { shape: 'triangle', fill: 'hollow' } },
  composition: { line: 'solid', width: 2, source: { shape: 'diamond', fill: 'filled' } },
  aggregation: { line: 'solid', width: 2, source: { shape: 'diamond', fill: 'hollow' } },
  association: { line: 'solid', width: 1.5 },
  dependency: { line: 'dashed', width: 1.5, target: { shape: 'vee', fill: 'filled' } },
  instantiation: { line: 'dashed', width: 1.5, target: { shape: 'vee', fill: 'filled' }, label: '«instantiate»' },
  reference: { line: 'dotted', width: 1.5 },
}

// Node types with their toggle chips. Entities and documents draw by default;
// every requirement and file at once would drown the overview.
const NODE_TYPES = ['entities', 'docs', 'requirements', 'files'] as const
type NodeType = (typeof NODE_TYPES)[number]

const nonPublic = (ent: Entity): boolean => !!ent.scope && ent.scope !== 'public'

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
  info: string
  warn: string
}

function readPalette(): Palette {
  const s = getComputedStyle(document.documentElement)
  const v = (name: string) => s.getPropertyValue(name).trim()
  return {
    ink: v('--ink'),
    muted: v('--muted'),
    line: v('--line'),
    accent: v('--accent'),
    panel: v('--panel'),
    info: v('--info'),
    warn: v('--warn'),
  }
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
    // Containment: a parent entity draws as a compound box around its children.
    {
      selector: ':parent',
      style: {
        shape: 'round-rectangle',
        'background-opacity': 0.06,
        'text-valign': 'top',
        'text-margin-y': -4,
      },
    },
    { selector: 'node[nonpublic = 1]', style: { 'border-style': 'dashed' } },
    {
      selector: 'node[t = "doc"]',
      style: {
        shape: 'round-rectangle',
        'border-color': p.accent,
        height: 'data(h)',
        'font-size': 9,
      },
    },
    {
      selector: 'node[t = "req"]',
      style: {
        shape: 'diamond',
        'border-color': p.warn,
        color: p.muted,
        'font-size': 8,
      },
    },
    {
      selector: 'node[t = "file"]',
      style: {
        shape: 'rectangle',
        'border-color': p.info,
        height: 'data(h)',
        'font-size': 9,
      },
    },
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
        color: p.muted,
        'font-size': 8,
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
      if (s.label) style.label = s.label
      return { selector: `edge[type = "${t}"]`, style }
    }),
    // A lifted arrow carries its count label; clicking expands it in the inspector.
    { selector: 'edge[lifted = 1]', style: { label: 'data(label)', 'text-rotation': 'autorotate' } },
    // The cross-type ties: membership, source anchors, sites, and the collapsed
    // doc-to-entity tie shown when requirements are hidden.
    { selector: 'edge[type = "member"]', style: { width: 1, 'line-style': 'solid', opacity: 0.35 } },
    { selector: 'edge[type = "anchor"]', style: { width: 1, 'line-style': 'dotted', opacity: 0.4 } },
    {
      selector: 'edge[type = "site"]',
      style: {
        width: 1,
        'line-style': 'dashed',
        opacity: 0.4,
        'target-arrow-shape': 'vee',
        'arrow-scale': 0.8,
      },
    },
    { selector: 'edge[type = "via"]', style: { width: 1, 'line-style': 'dotted', opacity: 0.3 } },
    {
      selector: 'edge[type = "own"]',
      style: {
        width: 1,
        'line-style': 'dashed',
        opacity: 0.3,
        'target-arrow-shape': 'vee',
        'arrow-scale': 0.8,
      },
    },
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

type ScopeFilter = 'all' | 'public' | 'non-public'
type FocusHops = 'off' | '1' | '2'

interface NodeData {
  id: string
  label: string
  size: number
  h?: number
  t: 'ent' | 'doc' | 'req' | 'file'
  nonpublic?: number
  // The containing entity, applied only when it draws too (compound nesting).
  pref?: string
  parent?: string
}

interface EdgeData {
  id: string
  source: string
  target: string
  type: string
  // A collapsed tie's first hidden intermediary, for inspection on tap.
  via?: string
  // The relationship behind a typed or lifted arrow, for the inspector.
  rel?: string
  lifted?: number
  label?: string
}

const base = (p: string) => p.split('/').pop() ?? p

const STRUCTURAL_KINDS = ['class', 'object', 'package', 'component', 'composite', 'deployment']

// Flow kinds draw as ordered steps with the participants as lanes, one step per
// member requirement (docs/frontends/gui.md#graph).
function FlowOverlay({ v }: { v: ViewDetail }) {
  const participants = useMemo(() => {
    const seen = new Map<string, string>()
    for (const s of v.steps) for (const p of s.participants) seen.set(p.id, p.name)
    return [...seen.entries()]
  }, [v.steps])
  return (
    <div className="wb-center-scroll" style={{ padding: 12 }}>
      <h2 style={{ marginTop: 0 }}>
        {v.title} <span className="chip sev-info">{v.kind}</span>
      </h2>
      <div style={{ overflowX: 'auto' }}>
        <table className="mono" style={{ borderCollapse: 'collapse' }}>
          <thead>
            <tr>
              <th style={{ textAlign: 'left', padding: 4 }}>#</th>
              <th style={{ textAlign: 'left', padding: 4 }}>step</th>
              {participants.map(([id, name]) => (
                <th key={id} style={{ padding: 4 }}>
                  <NodeLink id={id}>{name}</NodeLink>
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {v.steps.map((s, i) => (
              <tr key={s.requirement}>
                <td style={{ padding: 4, verticalAlign: 'top' }} className="muted">{i + 1}</td>
                <td style={{ padding: 4, maxWidth: 420 }}>
                  <NodeLink id={s.requirement} /> {s.statement}
                </td>
                {participants.map(([id]) => (
                  <td key={id} style={{ padding: 4, textAlign: 'center' }}>
                    {s.participants.some((p) => p.id === id) ? '●' : ''}
                  </td>
                ))}
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  )
}

// A state view draws the derived machine of its subject.
function StateOverlay({ v }: { v: ViewDetail }) {
  return (
    <div className="wb-center-scroll" style={{ padding: 12 }}>
      <h2 style={{ marginTop: 0 }}>
        {v.title} <span className="chip sev-info">state</span>
      </h2>
      {v.machines.length === 0 && <p className="muted">no derived machine for this subject</p>}
      {v.machines.map(({ id, machine }) => (
        <div key={id} className="card">
          <p style={{ margin: '2px 0' }}>
            <NodeLink id={machine.subject} />{' '}
            <span className="muted mono">
              states: {machine.states.join(', ')}
              {machine.initial ? ` (initial: ${machine.initial})` : ''}
            </span>
          </p>
          {machine.transitions.map((t, i) => (
            <p key={i} className="mono" style={{ margin: '1px 0' }}>
              {t.from} → {t.to}
              {t.trigger && <span className="muted"> on {t.trigger}</span>}
              {t.guard && <span className="muted"> [{t.guard}]</span>}{' '}
              <NodeLink id={t.requirement} />
            </p>
          ))}
        </div>
      ))}
    </div>
  )
}

export default function MapCenter() {
  const { data: graph, error } = useGraph()
  const { data: docsList } = useDocs()
  const { data: deliv } = useDeliverable()
  const [params, setParams] = useSearchParams()
  const [query, setQuery] = useState('')
  const [scope, setScope] = useState<ScopeFilter>('all')
  const [types, setTypes] = useState<Record<EdgeType, boolean>>(
    () => Object.fromEntries(EDGE_TYPES.map((t) => [t, t !== 'reference'])) as Record<EdgeType, boolean>,
  )
  const [nodeTypes, setNodeTypes] = useState<Record<NodeType, boolean>>({
    entities: true,
    docs: true,
    requirements: false,
    files: false,
  })
  const [focusHops, setFocusHops] = useState<FocusHops>('off')
  const boxRef = useRef<HTMLDivElement>(null)
  const cyRef = useRef<cytoscape.Core | null>(null)
  const focusParam = params.get('focus')
  const selected = params.get('node')
  const viewParam = params.get('view') ?? ''
  const viewQ = useView(viewParam)
  const overlay = viewParam !== '' ? viewQ.data : undefined

  // Selection goes to the inspector: node param, and focus follows for the map.
  const select = (id: string | null) => {
    setParams(
      (p) => {
        const next = new URLSearchParams(p)
        if (id === null) {
          next.delete('node')
          next.delete('focus')
        } else {
          next.set('node', id)
          next.set('focus', id)
        }
        return next
      },
      { replace: true },
    )
  }
  const selectRef = useRef(select)
  selectRef.current = select

  const clearOverlay = () => {
    setParams(
      (p) => {
        const next = new URLSearchParams(p)
        next.delete('view')
        return next
      },
      { replace: true },
    )
  }

  // The full typed graph, independent of what currently draws. Focus pulls
  // neighborhoods out of this, so hidden types surface around a selection.
  const full = useMemo(() => {
    const nodes = new Map<string, NodeData>()
    const edges = new Map<string, EdgeData>()
    if (!graph) return { nodes, edges }

    const degree = new Map<string, number>()
    for (const rel of Object.values(graph.relationships)) {
      for (const m of rel.members) {
        const id = resolveEntity(graph, m)
        if (id) degree.set(id, (degree.get(id) ?? 0) + 1)
      }
    }
    for (const [id, ent] of Object.entries(graph.entities)) {
      const d = degree.get(id) ?? 0
      const pref = ent.parent ? (resolveEntity(graph, ent.parent) ?? undefined) : undefined
      nodes.set(id, {
        id,
        label: ent.name,
        size: Math.round(16 + Math.sqrt(d) * 7),
        t: 'ent',
        nonpublic: nonPublic(ent) ? 1 : 0,
        pref,
      })
    }
    for (const d of docsList?.docs ?? []) {
      nodes.set(`doc:${d.path}`, { id: `doc:${d.path}`, label: base(d.path), size: 30, h: 20, t: 'doc' })
    }
    for (const [rid, r] of Object.entries(graph.requirements)) {
      nodes.set(rid, { id: rid, label: rid.replace(/^req:/, ''), size: 12, t: 'req' })
      for (const eid of r.entities ?? []) {
        const ent = resolveEntity(graph, eid)
        if (ent)
          edges.set(`m|${rid}|${ent}`, { id: `m|${rid}|${ent}`, source: rid, target: ent, type: 'member' })
      }
      // Only quote-provenanced requirements anchor in a document.
      if (r.source)
        edges.set(`a|${rid}`, { id: `a|${rid}`, source: rid, target: `doc:${r.source.doc}`, type: 'anchor' })
    }
    // One arrow per direction-and-type group of each relationship, UML notation.
    for (const [relId, rel] of Object.entries(graph.relationships)) {
      for (const c of rel.contributions ?? []) {
        const a = resolveEntity(graph, c.a)
        const b = resolveEntity(graph, c.b)
        if (!a || !b || a === b) continue
        const id = `${relId}|${c.type}|${a}|${b}`
        if (!edges.has(id)) edges.set(id, { id, source: a, target: b, type: c.type, rel: relId })
      }
    }
    for (const f of deliv?.files ?? []) {
      const owners = f.owners.entities.length + f.owners.requirements.length + f.owners.tests.length
      if (owners === 0) continue
      const fid = `file:${f.path}`
      nodes.set(fid, { id: fid, label: base(f.path), size: 30, h: 20, t: 'file' })
      for (const rid of [...f.owners.requirements, ...f.owners.tests]) {
        if (nodes.has(rid))
          edges.set(`s|${rid}|${f.path}`, { id: `s|${rid}|${f.path}`, source: rid, target: fid, type: 'site' })
      }
      for (const eid of f.owners.entities) {
        const ent = resolveEntity(graph, `ent:${eid}`) ?? resolveEntity(graph, eid)
        if (ent) edges.set(`o|${ent}|${f.path}`, { id: `o|${ent}|${f.path}`, source: ent, target: fid, type: 'own' })
      }
    }
    return { nodes, edges }
  }, [graph, docsList, deliv])

  // What draws: the chip filter in the overview; the full-graph neighborhood of
  // the selection when focus is on; the view's membership when a structural view
  // overlays (docs/frontends/gui.md#graph).
  const visible = useMemo(() => {
    // A structural view overlay draws that view's membership and nothing else.
    if (overlay && STRUCTURAL_KINDS.includes(overlay.kind)) {
      const nodes = new Map<string, NodeData>()
      const edges = new Map<string, EdgeData>()
      const memberIds = new Set(
        overlay.members.filter((m) => m.node === 'entity' && !m.hidden).map((m) => m.id),
      )
      for (const m of overlay.members) {
        if (m.node !== 'entity' || m.hidden) continue
        const pref = m.parent && memberIds.has(m.parent) ? m.parent : undefined
        nodes.set(m.id, {
          id: m.id,
          label: m.name ?? m.id,
          size: 24,
          t: 'ent',
          pref,
          parent: pref,
        })
      }
      overlay.arrows.forEach((a, i) => {
        if (!nodes.has(a.a) || !nodes.has(a.b)) return
        edges.set(`v|${i}`, {
          id: `v|${i}`,
          source: a.a,
          target: a.b,
          type: a.type,
          rel: a.rel,
          lifted: a.lifted ? 1 : 0,
          label: a.lifted ? `${a.type}: ${a.count}` : undefined,
        })
      })
      return { nodes, edges }
    }
    const want = new Set<string>()
    const focusOn = focusHops !== 'off' && selected && full.nodes.has(selected)
    if (focusOn) {
      const adj = new Map<string, Set<string>>()
      for (const e of full.edges.values()) {
        let s = adj.get(e.source)
        if (!s) adj.set(e.source, (s = new Set()))
        s.add(e.target)
        let t = adj.get(e.target)
        if (!t) adj.set(e.target, (t = new Set()))
        t.add(e.source)
      }
      let frontier = new Set([selected!])
      want.add(selected!)
      const hops = focusHops === '2' ? 2 : 1
      for (let i = 0; i < hops; i++) {
        const next = new Set<string>()
        for (const id of frontier)
          for (const n of adj.get(id) ?? []) {
            if (!want.has(n)) {
              want.add(n)
              next.add(n)
            }
          }
        frontier = next
      }
    } else {
      for (const n of full.nodes.values()) {
        if (n.t === 'ent') {
          if (!nodeTypes.entities) continue
          if (scope === 'public' && n.nonpublic) continue
          if (scope === 'non-public' && !n.nonpublic) continue
          want.add(n.id)
        } else if (n.t === 'doc' && nodeTypes.docs) want.add(n.id)
        else if (n.t === 'req' && nodeTypes.requirements) want.add(n.id)
        else if (n.t === 'file' && nodeTypes.files) want.add(n.id)
      }
    }
    const edges = new Map<string, EdgeData>()
    for (const e of full.edges.values()) {
      if (!want.has(e.source) || !want.has(e.target)) continue
      if ((EDGE_TYPES as readonly string[]).includes(e.type) && !types[e.type as EdgeType]) continue
      edges.set(e.id, e)
    }
    // Hidden types never break the picture: two visible nodes joined only through
    // hidden ones get a collapsed tie, whatever the chip combination (a document
    // to the files its hidden requirements implement, a document to an entity, an
    // entity to a file). BFS from each visible node through hidden nodes only; the
    // first hidden hop rides along as the tie's intermediary for inspection.
    if (!focusOn) {
      const adj = new Map<string, { to: string }[]>()
      const link = (a: string, b: string) => {
        let l = adj.get(a)
        if (!l) adj.set(a, (l = []))
        l.push({ to: b })
      }
      for (const e of full.edges.values()) {
        link(e.source, e.target)
        link(e.target, e.source)
      }
      const drawn = new Set<string>()
      for (const e of edges.values())
        drawn.add(e.source < e.target ? `${e.source}|${e.target}` : `${e.target}|${e.source}`)
      for (const v of want) {
        const seen = new Set<string>([v])
        let frontier: { at: string; gate: string }[] = (adj.get(v) ?? [])
          .filter((n) => !want.has(n.to) && full.nodes.has(n.to))
          .map((n) => ({ at: n.to, gate: n.to }))
        while (frontier.length > 0) {
          const next: { at: string; gate: string }[] = []
          for (const f of frontier) {
            if (seen.has(f.at)) continue
            seen.add(f.at)
            for (const n of adj.get(f.at) ?? []) {
              if (seen.has(n.to) || !full.nodes.has(n.to)) continue
              if (want.has(n.to)) {
                // Reached another visible node through hidden ones only.
                if (n.to === v) continue
                const key = v < n.to ? `${v}|${n.to}` : `${n.to}|${v}`
                if (drawn.has(key)) continue
                drawn.add(key)
                edges.set(`via|${key}`, {
                  id: `via|${key}`,
                  source: v,
                  target: n.to,
                  type: 'via',
                  via: f.gate,
                })
              } else {
                next.push({ at: n.to, gate: f.gate })
              }
            }
          }
          frontier = next
        }
      }
    }
    const nodes = new Map<string, NodeData>()
    for (const id of want) {
      const n = full.nodes.get(id)
      if (!n) continue
      // Containment nests a child inside its parent only when the parent draws too.
      const parent = n.pref && want.has(n.pref) ? n.pref : undefined
      nodes.set(id, { ...n, parent })
    }
    return { nodes, edges }
  }, [full, nodeTypes, types, scope, focusHops, selected, overlay])

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
    cy.on('tap', 'node', (e) => selectRef.current((e.target as cytoscape.NodeSingular).id()))
    cy.on('tap', 'edge', (e) => {
      const edge = e.target as cytoscape.EdgeSingular
      const type = edge.data('type') as string
      const rel = edge.data('rel') as string | undefined
      const id = edge.id()
      // A relationship (or lifted view) edge inspects the relationship: the
      // concrete edges beneath a lifted arrow list there, each walking to its
      // requirement and its sentence.
      if (rel) selectRef.current(rel)
      else if ((EDGE_TYPES as readonly string[]).includes(type)) selectRef.current(id)
      else if (type === 'member' || type === 'site') selectRef.current(id.split('|')[1])
      else if (type === 'anchor') selectRef.current(id.slice(2))
      else if (type === 'via') selectRef.current((edge.data('via') as string) ?? edge.source().id())
      else if (type === 'own') selectRef.current(edge.source().id())
    })
    cy.on('tap', (e) => {
      if (e.target === cy) selectRef.current(null)
    })
    cy.on('dbltap', 'node', (e) => {
      const n = e.target as cytoscape.NodeSingular
      selectRef.current(n.id())
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
    if (!cy) return
    const fresh: string[] = []
    cy.batch(() => {
      cy.edges().forEach((e) => {
        if (!visible.edges.has(e.id())) e.remove()
      })
      cy.nodes().forEach((n) => {
        const want = visible.nodes.get(n.id())
        // Compound membership cannot be re-parented in place; drop and re-add.
        if (!want || (n.data('parent') ?? undefined) !== want.parent) n.remove()
      })
      for (const [id, data] of visible.nodes) {
        const ex = cy.getElementById(id)
        if (ex.nonempty()) {
          ex.data(data as unknown as Record<string, unknown>)
        } else {
          const pos = posCache.get(id)
          cy.add({ group: 'nodes', data: data as unknown as Record<string, unknown>, position: pos ? { ...pos } : undefined })
          if (!pos) fresh.push(id)
        }
      }
      for (const [id, data] of visible.edges) {
        const ex = cy.getElementById(id)
        if (ex.nonempty()) ex.data(data as unknown as Record<string, unknown>)
        else if (cy.getElementById(data.source).nonempty() && cy.getElementById(data.target).nonempty())
          cy.add({ group: 'edges', data: data as unknown as Record<string, unknown> })
      }
    })

    if (fresh.length > 0) {
      const total = cy.nodes().length
      const randomize = fresh.length > total * 0.3
      const freshSet = new Set(fresh)
      const fixed: { nodeId: string; position: cytoscape.Position }[] = []
      if (!randomize) {
        cy.nodes().forEach((n) => {
          if (!freshSet.has(n.id()) && !n.isParent()) fixed.push({ nodeId: n.id(), position: { ...n.position() } })
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
  }, [visible])

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
  }, [query, visible])

  // Selection and ?focus= drive cy selection and centering.
  const centeredRef = useRef('')
  useEffect(() => {
    const cy = cyRef.current
    if (!cy) return
    cy.$(':selected').unselect()
    if (selected) {
      const n = cy.getElementById(selected)
      if (n.nonempty()) {
        n.select()
        if (focusParam === selected && centeredRef.current !== selected) {
          centeredRef.current = selected
          cy.center(n)
        }
      }
    }
  }, [selected, focusParam, visible])

  const empty = graph && Object.keys(graph.entities).length === 0
  // Flow and state views draw as their own panels, not on the canvas.
  const panelOverlay =
    overlay && !STRUCTURAL_KINDS.includes(overlay.kind)
      ? overlay.kind === 'state'
        ? 'state'
        : 'flow'
      : null

  return (
    <div className="map-root">
      <div className="wb-map-toolbar">
        {viewParam !== '' && (
          <span className="chip sev-info" title="a view overlays the map">
            view: {overlay?.title ?? viewParam}
            <button className="ide-mini" style={{ marginLeft: 4 }} onClick={clearOverlay} title="clear the overlay">
              ✕
            </button>
          </span>
        )}
        <input
          type="search"
          placeholder="search the map"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
        />
        <div className="map-types">
          {NODE_TYPES.map((t) => (
            <label key={t} className="mono">
              <input
                type="checkbox"
                checked={nodeTypes[t]}
                onChange={() => setNodeTypes({ ...nodeTypes, [t]: !nodeTypes[t] })}
              />
              {t}
            </label>
          ))}
        </div>
        <select value={scope} onChange={(e) => setScope(e.target.value as ScopeFilter)}>
          <option value="all">all scopes</option>
          <option value="public">public</option>
          <option value="non-public">non-public</option>
        </select>
        <details className="map-edgetypes">
          <summary className="mono">edges</summary>
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
        </details>
        <select
          value={focusHops}
          onChange={(e) => setFocusHops(e.target.value as FocusHops)}
          title="neighborhood focus: pulls in every adjacent node of every type"
        >
          <option value="off">focus: off</option>
          <option value="1">focus: 1 hop</option>
          <option value="2">focus: 2 hops</option>
        </select>
        {focusHops !== 'off' && !selected && (
          <span className="muted" style={{ fontSize: 11 }}>
            select a node to focus
          </span>
        )}
      </div>
      <div className="map-body">
        {panelOverlay === 'flow' && overlay && <FlowOverlay v={overlay} />}
        {panelOverlay === 'state' && overlay && <StateOverlay v={overlay} />}
        <div className="map-canvas" style={panelOverlay ? { display: 'none' } : undefined}>
          <div className="map-cy" ref={boxRef} />
          {error ? (
            <p className="error-inline map-empty">{String(error)}</p>
          ) : empty ? (
            <p className="empty map-empty">No entities in the graph yet. Run a compile to populate it.</p>
          ) : null}
        </div>
      </div>
    </div>
  )
}
