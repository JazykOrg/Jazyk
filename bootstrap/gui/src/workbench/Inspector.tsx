// The inspector: the detail pane for one selection, opened from anywhere,
// replacing nothing (docs/frontends/gui.md#inspector). Driven by ?node=; with no
// selection it shows the open center item's ties (a deliverable file's owners and
// sites, a document's links into the deliverable).
import { Link, useLocation } from 'react-router'
import { useQuery } from '@tanstack/react-query'
import { get, type VerifyRow } from '../lib/api'
import { useContextPack, useGraph, useJournal, useMatrix } from '../lib/queries'
import { useDocDelivLinks } from '../lib/links'
import { delivHref } from '../lib/nav'
import { useResolveId } from '../components/NodeLink'
import NodeLink from '../components/NodeLink'
import SectionLink from '../components/SectionLink'
import { VerifyChip } from '../components/Chip'
import {
  DiagnosticCard,
  EntityCard,
  RelationshipCard,
  RequirementCard,
  aggClass,
  reverseIndex,
} from '../components/Cards'
import type { FileResp } from './DelivFile'
import { useMemo } from 'react'

function VerifyLine({ id, row }: { id: string; row?: VerifyRow }) {
  return (
    <p style={{ margin: '2px 0' }}>
      <NodeLink id={id} /> <VerifyChip status={row?.status ?? 'unverified'} />
      {row?.test?.kind && <span className="chip sev-none">{row.test.kind}</span>}
      {row?.reason && <span className="muted"> · {row.reason}</span>}
    </p>
  )
}

// The deliverable files bound to this node, each linking to the file with the
// requirement's site revealed: the click-through from prose to implementation.
function ImplementedIn({ id }: { id: string }) {
  const links = useDocDelivLinks()
  const files = [...(links.reqToFiles.get(id) ?? [])]
  if (files.length === 0) return null
  return (
    <>
      <h2>implemented in</h2>
      {files.map((p) => (
        <p key={p} style={{ margin: '2px 0' }}>
          <Link className="mono" to={delivHref(p, id)}>
            {p}
          </Link>
        </p>
      ))}
    </>
  )
}

function EntityFiles({ id }: { id: string }) {
  const links = useDocDelivLinks()
  const graph = useGraph()
  const revIdx = useMemo(
    () => (graph.data ? reverseIndex(graph.data) : new Map<string, string[]>()),
    [graph.data],
  )
  const files = new Set<string>()
  for (const rid of revIdx.get(id) ?? []) for (const p of links.reqToFiles.get(rid) ?? []) files.add(p)
  if (files.size === 0) return null
  return (
    <>
      <h2>implemented in</h2>
      {[...files].map((p) => (
        <p key={p} style={{ margin: '2px 0' }}>
          <Link className="mono" to={delivHref(p)}>
            {p}
          </Link>
        </p>
      ))}
    </>
  )
}

function JournalHits({ id }: { id: string }) {
  const journal = useJournal(200)
  const hits = (journal.data?.entries ?? []).filter((e) =>
    e.mutations.some((m) => JSON.stringify(m).includes(id)),
  )
  if (hits.length === 0) return null
  return (
    <>
      <h2>journal</h2>
      {hits.slice(0, 8).map((e) => (
        <p key={e.generation} className="oneline mono" style={{ margin: '2px 0' }}>
          <Link to={`/journal/${e.generation}`}>g{e.generation}</Link> · {e.workItem.task} ·{' '}
          {e.workItem.target}
        </p>
      ))}
    </>
  )
}

function NodeDetail({ id }: { id: string }) {
  const graph = useGraph()
  const matrix = useMatrix()
  const isEntity = id.startsWith('ent:')
  const pack = useContextPack(isEntity ? id : '')
  const revIdx = useMemo(
    () => (graph.data ? reverseIndex(graph.data) : new Map<string, string[]>()),
    [graph.data],
  )
  if (graph.error) return <p className="error-inline">{graph.error.message}</p>
  if (!graph.data) return <p className="muted">loading…</p>
  const g = graph.data
  const rows = matrix.data?.rows ?? {}

  const entity = g.entities[id]
  if (entity) {
    const reqIds = revIdx.get(id) ?? []
    const rels = Object.entries(g.relationships).filter(([, r]) => r.members.includes(id))
    const diags = Object.entries(g.diagnostics).filter(
      ([, d]) => (d.subjects ?? []).includes(id) && d.triage !== 'suppressed',
    )
    return (
      <>
        <EntityCard id={id} e={entity} reqIds={reqIds} rows={rows} />
        <EntityFiles id={id} />
        <h2>verification</h2>
        {reqIds.length === 0 && <p className="muted">no requirements reference this entity</p>}
        {reqIds.length > 0 && (
          <div className={`card ${aggClass(reqIds, rows)}`}>
            {reqIds.map((rid) => (
              <VerifyLine key={rid} id={rid} row={rows[rid]} />
            ))}
          </div>
        )}
        {rels.length > 0 && (
          <>
            <h2>relationships</h2>
            {rels.map(([rid, r]) => (
              <RelationshipCard key={rid} id={rid} r={r} />
            ))}
          </>
        )}
        {diags.length > 0 && (
          <>
            <h2>diagnostics</h2>
            {diags.map(([did, d]) => (
              <DiagnosticCard key={did} id={did} d={d} />
            ))}
          </>
        )}
        {pack.data && (
          <>
            <h2>context pack</h2>
            <pre className="pack">{pack.data.pack}</pre>
          </>
        )}
        <JournalHits id={id} />
      </>
    )
  }

  const req = g.requirements[id]
  if (req) {
    const row = rows[id]
    return (
      <>
        <RequirementCard id={id} r={req} row={row} />
        <ImplementedIn id={id} />
        <h2>verification</h2>
        <VerifyLine id={id} row={row} />
        <JournalHits id={id} />
      </>
    )
  }

  const rel = g.relationships[id]
  if (rel) return <RelationshipCard id={id} r={rel} />

  const diag = g.diagnostics[id]
  if (diag) {
    if (diag.triage === 'suppressed') return <p className="muted">this diagnostic is suppressed</p>
    return <DiagnosticCard id={id} d={diag} />
  }

  return (
    <p className="muted">
      no node with id <span className="mono">{id}</span> in the graph
    </p>
  )
}

// The ties of the open deliverable file: owners, sites, lost sites flagged.
function FileTies({ path }: { path: string }) {
  const fileQ = useQuery({
    queryKey: ['deliverable', 'file', path],
    queryFn: () => get<FileResp>(`/api/deliverable/file?path=${encodeURIComponent(path)}`),
    staleTime: 5_000,
  })
  const matrix = useMatrix()
  const links = useDocDelivLinks()
  const rows = matrix.data?.rows ?? {}
  const f = fileQ.data
  if (!f) return <p className="muted">loading…</p>
  const sites = f.sites ?? []
  const lost = sites.filter((s) => s.line === null)
  const docs = [...(links.fileToDocs.get(path) ?? [])]
  return (
    <>
      <h2>entities</h2>
      {f.owners.entities.length === 0 && <p className="muted">none</p>}
      {f.owners.entities.map((slug) => (
        <p key={slug} style={{ margin: '2px 0' }}>
          {/* The ledger keys entities by slug; the graph id carries the prefix. */}
          <NodeLink id={`ent:${slug}`} />
        </p>
      ))}
      <h2>requirements</h2>
      {f.owners.requirements.length === 0 && <p className="muted">none</p>}
      {f.owners.requirements.map((id) => (
        <p key={id} style={{ margin: '2px 0' }}>
          <NodeLink id={id} /> <VerifyChip status={rows[id]?.status ?? 'unverified'} />
        </p>
      ))}
      <h2>tests</h2>
      {f.owners.tests.length === 0 && <p className="muted">none</p>}
      {f.owners.tests.map((id) => (
        <p key={id} style={{ margin: '2px 0' }}>
          <NodeLink id={id} /> <VerifyChip status={rows[id]?.status ?? 'unverified'} />
        </p>
      ))}
      {lost.length > 0 && (
        <>
          <h2>lost sites</h2>
          {lost.map((s, i) => (
            <p key={i} style={{ margin: '2px 0' }}>
              <NodeLink id={s.requirement} /> <span className="v-bad">site lost</span>
              {!s.exists && <span className="v-bad"> (requirement gone)</span>}
            </p>
          ))}
        </>
      )}
      {docs.length > 0 && (
        <>
          <h2>from documents</h2>
          {docs.map((d) => (
            <p key={d} style={{ margin: '2px 0' }}>
              <SectionLink doc={d} />
            </p>
          ))}
        </>
      )}
    </>
  )
}

// The open document's ties: the deliverable files its requirements produce.
function DocTies({ path }: { path: string }) {
  const links = useDocDelivLinks()
  const graph = useGraph()
  const files = [...(links.docToFiles.get(path) ?? [])]
  const reqs = Object.entries(graph.data?.requirements ?? {}).filter(
    ([, r]) => r.source.doc === path,
  )
  return (
    <>
      <h2>requirements here</h2>
      {reqs.length === 0 && <p className="muted">none extracted yet</p>}
      {reqs.slice(0, 40).map(([rid]) => (
        <p key={rid} style={{ margin: '2px 0' }}>
          <NodeLink id={rid} />
        </p>
      ))}
      <h2>implemented in</h2>
      {files.length === 0 && <p className="muted">no deliverable files bound</p>}
      {files.map((p) => (
        <p key={p} style={{ margin: '2px 0' }}>
          <Link className="mono" to={delivHref(p)}>
            {p}
          </Link>
        </p>
      ))}
    </>
  )
}

export default function Inspector({
  node,
  openNode,
  close,
}: {
  node: string | null
  openNode: (id: string) => void
  close: () => void
}) {
  const loc = useLocation()
  const resolved = useResolveId(node ?? '')
  void openNode

  // Contextual fallback: the open center item's ties when nothing is selected.
  const docPath = loc.pathname.startsWith('/files/docs/')
    ? decodeURIComponent(loc.pathname.slice('/files/docs/'.length))
    : ''
  const delivPath = loc.pathname.startsWith('/files/deliverable/')
    ? decodeURIComponent(loc.pathname.slice('/files/deliverable/'.length))
    : ''

  if (!node && !docPath && !delivPath) return null

  // The map's document and file nodes inspect as their ties, with an open link.
  const pseudoDoc = node?.startsWith('doc:') ? node.slice(4) : ''
  const pseudoFile = node?.startsWith('file:') ? node.slice(5) : ''

  return (
    <aside className="wb-inspector">
      <div className="wb-inspector-head">
        <span className="mono">{node ? resolved : docPath || delivPath}</span>
        {node ? (
          <button onClick={close} title="close">
            ✕
          </button>
        ) : null}
      </div>
      {pseudoDoc ? (
        <>
          <p style={{ margin: '2px 0' }}>
            <SectionLink doc={pseudoDoc}>open in the editor →</SectionLink>
          </p>
          <DocTies path={pseudoDoc} />
        </>
      ) : pseudoFile ? (
        <>
          <p style={{ margin: '2px 0' }}>
            <Link className="mono" to={delivHref(pseudoFile)}>
              open the file →
            </Link>
          </p>
          <FileTies path={pseudoFile} />
        </>
      ) : node ? (
        <NodeDetail id={resolved} />
      ) : delivPath ? (
        <FileTies path={delivPath} />
      ) : (
        <DocTies path={docPath} />
      )}
    </aside>
  )
}
