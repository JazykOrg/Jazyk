// Ephemeral client state: the live trace ring, job snapshots, connection health,
// and UI preferences. Server state lives in the query cache, never here.
import { create } from 'zustand'
import type { Job, TraceEvent } from './api'
import type { FolderMode } from './recent'

const TRACE_RING = 2000

// Browser storage can be absent or throw (a private window, blocked site data);
// a preference that cannot be stored is simply not remembered.
function readPref(key: string): string | null {
  try {
    return localStorage.getItem(key)
  } catch {
    return null
  }
}

function writePref(key: string, value: string) {
  try {
    localStorage.setItem(key, value)
  } catch {
    // not remembered
  }
}

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

export interface FolderState {
  // The recent entry's id: `folder:<uuid>`.
  id: string
  name: string
  mode: FolderMode
  // Changes the folder has not taken: held in read-only mode, kept after a failed
  // write, or kept from an earlier visit.
  held: number
  // Why the last write to the folder failed, until the kept changes land.
  error: string | null
  // The kept changes are being written now.
  writing: boolean
  // Why browser storage refused to keep the changes, if it did.
  storeError: string | null
}

// The action the split button runs (docs/frontends/gui.md#the-action-button).
export type ActionChoice = 'compile' | 'gen' | 'verify'

// The editor's one toggle (docs/frontends/gui.md#editor).
export type DocMode = 'read' | 'write' | 'diff'

export interface Caret {
  doc: string
  entity: string | null
  reqs: string[]
}

export interface DocStatus {
  dirty: boolean
  autosave: boolean
  error?: string
  conflict: boolean
  canDiff: boolean
}

// A change the editor did not make, marked in the text until it fades
// (docs/frontends/gui.md#editor): located by the text that landed (an applied
// option's reply) or by the lines the editor saw change.
export interface Flash {
  id: number
  doc: string
  label: string
  at: number
  fading: boolean
  // Whether its hold and fade have started, which happens when it is first shown.
  armed?: boolean
  text?: string
  lines?: [number, number]
}

// How long a mark stands, and how long it takes to fade after.
export const FLASH_HOLD_MS = 5000
export const FLASH_FADE_MS = 3000
// A change arriving this soon after an answer is the answer's.
const ANSWER_WINDOW_MS = 10 * 60_000

let flashSeq = 1

export interface StoredState {
  // The recent entry's id: `demo:<id>` or `browser:<uuid>`.
  id: string
  kind: 'demo' | 'browser'
  name: string
  // A demo whose copy carries edits.
  edited: boolean
  // Why the last store failed, and how many changes are in the tab only.
  error: string | null
  unsaved: number
  // Whether the browser can store at all; without it, changes stay in the tab.
  available: boolean
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
  // The Developer toggle: the compiler workbench and its words
  // (docs/frontends/gui.md#developer-toggle). Remembered per browser; a server
  // started with --dev turns it on for a browser that never chose.
  dev: boolean
  devChosen: boolean
  lastCommit: { generation: number; at: number } | null
  // The activity panel: collapsed is the one-line control bar.
  activityOpen: boolean
  // The open document's unsaved-edit state, for tree ops that need a save first.
  editorDirty: boolean
  // Read, Write, or Diff for every document at once, the last choice for the
  // session; null is the default for the open folder (docs/frontends/gui.md#editor).
  docMode: DocMode | null
  // What each open document's editor reports for a bar it does not draw itself (a
  // board window's): unsaved changes, saving on its own, an error, a conflict, and
  // whether it has a baseline to diff against.
  docStatus: Record<string, DocStatus>
  // Where the reader is in the open document: the line holding the cursor in
  // Write, the top of the view in Read. The outline follows it
  // (docs/frontends/gui.md#docs).
  editorLine: { doc: string; line: number } | null
  // What the caret rests on in Write: the entity of the mention under it and the
  // requirements whose quote holds it (docs/frontends/gui.md#editor).
  caret: Caret | null
  // A quote to reveal in its document's editor, taken by the editor showing that
  // document (a requirement's line on the board, docs/frontends/gui.md#the-linkage).
  reveal: { doc: string; quote: string; n: number } | null
  // The folder the browser backend has open, its mode, how many changes it has not
  // taken (held in read-only mode, or kept after a failed write), and why the last
  // write failed (docs/frontends/wasm.md#a-folder-on-disk). Null for a stored
  // project and the server.
  folder: FolderState | null
  // The project the tab keeps in browser storage: a browser project or a demo
  // (docs/frontends/wasm.md#browser-storage). Null for a folder and the server.
  stored: StoredState | null
  // The in-page model's load, while it runs or after it failed
  // (docs/frontends/wasm.md#the-demo).
  modelLoad: { text: string; failed?: boolean } | null
  // The action the split button's main part runs, when one was chosen from its
  // list; null takes the default for the state (docs/frontends/gui.md#the-action-button).
  action: ActionChoice | null
  // The remembered project is being reopened on page load: the routes wait
  // (docs/frontends/gui.md#the-open-project).
  reopening: boolean
  // A remembered folder that needs the browser's permission again: the workspace
  // shows one card, `Continue with <folder>`, whose click asks.
  pendingFolder: { id: string; name: string } | null
  turns: Record<string, TurnProgress>
  // The marks over changes the editor did not make, and when the page last sent an
  // answer, which names what a later change is (docs/frontends/gui.md#editor).
  flashes: Flash[]
  answeredAt: number
  addFlash: (f: { doc: string; label: string; text?: string; lines?: [number, number] }) => void
  armFlashes: (doc: string) => void
  noteAnswered: () => void
  // The words a change the editor did not make carries.
  changeLabel: () => string
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
  // Build threads closed from the list, by job id, for the life of the page
  // (docs/frontends/gui.md#chat).
  hiddenThreads: number[]
  hideThread: (jobId: number) => void
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
  setDev: (v: boolean, chosen?: boolean) => void
  setLastCommit: (generation: number) => void
  setActivityOpen: (v: boolean) => void
  setEditorDirty: (v: boolean) => void
  setDocMode: (mode: DocMode) => void
  setDocStatus: (doc: string, status: DocStatus | null) => void
  setEditorLine: (doc: string, line: number) => void
  setCaret: (doc: string, entity: string | null, reqs: string[]) => void
  setReveal: (doc: string, quote: string) => void
  takeReveal: (n: number) => void
  setFolder: (f: FolderState | null) => void
  setStored: (s: StoredState | null) => void
  setModelLoad: (m: { text: string; failed?: boolean } | null) => void
  setAction: (a: ActionChoice | null) => void
  setReopening: (v: boolean) => void
  setPendingFolder: (f: { id: string; name: string } | null) => void
}

const ACTIONS: ActionChoice[] = ['compile', 'gen', 'verify']
function readAction(): ActionChoice | null {
  const v = readPref('jazyk-action')
  return ACTIONS.includes(v as ActionChoice) ? (v as ActionChoice) : null
}

export const useApp = create<AppStore>((set, get) => ({
  connected: false,
  authRequired: false,
  tokenEpoch: 0,
  jobs: {},
  trace: [],
  watchMode: 'queue',
  genMode: 'manual',
  theme: (readPref('jazyk-theme') as 'auto' | 'light' | 'dark') || 'auto',
  dev: readPref('jazyk-dev') === 'on',
  devChosen: readPref('jazyk-dev') !== null,
  lastCommit: null,
  activityOpen: readPref('jazyk-activity') === 'open',
  editorDirty: false,
  docMode: null,
  docStatus: {},
  editorLine: null,
  caret: null,
  reveal: null,
  folder: null,
  stored: null,
  modelLoad: null,
  action: readAction(),
  reopening: false,
  pendingFolder: null,
  chatOpen: readPref('jazyk-chat') !== 'closed',
  // Follow in editor is on by default (docs/frontends/gui.md#chat).
  chatFollow: true,
  chatSelected: null,
  hiddenThreads: [],
  hideThread: (jobId) =>
    set((s) => (s.hiddenThreads.includes(jobId) ? s : { hiddenThreads: [...s.hiddenThreads, jobId] })),
  chatSessions: {},
  chatUpdates: {},
  flashes: [],
  answeredAt: 0,
  // A mark waits for its document to be shown: the hold and the fade start when the
  // editor first draws it (armFlashes), so a change made from the Inbox is still
  // marked when the document opens later. One never shown goes after ten minutes.
  addFlash: (f) => {
    const id = flashSeq++
    set((s) => ({ flashes: [...s.flashes, { ...f, id, at: Date.now(), fading: false, armed: false }] }))
    window.setTimeout(() => set((s) => ({ flashes: s.flashes.filter((x) => x.id !== id || x.armed) })), ANSWER_WINDOW_MS)
  },
  armFlashes: (doc) => {
    const fresh = get().flashes.filter((x) => x.doc === doc && !x.armed)
    if (fresh.length === 0) return
    const ids = new Set(fresh.map((x) => x.id))
    set((s) => ({ flashes: s.flashes.map((x) => (ids.has(x.id) ? { ...x, armed: true } : x)) }))
    window.setTimeout(() => set((s) => ({ flashes: s.flashes.map((x) => (ids.has(x.id) ? { ...x, fading: true } : x)) })), FLASH_HOLD_MS)
    window.setTimeout(() => set((s) => ({ flashes: s.flashes.filter((x) => !ids.has(x.id)) })), FLASH_HOLD_MS + FLASH_FADE_MS)
  },
  noteAnswered: () => set({ answeredAt: Date.now() }),
  changeLabel: () =>
    Date.now() - get().answeredAt < ANSWER_WINDOW_MS ? 'changed by your answer' : 'changed outside the editor',
  setChatOpen: (v) => {
    writePref('jazyk-chat', v ? 'open' : 'closed')
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
  setDev: (v, chosen = true) => {
    if (chosen) writePref('jazyk-dev', v ? 'on' : 'off')
    document.documentElement.dataset.dev = v ? 'on' : 'off'
    set({ dev: v, devChosen: chosen })
  },
  setTheme: (t) => {
    writePref('jazyk-theme', t)
    if (t === 'auto') delete document.documentElement.dataset.theme
    else document.documentElement.dataset.theme = t
    set({ theme: t })
  },
  setLastCommit: (generation) => set({ lastCommit: { generation, at: Date.now() } }),
  setActivityOpen: (v) => {
    writePref('jazyk-activity', v ? 'open' : 'closed')
    set({ activityOpen: v })
  },
  setEditorDirty: (v) => set((s) => (s.editorDirty === v ? s : { editorDirty: v })),
  setDocMode: (mode) => set((s) => (s.docMode === mode ? s : { docMode: mode })),
  setDocStatus: (doc, status) =>
    set((s) => {
      const had = s.docStatus[doc]
      if (!status) {
        if (!had) return s
        const next = { ...s.docStatus }
        delete next[doc]
        return { docStatus: next }
      }
      if (had && had.dirty === status.dirty && had.autosave === status.autosave && had.error === status.error && had.conflict === status.conflict && had.canDiff === status.canDiff)
        return s
      return { docStatus: { ...s.docStatus, [doc]: status } }
    }),
  setReveal: (doc, quote) => set((s) => ({ reveal: { doc, quote, n: (s.reveal?.n ?? 0) + 1 } })),
  takeReveal: (n) => set((s) => (s.reveal?.n === n ? { reveal: null } : s)),
  setCaret: (doc, entity, reqs) =>
    set((s) => {
      const c = s.caret
      if (!entity && reqs.length === 0) return c && c.doc === doc ? { caret: null } : s
      if (c && c.doc === doc && c.entity === entity && c.reqs.length === reqs.length && c.reqs.every((r, i) => r === reqs[i])) return s
      return { caret: { doc, entity, reqs } }
    }),
  setEditorLine: (doc, line) =>
    set((s) => (s.editorLine && s.editorLine.doc === doc && s.editorLine.line === line ? s : { editorLine: { doc, line } })),
  setFolder: (folder) => set({ folder }),
  setStored: (stored) => set({ stored }),
  setModelLoad: (modelLoad) => set({ modelLoad }),
  setAction: (action) => {
    if (action) writePref('jazyk-action', action)
    else {
      try {
        localStorage.removeItem('jazyk-action')
      } catch {
        // not remembered
      }
    }
    set({ action })
  },
  setReopening: (reopening) => set({ reopening }),
  setPendingFolder: (pendingFolder) => set({ pendingFolder }),
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
  document.documentElement.dataset.dev = useApp.getState().dev ? 'on' : 'off'
}
