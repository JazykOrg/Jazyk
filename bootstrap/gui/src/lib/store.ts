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
  // A request answered 401: the token is missing or stale (server restart).
  authRequired: boolean
  // Bumped when a new token is accepted; long-lived connections re-dial on it.
  tokenEpoch: number
  jobs: Record<number, Job>
  trace: JobTraceRow[]
  watchMode: 'off' | 'queue' | 'watch'
  genMode: 'manual' | 'auto'
  theme: 'auto' | 'light' | 'dark'
  lastCommit: { generation: number; at: number } | null
  // The activity panel: collapsed is the one-line control bar.
  activityOpen: boolean
  // The open document's unsaved-edit state, for tree ops that need a save first.
  editorDirty: boolean
  setConnected: (v: boolean) => void
  setAuthRequired: (v: boolean) => void
  bumpTokenEpoch: () => void
  upsertJob: (j: Job) => void
  setJobState: (id: number, state: string, result?: Job['result']) => void
  pushTrace: (row: JobTraceRow) => void
  setWatchMode: (v: 'off' | 'queue' | 'watch') => void
  setGenMode: (v: 'manual' | 'auto') => void
  setTheme: (t: 'auto' | 'light' | 'dark') => void
  setLastCommit: (generation: number) => void
  setActivityOpen: (v: boolean) => void
  setEditorDirty: (v: boolean) => void
}

export const useApp = create<AppStore>((set) => ({
  connected: false,
  authRequired: false,
  tokenEpoch: 0,
  jobs: {},
  trace: [],
  watchMode: 'queue',
  genMode: 'manual',
  theme: (localStorage.getItem('jazyk-theme') as 'auto' | 'light' | 'dark') || 'auto',
  lastCommit: null,
  activityOpen: localStorage.getItem('jazyk-activity') === 'open',
  editorDirty: false,
  setConnected: (v) => set({ connected: v }),
  setAuthRequired: (v) => set((s) => (s.authRequired === v ? s : { authRequired: v })),
  bumpTokenEpoch: () => set((s) => ({ tokenEpoch: s.tokenEpoch + 1 })),
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
  setWatchMode: (v) => set({ watchMode: v }),
  setGenMode: (v) => set({ genMode: v }),
  setTheme: (t) => {
    localStorage.setItem('jazyk-theme', t)
    if (t === 'auto') delete document.documentElement.dataset.theme
    else document.documentElement.dataset.theme = t
    set({ theme: t })
  },
  setLastCommit: (generation) => set({ lastCommit: { generation, at: Date.now() } }),
  setActivityOpen: (v) => {
    localStorage.setItem('jazyk-activity', v ? 'open' : 'closed')
    set({ activityOpen: v })
  },
  setEditorDirty: (v) => set((s) => (s.editorDirty === v ? s : { editorDirty: v })),
}))

export function applyTheme() {
  const t = useApp.getState().theme
  if (t !== 'auto') document.documentElement.dataset.theme = t
}
