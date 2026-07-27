// Ephemeral client state: the live trace ring, job snapshots, connection health,
// and UI preferences. Server state lives in the query cache, never here.
import { create } from 'zustand'
import type { Job, TraceEvent } from './api'

const TRACE_RING = 2000

export interface JobTraceRow {
  jobId: number
  seq: number
  event: TraceEvent
}

interface AppStore {
  connected: boolean
  jobs: Record<number, Job>
  trace: JobTraceRow[]
  watch: boolean
  theme: 'auto' | 'light' | 'dark'
  lastCommit: { generation: number; at: number } | null
  setConnected: (v: boolean) => void
  upsertJob: (j: Job) => void
  setJobState: (id: number, state: string, result?: Job['result']) => void
  pushTrace: (row: JobTraceRow) => void
  setWatch: (v: boolean) => void
  setTheme: (t: 'auto' | 'light' | 'dark') => void
  setLastCommit: (generation: number) => void
}

export const useApp = create<AppStore>((set) => ({
  connected: false,
  jobs: {},
  trace: [],
  watch: false,
  theme: (localStorage.getItem('jazyk-theme') as 'auto' | 'light' | 'dark') || 'auto',
  lastCommit: null,
  setConnected: (v) => set({ connected: v }),
  upsertJob: (j) => set((s) => ({ jobs: { ...s.jobs, [j.id]: j } })),
  setJobState: (id, state, result) =>
    set((s) => {
      const j = s.jobs[id]
      if (!j) return s
      return { jobs: { ...s.jobs, [id]: { ...j, state, result: result ?? j.result } } }
    }),
  pushTrace: (row) =>
    set((s) => ({
      trace: [...s.trace.slice(Math.max(0, s.trace.length - TRACE_RING + 1)), row],
    })),
  setWatch: (v) => set({ watch: v }),
  setTheme: (t) => {
    localStorage.setItem('jazyk-theme', t)
    if (t === 'auto') delete document.documentElement.dataset.theme
    else document.documentElement.dataset.theme = t
    set({ theme: t })
  },
  setLastCommit: (generation) => set({ lastCommit: { generation, at: Date.now() } }),
}))

export function applyTheme() {
  const t = useApp.getState().theme
  if (t !== 'auto') document.documentElement.dataset.theme = t
}
