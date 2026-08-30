// The board center: two columns, compile and GC. Compile cards group by readiness
// tier, the ready tier first; GC cards carry their cone state. A card click opens
// the follow session when a session holds the goal, otherwise the inspector with
// the explanation. Mirrors docs/frontends/gui.md#board.
import { useMemo } from 'react'
import { useSearchParams } from 'react-router'
import { post } from '../lib/api'
import type { BoardGoal, GoalState } from '../lib/api'
import { useBoard } from '../lib/queries'
import { useApp } from '../lib/store'
import { useInspector } from '../lib/nav'
import { linkifyIds } from '../components/NodeLink'
import PreviewPane from '../components/PreviewPane'
import './routes.css'

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
  const { openNode } = useInspector()
  const jobs = useApp((a) => a.jobs)
  const setChatOpen = useApp((a) => a.setChatOpen)
  const selectChat = useApp((a) => a.selectChat)
  const notes = useApp((a) => a.goalNotes)
  const note = notes[g.id]
  const running = Object.values(jobs).find((j) => j.state === 'running')
  const inSession = !!g.claimedBy || (running && g.batch && stateWord(g.state) === 'open')

  const open = () => {
    // A running session holding the goal opens as the follow session; otherwise
    // the inspector explains the goal.
    if (inSession && running) {
      selectChat(`follow-${running.id}`)
      setChatOpen(true)
      return
    }
    openNode(g.id)
  }

  const preview = (e: React.MouseEvent) => {
    e.stopPropagation()
    const next = new URLSearchParams(sp)
    next.set('preview', g.id)
    setSp(next, { replace: true })
  }

  const word = stateWord(g.state)
  const detail = stateDetail(g.state)
  const change = g.change as Record<string, unknown> | undefined
  const dismissable =
    (g.kind === 'split-view' || g.kind === 'abstract-entity') &&
    typeof change?.limit === 'string' &&
    typeof change?.count === 'number'
  const dismiss = (e: React.MouseEvent) => {
    e.stopPropagation()
    void post(`/api/facts/${encodeURIComponent(g.target)}/edit`, {
      field: `limits.${change!.limit as string}`,
      value: change!.count as number,
    })
  }

  const cls = note?.event === 'resolved' ? 'goal-resolved' : word === 'failed' ? 'goal-failed' : ''
  return (
    <div
      className={`card goal-card ${cls} ${sp.get('goal') === g.id ? 'job-sel' : ''}`}
      onClick={open}
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
        {g.gated && <span className="chip sev-info">gated</span>}
      </div>
      {note?.event === 'resolved' && (
        <p className="v-ok" style={{ margin: '2px 0' }}>✓ {linkifyIds(note.text || 'resolved')}</p>
      )}
      {changeLine(g.change) && <p className="muted" style={{ margin: '2px 0' }}>{changeLine(g.change)}</p>}
      {g.cause && (
        <p className="muted mono" style={{ margin: '2px 0' }}>
          cause: g{g.cause.generation} #{g.cause.mutation}
          {g.cause.via ? ` via ${g.cause.via}` : ''}
        </p>
      )}
      {detail && <p className="v-stale" style={{ margin: '2px 0' }}>{word}: {detail}</p>}
      {g.blockedBy && word === 'open' && (
        <p className="muted" style={{ margin: '2px 0' }}>waiting: {g.blockedBy}</p>
      )}
      {(g.hints ?? []).length > 0 && (
        <p className="muted" style={{ margin: '2px 0' }}>hints: {(g.hints ?? []).join(' · ')}</p>
      )}
      <p className="row" style={{ margin: '4px 0 0' }}>
        <button onClick={preview}>preview</button>
        <button
          onClick={(e) => {
            e.stopPropagation()
            openNode(g.id)
          }}
        >
          explain
        </button>
        {dismissable && (
          <button title={`raise the ${change!.limit as string} threshold on ${g.target} to ${change!.count as number}`} onClick={dismiss}>
            dismiss
          </button>
        )}
        {word === 'blocked' && (g.kind === 'answer' || g.kind === 'ratify') && (
          <QuestionJump />
        )}
      </p>
    </div>
  )
}

function QuestionJump() {
  const setChatOpen = useApp((a) => a.setChatOpen)
  return (
    <button
      onClick={(e) => {
        e.stopPropagation()
        setChatOpen(true)
      }}
      title="the question waits in the chat pane's questions list"
    >
      answer
    </button>
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
          {board.data.note ?? 'the board is empty; the graph reflects the docs'}
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
