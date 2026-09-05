// The board center: two columns, compile and GC. Compile cards group by readiness
// tier, the ready tier first; GC cards carry their cone state. A card click opens
// the follow session when a session holds the goal, otherwise the inspector with
// the explanation. Mirrors docs/frontends/gui.md#board.
import { useMemo, useState } from 'react'
import { Link, useSearchParams } from 'react-router'
import { useQueryClient } from '@tanstack/react-query'
import { post } from '../lib/api'
import type { BoardGoal, GoalState } from '../lib/api'
import { useBoard } from '../lib/queries'
import { useApp } from '../lib/store'
import { selectNodeParams, useInspector } from '../lib/nav'
import { pressable } from '../lib/a11y'
import { linkifyIds } from '../components/NodeLink'
import PreviewPane from '../components/PreviewPane'
import './routes.css'

// The diagnostic a blocked ratify or answer card waits on, named in its hints
// (`proposal diag:...`, `prompt diag:...`), so the card can jump to it.
function blockingDiag(g: BoardGoal): string | null {
  for (const h of g.hints ?? []) {
    const m = h.match(/\b(diag:[a-z0-9-]+)/)
    if (m) return m[1]
  }
  return null
}

export function stateWord(s: GoalState): string {
  if (typeof s === 'string') return s
  if ('blocked' in s) return 'blocked'
  return 'failed'
}

function stateDetail(s: GoalState): string | null {
  if (typeof s === 'string') return null
  if ('blocked' in s) return s.blocked.on
  return s.failed.reason
}

function changeLine(change: unknown): string {
  if (change == null) return ''
  if (typeof change === 'string') return change
  const c = change as Record<string, unknown>
  const kind = typeof c.kind === 'string' ? c.kind : ''
  const bits = Object.entries(c)
    .filter(([k, v]) => k !== 'kind' && (typeof v === 'string' || typeof v === 'number'))
    .slice(0, 3)
    .map(([k, v]) => `${k}: ${v}`)
  return [kind, bits.join(', ')].filter(Boolean).join(' · ')
}

function GoalCard({ g }: { g: BoardGoal }) {
  const [sp, setSp] = useSearchParams()
  const qc = useQueryClient()
  const { openNode } = useInspector()
  const jobs = useApp((a) => a.jobs)
  const setChatOpen = useApp((a) => a.setChatOpen)
  const selectChat = useApp((a) => a.selectChat)
  const notes = useApp((a) => a.goalNotes)
  const note = notes[g.id]
  const running = Object.values(jobs).find((j) => j.state === 'running')
  const inSession = !!g.claimedBy || (running && g.batch && stateWord(g.state) === 'open')
  const [busy, setBusy] = useState(false)
  const [err, setErr] = useState<string | null>(null)

  const open = () => {
    // A running session holding the goal opens as the follow session; otherwise
    // the inspector explains the goal. `?goal=` holds the selected card either way.
    const next = new URLSearchParams(sp)
    next.set('goal', g.id)
    if (inSession && running) {
      setSp(next, { replace: true })
      selectChat(`follow-${running.id}`)
      setChatOpen(true)
      return
    }
    selectNodeParams(next, g.id)
    setSp(next)
  }

  const preview = (e: React.MouseEvent) => {
    e.stopPropagation()
    const next = new URLSearchParams(sp)
    next.set('preview', g.id)
    setSp(next, { replace: true })
  }

  // The inspector on the goal, with its ripple pane opened at once.
  const ripple = (e: React.MouseEvent) => {
    e.stopPropagation()
    const next = new URLSearchParams(sp)
    selectNodeParams(next, g.id)
    next.set('ripple', '1')
    setSp(next)
  }

  const word = stateWord(g.state)
  const detail = stateDetail(g.state)
  const change = g.change as Record<string, unknown> | undefined
  const dismissable =
    (g.kind === 'split-view' || g.kind === 'abstract-entity') &&
    typeof change?.limit === 'string' &&
    typeof change?.count === 'number'
  // A decree that raises the node's own threshold: the goal stops deriving
  // until the raised threshold is crossed (docs/frontends/gui.md#board).
  const dismiss = async (e: React.MouseEvent) => {
    e.stopPropagation()
    setBusy(true)
    setErr(null)
    try {
      await post(`/api/facts/${encodeURIComponent(g.target)}/edit`, {
        field: `limits.${change!.limit as string}`,
        value: change!.count as number,
      })
      for (const key of ['board', 'status', 'graph', 'views', 'journal']) void qc.invalidateQueries({ queryKey: [key] })
    } catch (x) {
      setErr((x as Error).message)
    } finally {
      setBusy(false)
    }
  }

  // A blocked answer or ratify card jumps to where the human acts: the questions
  // list in the chat pane, and the diagnostic itself in the inspector.
  const blocking = word === 'blocked' && (g.kind === 'answer' || g.kind === 'ratify') ? blockingDiag(g) : null
  const jump = (e: React.MouseEvent) => {
    e.stopPropagation()
    if (blocking) openNode(blocking)
    setChatOpen(true)
  }

  const cls =
    note?.event === 'resolved'
      ? 'goal-resolved'
      : word === 'failed'
        ? 'goal-failed'
        : inSession && running
          ? 'goal-session'
          : ''
  return (
    <div
      className={`card goal-card ${cls} ${sp.get('goal') === g.id ? 'job-sel' : ''}`}
      title={inSession && running ? 'open the session holding this goal' : 'open the goal in the inspector'}
      {...pressable(open)}
    >
      <div className="row">
        <b>{g.kind}</b>
        <span className="mono">{g.target}</span>
        {g.unit && <span className="chip sev-none">{g.unit}</span>}
        <span className={`chip ${g.mandatory ? 'sev-warning' : 'sev-none'}`}>
          {g.mandatory ? 'mandatory' : 'optional'}
        </span>
        {inSession && <span className="chip v-stale">in session{g.batch ? ` ${g.batch}` : ''}</span>}
        {!inSession && word === 'open' && g.ready && <span className="chip v-ok">ready</span>}
        {!inSession && word === 'open' && !g.ready && <span className="chip sev-none">waiting</span>}
        {word !== 'open' && <span className={`chip ${word === 'failed' ? 'v-bad' : 'v-stale'}`}>{word}</span>}
        {g.gated && <span className="chip sev-info" title="awaiting a release">gated</span>}
      </div>
      {note?.event === 'resolved' && (
        <p className="v-ok" style={{ margin: '2px 0' }}>✓ {linkifyIds(note.text || 'resolved')}</p>
      )}
      {changeLine(g.change) && <p className="muted" style={{ margin: '2px 0' }}>{changeLine(g.change)}</p>}
      {g.cause && (
        <p className="muted mono" style={{ margin: '2px 0' }}>
          cause:{' '}
          <Link to={`/journal/${g.cause.generation}`} title="the journal entry that opened this goal" onClick={(e) => e.stopPropagation()}>
            g{g.cause.generation}
          </Link>{' '}
          #{g.cause.mutation}
          {g.cause.via ? ` via ${g.cause.via}` : ''}
        </p>
      )}
      {detail && <p className="v-stale" style={{ margin: '2px 0' }}>{word}: {detail}</p>}
      {g.blockedBy && word !== 'failed' && (
        <p className="muted" style={{ margin: '2px 0' }}>{word === 'open' ? 'waiting' : 'blocked'}: {g.blockedBy}</p>
      )}
      {word === 'parked' && (
        <p className="muted" style={{ margin: '2px 0' }}>out of budget; the next build resumes it first</p>
      )}
      {(g.hints ?? []).length > 0 && (
        <p className="muted" style={{ margin: '2px 0' }}>hints: {(g.hints ?? []).join(' · ')}</p>
      )}
      {err && <p className="goal-err">{err}</p>}
      <p className="row" style={{ margin: '4px 0 0' }}>
        <button onClick={preview} title="the prompt of the batch this goal would join">preview</button>
        <button
          onClick={(e) => {
            e.stopPropagation()
            openNode(g.id)
          }}
          title="the change record, the readiness sentence, what blocks it"
        >
          explain
        </button>
        <button onClick={ripple} title="the causal chain around this goal's target">ripple</button>
        {dismissable && (
          <button
            disabled={busy}
            title={`raise the ${change!.limit as string} threshold on ${g.target} to ${change!.count as number}: a decree, the goal stops deriving`}
            onClick={(e) => void dismiss(e)}
          >
            dismiss
          </button>
        )}
        {word === 'blocked' && (g.kind === 'answer' || g.kind === 'ratify') && (
          <button
            onClick={jump}
            title={g.kind === 'ratify' ? 'the ratification proposal waits in the questions list' : 'the question waits in the questions list'}
          >
            {g.kind === 'ratify' ? 'proposal' : 'answer'}
          </button>
        )}
      </p>
    </div>
  )
}

export default function Board() {
  const board = useBoard()
  const [sp, setSp] = useSearchParams()
  const previewTarget = sp.get('preview')

  const filters = {
    class: sp.get('class'),
    kind: sp.get('kind'),
    state: sp.get('state'),
    doc: sp.get('doc'),
  }

  const goals = useMemo(() => {
    const all = board.data?.goals ?? []
    return all.filter((g) => {
      if (filters.class && g.class !== filters.class) return false
      if (filters.kind && g.kind !== filters.kind) return false
      if (filters.state && stateWord(g.state) !== filters.state) return false
      if (filters.doc && !g.target.startsWith(filters.doc)) return false
      return true
    })
  }, [board.data, filters.class, filters.kind, filters.state, filters.doc])

  if (board.error)
    return (
      <p className="error-inline">
        {board.error.message}{' '}
        <a href="#retry" onClick={(e) => { e.preventDefault(); board.refetch() }}>retry</a>
      </p>
    )
  if (!board.data) return <p className="muted">loading…</p>

  const compile = goals.filter((g) => g.class === 'compile')
  const gc = goals.filter((g) => g.class === 'gc')
  // Compile cards group by readiness tier, the ready tier first.
  const tiers = new Map<number, BoardGoal[]>()
  for (const g of compile) {
    const t = g.tier ?? 9
    const list = tiers.get(t)
    if (list) list.push(g)
    else tiers.set(t, [g])
  }
  const tierOrder = [...tiers.keys()].sort((a, b) => {
    const ra = (tiers.get(a) ?? []).some((g) => g.ready)
    const rb = (tiers.get(b) ?? []).some((g) => g.ready)
    if (ra !== rb) return ra ? -1 : 1
    return a - b
  })
  const gcBurst = gc.find((g) => g.batch)

  const closePreview = () => {
    const next = new URLSearchParams(sp)
    next.delete('preview')
    setSp(next, { replace: true })
  }

  return (
    <div>
      <div className="row">
        <h1 style={{ margin: 0 }}>Board</h1>
        <span className="muted mono">{board.data.summary}</span>
        <span className="muted">verdict: {board.data.verdict}</span>
      </div>
      {previewTarget !== null && (
        <PreviewPane target={previewTarget === 'next' ? '' : previewTarget} onClose={closePreview} />
      )}
      {goals.length === 0 && (
        <p className="empty">
          {board.data.goals.length > 0
            ? 'no goals match the filters in the sidebar'
            : (board.data.note ?? 'the board is empty; the graph reflects the docs')}
        </p>
      )}
      <div className="grid2">
        <div>
          <h2>compile</h2>
          {compile.length === 0 && <p className="muted">no compile goals</p>}
          {tierOrder.map((t) => (
            <div key={t}>
              <p className="muted mono" style={{ margin: '8px 0 2px' }}>tier {t}</p>
              {(tiers.get(t) ?? []).map((g) => (
                <GoalCard key={g.id} g={g} />
              ))}
            </div>
          ))}
        </div>
        <div>
          <h2>
            garbage collection
            {gcBurst && <span className="chip v-stale" style={{ marginLeft: 8 }}>burst: {gcBurst.batch}</span>}
          </h2>
          {gc.length === 0 && <p className="muted">no GC goals</p>}
          {gc.map((g) => (
            <GoalCard key={g.id} g={g} />
          ))}
        </div>
      </div>
      <p className="muted" style={{ marginTop: 12 }}>
        decompilation stays outside the board: drafts run from the work views
      </p>
    </div>
  )
}
