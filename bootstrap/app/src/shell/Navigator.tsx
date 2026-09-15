// The navigator: the left column, one per lens (docs/frontends/gui.md#lenses).
import { NavLink, useLocation } from 'react-router'
import Explorer from '../workbench/Explorer'
import Outline from './Outline'
import GraphSidebar from '../workbench/GraphSidebar'
import BoardSidebar from '../workbench/BoardSidebar'
import { useInbox } from '../lib/product'
import { useDeliverable, useGenPending, useJournal, useMatrix, useWorkers } from '../lib/queries'
import { useWords } from '../lib/words'
import { verifyClass } from '../components/Chip'

function Row({ to, label, sub, end }: { to: string; label: string; sub?: React.ReactNode; end?: boolean }) {
  return (
    <NavLink to={to} end={end} className={({ isActive }) => (isActive ? 'active' : '')}>
      {label}
      {sub !== undefined && <span className="sub">{sub}</span>}
    </NavLink>
  )
}

function DocsNav() {
  return (
    <>
      <Explorer sections={['docs']} />
      <Outline />
    </>
  )
}

function ModelNav() {
  return <GraphSidebar />
}

function ProductNav() {
  const loc = useLocation()
  const w = useWords()
  const deliv = useDeliverable()
  const matrix = useMatrix()
  const workers = useWorkers()
  const pending = useGenPending()
  const counts = matrix.data?.counts ?? {}
  const files = deliv.data?.files.length ?? 0
  return (
    <>
      <div className="ws-nav-head">{w.product}</div>
      <div className="ws-nav-list">
        <Row to="/product" end label="Overview" />
        <Row to="/product/files" label="Files" sub={deliv.data ? files : '…'} />
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
        <Row to="/product/not-described" label={w.notDescribed} sub={workers.data ? (workers.data.unclaimed ?? 0) : '…'} />
        <Row to="/product/generated" label="Generated docs" />
        <Row to="/product/form" label={w.productForm} />
        <Row to="/product/build" label="To build" sub={pending.data ? pending.data.pending.length : '…'} />
      </div>
      {(loc.pathname.startsWith('/product/files') || loc.pathname.startsWith('/product/generated')) && (
        <Explorer sections={loc.pathname.startsWith('/product/files') ? ['deliverable'] : ['generated']} />
      )}
    </>
  )
}

function InboxNav() {
  const w = useWords()
  const inbox = useInbox()
  const d = inbox.data
  const problems = Object.values(d?.problems ?? {}).reduce((a, b) => a + b, 0)
  return (
    <>
      <div className="ws-nav-head">Inbox</div>
      <div className="ws-nav-list">
        <Row to="/inbox" end label="Everything" sub={d ? d.total : '…'} />
        <Row to="/inbox/questions" label={w.questions} sub={d ? d.questions : '…'} />
        <Row to="/inbox/proposals" label={w.proposals} sub={d ? d.proposals : '…'} />
        <Row to="/inbox/problems" label={w.problems} sub={d ? problems : '…'} />
        <Row to="/inbox/pending" label={w.pendingChanges} sub={d ? d.pending.docs.length + (d.pending.entities > 0 ? 1 : 0) : '…'} />
        <Row to="/inbox/drafts" label={w.drafts} sub={d ? d.drafts : '…'} />
      </div>
    </>
  )
}

function HistoryNav() {
  const w = useWords()
  const journal = useJournal(60)
  const entries = journal.data?.entries ?? []
  const human = (k?: string) => k === 'edit' || k === 'dual-write' || k === 'decree' || k === 'ratify' || k === 'answer' || k === 'triage'
  return (
    <>
      <div className="ws-nav-head">{w.history}</div>
      <div className="ws-nav-list">
        <Row to="/history" end label="All syncs" />
        {entries.map((e) => (
          <NavLink key={e.generation} to={`/history/${e.generation}`} className={({ isActive }) => `mono${isActive ? ' active' : ''}`}>
            <span className={human(e.kind) ? 'v-ok' : ''}>{w.syncNumber(e.generation)}</span>
            <span className="sub">{e.kind ?? 'session'}</span>
          </NavLink>
        ))}
      </div>
    </>
  )
}

function DevNav() {
  const loc = useLocation()
  if (loc.pathname.startsWith('/dev/board')) return <BoardSidebar />
  return (
    <>
      <div className="ws-nav-head">Developer</div>
      <div className="ws-nav-list">
        <Row to="/dev/board" label="Board" />
        <Row to="/dev/runs" label="Runs" />
        <Row to="/dev/prompt" label="Prompt" />
        <Row to="/dev/journal" label="Journal" />
        <Row to="/dev/workers" label="Workers" />
        <Row to="/dev/map" label="Map" />
        <Row to="/dev/benchmarks" label="Benchmarks" />
        <Row to="/dev/feedback" label="Feedback" />
      </div>
    </>
  )
}

function SettingsNav() {
  return (
    <>
      <div className="ws-nav-head">Settings</div>
      <div className="ws-nav-list">
        <Row to="/settings" end label="Project" />
      </div>
      <p className="muted" style={{ padding: '8px 12px', fontSize: 12 }}>
        Edited as a form; saving rewrites <span className="mono">jazyk.toml</span> and applies live.
      </p>
    </>
  )
}

export default function Navigator() {
  const loc = useLocation()
  const p = loc.pathname
  let body: React.ReactNode
  if (p.startsWith('/docs')) body = <DocsNav />
  else if (p.startsWith('/model')) body = <ModelNav />
  else if (p.startsWith('/product')) body = <ProductNav />
  else if (p.startsWith('/inbox')) body = <InboxNav />
  else if (p.startsWith('/history')) body = <HistoryNav />
  else if (p.startsWith('/dev')) body = <DevNav />
  else if (p.startsWith('/settings')) body = <SettingsNav />
  else body = null
  if (body === null) return null
  return <aside className="ws-nav">{body}</aside>
}
