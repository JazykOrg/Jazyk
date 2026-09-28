// The live spine: one SSE connection to the connected server drives query
// invalidation and the zustand slices. A dropped connection falls back to polling
// /api/status until it heals.
import { QueryClient } from '@tanstack/react-query'
import { apiUrl, get, type Job, type Status, type TraceEvent } from './api'
import { useApp } from './store'
import { failureLine, noClues, noteClue, parseGoalId, type Clues, type GoalRef } from './failure'

type WireEvent = {
  seq?: number
  type: string
  [k: string]: unknown
}

const str = (v: unknown) => (typeof v === 'string' ? v : '')
const num = (v: unknown) => (typeof v === 'number' ? v : 0)

// The document a goal target names: `doc.md`, or the doc half of `doc.md#/ref`.
function targetDoc(target: string): string | null {
  const doc = target.includes('#') ? target.slice(0, target.indexOf('#')) : target
  return doc.endsWith('.md') ? doc : null
}

// Per session label: the batch's goals and what its session left that explains an
// end, so a failed batch says why in words (docs/frontends/gui.md#chat).
const batchGoals = new Map<string, GoalRef[]>()
const labelClues = new Map<string, Clues>()

// A session's goals: the batch's own, else the ids its terminal event carries.
function sessionGoals(label: string, ev: TraceEvent): GoalRef[] {
  const known = batchGoals.get(label)
  if (known && known.length > 0) return known
  return Array.isArray(ev.goals) ? (ev.goals as string[]).map(parseGoalId) : []
}

// The events that say where a build is working, into the session-progress slice.
// The files tree and the editor render it in place (docs/frontends/gui.md#files).
function applyProgress(ev: TraceEvent) {
  const app = useApp.getState()
  const label = str(ev.label)
  labelClues.set(label, noteClue(labelClues.get(label) ?? noClues(), ev))
  switch (ev.kind) {
    case 'batchStart': {
      // One queued entry per batch, keyed by the batch id the session events use.
      const goals = Array.isArray(ev.goals) ? (ev.goals as { id?: string; kind?: string; target?: string }[]) : []
      batchGoals.set(label, goals)
      const sectionGoals = goals.filter((g) => g.kind === 'reconcile-section')
      const first = goals[0]
      const doc = goals.map((g) => targetDoc(str(g.target))).find((d) => d !== null) ?? null
      app.queueSession({
        label: str(ev.label),
        task: str(first?.kind ?? ev.class),
        target: str(first?.target),
        doc,
        sections: sectionGoals
          .map((g) => str(g.target))
          .filter((t) => t.includes('#'))
          .map((t) => t.slice(t.indexOf('#') + 1)),
      })
      break
    }
    case 'sessionStart':
      app.turnStarted({
        label: str(ev.label),
        task: str(ev.task),
        target: str(ev.target),
        doc: typeof ev.doc === 'string' ? ev.doc : null,
        sections: Array.isArray(ev.sections) ? (ev.sections as string[]) : [],
      })
      break
    case 'section':
      app.turnSection(str(ev.label), str(ev.section))
      break
    case 'sessionDone': {
      const summary = str(ev.summary)
      const n = num(ev.staged)
      const staged = `Read: ${n} change${n === 1 ? '' : 's'} saved`
      app.turnEnded(label, 'done', summary ? `${staged}. ${summary}` : staged)
      break
    }
    case 'sessionFailed':
    case 'sessionEnded': {
      const stop = str(ev.error) || str(ev.reason)
      // A ledger batch returned for its ledger to judge is no failure of its own.
      if (ev.kind === 'sessionEnded' && !/cancel/i.test(stop)) {
        app.turnEnded(label, 'done', stop)
        break
      }
      const clues = labelClues.get(label) ?? noClues()
      const line = failureLine(sessionGoals(label, ev), clues, stop, typeof ev.attempt === 'number' ? ev.attempt : undefined)
      app.turnEnded(label, 'failed', line)
      break
    }
    case 'goal':
      // Resolved cards turn into their justification and linger on the board.
      app.noteGoal(str(ev.goal), str(ev.event), str(ev.justification) || str(ev.reason))
      break
  }
}

// One event fans out to the query keys it invalidates; only mounted queries refetch.
function dispatch(qc: QueryClient, ev: WireEvent) {
  const app = useApp.getState()
  switch (ev.type) {
    case 'store.generation': {
      app.setLastCommit(ev.generation as number)
      // 'out': the generated pages and renderings rewrite on every commit.
      for (const key of ['status', 'graph', 'coverage', 'journal', 'docs', 'pending', 'matrix', 'overview', 'board', 'views', 'tree', 'preview', 'explain', 'out'])
        qc.invalidateQueries({ queryKey: [key] })
      qc.invalidateQueries({ queryKey: ['node'] })
      break
    }
    case 'store.lock':
      qc.invalidateQueries({ queryKey: ['status'] })
      break
    // The board was re-derived; the cards refetch (docs/frontends/gui.md#events).
    case 'board.changed':
      qc.invalidateQueries({ queryKey: ['board'] })
      qc.invalidateQueries({ queryKey: ['status'] })
      break
    case 'goal.opened':
      app.noteGoal(str(ev.goal), 'opened', '')
      qc.invalidateQueries({ queryKey: ['board'] })
      break
    case 'goal.resolved':
      app.noteGoal(str(ev.goal), 'resolved', str(ev.justification))
      qc.invalidateQueries({ queryKey: ['board'] })
      break
    case 'docs.changed':
      qc.invalidateQueries({ queryKey: ['docs'] })
      break
    case 'pending.changed':
      qc.invalidateQueries({ queryKey: ['pending'] })
      qc.invalidateQueries({ queryKey: ['matrix'] })
      break
    case 'control.changed':
      qc.invalidateQueries({ queryKey: ['workers'] })
      break
    case 'watch.state':
      app.setWatchMode(ev.mode as 'off' | 'queue' | 'watch')
      if (ev.gen) app.setGenMode(ev.gen as 'manual' | 'auto')
      break
    case 'job.queued':
      app.upsertJob(ev.job as Job)
      qc.invalidateQueries({ queryKey: ['jobs'] })
      break
    case 'job.started':
      app.setJobState(ev.jobId as number, 'running')
      // The job's build thread opens itself: the column opens if it was collapsed
      // and selects the thread, so the run is watched from its first line
      // (docs/frontends/gui.md#chat).
      app.setChatOpen(true)
      app.selectChat(`follow-${ev.jobId as number}`)
      qc.invalidateQueries({ queryKey: ['jobs'] })
      break
    case 'job.trace': {
      const event = ev.event as TraceEvent
      app.pushTrace({
        jobId: ev.jobId as number,
        // The per-job event number (not the global stream seq): rows merge exactly
        // onto a fetched transcript baseline.
        seq: (ev.n as number) ?? 0,
        event,
      })
      applyProgress(event)
      // A feedback call lands mid-run; the view refreshes on the call, not at the end.
      if (event.kind === 'toolCall' && event.name === 'report_feedback')
        qc.invalidateQueries({ queryKey: ['feedback'] })
      // A commit lands mid-build; the graph views refresh on it, not at the end. The
      // server also says so as store.generation; a tab has only the trace.
      if (event.kind === 'commit' || event.kind === 'sessionDone') {
        for (const key of ['status', 'graph', 'coverage', 'journal', 'docs', 'board', 'views', 'tree', 'out'])
          qc.invalidateQueries({ queryKey: [key] })
        qc.invalidateQueries({ queryKey: ['node'] })
      }
      break
    }
    case 'job.finished':
      app.turnsSettle()
      app.clearGoalNotes()
      app.setJobState(ev.jobId as number, ev.state as string, ev.result as Job['result'])
      for (const key of ['jobs', 'pending', 'matrix', 'status', 'deliverable', 'benchmarks', 'board', 'views', 'tree'])
        qc.invalidateQueries({ queryKey: [key] })
      break
    // The chat pane (docs/frontends/gui.md#chat).
    case 'chat.sessions':
      app.setChatSessions((ev.sessions as never[]) ?? [])
      break
    case 'chat.update':
      app.pushChatUpdate(ev.sessionId as string, {
        n: (ev.n as number) ?? 0,
        update: (ev.update as Record<string, unknown>) ?? {},
      })
      break
    case 'chat.permission': {
      // The sessions snapshot follows on its own event; selecting the asking
      // session puts the buttons in front of the user.
      app.selectChat(ev.sessionId as string)
      app.setChatOpen(true)
      break
    }
    // Settings changed the project itself; anything derived may differ.
    case 'settings.changed':
      qc.invalidateQueries()
      break
    case 'resync':
      qc.invalidateQueries()
      break
  }
}

export function startEventStream(qc: QueryClient) {
  const app = useApp.getState()
  let es: EventSource | null = null
  let pollTimer: number | null = null
  let lastGeneration = -1

  const startPolling = () => {
    if (pollTimer !== null) return
    pollTimer = window.setInterval(async () => {
      try {
        const s = await get<Status>(`/api/status`)
        if (s.generation !== lastGeneration) {
          lastGeneration = s.generation
          dispatch(qc, { type: 'store.generation', generation: s.generation })
        }
      } catch {
        // stay in fallback until the stream reconnects
      }
    }, 5000)
  }
  const stopPolling = () => {
    if (pollTimer !== null) {
      clearInterval(pollTimer)
      pollTimer = null
    }
  }

  const connect = () => {
    // EventSource cannot set headers; the token rides the query string. The URL is
    // the server's own: the page is on another origin
    // (docs/frontends/gui.md#connecting-to-a-local-jazyk).
    es = new EventSource(apiUrl('/api/events'))
    es.onopen = () => {
      useApp.getState().setConnected(true)
      stopPolling()
      // Anything may have happened while disconnected.
      qc.invalidateQueries()
      void seed()
    }
    es.onmessage = (m) => {
      try {
        dispatch(qc, JSON.parse(m.data))
      } catch {
        // a malformed event is dropped, never fatal
      }
    }
    es.onerror = () => {
      useApp.getState().setConnected(false)
      startPolling()
      // EventSource retries on its own; nothing else to do.
    }
  }

  // Seed job state so the activity view knows about jobs from before this page
  // load. A build already running also replays its events through the progress
  // slice, so a reload (or a healed connection) shows where it is, not an empty tree.
  const seed = () =>
    get<{ jobs: Job[] }>(`/api/jobs`)
      .then((r) => {
        r.jobs.forEach((j) => app.upsertJob(j))
        const running = r.jobs.find((j) => j.state === 'running')
        if (!running) return
        return get<{ events?: { event: TraceEvent }[] }>(`/api/jobs/${running.id}`)
          .then((j) => (j.events ?? []).forEach((e) => applyProgress(e.event)))
          .catch(() => {})
      })
      .catch(() => {})
  void seed()
  // The chat pane's session list survives a reload the same way jobs do.
  get<{ sessions: never[] }>(`/api/chat/sessions`)
    .then((r) => app.setChatSessions(r.sessions ?? []))
    .catch(() => {})
  get<{ mode: 'off' | 'queue' | 'watch'; gen?: 'manual' | 'auto' }>(`/api/watch`)
    .then((r) => {
      app.setWatchMode(r.mode)
      if (r.gen) app.setGenMode(r.gen)
    })
    .catch(() => {})

  // A new token means the EventSource URL is stale: drop it and dial fresh.
  const unsub = useApp.subscribe((s, prev) => {
    if (s.tokenEpoch !== prev.tokenEpoch) {
      es?.close()
      connect()
    }
  })

  // Finished sessions linger, then fade; one ticker retires them.
  const sweep = window.setInterval(() => useApp.getState().turnsSweep(), 1000)

  connect()
  return () => {
    unsub()
    es?.close()
    stopPolling()
    clearInterval(sweep)
  }
}

// The wasm backend's events (docs/frontends/wasm.md#chat, and jobs in a tab under
// #the-demo): the worker and the page post them in the event stream's shape, so
// they take the stream's own path and the workbench renders a browser build exactly
// as it renders a server one.
export function pushWireEvent(qc: QueryClient, event: Record<string, unknown>) {
  if (typeof event.type !== 'string') return
  dispatch(qc, event as WireEvent)
}

// Everything a commit can change, refetched. The server announces a commit over
// the event stream; the wasm backend has no stream, so the demo calls this after
// any write request returns (docs/frontends/wasm.md#the-trace).
export function refreshAfterWrite(qc: QueryClient) {
  for (const key of ['status', 'graph', 'coverage', 'journal', 'docs', 'pending', 'matrix', 'overview', 'board', 'views', 'tree', 'preview', 'explain', 'out', 'workers', 'inbox', 'questions'])
    qc.invalidateQueries({ queryKey: [key] })
  qc.invalidateQueries({ queryKey: ['node'] })
}
