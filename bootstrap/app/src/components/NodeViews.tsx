// The entity, requirement, criterion, and relation views, shared by the Model
// pages and the inspector, laid out under the presentation rules: names and
// sentences, each fact once, the compiler's records in Developer details
// (docs/frontends/gui.md#model, #presentation).
import { useMemo } from 'react'
import { Link } from 'react-router'
import type { Graph, Relationship, VerifyRow } from '../lib/api'
import { useDocDelivLinks } from '../lib/links'
import { delivHref } from '../lib/nav'
import { useEntityCard, useView } from '../lib/queries'
import { useApp } from '../lib/store'
import { criterionKindWord, kindWord, relationPhrase } from '../lib/present'
import { reverseIndex, ProvenanceLine } from './Cards'
import Connections from './Connections'
import DiagramSvg from './DiagramSvg'
import FactField from './FactField'
import { DevDetails, InlineText, NameLink, Part, ProblemItem, RequirementList, ShowMore, SourceQuote, StatusWord } from './Present'

const VIEW_KIND: Record<string, string> = {
  class: 'Structure',
  component: 'Structure',
  sequence: 'Flow',
  'use-case': 'Use cases',
  usecase: 'Use cases',
  state: 'States',
  object: 'Instances',
}

function viewKindWord(g: Graph, vid: string): string {
  const k = g.views[vid]?.kind ?? vid.replace(/^view:/, '').split('/')[0]
  return VIEW_KIND[k] ?? k
}

// The files that implement any of the requirements.
function useFiles(reqIds: string[]): string[] {
  const links = useDocDelivLinks()
  const files = new Set<string>()
  for (const rid of reqIds) for (const f of links.reqToFiles.get(rid) ?? []) files.add(f)
  return [...files]
}

function FilesLine({ files, site }: { files: string[]; site?: string }) {
  if (files.length === 0) return <span className="muted">Not generated yet</span>
  return (
    <span>
      Generated in{' '}
      {files.slice(0, 3).map((f, i) => (
        <span key={f}>
          {i > 0 && ', '}
          <Link className="mono" to={delivHref(f, site)}>
            {f.split('/').pop()}
          </Link>
        </span>
      ))}
      {files.length > 3 && ` and ${files.length - 3} more`}
    </span>
  )
}

// The entity's own picture, when its inside has one: the structure of its parts.
function InsidePicture({ vid }: { vid: string }) {
  const view = useView(vid)
  if (!view.data?.svg) return null
  return (
    <div className="inside-picture">
      <DiagramSvg svg={view.data.svg} />
    </div>
  )
}

export function EntityView({
  id,
  graph: g,
  rows,
  page,
  devExtra,
}: {
  id: string
  graph: Graph
  rows: Record<string, VerifyRow>
  page?: boolean
  devExtra?: React.ReactNode
}) {
  const e = g.entities[id]
  const card = useEntityCard(id)
  const dev = useApp((a) => a.dev)
  const reqIds = useMemo(() => reverseIndex(g).get(id) ?? [], [g, id])
  const files = useFiles(reqIds)
  const c = card.data
  const insideSvg = useView(c?.inside ?? '').data?.svg
  if (!e) return null

  const passing = reqIds.filter((r) => rows[r]?.status === 'verified').length
  const failing = reqIds.filter((r) => ['failing', 'partial'].includes(rows[r]?.status ?? '')).length
  // The path above the entity, without the scope root and the entity itself.
  const path = (c?.breadcrumb ?? []).filter((b) => !b.id.startsWith('scope:') && b.id !== id)
  // The diagrams that show its inside, its flows, and where it sits; only those the
  // graph holds.
  const diagrams: { vid: string; label: string }[] = []
  const add = (vid: string | null | undefined, label?: string) => {
    if (!vid || !g.views[vid] || diagrams.some((d) => d.vid === vid)) return
    diagrams.push({ vid, label: label ?? g.views[vid].title ?? vid })
  }
  add(c?.inside, `Inside ${e.name}`)
  for (const f of c?.insideFlows ?? []) add(f)
  add(c?.context, `${e.name} among its neighbors`)
  for (const f of c?.flows ?? []) add(f)
  const machine = Object.entries(g.stateMachines ?? {}).find(([, m]) => m.subject === id)
  const problems = Object.entries(g.diagnostics).filter(
    ([, d]) => (d.subjects ?? []).includes(id) && d.triage !== 'suppressed' && (d.lifecycle ?? 'open') === 'open',
  )
  const Title = page ? 'h1' : 'h3'
  // The inside picture shows the children; without it the connections do.
  const pictureInside = !!(page && c?.inside && insideSvg)

  return (
    <div className="node-view">
      <header className="node-head">
        <p className="node-kicker">
          {kindWord(e.stereotype) || 'Entity'}
          {path.length > 0 && (
            <>
              {' in '}
              {path.map((b, i) => (
                <span key={b.id}>
                  {i > 0 && ' › '}
                  <Link to={`/model/entities/${encodeURIComponent(b.id)}`}>{b.name}</Link>
                </span>
              ))}
            </>
          )}
        </p>
        <Title className="node-title">{e.name}</Title>
        <div className="node-definition">
          <FactField id={id} field="definition" value={e.definition ?? ''} multiline label="definition" />
        </div>
        {(e.aliases ?? []).length > 0 && <p className="muted">Also called {(e.aliases ?? []).join(', ')}.</p>}
        {c && c.provenance !== 'quote' && (
          <p className="node-inferred">
            No document names this. jazyk grouped it from the documents
            {c.proposal ? (
              <>
                {'; '}
                <NameLink id={c.proposal}>review the proposal</NameLink>
              </>
            ) : null}
            .
          </p>
        )}
        {(e.attributes ?? []).length > 0 && (
          <table className="attr-table">
            <tbody>
              {(e.attributes ?? []).map((a) => (
                <tr key={a.name}>
                  <th>{a.name}</th>
                  <td className="muted">{a.type ?? ''}</td>
                  <td>{a.value !== undefined ? <FactField id={id} field={`attributes.${a.name}.value`} value={a.value ?? ''} label={a.name} /> : null}</td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </header>

      <p className="status-line">
        {reqIds.length === 0
          ? 'No requirements yet'
          : `${reqIds.length} requirement${reqIds.length === 1 ? '' : 's'}, ${passing === 0 ? 'none' : passing} passing${failing ? `, ${failing} failing` : ''}`}
        {' · '}
        <FilesLine files={files} />
      </p>

      {pictureInside && c?.inside && (
        <Part title={`Inside ${e.name}`}>
          <InsidePicture vid={c.inside} />
        </Part>
      )}

      <Part title="Connections">
        <Connections id={id} graph={g} withChildren={!pictureInside} />
      </Part>

      {machine && (
        <Part title="States">
          <p>
            {machine[1].states.join(' → ')}{' '}
            {g.views[`view:state/${machine[0].replace(/^sm:/, '')}`] && <NameLink id={`view:state/${machine[0].replace(/^sm:/, '')}`}>see the states</NameLink>}
          </p>
        </Part>
      )}

      {diagrams.length > 0 && (
        <Part title="Diagrams">
          <div className="diagram-list">
            {diagrams.map((d) => (
              <Link key={d.vid} to={`/model/diagrams/${encodeURIComponent(d.vid)}`} className="diagram-link">
                {d.label} <span className="muted">· {viewKindWord(g, d.vid).toLowerCase()}</span>
              </Link>
            ))}
          </div>
        </Part>
      )}

      <Part title="Requirements">
        <RequirementList ids={reqIds} rows={rows} first={page ? 8 : 5} />
      </Part>

      {(e.mentions ?? []).length > 0 && (
        <Part title="In the documents">
          <ShowMore items={e.mentions ?? []} first={3} render={(m, i) => <SourceQuote key={i} doc={m.doc} section={m.section} quote={m.quote} />} />
        </Part>
      )}

      {problems.length > 0 && (
        <Part title="Problems">
          {problems.map(([did, d]) => (
            <ProblemItem key={did} id={did} d={d} />
          ))}
        </Part>
      )}

      {dev && (
        <DevDetails>
          <p className="mono">
            {id}
            {e.stereotype ? ` «${e.stereotype}»` : ''}
            {e.scope ? ` scope ${e.scope}` : ''}
          </p>
          {e.provenance && <ProvenanceLine p={e.provenance} />}
          {devExtra}
        </DevDetails>
      )}
    </div>
  )
}

// A relation from a requirement's edge, in words: "Backend uses Database".
function EdgeSentence({ a, b, type, cardinality }: { a: string; b: string; type?: string; cardinality?: string }) {
  return (
    <p className="edge-sentence">
      <NameLink id={a} /> {relationPhrase(type, true)} <NameLink id={b} />
      {cardinality && <span className="muted"> ({cardinality})</span>}
    </p>
  )
}

export function CriterionItem({ id, graph: g }: { id: string; graph: Graph }) {
  const c = g.criteria?.[id]
  const dev = useApp((a) => a.dev)
  if (!c) return null
  return (
    <div className="criterion">
      <p
        className="criterion-line"
        title={[c.given && `Given ${c.given}.`, c.when && `When ${c.when}.`, c.then && `Then ${c.then}.`].filter(Boolean).join(' ') || undefined}
      >
        <span className="chip sev-none">{criterionKindWord(c.kind)}</span> <InlineText text={c.statement} />
        {c.measure && <span className="muted"> ({c.measure})</span>}
        {dev && <span className="mono muted dev-id"> {id}</span>}
      </p>
      {dev && (c.given || c.when || c.then) && (
        <p className="criterion-gwt muted">
          {c.given && <>Given {c.given}. </>}
          {c.when && <>When {c.when}. </>}
          {c.then && <>Then {c.then}.</>}
        </p>
      )}
      {c.stale && <p className="v-stale">Written for an earlier wording of the requirement.</p>}
      {c.source ? (
        <SourceQuote doc={c.source.doc} section={c.source.section} quote={c.source.quote} />
      ) : c.provenance?.derived ? (
        <p className="muted criterion-origin">Inferred by jazyk: no document states this example yet.</p>
      ) : null}
    </div>
  )
}

export function RequirementView({
  id,
  graph: g,
  row,
  page,
  devExtra,
}: {
  id: string
  graph: Graph
  row?: VerifyRow
  page?: boolean
  devExtra?: React.ReactNode
}) {
  const r = g.requirements[id]
  const dev = useApp((a) => a.dev)
  const files = useFiles([id])
  if (!r) return null
  const criteria = Object.entries(g.criteria ?? {})
    .filter(([, c]) => c.requirement === id)
    .map(([cid]) => cid)
  const Title = page ? 'h1' : 'h3'
  const derived = r.provenance?.derived
  const decree = r.provenance?.decree
  const hasDetails = (r.edges ?? []).length > 0 || !!r.transition

  return (
    <div className="node-view">
      <header className="node-head">
        <p className="node-kicker">
          Requirement
          {(r.entities ?? []).length > 0 && (
            <>
              {' about '}
              {(r.entities ?? []).map((eid, i) => (
                <span key={eid}>
                  {i > 0 && ', '}
                  <Link to={`/model/entities/${encodeURIComponent(eid)}`}>{g.entities[eid]?.name ?? eid}</Link>
                </span>
              ))}
            </>
          )}
        </p>
        <Title className="node-title statement">
          <FactField id={id} field="statement" value={r.statement} multiline label="statement" />
        </Title>
      </header>

      <p className="status-line">
        <StatusWord status={row?.status} />
        {row?.test?.label && (
          <>
            {' · test '}
            <span className="mono">{row.test.label}</span>
          </>
        )}
        {' · '}
        <FilesLine files={files} site={id} />
      </p>

      <Part title="Where it comes from">
        {r.source ? (
          <SourceQuote doc={r.source.doc} section={r.source.section} quote={r.source.quote} />
        ) : derived ? (
          <div className="node-inferred">
            <p>Inferred by jazyk: no document states it yet.</p>
            <p className="muted">{derived.reasoning}</p>
          </div>
        ) : decree ? (
          <div className="node-inferred">
            <p>Your edit, not yet in the documents{decree.note ? `: ${decree.note}` : '.'}</p>
          </div>
        ) : (
          <p className="muted">Unknown.</p>
        )}
      </Part>

      <Part title="Acceptance criteria">
        {criteria.length === 0 ? (
          <p className="muted">None yet. A build writes the examples that show this requirement is met.</p>
        ) : (
          criteria.map((cid) => <CriterionItem key={cid} id={cid} graph={g} />)
        )}
      </Part>

      {hasDetails && (
        <Part title="What it states">
          {(r.edges ?? []).map((ed, i) => (
            <EdgeSentence key={i} a={ed.a} b={ed.b} type={ed.type} cardinality={ed.cardinality} />
          ))}
          {r.transition && (
            <p className="edge-sentence">
              <NameLink id={r.transition.subject} /> goes from <b>{r.transition.from}</b> to <b>{r.transition.to}</b>
              {r.transition.trigger && <> on {r.transition.trigger}</>}
              {r.transition.guard && <span className="muted"> (only if {r.transition.guard})</span>}
            </p>
          )}
        </Part>
      )}

      {dev && (
        <DevDetails>
          <p className="mono">{id}</p>
          {(r.facets ?? []).map((f, i) => (
            <p key={i} style={{ margin: '2px 0' }}>
              <span className="chip sev-none">{f.facet}</span>
              {f.measure ? ` (${f.measure})` : ''} <span className="muted">{f.reasoning}</span>
            </p>
          ))}
          {row?.evidence && <p className="muted">{row.evidence}</p>}
          {row?.reason && <p className="muted mono">trace: {row.status} ({row.reason})</p>}
          {devExtra}
        </DevDetails>
      )}
    </div>
  )
}

// A criterion opened alone: its statement, its requirement, where it comes from.
export function CriterionView({ id, graph: g }: { id: string; graph: Graph }) {
  const c = g.criteria?.[id]
  if (!c) return null
  return (
    <div className="node-view">
      <p className="node-kicker">
        Acceptance criterion of <NameLink id={c.requirement} max={80} />
      </p>
      <CriterionItem id={id} graph={g} />
    </div>
  )
}

// A relation in words: each direction and type with the sentences behind it.
export function RelationView({ id, r }: { id: string; r: Relationship }) {
  const dev = useApp((a) => a.dev)
  return (
    <div className="node-view">
      {(r.contributions ?? []).map((c, i) => (
        <div key={i} className="relation-group">
          <EdgeSentence a={c.a} b={c.b} type={c.type} cardinality={c.cardinality} />
          <div className="req-list">
            {c.requirements.map((q) => (
              <div key={q} className="req-row">
                <NameLink id={q} />
              </div>
            ))}
          </div>
        </div>
      ))}
      {dev && (
        <DevDetails>
          <p className="mono">{id}</p>
        </DevDetails>
      )}
    </div>
  )
}
