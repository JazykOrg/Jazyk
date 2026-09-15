// The workspace shell: top bar | navigator | center + inspector | chat column,
// status bar under everything. Navigation swaps panes, never the page
// (docs/frontends/gui.md#layout).
import { useEffect, useState } from 'react'
import { Link, Navigate, Route, Routes, useLocation, useParams } from 'react-router'
import TopBar from './shell/TopBar'
import StatusBar from './shell/StatusBar'
import Navigator from './shell/Navigator'
import Home from './shell/Home'
import CommandPalette from './components/CommandPalette'
import ConnectionGuard from './components/ConnectionGuard'
import { useApp } from './lib/store'
import { useInspector } from './lib/nav'
import { ExploreHistory } from './lib/explore'
import { useCapabilities } from './lib/capabilities'
import { useProject } from './lib/queries'
import DocEditor from './workbench/DocEditor'
import DelivFile from './workbench/DelivFile'
import OutFile from './workbench/OutFile'
import MapCenter from './workbench/MapCenter'
import Inspector from './workbench/Inspector'
import ChatPane from './chat/ChatPane'
import Board from './routes/Board'
import GenTask from './routes/GenTask'
import VerifyMatrix from './routes/VerifyMatrix'
import Benchmarks from './routes/Benchmarks'
import Feedback from './routes/Feedback'
import Settings from './routes/Settings'
import Changeset from './routes/Changeset'
import ReleaseDiff from './routes/ReleaseDiff'
import Inbox from './routes/Inbox'
import History from './routes/History'
import { Journal, Prompt, Runs, Workers } from './routes/DevPages'
import { DiagramGallery, DiagramPage, EntityPage, ModelHome, RequirementPage } from './routes/ModelPages'
import { FilesHome, GeneratedHome, NotDescribed, ProductForm, ProductHome, Tests, ToBuild } from './routes/ProductPages'
import './workbench/workbench.css'
import './shell/shell.css'

// A legacy path with a splat, remapped under a new prefix, query preserved.
function LegacyPath({ to }: { to: string }) {
  const params = useParams()
  const loc = useLocation()
  return <Navigate to={`${to}/${params['*'] ?? ''}${loc.search}`} replace />
}

function LegacyQuery({ to }: { to: string }) {
  const loc = useLocation()
  return <Navigate to={`${to}${loc.search}`} replace />
}

function LegacyNode() {
  const { id } = useParams()
  const node = decodeURIComponent(id ?? '')
  return <Navigate to={`/model?node=${encodeURIComponent(node)}`} replace />
}

function Scroll({ children }: { children: React.ReactNode }) {
  return <div className="ws-scroll">{children}</div>
}

// The Developer lens exists only while the toggle is on; a URL under it turns the
// toggle on rather than 404ing, so a shared link lands where it points.
function DevGate({ children }: { children: React.ReactNode }) {
  const dev = useApp((a) => a.dev)
  const setDev = useApp((a) => a.setDev)
  useEffect(() => {
    if (!dev) setDev(true)
  }, [dev, setDev])
  return <>{children}</>
}

function DocsHome() {
  return (
    <Scroll>
      <div className="empty-state" style={{ marginTop: 60 }}>
        <b>Pick a document</b>
        The documents on the left are the source. Open one to read it with what the model extracted, or create one with +.
      </div>
    </Scroll>
  )
}

function DevHome() {
  return (
    <Scroll>
      <h1>Developer</h1>
      <p className="muted">The compiler workbench: the goal board, the runs, the prompts, the journal, the workers, the map, the benchmarks, the feedback log.</p>
      <p>
        <Link to="/dev/board">Board</Link> · <Link to="/dev/runs">Runs</Link> · <Link to="/dev/prompt">Prompt</Link> · <Link to="/dev/journal">Journal</Link> ·{' '}
        <Link to="/dev/workers">Workers</Link> · <Link to="/dev/map">Map</Link> · <Link to="/dev/benchmarks">Benchmarks</Link> · <Link to="/dev/feedback">Feedback</Link>
      </p>
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
  const { node, openNode, closeNode } = useInspector()
  const [palette, setPalette] = useState(0)
  const home = loc.pathname === '/'

  return (
    <div className="ws">
      <TopBar openPalette={() => setPalette((n) => n + 1)} />
      <div className="ws-body">
        {!home && <Navigator />}
        <div className="ws-centerrow">
          <main className="ws-center">
            <Routes>
              <Route path="/" element={<Home />} />
              <Route path="/docs" element={<DocsHome />} />
              <Route path="/docs/*" element={<DocEditor />} />
              <Route path="/model" element={<ModelHome />} />
              <Route path="/model/entities" element={<ModelHome />} />
              <Route path="/model/entities/:id" element={<EntityPage />} />
              <Route path="/model/requirements" element={<ModelHome />} />
              <Route path="/model/requirements/:id" element={<RequirementPage />} />
              <Route path="/model/diagrams" element={<DiagramGallery />} />
              <Route path="/model/diagrams/:id" element={<DiagramPage />} />
              <Route path="/model/*" element={<ModelHome />} />
              <Route path="/product" element={<ProductHome />} />
              <Route path="/product/files" element={<FilesHome />} />
              <Route path="/product/files/*" element={<DelivFile />} />
              <Route path="/product/trace" element={<Scroll><VerifyMatrix /></Scroll>} />
              <Route path="/product/tests" element={<Tests />} />
              <Route path="/product/not-described" element={<NotDescribed />} />
              <Route path="/product/generated" element={<GeneratedHome />} />
              <Route path="/product/generated/*" element={<OutFile />} />
              <Route path="/product/form" element={<ProductForm />} />
              <Route path="/product/build" element={<ToBuild />} />
              <Route path="/inbox" element={<Inbox />} />
              <Route path="/inbox/:section" element={<Inbox />} />
              <Route path="/history" element={<History />} />
              <Route path="/history/diff" element={<Scroll><ReleaseDiff /></Scroll>} />
              <Route path="/history/:gen" element={<Scroll><Changeset /></Scroll>} />
              <Route path="/settings" element={<Scroll><Settings /></Scroll>} />
              <Route path="/dev" element={<DevGate><DevHome /></DevGate>} />
              <Route path="/dev/board" element={<DevGate><Scroll><Board /></Scroll></DevGate>} />
              <Route path="/dev/runs" element={<DevGate><Runs /></DevGate>} />
              <Route path="/dev/prompt" element={<DevGate><Prompt /></DevGate>} />
              <Route path="/dev/journal" element={<DevGate><Journal /></DevGate>} />
              <Route path="/dev/workers" element={<DevGate><Workers /></DevGate>} />
              <Route path="/dev/map" element={<DevGate><MapCenter /></DevGate>} />
              <Route path="/dev/benchmarks" element={<DevGate><Scroll><Benchmarks /></Scroll></DevGate>} />
              <Route path="/dev/feedback" element={<DevGate><Scroll><Feedback /></Scroll></DevGate>} />
              <Route path="/dev/gen/:id" element={<DevGate><Scroll><GenTask /></Scroll></DevGate>} />
              {/* Every URL the previous layout served redirects to its new place. */}
              <Route path="/files" element={<Navigate to="/docs" replace />} />
              <Route path="/files/docs/*" element={<LegacyPath to="/docs" />} />
              <Route path="/files/deliverable/*" element={<LegacyPath to="/product/files" />} />
              <Route path="/files/out/*" element={<LegacyPath to="/product/generated" />} />
              <Route path="/docs-legacy/*" element={<LegacyPath to="/docs" />} />
              <Route path="/deliverable/*" element={<LegacyPath to="/product/files" />} />
              <Route path="/graph" element={<LegacyQuery to="/dev/map" />} />
              <Route path="/map" element={<LegacyQuery to="/dev/map" />} />
              <Route path="/ir" element={<Navigate to="/model" replace />} />
              <Route path="/ir/:tab" element={<Navigate to="/model" replace />} />
              <Route path="/n/:id" element={<LegacyNode />} />
              <Route path="/board" element={<LegacyQuery to="/dev/board" />} />
              <Route path="/work" element={<Navigate to="/product/build" replace />} />
              <Route path="/work/verify" element={<Navigate to="/product/trace" replace />} />
              <Route path="/work/gen/:id" element={<LegacyGen />} />
              <Route path="/build" element={<Navigate to="/dev/runs" replace />} />
              <Route path="/journal" element={<Navigate to="/history" replace />} />
              <Route path="/journal/diff" element={<LegacyQuery to="/history/diff" />} />
              <Route path="/journal/:gen" element={<LegacyGenPage />} />
              <Route path="/feedback" element={<LegacyQuery to="/dev/feedback" />} />
              <Route path="/benchmarks" element={<Navigate to="/dev/benchmarks" replace />} />
              <Route path="*" element={<Scroll><p className="empty">no such page</p></Scroll>} />
            </Routes>
          </main>
          {!home && <Inspector node={node} openNode={openNode} close={closeNode} />}
        </div>
        {!home && <ChatPane />}
      </div>
      <StatusBar />
      <CommandPalette openSignal={palette} />
      <CommitNote />
      <ConnectionGuard />
      <ExploreHistory />
      <DevInit />
    </div>
  )
}

function LegacyGen() {
  const { id } = useParams()
  return <Navigate to={`/dev/gen/${id ?? ''}`} replace />
}

function LegacyGenPage() {
  const { gen } = useParams()
  return <Navigate to={`/history/${gen ?? ''}`} replace />
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
      sync #{last.generation} landed →
    </Link>
  )
}
