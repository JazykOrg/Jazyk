// The live spine: one SSE connection drives query invalidation and the zustand
// slices. A dropped connection falls back to polling /api/status until it heals.
import { QueryClient } from '@tanstack/react-query'
import { get, tokenParam, type Job, type Status, type TraceEvent } from './api'
import { useApp } from './store'

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

// The events that say where a build is working, into the session-progress slice.
// The files tree and the editor render it in place (docs/frontends/gui.md#files).
function applyProgress(ev: TraceEvent) {
  const app = useApp.getState()
  switch (ev.kind) {
    case 'batchStart': {
      // One queued entry per batch, keyed by the batch id the session events use.
      const goals = Array.isArray(ev.goals) ? (ev.goals as { id?: string; kind?: string; target?: string }[]) : []
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
      const staged = `${num(ev.staged)} staged · ${num(ev.rounds)} rounds`
      app.turnEnded(str(ev.label), 'done', summary ? `${staged} · ${summary}` : staged)
      break
    }
    case 'sessionFailed':
      app.turnEnded(str(ev.label), 'failed', str(ev.error))
      break
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
      for (const key of ['status', 'graph', 'coverage', 'journal', 'docs', 'pending', 'matrix', 'overview', 'board', 'views', 'tree', 'preview', 'explain'])
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
    // EventSource cannot set headers; the token rides the query string.
    es = new EventSource(`/api/events?${tokenParam()}`)
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
