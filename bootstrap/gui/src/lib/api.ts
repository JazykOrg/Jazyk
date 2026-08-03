// The typed API client. The session token arrives in the URL fragment
// (#token=...), is stored per origin, and travels as a bearer header.
import { useApp } from './store'

let token: string | null = null

// localStorage is per origin and origins include the port, so each port keeps its
// own token: a reload, a new tab, or a browser restart on the same port resumes
// without the fragment.
const TOKEN_KEY = 'jazyk-token'

export function initToken() {
  const m = window.location.hash.match(/token=([0-9a-f]+)/)
  if (m) {
    token = m[1]
    localStorage.setItem(TOKEN_KEY, m[1])
    // Drop the fragment so copied URLs do not carry the secret.
    history.replaceState(null, '', window.location.pathname + window.location.search)
  } else {
    // Tabs from before the localStorage switch kept the token per tab.
    token = localStorage.getItem(TOKEN_KEY) ?? sessionStorage.getItem(TOKEN_KEY)
  }
}

// A replacement token entered through the prompt (server restarted mid-session).
export function setToken(t: string) {
  token = t
  localStorage.setItem(TOKEN_KEY, t)
}

export function tokenParam(): string {
  return token ? `token=${token}` : ''
}

async function req<T>(method: string, path: string, body?: unknown): Promise<T> {
  const headers: Record<string, string> = {}
  if (token) headers['Authorization'] = `Bearer ${token}`
  if (body !== undefined) headers['Content-Type'] = 'application/json'
  const res = await fetch(path, {
    method,
    headers,
    body: body !== undefined ? JSON.stringify(body) : undefined,
  })
  if (!res.ok) {
    // A stale token blocks the app with the token prompt instead of failing quietly.
    if (res.status === 401) useApp.getState().setAuthRequired(true)
    let msg = `${res.status}`
    try {
      const j = await res.json()
      if (j.error) msg = j.error
      if (res.status === 409) throw Object.assign(new Error(msg), { conflict: true, body: j })
    } catch (e) {
      if ((e as { conflict?: boolean }).conflict) throw e
    }
    throw Object.assign(new Error(msg), { status: res.status })
  }
  return res.json()
}

// A read where absence is an answer, not an error (e.g. a missing diff baseline).
export async function getOr404<T>(path: string): Promise<T | null> {
  try {
    return await req<T>('GET', path)
  } catch (e) {
    if ((e as { status?: number }).status === 404) return null
    throw e
  }
}

export const get = <T = unknown,>(path: string) => req<T>('GET', path)
export const post = <T = unknown,>(path: string, body?: unknown) => req<T>('POST', path, body)
export const put = <T = unknown,>(path: string, body?: unknown) => req<T>('PUT', path, body)

// ---- shapes (mirrors the YAML shards; see docs/compiler/graph.md) ----

export interface SourceRef {
  doc: string
  section: string
  quote: string
}

export interface Entity {
  name: string
  aliases?: string[]
  definition?: string
  scope?: string
  mentions?: SourceRef[]
  confidence?: number
  reasoning?: string
  created?: string
  updated?: string
}

export interface ReqEdge {
  a: string
  b: string
  type?: string
}

export interface Requirement {
  ears: string
  entities?: string[]
  edges?: ReqEdge[]
  source: SourceRef
  confidence?: number
  reasoning?: string
  created?: string
  updated?: string
}

export interface Relationship {
  type: string
  members: string[]
  requirements: string[]
}

export interface Diagnostic {
  rule: string
  severity: string
  subjects?: string[]
  message: string
  reasoning?: string
  lifecycle?: string
  triage?: string | null
  created?: string
  updated?: string
}

export interface Graph {
  generation: number
  entities: Record<string, Entity>
  requirements: Record<string, Requirement>
  relationships: Record<string, Relationship>
  diagnostics: Record<string, Diagnostic>
  redirects: Record<string, string>
}

export interface Status {
  generation: number
  verdict: string
  spent: { turns: number; rounds: number; tokens: number }
  parked: { task: string; target: string }[]
  counts: { entities: number; requirements: number; relationships: number }
  coverage: { covered: number; total: number }
  diagnostics: Record<string, number>
}

export interface Project {
  root: string
  out: string
  docsGlob: string[]
  roots: string[]
  deliverable: string
  llm: { model: string; baseUrl: string }
  version: string
}

export interface DocInfo {
  path: string
  contentHash: string
  graphHash: string | null
  stale: boolean
  diagnostics?: Record<string, number>
}

export interface Section {
  title: string
  kind: string
  order: number
  parent?: string
  raw: string
  hash: string
  lines: [number, number]
}

export interface DocRecord {
  contentHash: string
  sections: Record<string, Section>
  // Absent when nothing is covered yet: the server skips empty maps.
  coverage?: Record<string, { state: string; note?: string; claimedBy?: string }>
}

export interface JournalEntry {
  generation: number
  build: string
  workItem: { task: string; target: string; dirtySections?: string[]; staleAnchors?: string[] }
  mutations: Record<string, unknown>[]
  rounds: number
  tokens: number
}

// One line of the feedback log: what a model found ambiguous, wrong, or confusing
// about jazyk's own prompts and tools (docs/compiler/tools.md#feedback-tool).
export interface FeedbackEntry {
  at: string
  kind: string
  subject?: string
  message: string
  source?: string
  task?: string
  target?: string
  model?: string
  codec?: string
  generation?: number
  run?: string
  client?: string
}

export interface Job {
  id: number
  kind: { kind: string; entities?: string[]; targets?: string[] }
  state: string
  queuedAt: string
  startedAt: string | null
  finishedAt: string | null
  result: Record<string, unknown> | null
  events?: { jobId: number; event: TraceEvent }[]
}

export interface TraceEvent {
  kind: string
  label?: string
  [k: string]: unknown
}

export interface DelivOwners {
  entities: string[]
  requirements: string[]
  tests: string[]
}

export interface DelivFileInfo {
  path: string
  size: number
  owners: DelivOwners
}

export interface Deliverable {
  root: string
  files: DelivFileInfo[]
}

// ---- benchmarks (docs/frontends/gui.md#benchmarks) ----

export interface BenchmarkCase {
  score: number
  checks: string
  rounds: number
  tokens: number
  parRounds: number
  efficiency: number
  fail: string
}

export interface BenchmarkReport {
  verdicts: { compilation: string; generation: string; verification: string }
  scores: { extraction: number; review: number; generation: number; verification: number }
  checks: string
  efficiency: number
  tokens: number
  throughput: number
  cases: Record<string, BenchmarkCase>
}

export interface BenchmarkResult {
  model: string
  baseUrl: string
  gradedAt: number
  caseSetHash: string
  // Whether the grade was taken on this binary's case set.
  current: boolean
  source: 'embedded' | 'history' | 'project'
  codecs: { native?: BenchmarkReport; text?: BenchmarkReport }
}

export interface BenchmarkTable {
  caseSetHash: string
  results: BenchmarkResult[]
}

export interface BenchmarkModels {
  baseUrl: string
  models: string[]
  error?: string
}

export interface VerifyRow {
  status: string
  reason?: string
  entity?: string
  test?: { kind?: string; run?: string; label?: string }
  lastRun?: string
  evidence?: string
}
