// The typed API client. The session token arrives in the URL fragment
// (#token=...), is kept for the session, and travels as a bearer header.

let token: string | null = null

export function initToken() {
  const m = window.location.hash.match(/token=([0-9a-f]+)/)
  if (m) {
    token = m[1]
    sessionStorage.setItem('jazyk-token', m[1])
    // Drop the fragment so copied URLs do not carry the secret.
    history.replaceState(null, '', window.location.pathname + window.location.search)
  } else {
    token = sessionStorage.getItem('jazyk-token')
  }
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
    let msg = `${res.status}`
    try {
      const j = await res.json()
      if (j.error) msg = j.error
      if (res.status === 409) throw Object.assign(new Error(msg), { conflict: true, body: j })
    } catch (e) {
      if ((e as { conflict?: boolean }).conflict) throw e
    }
    throw new Error(msg)
  }
  return res.json()
}

export const get = <T = unknown,>(path: string) => req<T>('GET', path)
export const post = <T = unknown,>(path: string, body?: unknown) => req<T>('POST', path, body)
export const put = <T = unknown,>(path: string, body?: unknown) => req<T>('PUT', path, body)

// ---- shapes (mirrors the YAML shards; see docs2/compiler/graph.md) ----

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
  coverage: Record<string, { state: string; note?: string; claimedBy?: string }>
}

export interface JournalEntry {
  generation: number
  build: string
  workItem: { task: string; target: string; dirtySections?: string[]; staleAnchors?: string[] }
  mutations: Record<string, unknown>[]
  rounds: number
  tokens: number
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

export interface VerifyRow {
  status: string
  reason?: string
  entity?: string
  test?: { kind?: string; run?: string; label?: string }
  lastRun?: string
  evidence?: string
}
