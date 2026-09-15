// The Product lens's center pages (docs/frontends/gui.md#product): the overview,
// the files landing, the tests, Not described with the Document action, the
// generated docs landing, the product form.
import { Link } from 'react-router'
import { post } from '../lib/api'
import { useDeliverable, useMatrix, useOutList, useProject, useStatus, useWorkers } from '../lib/queries'
import { useApp } from '../lib/store'
import { useWords } from '../lib/words'
import { verifyClass } from '../components/Chip'
import { outHref } from '../lib/nav'
import NodeLink from '../components/NodeLink'
import Work from './Work'
import '../shell/shell.css'

export function ProductHome() {
  const w = useWords()
  const { data: proj } = useProject()
  const deliv = useDeliverable()
  const matrix = useMatrix()
  const workers = useWorkers()
  const { data: s } = useStatus()
  const counts = matrix.data?.counts ?? {}
  const files = deliv.data?.files ?? []
  const described = files.filter((f) => f.owners.entities.length + f.owners.requirements.length > 0).length
  return (
    <div className="ws-scroll narrow">
      <div className="page-head">
        <h1>{w.product}</h1>
        {proj && <span className="mono muted">{proj.deliverable}</span>}
      </div>
      {deliv.data && files.length === 0 && (
        <div className="empty-state">
          <b>No product yet</b>
          {proj?.deliverable ? `Nothing is in ${proj.deliverable} yet. Run ${w.build.toLowerCase()} to build it from the documents, or point Settings at an existing product folder.` : 'Name a product folder in Settings.'}
          <div className="actions">
            <Link to="/settings" className="btn">
              Settings
            </Link>
          </div>
        </div>
      )}
      {files.length > 0 && (
        <div className="stat-strip">
          <Link to="/product/files" className="stat">
            <b>{files.length}</b> files
          </Link>
          <span className="stat" title="product files a document describes">
            <b>{described}</b> described
          </span>
          <Link to="/product/not-described" className="stat" title="product files no document describes">
            <b>{workers.data?.unclaimed ?? files.length - described}</b> {w.notDescribed.toLowerCase()}
          </Link>
          {s && (
            <span className="stat" title="sections of the documents the model has read">
              <b>{s.coverage.covered}</b>/{s.coverage.total} sections read
            </span>
          )}
        </div>
      )}
      {Object.keys(counts).length > 0 && (
        <>
          <h2>{w.trace}</h2>
          <p>
            {Object.entries(counts).map(([st, n]) => (
              <span key={st} className={verifyClass(st)} style={{ marginRight: 12 }}>
                {n} {st}
              </span>
            ))}
            <Link to="/product/trace">open the matrix →</Link>
          </p>
        </>
      )}
    </div>
  )
}

export function FilesHome() {
  const w = useWords()
  const deliv = useDeliverable()
  const files = deliv.data?.files ?? []
  return (
    <div className="ws-scroll narrow">
      <h1>Files</h1>
      {deliv.data && files.length === 0 && (
        <div className="empty-state">
          <b>No product files</b>
          Run {w.build.toLowerCase()} to build the product from the documents.
        </div>
      )}
      {files.length > 0 && <p className="muted">Pick a file on the left. Each file shows which entities and requirements it carries.</p>}
    </div>
  )
}

export function Tests() {
  const matrix = useMatrix()
  const rows = Object.entries(matrix.data?.rows ?? {}).filter(([, r]) => r.test?.label || r.test?.run)
  return (
    <div className="ws-scroll narrow">
      <h1>Tests</h1>
      {matrix.data && rows.length === 0 && (
        <div className="empty-state">
          <b>No tests yet</b>
          A build writes one test per criterion; a check runs them.
        </div>
      )}
      {rows.map(([rid, r]) => (
        <div key={rid} className="card">
          <p className="row" style={{ margin: 0 }}>
            <NodeLink id={rid} />
            <span className={`chip ${verifyClass(r.status)}`}>{r.status}</span>
            <span className="mono muted">{r.test?.kind}</span>
            <span className="mono muted">{r.test?.label}</span>
          </p>
          {r.test?.run && <p className="mono muted" style={{ margin: '2px 0' }}>{r.test.run}</p>}
          {r.evidence && <p className="muted" style={{ margin: '2px 0' }}>{r.evidence}</p>}
        </div>
      ))}
    </div>
  )
}

export function NotDescribed() {
  const w = useWords()
  const workers = useWorkers()
  const deliv = useDeliverable()
  const jobs = useApp((a) => a.jobs)
  const busy = Object.values(jobs).some((j) => j.state === 'running' || j.state === 'queued')
  const files = (deliv.data?.files ?? []).filter((f) => f.owners.entities.length + f.owners.requirements.length === 0)
  const n = workers.data?.unclaimed ?? files.length
  return (
    <div className="ws-scroll narrow">
      <div className="page-head">
        <h1>{w.notDescribed}</h1>
        <span className="muted">{n === 0 ? 'every product file is described by a document' : `${n} file${n === 1 ? '' : 's'} no document describes`}</span>
      </div>
      {n === 0 && deliv.data && (
        <div className="empty-state">
          <b>Everything is described</b>
          Every product file carries at least one requirement from the documents.
        </div>
      )}
      {n > 0 && (
        <div className="card">
          <p className="row" style={{ margin: '2px 0' }}>
            <b>{w.document}</b>
            <span className="muted">Write documents describing what these files do. The drafts land in the Inbox for review.</span>
            <button disabled={busy} onClick={() => post('/api/jobs', { kind: 'decompile' })}>
              {w.document} the product ▸
            </button>
          </p>
          {(workers.data?.decompileReleased ?? []).length > 0 && <p className="muted mono">released for: {(workers.data?.decompileReleased ?? []).join(', ')}</p>}
        </div>
      )}
      {files.map((f) => (
        <p key={f.path} className="mono" style={{ margin: '2px 0' }}>
          <Link to={`/product/files/${f.path}`}>{f.path}</Link> <span className="muted">{f.size} bytes</span>
        </p>
      ))}
    </div>
  )
}

export function GeneratedHome() {
  const out = useOutList('docsgen')
  const pages = (out.data?.files ?? []).filter((f) => f.path.endsWith('.md') && !f.path.includes('/', 8))
  return (
    <div className="ws-scroll narrow">
      <h1>Generated docs</h1>
      <p className="muted">Reference pages written from the model after every sync: one document per entity, cards, level pages, diagram pages.</p>
      {out.data && pages.length === 0 && (
        <div className="empty-state">
          <b>No pages yet</b>
          The first sync writes them.
        </div>
      )}
      {pages.map((p) => (
        <p key={p.path} style={{ margin: '2px 0' }}>
          <Link to={outHref(p.path)}>{p.path.replace(/^docsgen\//, '')}</Link>
        </p>
      ))}
    </div>
  )
}

export function ProductForm() {
  const w = useWords()
  const { data: proj } = useProject()
  const deliv = useDeliverable()
  const m = deliv.data?.medium
  const b = deliv.data?.build
  return (
    <div className="ws-scroll narrow">
      <h1>{w.productForm}</h1>
      <p className="muted">The recorded form of the product: what it is, the toolchain that produces it, the build command, the last run.</p>
      {deliv.data && !m && (
        <div className="empty-state">
          <b>Not decided yet</b>
          The first {w.build.toLowerCase()} decides the form and records it here.
        </div>
      )}
      {m && (
        <div className="card">
          <p className="row" style={{ margin: '2px 0' }}>
            <b>form</b> <span>{m.form}</span>
            <span className="chip sev-none">{m.produced}</span>
          </p>
          {m.toolchain && (
            <p className="row" style={{ margin: '2px 0' }}>
              <b>toolchain</b> <span className="mono">{m.toolchain}</span>
            </p>
          )}
          {m.artifact && (
            <p className="row" style={{ margin: '2px 0' }}>
              <b>artifact</b> <span className="mono">{m.artifact}</span>
            </p>
          )}
          {proj && (
            <p className="row" style={{ margin: '2px 0' }}>
              <b>folder</b> <span className="mono">{proj.deliverable}</span>
            </p>
          )}
        </div>
      )}
      {b && (
        <div className="card">
          <p className="row" style={{ margin: '2px 0' }}>
            <b>build</b> <span className="mono">{b.run ?? ''}</span>
            {b.cwd && <span className="muted mono">in {b.cwd}</span>}
          </p>
          {(b.produces ?? []).length > 0 && (
            <p className="row" style={{ margin: '2px 0' }}>
              <b>produces</b> <span className="mono">{(b.produces ?? []).join(', ')}</span>
            </p>
          )}
          {b.lastRun && (
            <p className="row" style={{ margin: '2px 0' }}>
              <b>last run</b>
              <span className={b.lastRun.exitCode === 0 ? 'v-ok' : 'v-bad'}>{b.lastRun.exitCode === 0 ? 'succeeded' : `exit ${b.lastRun.exitCode ?? '?'}`}</span>
              {b.lastRun.at && <span className="muted mono">{String(b.lastRun.at)}</span>}
            </p>
          )}
          {b.lastRun?.evidence && <p className="muted mono" style={{ margin: '2px 0' }}>{String(b.lastRun.evidence)}</p>}
        </div>
      )}
    </div>
  )
}

export function ToBuild() {
  return (
    <div className="ws-scroll">
      <Work />
    </div>
  )
}
