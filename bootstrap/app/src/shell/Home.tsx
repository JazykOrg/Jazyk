// Home: the project picker (docs/frontends/gui.md#home). Short, with the ways in
// as the prominent thing: Open now when a project is open, four cards side by
// side, then Recent. The backend decides which entries exist.
import { useState } from 'react'
import { Link } from 'react-router'
import { storageAvailable } from '../lib/browser-store'
import { useCapabilities } from '../lib/capabilities'
import { folderPickerAvailable } from '../lib/folder'
import { useProjectOpen } from '../lib/open'
import { whereChangesGo } from '../lib/place'
import { DEMOS } from '../lib/demos'
import { holdsData, kindWords, setReadOnlyPref, useReadOnlyPref, useRecent, type RecentEntry } from '../lib/recent'
import { useProjects, type ProjectSummary } from '../lib/product'
import { useProject } from '../lib/queries'
import { useApp } from '../lib/store'
import { shapeWords, verdictWords } from '../lib/words'
import Wizard, { useSettingsExists } from './Wizard'
import ConnectCard from './ConnectCard'

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

// Try a demo: one row per bundled example. Works everywhere.
function DemoCard({ canStore }: { canStore: boolean }) {
  return (
    <section className="home-card">
      <h2>Try a demo</h2>
      <p className="muted">A bundled example, opened in this tab. Nothing is uploaded.</p>
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
    </section>
  )
}

// Start a project in this browser: a name, then Create. It starts in browser
// storage (docs/frontends/wasm.md#a-project-in-the-browser).
function NewProjectCard({ canStore }: { canStore: boolean }) {
  const [name, setName] = useState('')
  return (
    <section className={`home-card${canStore ? '' : ' home-nostore'}`}>
      <h2>Start a project in this browser</h2>
      <p className="muted">
        A <span className="mono">jazyk.toml</span> and a <span className="mono">docs/README.md</span> to write in, kept in this browser only. Save it to a folder
        or download it whenever you like.
      </p>
      {canStore ? (
        <form
          className="home-new-row"
          onSubmit={(e) => {
            e.preventDefault()
            send('jazyk:new-project', { name: name.trim() || 'Untitled project' })
          }}
        >
          <input type="text" value={name} onChange={(e) => setName(e.target.value)} placeholder="Project name, e.g. Garden planner" aria-label="Project name" />
          <button type="submit" className="primary">
            Create
          </button>
        </form>
      ) : (
        <p className="muted">This browser offers no storage to this page (a private window may not), so a project started here could not survive a reload.</p>
      )}
    </section>
  )
}

// Open a folder on this computer, with the read-only choice
// (docs/frontends/wasm.md#a-folder-on-disk). A browser without the File System
// Access API says what it needs and points at the next card.
function FolderCard({ picker }: { picker: boolean }) {
  const readOnly = useReadOnlyPref()
  if (!picker)
    return (
      <section className="home-card home-nofolder">
        <h2>Open a folder on this computer</h2>
        <p className="muted">
          Opening a folder needs Chrome, Edge, or another Chromium browser. To work on a folder from this browser, connect to jazyk on this computer
          (the next card).
        </p>
      </section>
    )
  return (
    <section className="home-card">
      <h2>Open a folder on this computer</h2>
      <p className="muted">
        Any folder. One without a <span className="mono">jazyk.toml</span> is set up here first. Everything runs in this browser.
      </p>
      <div className="actions">
        <button
          className="primary"
          onClick={() => send('jazyk:pick-folder', { mode: readOnly ? 'read' : 'write' })}
          title={readOnly ? 'changes stay in this browser until you write them' : 'edits and builds write back into the folder'}
        >
          Open folder…
        </button>
        <label className="muted home-readonly" title="keep every change in this browser until you choose to write it">
          <input
            type="checkbox"
            checked={readOnly}
            onChange={(e) => setReadOnlyPref(e.target.checked)}
          />{' '}
          read-only
        </label>
      </div>
    </section>
  )
}

// Open now: the project open in this tab, where its changes go, and the way back
// (docs/frontends/gui.md#home).
function OpenNow({ name }: { name: string }) {
  const caps = useCapabilities()
  const folder = useApp((a) => a.folder)
  const stored = useApp((a) => a.stored)
  return (
    <section className="home-open">
      <h2>Open now</h2>
      <p>
        <b className="name">{name}</b>
        <span className="muted"> · {whereChangesGo(caps.data, folder, stored)}</span>
      </p>
      <Link to="/docs" className="btn primary">
        Back to the project
      </Link>
    </section>
  )
}

export default function Home() {
  const caps = useCapabilities()
  const projects = useProjects()
  const { data: proj } = useProject()
  const dev = useApp((a) => a.dev)
  // In the browser the open project's own name, not its root in the tab's tree.
  const placeName = useApp((a) => a.folder?.name ?? a.stored?.name)
  const kind = caps.data?.projects ?? 'list'
  const open = useProjectOpen() === true
  const settings = useSettingsExists()
  const needsSetup = settings.data ? !settings.data.exists : false
  const recent = useRecent()
  const canStore = storageAvailable()
  const picker = kind === 'folder-picker' || (kind === 'bundled' && folderPickerAvailable())
  const name = placeName ?? proj?.root.replace(/\/+$/, '').split('/').pop() ?? 'the project'

  return (
    <div className="ws-scroll">
      <div className="home">
        <h1>jazyk</h1>
        <p className="tag">Documents that compile. Open a project to read, edit, and build from its documents.</p>

        {needsSetup && proj && settings.data && <Wizard payload={settings.data} root={placeName ?? proj.root} />}

        {open && !needsSetup && <OpenNow name={name} />}

        {(kind === 'bundled' || kind === 'folder-picker') && (
          <div className="home-cards">
            <DemoCard canStore={canStore} />
            <NewProjectCard canStore={canStore} />
            <FolderCard picker={picker} />
            <ConnectCard />
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
