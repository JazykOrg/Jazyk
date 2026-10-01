// The top bar: the project menu, Product, the Developer menu while the toggle is on,
// the command palette, the action button, and settings
// (docs/frontends/gui.md#layout). Until a project is open, the project menu alone.
// The Inbox and History open from the project tree and the status bar.
import { NavLink, useLocation } from 'react-router'
import { useProject } from '../lib/queries'
import { useApp } from '../lib/store'
import { useWords } from '../lib/words'
import ActionButton from './ActionButton'
import ProjectMenu from './ProjectMenu'

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
  const dev = useApp((a) => a.dev)
  const w = useWords()
  const loc = useLocation()
  const lens = (p: string) => loc.pathname === p || loc.pathname.startsWith(p + '/')

  const project = <ProjectMenu open={open} name={placeName ?? projectName(proj?.root)} root={proj?.root} />
  if (!open) return <header className="ws-top">{project}</header>

  return (
    <header className="ws-top">
      <button className="ws-navtoggle" onClick={toggleNav} title="the navigator" aria-label="the navigator">
        ☰
      </button>
      {project}
      <nav className="ws-lenses ws-lenses-bare">
        <NavLink to="/product" end className={loc.pathname === '/product' ? 'active' : ''} title="the product overview">
          {w.product}
        </NavLink>
        {dev && (
          <NavLink to="/dev" className={`dev${lens('/dev') ? ' active' : ''}`} title="the compiler workbench">
            Developer
          </NavLink>
        )}
      </nav>
      <span className="ws-spacer" />
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
