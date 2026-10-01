// The app shell: the workspace once a project is open (top bar, navigator, the open
// items, the chat column), Home and the connect route before; the status bar under
// everything (docs/frontends/gui.md#layout).
import { useEffect, useState } from 'react'
import { Link, Navigate, useLocation } from 'react-router'
import TopBar from './shell/TopBar'
import AppRoutes from './routes/AppRoutes'
import StatusBar from './shell/StatusBar'
import CommandPalette from './components/CommandPalette'
import ConnectionGuard from './components/ConnectionGuard'
import { useApp } from './lib/store'
import { ExploreHistory } from './lib/explore'
import { useCapabilities } from './lib/capabilities'
import { useProject } from './lib/queries'
import { useProjectOpen } from './lib/open'
// The map pulls in the graph layout library; it loads when the map first opens
// (docs/frontends/wasm.md#building).
import Workspace from './workspace/Workspace'
import './workbench/workbench.css'
import './shell/shell.css'




function Scroll({ children }: { children: React.ReactNode }) {
  return <div className="ws-scroll">{children}</div>
}


// A remembered folder that needs the browser's permission again: one card in the
// workspace's place, whose click asks (docs/frontends/gui.md#the-open-project).
function ContinueCard({ name }: { name: string }) {
  return (
    <Scroll>
      <div className="empty-state ws-continue" style={{ marginTop: 60 }}>
        <b>Continue with {name}</b>
        The browser needs your permission to open the folder again.
        <div className="actions">
          <button className="primary" onClick={() => window.dispatchEvent(new CustomEvent('jazyk:continue-folder'))}>
            Continue with {name}
          </button>
          <button onClick={() => window.dispatchEvent(new CustomEvent('jazyk:decline-folder'))}>Not now</button>
        </div>
      </div>
    </Scroll>
  )
}



// The server's --dev turns the toggle on for a browser that never chose.
function DevInit() {
  const caps = useCapabilities()
  const { data: proj } = useProject()
  const chosen = useApp((a) => a.devChosen)
  const setDev = useApp((a) => a.setDev)
  useEffect(() => {
    if (chosen) return
    const dev = caps.data?.dev ?? (proj as { dev?: boolean } | undefined)?.dev
    if (dev) setDev(true, false)
  }, [caps.data, proj, chosen, setDev])
  return null
}

export default function App() {
  const loc = useLocation()
  const [palette, setPalette] = useState(0)
  // The connect route needs no project either: it is how one gets opened.
  const home = loc.pathname === '/' || loc.pathname === '/connect'
  const open = useProjectOpen()
  const pendingFolder = useApp((a) => a.pendingFolder)
  // The remembered project reopens before any route renders, Home included
  // (docs/frontends/gui.md#the-open-project).
  const reopening = useApp((a) => a.reopening)
  // On a narrow screen the navigator is a drawer, closed again by any navigation.
  const [navOpen, setNavOpen] = useState(false)
  useEffect(() => setNavOpen(false), [loc.pathname, loc.search])
  // The panes need a project; until one is open every route but Home goes home.
  const panes = !home && open === true
  // The workspace (docs/frontends/gui.md#workspace): Tabs or a Board once a project
  // is open; Home, the connect route, and a folder waiting for permission otherwise.
  const workspace = panes && !reopening

  return (
    <div className={`ws${navOpen ? ' nav-open' : ''}`}>
      {workspace ? (
        <Workspace openPalette={() => setPalette((n) => n + 1)} />
      ) : (
      <>
      <TopBar openPalette={() => setPalette((n) => n + 1)} open={open === true} toggleNav={() => setNavOpen((v) => !v)} />
      <div className="ws-body">
        <div className="ws-centerrow">
          <main className="ws-center">
            {reopening ? null : !home && !open && pendingFolder ? (
              <ContinueCard name={pendingFolder.name} />
            ) : !home && open === false ? (
              <Navigate to="/" replace />
            ) : !home && open === undefined ? null : (
            <AppRoutes />
            )}
          </main>
        </div>
      </div>
      </>
      )}
      <StatusBar />
      {open === true && <CommandPalette openSignal={palette} />}
      <CommitNote />
      <ConnectionGuard />
      <ExploreHistory />
      <DevInit />
    </div>
  )
}



// The only transient surface: a one-line committed-changeset notice, itself a link
// into the app (never a page load). It leaves on its own a few seconds later.
function CommitNote() {
  const last = useApp((a) => a.lastCommit)
  const [, tick] = useState(0)
  useEffect(() => {
    if (!last) return
    const t = window.setTimeout(() => tick((n) => n + 1), 6100)
    return () => window.clearTimeout(t)
  }, [last])
  if (!last || Date.now() - last.at > 6000) return null
  return (
    <Link className="commit-note mono" to={`/history/${last.generation}`}>
      build #{last.generation} landed →
    </Link>
  )
}
