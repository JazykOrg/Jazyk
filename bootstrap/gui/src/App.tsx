// The workbench shell: rail | sidebar | center + inspector, activity panel and
// status bar below. Navigation swaps panes, never the page
// (docs/frontends/gui.md#layout).
import { useEffect } from 'react'
import { Link, Navigate, NavLink, Route, Routes, useLocation, useParams } from 'react-router'
import StatusBar from './components/StatusBar'
import CommandPalette from './components/CommandPalette'
import ConnectionGuard from './components/ConnectionGuard'
import { useApp } from './lib/store'
import { useInspector } from './lib/nav'
import Explorer from './workbench/Explorer'
import GraphSidebar from './workbench/GraphSidebar'
import WorkSidebar from './workbench/WorkSidebar'
import DocEditor from './workbench/DocEditor'
import DelivFile from './workbench/DelivFile'
import MapCenter from './workbench/MapCenter'
import Inspector from './workbench/Inspector'
import Activity from './workbench/Activity'
import FilesHome from './workbench/FilesHome'
import Work from './routes/Work'
import GenTask from './routes/GenTask'
import VerifyMatrix from './routes/VerifyMatrix'
import Settings from './routes/Settings'
import Changeset from './routes/Changeset'
import ReleaseDiff from './routes/ReleaseDiff'
import './workbench/workbench.css'

const RAIL = [
  ['/files', '▤', 'files'],
  ['/graph', '⬡', 'graph'],
  ['/work', '⚒', 'work'],
  ['/settings', '⚙', 'settings'],
] as const

function railMode(pathname: string): string {
  if (pathname.startsWith('/graph')) return '/graph'
  if (pathname.startsWith('/work')) return '/work'
  if (pathname.startsWith('/settings')) return '/settings'
  return '/files'
}

// A legacy path with a splat, remapped under a new prefix, query preserved.
function LegacyPath({ to }: { to: string }) {
  const params = useParams()
  const loc = useLocation()
  return <Navigate to={`${to}/${params['*'] ?? ''}${loc.search}`} replace />
}

// Legacy /n/:id lands on the graph with the node inspected and focused.
function LegacyNode() {
  const { id } = useParams()
  const node = decodeURIComponent(id ?? '')
  return <Navigate to={`/graph?node=${encodeURIComponent(node)}&focus=${encodeURIComponent(node)}`} replace />
}

// Legacy /build and /journal: the activity panel, on the files center.
function LegacyActivity() {
  const setOpen = useApp((a) => a.setActivityOpen)
  useEffect(() => setOpen(true), [setOpen])
  return <Navigate to="/files" replace />
}

function Scroll({ children }: { children: React.ReactNode }) {
  return <div className="wb-center-scroll">{children}</div>
}

export default function App() {
  const theme = useApp((a) => a.theme)
  const setTheme = useApp((a) => a.setTheme)
  const loc = useLocation()
  const mode = railMode(loc.pathname)
  const { node, openNode, closeNode } = useInspector()

  return (
    <>
      <div className="wb">
        <nav className="wb-rail">
          {RAIL.map(([to, glyph, label]) => (
            <NavLink key={to} to={to} className={mode === to ? 'active' : ''} title={label}>
              <span className="glyph">{glyph}</span>
              {label}
            </NavLink>
          ))}
          <div className="rail-bottom">
            <a
              href="#theme"
              className="muted"
              onClick={(e) => {
                e.preventDefault()
                setTheme(theme === 'auto' ? 'dark' : theme === 'dark' ? 'light' : 'auto')
              }}
              title="theme: auto / dark / light"
            >
              {theme}
            </a>
          </div>
        </nav>

        <aside className="wb-side">
          {mode === '/files' && <Explorer />}
          {mode === '/graph' && <GraphSidebar />}
          {mode === '/work' && <WorkSidebar />}
          {mode === '/settings' && (
            <div className="wb-side-pad muted">
              <p>
                project settings, edited as a form; saving rewrites <span className="mono">jazyk.toml</span> and
                applies live.
              </p>
              <p>
                <Link to="/settings">open the form</Link>
              </p>
            </div>
          )}
        </aside>

        <div className="wb-stack">
          <div className="wb-centerrow">
            <main className="wb-center">
              <Routes>
                <Route path="/" element={<Navigate to="/files" replace />} />
                <Route path="/files" element={<Scroll><FilesHome /></Scroll>} />
                <Route path="/files/docs/*" element={<DocEditor />} />
                <Route path="/files/deliverable/*" element={<DelivFile />} />
                <Route path="/graph" element={<MapCenter />} />
                <Route path="/work" element={<Scroll><Work /></Scroll>} />
                <Route path="/work/gen/:id" element={<Scroll><GenTask /></Scroll>} />
                <Route path="/work/verify" element={<Scroll><VerifyMatrix /></Scroll>} />
                <Route path="/settings" element={<Scroll><Settings /></Scroll>} />
                <Route path="/journal/diff" element={<Scroll><ReleaseDiff /></Scroll>} />
                <Route path="/journal/:gen" element={<Scroll><Changeset /></Scroll>} />
                {/* Routes from the earlier tabbed layout redirect to their new homes. */}
                <Route path="/docs/*" element={<LegacyPath to="/files/docs" />} />
                <Route path="/deliverable/*" element={<LegacyPath to="/files/deliverable" />} />
                <Route path="/map" element={<Navigate to={`/graph${loc.search}`} replace />} />
                <Route path="/ir" element={<Navigate to="/graph" replace />} />
                <Route path="/ir/:tab" element={<Navigate to="/graph" replace />} />
                <Route path="/n/:id" element={<LegacyNode />} />
                <Route path="/build" element={<LegacyActivity />} />
                <Route path="/journal" element={<LegacyActivity />} />
                <Route path="*" element={<Scroll><p className="empty">no such page</p></Scroll>} />
              </Routes>
            </main>
            <Inspector node={node} openNode={openNode} close={closeNode} />
          </div>
          <Activity />
        </div>
      </div>
      <StatusBar />
      <CommandPalette />
      <CommitNote />
      <ConnectionGuard />
    </>
  )
}

// The only transient surface: a one-line committed-changeset notice, itself a link.
function CommitNote() {
  const last = useApp((a) => a.lastCommit)
  if (!last || Date.now() - last.at > 6000) return null
  return (
    <a className="commit-note mono" href={`/journal/${last.generation}`}>
      g{last.generation} committed →
    </a>
  )
}
