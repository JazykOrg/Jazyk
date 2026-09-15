// The trail: where a node sits between the documents and the product, one step
// per hop with its status dot (docs/frontends/gui.md#inspector). A requirement
// walks section, requirement, criteria, test, files; an entity walks documents,
// entity, requirements, files.
import { Link } from 'react-router'
import type { Graph, VerifyRow } from '../lib/api'
import { delivHref, docHref, useNodeHref } from '../lib/nav'
import { useDocDelivLinks } from '../lib/links'
import { verifyClass } from './Chip'

function Step({ to, label, sub, cls, last }: { to?: string; label: string; sub?: string; cls?: string; last?: boolean }) {
  const inner = (
    <>
      <span className={`trail-dot ${cls ?? 'v-none'}`} />
      <span className="trail-label">{label}</span>
      {sub && <span className="trail-sub">{sub}</span>}
    </>
  )
  return (
    <>
      {to ? (
        <Link to={to} className="trail-step">
          {inner}
        </Link>
      ) : (
        <span className="trail-step">{inner}</span>
      )}
      {!last && <span className="trail-arrow">→</span>}
    </>
  )
}

export function RequirementTrail({ id, graph, row }: { id: string; graph: Graph; row?: VerifyRow }) {
  const r = graph.requirements[id]
  const links = useDocDelivLinks()
  const nodeHref = useNodeHref()
  if (!r) return null
  const files = [...(links.reqToFiles.get(id) ?? [])]
  const status = row?.status ?? 'missing'
  const vcls = verifyClass(status)
  const testLabel = row?.test?.label ?? row?.test?.kind
  return (
    <div className="trail">
      {r.source ? (
        <Step to={docHref(r.source.doc, r.source.section, r.source.quote)} label={r.source.doc.split('/').pop() ?? r.source.doc} sub={r.source.section} cls="v-ok" />
      ) : (
        <Step label="no sentence yet" sub={r.provenance?.derived ? 'inferred' : 'your edit'} cls="v-stale" />
      )}
      <Step to={nodeHref(id)} label="requirement" sub={id} cls={vcls} />
      <Step label={testLabel ? 'test' : 'no test yet'} sub={testLabel ?? undefined} cls={testLabel ? vcls : 'v-none'} />
      <Step
        to={files[0] ? delivHref(files[0], id) : undefined}
        label={files.length === 0 ? 'no file yet' : files.length === 1 ? (files[0].split('/').pop() ?? files[0]) : `${files.length} files`}
        cls={files.length > 0 ? vcls : 'v-none'}
        last
      />
    </div>
  )
}

export function EntityTrail({ id, graph, rows, reqIds }: { id: string; graph: Graph; rows: Record<string, VerifyRow>; reqIds: string[] }) {
  const e = graph.entities[id]
  const links = useDocDelivLinks()
  const nodeHref = useNodeHref()
  if (!e) return null
  const docs = [...new Set((e.mentions ?? []).map((m) => m.doc))]
  const files = new Set<string>()
  for (const rid of reqIds) for (const f of links.reqToFiles.get(rid) ?? []) files.add(f)
  let ok = 0
  let bad = 0
  let stale = 0
  for (const rid of reqIds) {
    const st = rows[rid]?.status
    if (st === 'verified') ok++
    else if (st === 'failing' || st === 'partial') bad++
    else if (st?.startsWith('stale')) stale++
  }
  const rcls = bad > 0 ? 'v-bad' : stale > 0 ? 'v-stale' : ok > 0 && ok === reqIds.length ? 'v-ok' : 'v-none'
  const first = e.mentions?.[0]
  return (
    <div className="trail">
      <Step
        to={first ? docHref(first.doc, first.section, first.quote) : undefined}
        label={docs.length === 0 ? 'no document' : docs.length === 1 ? (docs[0].split('/').pop() ?? docs[0]) : `${docs.length} documents`}
        cls={docs.length > 0 ? 'v-ok' : 'v-stale'}
      />
      <Step to={nodeHref(id)} label={e.name} sub={e.stereotype ? `«${e.stereotype}»` : undefined} cls="v-ok" />
      <Step label={`${reqIds.length} requirement${reqIds.length === 1 ? '' : 's'}`} sub={reqIds.length > 0 ? `${ok} verified${bad ? `, ${bad} failing` : ''}${stale ? `, ${stale} stale` : ''}` : undefined} cls={rcls} />
      <Step
        to={[...files][0] ? delivHref([...files][0]) : undefined}
        label={files.size === 0 ? 'no files yet' : `${files.size} file${files.size === 1 ? '' : 's'}`}
        cls={files.size > 0 ? rcls : 'v-none'}
        last
      />
    </div>
  )
}
