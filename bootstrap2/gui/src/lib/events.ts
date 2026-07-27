// The live spine: one SSE connection drives query invalidation and the zustand
// slices. A dropped connection falls back to polling /api/status until it heals.
import { QueryClient } from '@tanstack/react-query'
import { get, tokenParam, type Job, type Status } from './api'
import { useApp } from './store'

type WireEvent = {
  seq?: number
  type: string
  [k: string]: unknown
}

// One event fans out to the query keys it invalidates; only mounted queries refetch.
function dispatch(qc: QueryClient, ev: WireEvent) {
  const app = useApp.getState()
  switch (ev.type) {
    case 'store.generation': {
      app.setLastCommit(ev.generation as number)
      for (const key of ['status', 'graph', 'coverage', 'journal', 'docs', 'pending', 'matrix', 'overview'])
        qc.invalidateQueries({ queryKey: [key] })
      qc.invalidateQueries({ queryKey: ['node'] })
      break
    }
    case 'store.lock':
      qc.invalidateQueries({ queryKey: ['status'] })
      break
    case 'docs.changed':
      qc.invalidateQueries({ queryKey: ['docs'] })
      break
    case 'pending.changed':
      qc.invalidateQueries({ queryKey: ['pending'] })
      qc.invalidateQueries({ queryKey: ['matrix'] })
      break
    case 'watch.state':
      app.setWatchMode(ev.mode as 'off' | 'queue' | 'watch')
      break
    case 'job.queued':
      app.upsertJob(ev.job as Job)
      qc.invalidateQueries({ queryKey: ['jobs'] })
      break
    case 'job.started':
      app.setJobState(ev.jobId as number, 'running')
      qc.invalidateQueries({ queryKey: ['jobs'] })
      break
    case 'job.trace':
      app.pushTrace({
        jobId: ev.jobId as number,
        seq: (ev.seq as number) ?? 0,
        event: ev.event as { kind: string },
      })
      break
    case 'job.finished':
      app.setJobState(ev.jobId as number, ev.state as string, ev.result as Job['result'])
      for (const key of ['jobs', 'pending', 'matrix', 'status'])
        qc.invalidateQueries({ queryKey: [key] })
      break
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

  // Seed job state so the Build view knows about jobs from before this page load.
  get<{ jobs: Job[] }>(`/api/jobs`)
    .then((r) => r.jobs.forEach((j) => app.upsertJob(j)))
    .catch(() => {})
  get<{ mode: 'off' | 'queue' | 'watch' }>(`/api/watch`)
    .then((r) => app.setWatchMode(r.mode))
    .catch(() => {})

  connect()
  return () => {
    es?.close()
    stopPolling()
  }
}
