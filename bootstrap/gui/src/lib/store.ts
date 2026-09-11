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

// Where a build is working, keyed by the session label (the batch id), the same
// key the harness puts on every event (docs/compiler/sessions.md#trace-events).
// The files tree and the editor read this to show progress in place.
export interface TurnProgress {
  label: string
  task: string
  target: string
  // The document, when the session reconciles one. Null for judgment sessions.
  doc: string | null
  state: 'queued' | 'running' | 'done' | 'failed'
  // The batch's sections, and the ones the session has reached so far.
  sections: string[]
  touched: string[]
  active: string | null
  result: string | null
  since: number
  // When the entry disappears. Null keeps it: a running session, or a held one.
  until: number | null
  // The pointer is on it, in the tree or in the text: hold the result.
  held: boolean
}

// How long a finished session stays visible before it fades.
export const LINGER_MS = 6000

// A goal state change seen on the live stream: resolved cards turn into their
// justification and stay until the build ends (docs/frontends/gui.md#board).
export interface GoalNote {
  event: string
  text: string
  at: number
}

// One chat session as the server reports it (docs/frontends/gui.md#chat).
export interface ChatSessionInfo {
  id: string
  title: string
  state: string
  updates: number
  pending: { id: string; request?: unknown }[]
}

export interface ChatUpdateRow {
  n: number
  update: Record<string, unknown>
}

const CHAT_RING = 2000

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
  turns: Record<string, TurnProgress>
  goalNotes: Record<string, GoalNote>
  noteGoal: (goal: string, event: string, text: string) => void
  clearGoalNotes: () => void
  queueSession: (p: { label: string; task: string; target: string; doc: string | null; sections: string[] }) => void
  turnStarted: (p: { label: string; task: string; target: string; doc: string | null; sections: string[] }) => void
  turnSection: (label: string, section: string) => void
  turnEnded: (label: string, state: 'done' | 'failed', result: string) => void
  turnHold: (label: string, held: boolean) => void
  // Nothing is running anymore: whatever is still marked running has ended with the
  // job, so let it fade like the rest.
  turnsSettle: () => void
  // Drop what has lingered long enough. Held entries stay.
  turnsSweep: () => void
  // The chat pane (docs/frontends/gui.md#chat).
  chatOpen: boolean
  chatFollow: boolean
  chatSelected: string | null
  chatSessions: Record<string, ChatSessionInfo>
  chatUpdates: Record<string, ChatUpdateRow[]>
  setChatOpen: (v: boolean) => void
  setChatFollow: (v: boolean) => void
  selectChat: (id: string | null) => void
  setChatSessions: (list: ChatSessionInfo[]) => void
  pushChatUpdate: (sessionId: string, row: ChatUpdateRow) => void
  seedChatUpdates: (sessionId: string, rows: ChatUpdateRow[]) => void
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
  chatOpen: localStorage.getItem('jazyk-chat') === 'open',
  chatFollow: false,
  chatSelected: null,
  chatSessions: {},
  chatUpdates: {},
  setChatOpen: (v) => {
    localStorage.setItem('jazyk-chat', v ? 'open' : 'closed')
    set({ chatOpen: v })
  },
  setChatFollow: (v) => set({ chatFollow: v }),
  selectChat: (id) => set({ chatSelected: id }),
  setChatSessions: (list) =>
    set(() => ({ chatSessions: Object.fromEntries(list.map((s) => [s.id, s])) })),
  pushChatUpdate: (sessionId, row) =>
    set((s) => {
      const ring = s.chatUpdates[sessionId] ?? []
      // Replays after a reconnect re-send rows the ring already holds.
      if (ring.length > 0 && row.n <= ring[ring.length - 1].n) return s
      const next = [...ring.slice(Math.max(0, ring.length - CHAT_RING + 1)), row]
      return { chatUpdates: { ...s.chatUpdates, [sessionId]: next } }
    }),
  seedChatUpdates: (sessionId, rows) =>
    set((s) => ({ chatUpdates: { ...s.chatUpdates, [sessionId]: rows.slice(-CHAT_RING) } })),
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
  turns: {},
  goalNotes: {},
  noteGoal: (goal, event, text) =>
    set((s) => ({
      goalNotes: { ...s.goalNotes, [goal]: { event, text, at: Date.now() } },
    })),
  clearGoalNotes: () => set({ goalNotes: {} }),
  queueSession: (p) =>
    set((s) => {
      // A batch names what its session will run; an entry already in flight
      // keeps its state.
      if (s.turns[p.label] && s.turns[p.label].state === 'running') return s
      return {
        turns: {
          ...s.turns,
          [p.label]: {
            ...p,
            state: 'queued',
            touched: [],
            active: null,
            result: null,
            since: Date.now(),
            until: null,
            held: s.turns[p.label]?.held ?? false,
          },
        },
      }
    }),
  turnStarted: (p) =>
    set((s) => ({
      turns: {
        ...s.turns,
        [p.label]: {
          ...p,
          state: 'running',
          // A retry re-runs the same item: the path through the document starts over.
          touched: [],
          active: null,
          result: null,
          since: Date.now(),
          until: null,
          held: s.turns[p.label]?.held ?? false,
        },
      },
    })),
  turnSection: (label, section) =>
    set((s) => {
      const t = s.turns[label]
      if (!t) return s
      return {
        turns: {
          ...s.turns,
          [label]: {
            ...t,
            active: section,
            touched: t.touched.includes(section) ? t.touched : [...t.touched, section],
          },
        },
      }
    }),
  turnEnded: (label, state, result) =>
    set((s) => {
      const t = s.turns[label]
      if (!t) return s
      return {
        turns: {
          ...s.turns,
          [label]: {
            ...t,
            state,
            result,
            active: null,
            since: Date.now(),
            until: t.held ? null : Date.now() + LINGER_MS,
          },
        },
      }
    }),
  turnHold: (label, held) =>
    set((s) => {
      const t = s.turns[label]
      if (!t || t.held === held) return s
      // Letting go re-arms the fade from now, so a result is never yanked away
      // the instant the pointer leaves it.
      const until = held || t.state === 'running' || t.state === 'queued' ? null : Date.now() + LINGER_MS
      return { turns: { ...s.turns, [label]: { ...t, held, until } } }
    }),
  turnsSettle: () =>
    set((s) => {
      const turns = { ...s.turns }
      for (const [k, t] of Object.entries(turns)) {
        if (t.state === 'running' || t.state === 'queued') {
          turns[k] = {
            ...t,
            state: 'done',
            active: null,
            result: t.result ?? 'the run ended here',
            until: t.held ? null : Date.now() + LINGER_MS,
          }
        }
      }
      return { turns }
    }),
  turnsSweep: () =>
    set((s) => {
      const now = Date.now()
      const keep = Object.entries(s.turns).filter(([, t]) => t.held || t.until === null || t.until > now)
      if (keep.length === Object.keys(s.turns).length) return s
      return { turns: Object.fromEntries(keep) }
    }),
}))

export function applyTheme() {
  const t = useApp.getState().theme
  if (t !== 'auto') document.documentElement.dataset.theme = t
}
