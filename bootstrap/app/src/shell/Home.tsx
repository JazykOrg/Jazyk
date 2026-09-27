// Home: the project picker (docs/frontends/gui.md#home). The backend decides which
// entries exist; the served project is the first row.
import { useState } from 'react'
import { Link } from 'react-router'
import { storageAvailable } from '../lib/browser-store'
import { useCapabilities } from '../lib/capabilities'
import { folderPickerAvailable } from '../lib/folder'
import { holdsData, readOnlyPref, setReadOnlyPref, useRecent, type RecentEntry } from '../lib/recent'
import { useProjects, type ProjectSummary } from '../lib/product'
import { useProject } from '../lib/queries'
import { useApp } from '../lib/store'
import { shapeWords, verdictWords } from '../lib/words'
import Wizard, { useSettingsExists } from './Wizard'
import ConnectCard from './ConnectCard'

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

const send = (name: string, detail?: unknown) => window.dispatchEvent(new CustomEvent(name, { detail }))

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

// The kind of a recent entry in words (docs/frontends/gui.md#home).
function kindWords(r: RecentEntry): string {
  if (r.kind === 'demo') return r.edited ? 'demo with your edits' : 'demo'
  if (r.kind === 'browser') return 'browser project · only in this browser'
  if (r.kind === 'server') return `jazyk on this computer · port ${r.port ?? r.id.slice('server:'.length)}`
  const kept = r.kept ?? 0
  const mode = `folder · ${r.mode === 'write' ? 'writing' : 'read-only'}`
  return kept > 0 ? `${mode} · ${kept} change${kept === 1 ? '' : 's'} kept in this browser` : mode
}

// The browser's own recent list (docs/frontends/gui.md#home): a row reopens its
// demo, browser project, or folder; an edited demo resets to its original; the cross
// forgets it, deleting what the browser stored for it after asking.
function RecentRow({ r }: { r: RecentEntry }) {
  const title =
    r.kind === 'server'
      ? `reconnect; run jazyk gui in ${r.root ?? 'the project'} if it is not running`
      : r.kind === 'folder'
      ? 'reopen; the browser asks for access again'
      : r.kind === 'browser'
        ? 'reopen; this project is stored in this browser only'
        : r.edited
          ? 'reopen with your edits'
          : 'reopen'
  return (
    <div className="proj recent">
      <button className="recent-open" onClick={() => send('jazyk:open-recent', r.id)} title={title}>
        <span className="name">{r.name}</span>
        <span className={`shape${(r.kept ?? 0) > 0 ? ' kept' : ''}`}>{kindWords(r)}</span>
        <span className="state muted">{new Date(r.openedAt).toLocaleDateString()}</span>
      </button>
      {r.kind === 'demo' && r.edited && (
        <button className="recent-reset" onClick={() => send('jazyk:reset-demo', r.id)} title="discard your edits and go back to the demo as shipped">
          Reset to the original
        </button>
      )}
      <button
        className="recent-forget"
        onClick={() => send('jazyk:forget-recent', r.id)}
        title={holdsData(r) ? 'forget this entry and delete what this browser stored for it' : 'forget this entry'}
        aria-label={`forget ${r.name}`}
      >
        ×
      </button>
    </div>
  )
}

// New project: a name, then Create. The project starts in browser storage
// (docs/frontends/wasm.md#a-project-in-the-browser).
function NewProjectForm({ onClose }: { onClose: () => void }) {
  const [name, setName] = useState('')
  return (
    <form
      className="home-new"
      onSubmit={(e) => {
        e.preventDefault()
        send('jazyk:new-project', { name: name.trim() || 'Untitled project' })
        onClose()
      }}
    >
      <div className="home-new-row">
        <input type="text" autoFocus value={name} onChange={(e) => setName(e.target.value)} placeholder="Project name, e.g. Garden planner" aria-label="Project name" />
        <button type="submit" className="primary">
          Create
        </button>
        <button type="button" onClick={onClose}>
          Cancel
        </button>
      </div>
      <p className="muted">
        It starts with a <span className="mono">jazyk.toml</span> and a <span className="mono">docs/README.md</span> to write in, and it is kept in
        this browser only. Save it to a folder or download it whenever you like.
      </p>
    </form>
  )
}

// Where changes go, from the descriptor (docs/frontends/gui.md#backends-and-capabilities).
function Banner({ backend, persistence }: { backend?: string; persistence?: string }) {
  if (backend === 'demo')
    return (
      <div className="banner">
        {persistence === 'memory' ? 'Demo: changes stay in this tab.' : 'Demos keep your edits in this browser until you reset them.'}
      </div>
    )
  if (backend === 'wasm' && persistence === 'folder')
    return <div className="banner">Running in this browser over a folder on your computer. Nothing leaves the machine.</div>
  if (backend === 'wasm' && persistence === 'browser')
    return (
      <div className="banner">
        Running in this browser over a project stored in this browser only. Save it to a folder or download it to keep a copy on disk.
      </div>
    )
  return null
}

export default function Home() {
  const caps = useCapabilities()
  const projects = useProjects()
  const { data: proj } = useProject()
  const dev = useApp((a) => a.dev)
  // In the browser the open project's own name, not its root in the tab's tree.
  const placeName = useApp((a) => a.folder?.name ?? a.stored?.name)
  const kind = caps.data?.projects ?? 'list'
  const settings = useSettingsExists()
  const needsSetup = settings.data ? !settings.data.exists : false
  const recent = useRecent()
  const [readOnly, setReadOnly] = useState(readOnlyPref)
  const [naming, setNaming] = useState(false)
  const canStore = storageAvailable()
  const picker = kind === 'folder-picker' || (kind === 'bundled' && folderPickerAvailable())

  return (
    <div className="ws-scroll">
      <div className="home">
        <h1>jazyk</h1>
        <p className="tag">Documents that compile. Open a project to read, edit, and build from its documents.</p>

        <Banner backend={caps.data?.backend} persistence={caps.data?.persistence} />

        {needsSetup && proj && settings.data && <Wizard payload={settings.data} root={proj.root} />}

        {proj && !needsSetup && (
          <div className="actions">
            <Link to="/docs" className="btn">
              Open {placeName ?? proj.root.replace(/\/+$/, '').split('/').pop()} →
            </Link>
          </div>
        )}

        {recent.length > 0 && (
          <>
            <h2>Recent</h2>
            {recent.map((r) => (
              <RecentRow key={r.id} r={r} />
            ))}
          </>
        )}

        {(kind === 'bundled' || kind === 'folder-picker') && (
          <>
            <h2>Local project</h2>
            <p className="muted">
              Start a new project kept in this browser, or open a folder on this computer that has a <span className="mono">jazyk.toml</span> at
              its top. Everything runs here, in this browser; nothing is uploaded.
            </p>
            <div className="actions">
              <button
                onClick={() => setNaming(true)}
                disabled={!canStore || naming}
                title={canStore ? 'start a project from scratch, stored in this browser' : 'this browser offers no storage to this page'}
              >
                New project
              </button>
              {picker && (
                <>
                  <button
                    onClick={() => send('jazyk:pick-folder', { mode: readOnly ? 'read' : 'write' })}
                    title={readOnly ? 'changes stay in this browser until you write them' : 'edits and syncs write back into the folder'}
                  >
                    Open folder…
                  </button>
                  <label className="muted home-readonly" title="keep every change in this browser until you choose to write it">
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
            {naming && <NewProjectForm onClose={() => setNaming(false)} />}
            <ConnectCard />
            {!canStore && (
              <div className="banner home-nostore">
                This browser offers no storage to this page (a private window may not), so a new project could not survive a reload.
              </div>
            )}
            {!picker && (
              <div className="banner home-nofolder">
                This browser cannot open a folder: it has no File System Access API. Use Chrome, Edge, or another Chromium browser to open a
                local project. New projects and the demos work here.
              </div>
            )}
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
                  onClick={() => send('jazyk:open-demo', d.id)}
                  title={canStore ? 'opens in this tab; your edits are kept in this browser' : 'opens in this tab; changes stay in the tab'}
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

        {!needsSetup && kind !== 'bundled' && (
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
