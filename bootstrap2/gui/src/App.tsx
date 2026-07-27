import { NavLink, Navigate, Route, Routes } from 'react-router'
import StatusBar from './components/StatusBar'
import { useApp } from './lib/store'
import Home from './routes/Home'
import Ide from './routes/Ide'
import Build from './routes/Build'
import Ir from './routes/Ir'
import NodePage from './routes/NodePage'
import MapView from './routes/MapView'
import Journal from './routes/Journal'
import Changeset from './routes/Changeset'
import ReleaseDiff from './routes/ReleaseDiff'
import Work from './routes/Work'
import GenTask from './routes/GenTask'
import VerifyMatrix from './routes/VerifyMatrix'
import Settings from './routes/Settings'
import Deliverable from './routes/Deliverable'
import CommandPalette from './components/CommandPalette'

const NAV = [
  ['/', 'Home'],
  ['/docs', 'Docs'],
  ['/build', 'Build'],
  ['/ir/entities', 'IR'],
  ['/map', 'Map'],
  ['/journal', 'Journal'],
  ['/work', 'Work'],
  ['/deliverable', 'Deliverable'],
  ['/settings', 'Settings'],
] as const

export default function App() {
  const theme = useApp((a) => a.theme)
  const setTheme = useApp((a) => a.setTheme)
  return (
    <>
      <div className="shell">
        <nav className="nav">
          {NAV.map(([to, label]) => (
            <NavLink
              key={to}
              to={to}
              end={to === '/'}
              className={({ isActive }) =>
                isActive || (to === '/ir/entities' && location.pathname.startsWith('/ir'))
                  ? 'active'
                  : ''
              }
            >
              {label}
            </NavLink>
          ))}
          <div className="nav-bottom">
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
        <Routes>
          <Route path="/" element={<Page><Home /></Page>} />
          <Route path="/docs/*" element={<div className="content full"><Ide /></div>} />
          <Route path="/build" element={<Page><Build /></Page>} />
          <Route path="/ir/:tab" element={<Page><Ir /></Page>} />
          <Route path="/ir" element={<Navigate to="/ir/entities" replace />} />
          <Route path="/n/:id" element={<Page><NodePage /></Page>} />
          <Route path="/map" element={<div className="content full"><MapView /></div>} />
          <Route path="/journal" element={<Page><Journal /></Page>} />
          <Route path="/journal/diff" element={<Page><ReleaseDiff /></Page>} />
          <Route path="/journal/:gen" element={<Page><Changeset /></Page>} />
          <Route path="/work" element={<Page><Work /></Page>} />
          <Route path="/work/gen/:id" element={<Page><GenTask /></Page>} />
          <Route path="/work/verify" element={<Page><VerifyMatrix /></Page>} />
          <Route path="/settings" element={<Page><Settings /></Page>} />
          <Route path="/deliverable/*" element={<div className="content full"><Deliverable /></div>} />
          <Route path="*" element={<Page><p className="empty">no such page</p></Page>} />
        </Routes>
      </div>
      <StatusBar />
      <CommandPalette />
      <CommitNote />
    </>
  )
}

function Page({ children }: { children: React.ReactNode }) {
  return <main className="content">{children}</main>
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
