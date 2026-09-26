// Home: the project picker (docs/frontends/gui.md#home). The backend decides which
// entries exist; the served project is the first row.
import { useState } from 'react'
import { Link } from 'react-router'
import { useCapabilities } from '../lib/capabilities'
import { folderPickerAvailable } from '../lib/folder'
import { readOnlyPref, removeRecent, setReadOnlyPref, useRecent, type RecentEntry } from '../lib/recent'
import { useProjects, type ProjectSummary } from '../lib/product'
import { useProject } from '../lib/queries'
import { useApp } from '../lib/store'
import { shapeWords, verdictWords } from '../lib/words'
import Wizard, { useSettingsExists } from './Wizard'

// The bundled examples (docs/frontends/gui.md#home). The editor entry fetches the
// fixture named by the id.
export const DEMOS = [
  { id: 'shop', name: 'The shop', blurb: 'a small online shop, three entities' },
  { id: 'sort', name: 'The sort utility', blurb: 'a sort described near pseudo-code, with its code and tests' },
  { id: 'erp', name: 'The ERP', blurb: 'a warehouse system across six documents' },
  { id: 'slides', name: 'The slide deck', blurb: 'a deck specified per slide, built to a file' },
  { id: 'novel', name: 'The novel', blurb: 'a narrative: characters, arcs, settings' },
  { id: 'org', name: 'The organization', blurb: 'a company handbook' },
]

// One server serves one project: the served row opens, the others say where to
// start a server (docs/frontends/gui.md#home).
function ProjectRow({ p, dev }: { p: ProjectSummary; dev: boolean }) {
  const waiting = p.verdict?.blocked ?? 0
  const errors = p.diagnostics?.error ?? 0
  const body = (
    <>
      <span className="name">
        {p.name || p.root}
        {p.current && <span className="muted"> · open</span>}
      </span>
      <span className="shape">{p.missing ? 'no jazyk.toml' : shapeWords(p.shape)}</span>
      <span className={`state${p.verdict?.state === 'incomplete' ? ' v-stale' : ''}`}>
        {p.missing ? '' : verdictWords(p.verdict, dev)}
        {waiting > 0 ? ` · ${waiting} waiting` : ''}
        {errors > 0 ? ` · ${errors} error${errors === 1 ? '' : 's'}` : ''}
      </span>
      <span className="path">{p.root}</span>
    </>
  )
  if (p.current)
    return (
      <Link to="/docs" className="proj" title="the project this server serves">
        {body}
      </Link>
    )
  return (
    <div className="proj other" title={`run jazyk gui in ${p.root} to open it`}>
      {body}
    </div>
  )
}

// The browser's own recent list (docs/frontends/gui.md#home): a row reopens its
// demo or folder, the cross forgets it.
function RecentRow({ r }: { r: RecentEntry }) {
  const kind = r.kind === 'demo' ? 'demo' : `folder · ${r.mode === 'write' ? 'writing' : 'read-only'}`
  return (
    <div className="proj recent">
      <button
        className="recent-open"
        onClick={() => window.dispatchEvent(new CustomEvent('jazyk:open-recent', { detail: r.id }))}
        title={r.kind === 'folder' ? 'reopen; the browser asks for access again' : 'reopen'}
      >
        <span className="name">{r.name}</span>
        <span className="shape">{kind}</span>
        <span className="state muted">{new Date(r.openedAt).toLocaleDateString()}</span>
      </button>
      <button className="recent-forget" onClick={() => removeRecent(r.id)} title="forget this entry" aria-label={`forget ${r.name}`}>
        ×
      </button>
    </div>
  )
}

export default function Home() {
  const caps = useCapabilities()
  const projects = useProjects()
  const { data: proj } = useProject()
  const dev = useApp((a) => a.dev)
  const kind = caps.data?.projects ?? 'list'
  const settings = useSettingsExists()
  const needsSetup = settings.data ? !settings.data.exists : false
  const recent = useRecent()
  const [readOnly, setReadOnly] = useState(readOnlyPref)
  const picker = kind === 'folder-picker' || (kind === 'bundled' && folderPickerAvailable())

  return (
    <div className="ws-scroll">
      <div className="home">
        <h1>jazyk</h1>
        <p className="tag">Documents that compile. Open a project to read, edit, and build from its documents.</p>

        {caps.data?.backend === 'demo' && <div className="banner">Demo: changes stay in this tab.</div>}
        {caps.data?.backend === 'wasm' && <div className="banner">Running in this browser over a folder on your computer. Nothing leaves the machine.</div>}

        {needsSetup && proj && settings.data && <Wizard payload={settings.data} root={proj.root} />}

        <div className="actions">
          {proj && !needsSetup && (
            <Link to="/docs" className="btn">
              Open {proj.root.replace(/\/+$/, '').split('/').pop()} →
            </Link>
          )}
          {picker && (
            <>
              <button
                onClick={() => window.dispatchEvent(new CustomEvent('jazyk:pick-folder', { detail: { mode: readOnly ? 'read' : 'write' } }))}
                title={readOnly ? 'a directory with a jazyk.toml; changes stay in this tab until you write them' : 'a directory with a jazyk.toml; edits and builds write back into it'}
              >
                Open folder…
              </button>
              <label className="muted home-readonly" title="hold every change in this tab until you choose to write it">
                <input
                  type="checkbox"
                  checked={readOnly}
                  onChange={(e) => {
                    setReadOnly(e.target.checked)
                    setReadOnlyPref(e.target.checked)
                  }}
                />{' '}
                read-only
              </label>
            </>
          )}
        </div>

        {kind === 'bundled' && recent.length > 0 && (
          <>
            <h2>Recent</h2>
            {recent.map((r) => (
              <RecentRow key={r.id} r={r} />
            ))}
          </>
        )}

        {kind === 'bundled' && (
          <>
            <h2>Demos</h2>
            <div className="demos">
              {DEMOS.map((d) => (
                <button
                  key={d.id}
                  className="demo"
                  onClick={() => window.dispatchEvent(new CustomEvent('jazyk:open-demo', { detail: d.id }))}
                  title="opens in this tab; changes stay in the tab"
                >
                  <span className="name">{d.name}</span>
                  <span className="blurb">{d.blurb}</span>
                </button>
              ))}
            </div>
          </>
        )}

        {kind === 'list' && (
          <>
            <h2>Recent</h2>
            {projects.isError && (
              <p className="muted">This server lists no other projects. Start `jazyk gui` in a project folder to open it.</p>
            )}
            {(projects.data?.projects ?? []).map((p) => (
              <ProjectRow key={p.root} p={p} dev={dev} />
            ))}
          </>
        )}

        {!needsSetup && (
          <>
            <h2>New project</h2>
            <p className="muted">
              Point jazyk at a folder of documents, a product, or both: run <span className="mono">jazyk gui</span> in that folder and this
              screen walks you through the rest.
            </p>
          </>
        )}
      </div>
    </div>
  )
}
