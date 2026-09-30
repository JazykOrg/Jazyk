// The workspace (docs/frontends/gui.md#workspace-experimental, plans/workspace.md): the
// top bar without lenses, one navigator, the open items as Tabs or a Board, the
// Connections panel under them in the inspector's place, and the chat column.
//
// The address bar is the focused item's path. A navigation anywhere (a link in a
// page, the navigator, search, back and forward) shows the item for its path instead
// of replacing the center, and a `?node=` in it selects that node for Connections.
import { lazy, Suspense, useEffect, useRef, useState } from 'react'
import { useLocation, useNavigate } from 'react-router'
import TopBar from '../shell/TopBar'
import ChatPane from '../chat/ChatPane'
import { useProject } from '../lib/queries'
import { useApp } from '../lib/store'
import { useNarrow } from '../lib/narrow'
import { treeProjectKey } from '../lib/tree'
import { useWorkspace } from './items'
import { isShellPath, itemKey, splitSelection } from './paths'
import { findItem, pinPath, showPath } from './actions'
import WorkspaceNavigator, { takePreviewOpen } from './Navigator'
import ConnectionsPanel from './ConnectionsPanel'
import Tabs, { NarrowTabs } from './Tabs'
import './workspace.css'

// The board carries the canvas library; it loads when the Board arrangement first shows.
const BoardArrangement = lazy(() => import('./board/Board'))

export default function Workspace({ openPalette }: { openPalette: () => void }) {
  const loc = useLocation()
  const navigate = useNavigate()
  const narrow = useNarrow()
  const arrangement = useWorkspace((w) => w.arrangement)
  const project = useWorkspace((w) => w.project)
  const loadProject = useWorkspace((w) => w.loadProject)
  const { data: proj } = useProject()
  const key = treeProjectKey(proj?.root ?? '') || null
  const [navOpen, setNavOpen] = useState(false)
  useEffect(() => setNavOpen(false), [loc.pathname, loc.search])

  useEffect(() => {
    if (key) loadProject(key)
  }, [key, loadProject])

  // Where the address bar is, without the selection: a focus that already matches it
  // navigates nowhere.
  const here = useRef(splitSelection(loc.pathname + loc.search + loc.hash).path)

  // The address bar into the items.
  useEffect(() => {
    if (!project) return
    const { path, node } = splitSelection(loc.pathname + loc.search + loc.hash)
    here.current = path
    if (node) useWorkspace.getState().select(node)
    if (isShellPath(path)) return
    const preview = takePreviewOpen()
    const from = useWorkspace.getState().focused ?? undefined
    showPath(path, { preview, from: from && itemKey(from) !== itemKey(path) ? from : undefined })
    // loc.key: the same path navigated again still shows (and selects) it.
  }, [loc.key, loc.pathname, loc.search, loc.hash, project])

  // The focused item into the address bar: a tab brought to the front, a tab closed.
  useEffect(
    () =>
      useWorkspace.subscribe((s, prev) => {
        if (s.project !== prev.project || s.focused === prev.focused) return
        if (s.focused === null) {
          if (s.items.length === 0 && !isShellPath(here.current)) {
            here.current = '/docs'
            navigate('/docs', { replace: true })
          }
          return
        }
        if (s.focused === here.current) return
        here.current = s.focused
        navigate(s.focused)
      }),
    [navigate],
  )

  // An edit keeps a preview item.
  useEffect(
    () =>
      useApp.subscribe((a, prev) => {
        if (!a.editorDirty || prev.editorDirty) return
        const f = useWorkspace.getState().focused
        if (f && findItem(f)?.preview) pinPath(f)
      }),
    [],
  )

  const show = (path: string) => showPath(path)

  return (
    <>
      <TopBar openPalette={openPalette} open toggleNav={() => setNavOpen((v) => !v)} workspace />
      <div className={`ws-body wsw${navOpen ? ' nav-open' : ''}`}>
        <WorkspaceNavigator />
        {navOpen && <div className="ws-scrim" onClick={() => setNavOpen(false)} />}
        <div className="ws-centerrow">
          <main className="ws-center wsw-center">
            <div className="wsw-items">{arrangement === 'board' ? <Suspense fallback={null}><BoardArrangement /></Suspense> : narrow ? <NarrowTabs show={show} /> : <Tabs />}</div>
            <ConnectionsPanel narrow={narrow} />
          </main>
        </div>
        {!narrow && <ChatPane />}
      </div>
    </>
  )
}
