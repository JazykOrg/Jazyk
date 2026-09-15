// The top bar: the project switcher, the three lenses, Inbox with its count,
// History, the command palette, the action button, settings
// (docs/frontends/gui.md#layout).
import { Link, NavLink, useLocation } from 'react-router'
import { useProject } from '../lib/queries'
import { useInbox } from '../lib/product'
import { useApp } from '../lib/store'
import { useWords } from '../lib/words'
import ActionButton from './ActionSheet'

function projectName(root: string | undefined): string {
  if (!root) return 'jazyk'
  return root.replace(/\/+$/, '').split('/').pop() || root
}

export default function TopBar({ openPalette }: { openPalette: () => void }) {
  const { data: proj } = useProject()
  const inbox = useInbox()
  const dev = useApp((a) => a.dev)
  const w = useWords()
  const loc = useLocation()
  const total = inbox.data?.total ?? 0
  const errors = inbox.data?.problems?.error ?? 0
  const lens = (p: string) => loc.pathname === p || loc.pathname.startsWith(p + '/')

  return (
    <header className="ws-top">
      <Link to="/" className="ws-project" title={proj?.root ?? 'choose a project'}>
        <span className="ws-mark">J</span>
        <span>{projectName(proj?.root)}</span>
      </Link>
      <nav className="ws-lenses">
        <NavLink to="/docs" className={lens('/docs') ? 'active' : ''}>
          Docs
        </NavLink>
        <NavLink to="/model" className={lens('/model') ? 'active' : ''}>
          Model
        </NavLink>
        <NavLink to="/product" className={lens('/product') ? 'active' : ''}>
          {w.product}
        </NavLink>
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
