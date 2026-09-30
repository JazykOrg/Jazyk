// The top bar: the project menu, the three lenses, Inbox with its count, History,
// the command palette, the action button, settings (docs/frontends/gui.md#layout).
// Until a project is open, the project menu alone. In the workspace
// (docs/frontends/gui.md#workspace-experimental) there are no lenses: Inbox, History,
// and the arrangement switch stay.
import { NavLink, useLocation } from 'react-router'
import { useProject } from '../lib/queries'
import { useInbox } from '../lib/product'
import { useApp } from '../lib/store'
import { useWords } from '../lib/words'
import ActionButton from './ActionButton'
import ProjectMenu from './ProjectMenu'
import ArrangementSwitch from '../workspace/ArrangementSwitch'

function projectName(root: string | undefined): string {
  if (!root) return 'jazyk'
  return root.replace(/\/+$/, '').split('/').pop() || root
}

export default function TopBar({
  openPalette,
  open,
  toggleNav,
  workspace,
}: {
  openPalette: () => void
  open: boolean
  toggleNav: () => void
  workspace?: boolean
}) {
  const { data: proj } = useProject()
  // In the browser the project's own name: a folder's, a demo's, a browser project's.
  const placeName = useApp((a) => a.folder?.name ?? a.stored?.name)
  const inbox = useInbox()
  const dev = useApp((a) => a.dev)
  const w = useWords()
  const loc = useLocation()
  const total = inbox.data?.total ?? 0
  const errors = inbox.data?.problems?.error ?? 0
  const lens = (p: string) => loc.pathname === p || loc.pathname.startsWith(p + '/')

  const project = <ProjectMenu open={open} name={placeName ?? projectName(proj?.root)} root={proj?.root} />
  if (!open) return <header className="ws-top">{project}</header>

  return (
    <header className="ws-top">
      <button className="ws-navtoggle" onClick={toggleNav} title="the navigator" aria-label="the navigator">
        ☰
      </button>
      {project}
      <nav className={workspace ? 'ws-lenses ws-lenses-bare' : 'ws-lenses'}>
        {!workspace && (
          <>
            <NavLink to="/docs" className={lens('/docs') ? 'active' : ''}>
              Docs
            </NavLink>
            <NavLink to="/model" className={lens('/model') ? 'active' : ''}>
              Model
            </NavLink>
            <NavLink to="/product" className={lens('/product') ? 'active' : ''}>
              {w.product}
            </NavLink>
          </>
        )}
        <NavLink to="/inbox" className={lens('/inbox') ? 'active' : ''} title="what jazyk needs from you">
          Inbox
          <span className={`ws-count${total === 0 ? ' zero' : errors > 0 ? ' err' : ''}`}>{total}</span>
        </NavLink>
        <NavLink to="/history" className={lens('/history') ? 'active' : ''}>
          {w.history}
        </NavLink>
        {dev && (
          <NavLink to="/dev" className={`dev${lens('/dev') ? ' active' : ''}`} title="the compiler workbench">
            Developer
          </NavLink>
        )}
      </nav>
      <span className="ws-spacer" />
      {workspace && <ArrangementSwitch />}
      <button className="ws-palette" onClick={openPalette} title="search the model (⌘K)">
        search… <span className="muted">⌘K</span>
      </button>
      <ActionButton />
      <NavLink to="/settings" className={`ws-link${lens('/settings') ? ' active' : ''}`} title="project settings">
        ⚙
      </NavLink>
    </header>
  )
}
