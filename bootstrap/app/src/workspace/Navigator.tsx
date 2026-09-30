// The workspace's one navigator (docs/frontends/gui.md#workspace-experimental):
// Documents, the open document's Outline, Product (Overview, the file tree, Trace,
// Tests), and Diagrams and Generated collapsed. There is no Model head. A single
// click opens a preview item, a double click keeps it.
import { useState } from 'react'
import { NavLink } from 'react-router'
import Explorer from '../workbench/Explorer'
import Outline from '../shell/Outline'
import { useDeliverable, useMatrix, useViews } from '../lib/queries'
import { useWords } from '../lib/words'
import { verifyClass } from '../components/Chip'
import { pinPath } from './actions'
import { hrefPath } from './paths'

// The next navigation is a navigator single click: it opens a preview item. The flag
// lapses on its own, so a click that navigates nowhere does not mark a later one.
let previewUntil = 0
export function takePreviewOpen(): boolean {
  const on = Date.now() < previewUntil
  previewUntil = 0
  return on
}

function anchorPath(target: EventTarget | null): string | null {
  const a = (target as HTMLElement | null)?.closest?.('a')
  return a ? hrefPath(a.getAttribute('href')) : null
}

function Row({ to, label, sub, end }: { to: string; label: string; sub?: React.ReactNode; end?: boolean }) {
  return (
    <NavLink to={to} end={end} className={({ isActive }) => (isActive ? 'active' : '')}>
      {label}
      {sub !== undefined && <span className="sub">{sub}</span>}
    </NavLink>
  )
}

// A head that folds its section, remembered per browser.
function Fold({ id, title, count, children, closed: closedByDefault }: { id: string; title: string; count?: React.ReactNode; children: React.ReactNode; closed?: boolean }) {
  const key = `jazyk.workspace.nav:${id}`
  const [open, setOpen] = useState(() => {
    try {
      const v = localStorage.getItem(key)
      return v === null ? !closedByDefault : v === 'open'
    } catch {
      return !closedByDefault
    }
  })
  const toggle = () => {
    setOpen((o) => {
      try {
        localStorage.setItem(key, o ? 'closed' : 'open')
      } catch {
        // not remembered
      }
      return !o
    })
  }
  return (
    <div className={`wsn-fold${open ? ' open' : ''}`} data-fold={id}>
      <button className="wsn-head" aria-expanded={open} onClick={toggle}>
        <span className="wsn-chev" aria-hidden="true">
          {open ? '▾' : '▸'}
        </span>
        {title}
        {count !== undefined && <span className="wsn-count">{count}</span>}
      </button>
      {open && children}
    </div>
  )
}

function Product() {
  const w = useWords()
  const matrix = useMatrix()
  const deliv = useDeliverable()
  const counts = matrix.data?.counts ?? {}
  return (
    <Fold id="product" title={w.product}>
      <div className="ws-nav-list">
        <Row to="/product" end label="Overview" />
      </div>
      <div className="wsn-files">
        <Explorer sections={['deliverable']} />
      </div>
      <div className="ws-nav-list">
        <Row
          to="/product/trace"
          label={w.trace}
          sub={
            <>
              {Object.entries(counts)
                .slice(0, 3)
                .map(([st, n]) => (
                  <span key={st} className={verifyClass(st)} style={{ marginLeft: 6 }}>
                    {n} {st}
                  </span>
                ))}
            </>
          }
        />
        <Row to="/product/tests" label="Tests" />
      </div>
      {deliv.isError && <p className="muted ide-pad">could not load the product</p>}
    </Fold>
  )
}

function Diagrams() {
  const w = useWords()
  const views = useViews()
  const list = views.data?.views ?? []
  return (
    <Fold id="diagrams" title={w.diagrams} count={views.data ? list.length : undefined} closed>
      <div className="ws-nav-list">
        {views.data && list.length === 0 && <p className="muted ide-pad">no diagrams yet: a build draws them</p>}
        {list.map((v) => (
          <Row key={v.id} to={`/model/diagrams/${encodeURIComponent(v.id)}`} label={v.title} />
        ))}
      </div>
    </Fold>
  )
}

export default function WorkspaceNavigator() {
  return (
    <aside
      className="ws-nav wsn"
      aria-label="Navigator"
      // A single click on a row opens a preview; a double click keeps it.
      onClickCapture={(e) => {
        if (e.detail <= 1 && anchorPath(e.target)) previewUntil = Date.now() + 600
      }}
      onDoubleClickCapture={(e) => {
        const p = anchorPath(e.target)
        if (p) pinPath(p)
      }}
    >
      <Fold id="documents" title="Documents">
        <Explorer sections={['docs']} />
      </Fold>
      <Outline />
      <Product />
      <Diagrams />
      <Fold id="generated" title="Generated" closed>
        <Explorer sections={['generated']} />
      </Fold>
    </aside>
  )
}
