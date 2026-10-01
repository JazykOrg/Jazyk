// The workspace (docs/frontends/gui.md#workspace, plans/workspace.md): the
// top bar without lenses, one navigator, the open items as Tabs or a Board, and the
// chat column. There is no inspector: a node opens as its own item.
//
// The address bar is the focused item's path. A navigation anywhere (a link in a
// page, the navigator, search, back and forward) shows the item for its path instead
// of replacing the center, and a `?node=` in it (a Cmd-click on a mention, a marker,
// a diagram box) selects that node and opens its page as an item.
import { lazy, Suspense, useEffect, useRef, useState } from 'react'
import { useLocation, useNavigate } from 'react-router'
import TopBar from '../shell/TopBar'
import ChatPane from '../chat/ChatPane'
import { useProject } from '../lib/queries'
import { useApp } from '../lib/store'
import { useNarrow } from '../lib/narrow'
import { treeProjectKey } from '../lib/tree'
import { useWorkspace } from './items'
import { isShellPath, itemKey, nodePagePath, splitSelection } from './paths'
import { findItem, pinPath, showPath, skipClosed } from './actions'
import WorkspaceNavigator, { takePreviewOpen } from './Navigator'
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
  // A `?node=` step stands for the node's page: the page takes its place in the
  // history rather than a step after it, so back leaves the page.
  const replaceNext = useRef(false)

  // The address bar into the items.
  useEffect(() => {
    if (!project) return
    const { path, node } = splitSelection(loc.pathname + loc.search + loc.hash)
    here.current = path
    // A close is a back: the steps of the closed item are walked past.
    const skip = skipClosed(path)
    if (skip === 'skip') return
    if (skip === 'other') {
      const first = useWorkspace.getState().items[0]
      if (first) useWorkspace.getState().focusItem(first.path)
      return
    }
    if (node) useWorkspace.getState().select(node)
    const preview = takePreviewOpen()
    const from = useWorkspace.getState().focused ?? undefined
    if (!isShellPath(path)) showPath(path, { preview, from: from && itemKey(from) !== itemKey(path) ? from : undefined })
    const page = node ? nodePagePath(node) : null
    if (page && itemKey(page) !== itemKey(path)) {
      replaceNext.current = true
      showPath(page, { from: isShellPath(path) ? undefined : path })
    }
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
        const replace = replaceNext.current
        replaceNext.current = false
        navigate(s.focused, { replace })
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
          </main>
        </div>
        {!narrow && <ChatPane />}
      </div>
    </>
  )
}
