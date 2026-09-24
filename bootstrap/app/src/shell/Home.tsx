// Home: the project picker (docs/frontends/gui.md#home). The backend decides which
// entries exist; the served project is the first row.
import { Link } from 'react-router'
import { useCapabilities } from '../lib/capabilities'
import { folderPickerAvailable } from '../lib/folder'
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

export default function Home() {
  const caps = useCapabilities()
  const projects = useProjects()
  const { data: proj } = useProject()
  const dev = useApp((a) => a.dev)
  const kind = caps.data?.projects ?? 'list'
  const settings = useSettingsExists()
  const needsSetup = settings.data ? !settings.data.exists : false

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
          {(kind === 'folder-picker' || (kind === 'bundled' && folderPickerAvailable())) && (
            <button onClick={() => window.dispatchEvent(new CustomEvent('jazyk:pick-folder'))} title="a directory with a jazyk.toml; edits and builds write back into it">
              Open folder…
            </button>
          )}
        </div>

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
