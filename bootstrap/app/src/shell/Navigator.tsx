// The sections of an area (the Inbox, History, the Developer pages, the model,
// Settings), which the workspace navigator leads with while the focused item belongs
// to it (docs/frontends/gui.md#workspace).
import { NavLink, useLocation } from 'react-router'
import GraphSidebar from '../workbench/GraphSidebar'
import BoardSidebar from '../workbench/BoardSidebar'
import { useInbox } from '../lib/product'
import { useJournal } from '../lib/queries'
import { useWords } from '../lib/words'

function Row({ to, label, sub, end }: { to: string; label: string; sub?: React.ReactNode; end?: boolean }) {
  return (
    <NavLink to={to} end={end} className={({ isActive }) => (isActive ? 'active' : '')}>
      {label}
      {sub !== undefined && <span className="sub">{sub}</span>}
    </NavLink>
  )
}

function ModelNav() {
  return <GraphSidebar />
}

function InboxNav() {
  const w = useWords()
  const inbox = useInbox()
  const d = inbox.data
  const problems = Object.values(d?.problems ?? {}).reduce((a, b) => a + b, 0)
  // A rollup that failed says so where its counts would be, never a count of zero.
  const missing = inbox.error ? (
    <span className="sev-error" title={`the Inbox could not load: ${(inbox.error as Error).message}`}>
      !
    </span>
  ) : (
    '…'
  )
  return (
    <>
      <div className="ws-nav-head">Inbox</div>
      <div className="ws-nav-list">
        <Row to="/inbox" end label="Everything" sub={d ? d.total : missing} />
        <Row to="/inbox/questions" label={w.questions} sub={d ? d.questions : missing} />
        <Row to="/inbox/proposals" label={w.proposals} sub={d ? d.proposals : missing} />
        <Row to="/inbox/problems" label={w.problems} sub={d ? problems : missing} />
        <Row to="/inbox/pending" label={w.pendingChanges} sub={d ? d.pending.docs.length + (d.pending.entities > 0 ? 1 : 0) : missing} />
        <Row to="/inbox/drafts" label={w.drafts} sub={d ? d.drafts : missing} />
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
        <Row to="/history" end label="All builds" />
        {entries.map((e) => (
          <NavLink key={e.generation} to={`/history/${e.generation}`} className={({ isActive }) => `mono${isActive ? ' active' : ''}`}>
            <span className={human(e.kind) ? 'v-ok' : ''}>{w.buildNumber(e.generation)}</span>
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

// The sections for a path's area, or null when the area has none of its own.
export function AreaSections({ path }: { path: string }) {
  if (path.startsWith('/model')) return <ModelNav />
  if (path.startsWith('/inbox')) return <InboxNav />
  if (path.startsWith('/history')) return <HistoryNav />
  if (path.startsWith('/dev')) return <DevNav />
  if (path.startsWith('/settings')) return <SettingsNav />
  return null
}

export function hasAreaSections(path: string): boolean {
  return ['/model', '/inbox', '/history', '/dev', '/settings'].some((p) => path === p || path.startsWith(p + '/'))
}
