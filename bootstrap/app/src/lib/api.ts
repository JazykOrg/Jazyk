// The typed API client over the transport seam (lib/backend.ts). The session
// token arrives on the connect route (`#/connect?port=&token=`), is stored per port
// under `jazyk.server.<port>`, and travels as a bearer header on the server backend
// (docs/frontends/gui.md#serving).
import { backend, serverBase } from './backend'
import { useApp } from './store'

// The server this page is connected to, if any: its port and the token in use.
let server: { port: number; token: string | null } | null = null

export const serverTokenKey = (port: number) => `jazyk.server.${port}`

// The token stored for a port, or null when none was ever stored.
export function storedToken(port: number): string | null {
  try {
    return localStorage.getItem(serverTokenKey(port))
  } catch {
    return null
  }
}

export function storeToken(port: number, token: string | null) {
  try {
    if (token) localStorage.setItem(serverTokenKey(port), token)
    else localStorage.removeItem(serverTokenKey(port))
  } catch {
    // storage may be unavailable; the token still lives in memory
  }
}

// The connection in use: a token given (from the connect route) is stored under the
// port; none given resumes with the stored one.
export function setServer(port: number, token?: string | null): string | null {
  const t = token ?? storedToken(port)
  if (token) storeToken(port, token)
  server = { port, token: t }
  return t
}

export function clearServer() {
  server = null
}

export function currentServer(): { port: number; token: string | null } | null {
  return server
}

export function currentToken(): string | null {
  return server?.token ?? null
}

// A replacement token entered through the prompt (server restarted mid-session):
// it replaces the stored one for the connected port.
export function setToken(t: string) {
  if (!server) return
  server = { ...server, token: t }
  storeToken(server.port, t)
}

export function tokenParam(): string {
  const t = currentToken()
  return t ? `token=${t}` : ''
}

// An absolute URL on the connected server for what cannot send a header: an image,
// an export link, the event stream, with the token in the query. Relative (and so
// answered by the backend in the tab) while no server is connected.
export function apiUrl(path: string): string {
  const base = serverBase() ?? ''
  const qs = tokenParam()
  if (!qs) return `${base}${path}`
  return `${base}${path}${path.includes('?') ? '&' : '?'}${qs}`
}

export interface ApiError extends Error {
  status?: number
  conflict?: boolean
  body?: unknown
  // A 501: the backend does not serve this route, and says why.
  unserved?: boolean
}

function failure(status: number, body: unknown): ApiError {
  let msg = `${status}`
  if (body && typeof body === 'object' && typeof (body as { error?: unknown }).error === 'string')
    msg = (body as { error: string }).error
  const e = new Error(msg) as ApiError
  e.status = status
  e.body = body
  if (status === 409) e.conflict = true
  if (status === 501) e.unserved = true
  return e
}

async function req<T>(method: string, path: string, body?: unknown): Promise<T> {
  const r = await backend().request(method, path, body)
  if (r.status < 200 || r.status >= 300) {
    // A stale token blocks the app with the token prompt instead of failing quietly.
    if (r.status === 401) useApp.getState().setAuthRequired(true)
    throw failure(r.status, r.body)
  }
  return r.body as T
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

// A read that answers with the file's text rather than JSON (the out directory's
// files). The same token and the same 401 handling as the JSON reads.
export async function getText(path: string): Promise<string> {
  const r = await backend().request('GET', path)
  if (r.status < 200 || r.status >= 300) {
    if (r.status === 401) useApp.getState().setAuthRequired(true)
    throw failure(r.status, r.body)
  }
  return r.text
}

export const get = <T = unknown,>(path: string) => req<T>('GET', path)
export const post = <T = unknown,>(path: string, body?: unknown) => req<T>('POST', path, body)
export const put = <T = unknown,>(path: string, body?: unknown) => req<T>('PUT', path, body)
export const del = <T = unknown,>(path: string) => req<T>('DELETE', path)

// ---- shapes (mirrors the YAML shards; see docs/compiler/graph.md) ----

export interface SourceRef {
  doc: string
  section: string
  quote: string
}

// One-key map naming the provenance kind (docs/compiler/model.md#provenance).
export interface Provenance {
  quote?: SourceRef
  derived?: { from: string[]; reasoning: string }
  decree?: { author: string; at: string; note?: string }
}

export interface Attribute {
  name: string
  type?: string
  value?: string
  provenance?: Provenance
}

export interface Entity {
  name: string
  aliases?: string[]
  definition?: string
  scope?: string
  stereotype?: string
  parent?: string
  attributes?: Attribute[]
  mentions?: SourceRef[]
  provenance?: Provenance
  limits?: Record<string, number>
  confidence?: number
  reasoning?: string
  created?: string
  updated?: string
}

export interface ReqEdge {
  a: string
  b: string
  type?: string
  cardinality?: string
}

export interface Transition {
  subject: string
  from: string
  to: string
  trigger?: string
  guard?: string
}

export interface Facet {
  facet: string
  reasoning: string
  measure?: string
}

export interface Requirement {
  statement: string
  entities?: string[]
  edges?: ReqEdge[]
  transition?: Transition
  facets?: Facet[]
  // Exactly one of source (the quote form) or provenance (derived | decree).
  source?: SourceRef
  provenance?: Provenance
  confidence?: number
  reasoning?: string
  created?: string
  updated?: string
}

// One direction-and-type group with the requirements behind it.
export interface Contribution {
  a: string
  b: string
  type: string
  cardinality?: string
  requirements: string[]
}

export interface Relationship {
  members: string[]
  contributions: Contribution[]
}

export interface ViewQuery {
  scope?: string
  parent?: string
  stereotype?: string
  depth?: number
}

export interface View {
  kind: string
  title: string
  members?: string[]
  excluded?: { id: string; note: string }[]
  query?: ViewQuery
  collapse?: string[]
  provenance?: Provenance
  default?: boolean
  limits?: Record<string, number>
  created?: string
  updated?: string
}

export interface StateTransition {
  from: string
  to: string
  trigger?: string
  guard?: string
  requirements: string[]
}

export interface StateMachine {
  subject: string
  states: string[]
  initial?: string
  transitions: StateTransition[]
}

export interface Cause {
  generation: number
  mutation: number
  via?: string
}

// open | parked as strings; blocked and failed carry their payload.
export type GoalState = 'open' | 'parked' | { blocked: { on: string } } | { failed: { reason: string } }

export interface Goal {
  id: string
  kind: string
  class: string
  mandatory: boolean
  target: string
  unit?: string
  change?: unknown
  cause?: Cause
  state: GoalState
  hints?: string[]
}

export interface Verdict {
  state: string
  open?: number
  failed?: number
  blocked?: number
  optional?: number
}

export function verdictText(v: Verdict | undefined | null): string {
  if (!v || !v.state) return 'no build yet'
  if (v.state === 'converged') {
    let s = 'converged'
    if (v.blocked) s += `, ${v.blocked} blocked`
    if (v.optional) s += `, ${v.optional} optional advised`
    return s
  }
  return `${v.state}: ${v.open ?? 0} open, ${v.failed ?? 0} failed, ${v.blocked ?? 0} blocked, ${v.optional ?? 0} optional advised`
}

export interface Counts {
  open: number
  parked: number
  failed: number
  blocked: number
  optional: number
  ready: number
  gated: number
  claimed: number
  by_class: Record<string, number>
  by_kind: Record<string, number>
}

// One goal as GET /api/board serves it (docs/frontends/gui.md#board).
export interface BoardGoal extends Goal {
  ready: boolean
  gated: boolean
  tier?: number | null
  blockedBy?: string
  claimedBy?: string
  batch?: string
}

export interface BoardBatch {
  id: string
  class: string
  tier?: number | null
  executor?: string | null
  locality: string
  goals: { id: string; kind: string; target: string; mandatory: boolean }[]
}

export interface BoardData {
  generation: number
  goals: BoardGoal[]
  batches: BoardBatch[]
  counts: Counts
  verdict: string
  summary: string
  note?: string
  next?: string
}

export interface PreviewData {
  batch: BoardBatch | null
  prompt: string | null
  toolset?: string[]
  executor?: string
  executorError?: string
  humanBlocked?: boolean
  // In compile: manual before a release: the batch the release would form.
  gated?: boolean
  note?: string
}

export interface LimitState {
  limit: string
  count: number
  soft: number
  hard: number
  over: boolean
  overHard: boolean
}

export interface ViewInfo {
  id: string
  kind: string
  title: string
  default: boolean
  members: number
  edges: number
  limits: LimitState[]
}

export interface ViewMember {
  id: string
  node: 'entity' | 'requirement' | 'gone'
  name?: string
  stereotype?: string
  parent?: string
  hidden?: boolean
  // Set on an entity the detail expansion drew: the level it came in at
  // (docs/frontends/gui.md#explore).
  detail?: number
  statement?: string
  entities?: string[]
  transition?: Transition
}

export interface ViewArrow {
  a: string
  b: string
  type: string
  lifted: boolean
  count: number
  cardinality?: string
  requirements: string[]
  concrete: { a: string; b: string; type: string; cardinality?: string; requirements: string[] }[]
  rel: string
}

export interface ViewStep {
  requirement: string
  statement: string
  participants: { id: string; name: string }[]
  transition?: Transition
}

// One link down: a drawn member and the level view of its own children
// (docs/compiler/concepts/levels.md#drill-down).
export interface ViewChild {
  member: string
  view: string
}

export interface ViewDetail {
  id: string
  kind: string
  title: string
  default: boolean
  members: ViewMember[]
  excluded: { id: string; note: string }[]
  collapse: string[]
  query?: ViewQuery
  provenance?: Provenance
  limits: LimitState[]
  arrows: ViewArrow[]
  steps: ViewStep[]
  machines: { id: string; machine: StateMachine }[]
  children: ViewChild[]
  // The levels of detail applied beneath the members, and whether one more would
  // draw anything (docs/frontends/gui.md#explore).
  detail: number
  deeper: boolean
  puml?: string | null
  svg?: string | null
  renderError?: string | null
}

// ---- the walk (GET /api/entities/{id}/card, GET /api/views/{id}/page): the shared
// card model, one level in every direction (docs/consumers/docsgen.md#entity-cards,
// docs/frontends/gui.md#explore) ----

export interface CardCrumb {
  id: string
  name: string
}

export interface CardKin {
  id: string
  name: string
  childCount: number
}

export interface CardRelation {
  other: string
  type: string
  // `a` when the entity acts on the other, `b` when it is acted on, `both` either way.
  direction: 'a' | 'b' | 'both'
  count: number
}

export interface EntityCardData {
  id: string
  name: string
  stereotype: string | null
  definition: string
  provenance: 'quote' | 'derived' | 'decree'
  // From the scope root down to the entity itself, last.
  breadcrumb: CardCrumb[]
  // The structural level view of the parent's level: where the entity is used.
  context: string | null
  // The entity's own level view, null on a leaf.
  inside: string | null
  insideFlows: string[]
  relationships: CardRelation[]
  // The flow views at the parent's level the entity is drawn in.
  flows: string[]
  siblings: CardKin[]
  children: CardKin[]
  requirementCount: number
  proposal: string | null
}

export interface PageDrawn {
  id: string
  name: string
  stereotype: string | null
  levelView: string | null
}

export interface PageStep {
  requirement: string
  statement: string
  from: string
  to: string
}

export interface ViewPageData {
  id: string
  kind: string
  title: string
  level: { target: string; breadcrumb: CardCrumb[] } | null
  drawn: PageDrawn[]
  steps: PageStep[]
  around: { sameLevel: string[]; above: string | null; below: string[] }
}

// The containment tree (GET /api/tree): one root per scope, nodes nested by
// `parent`, each with its child count, its structural level view, and the flow views
// derived for its level (docs/frontends/gui.md#graph).
export interface TreeNode {
  id: string
  name: string
  stereotype?: string | null
  grouping: boolean
  count: number
  levelView: string | null
  views: string[]
  children: TreeNode[]
}

export interface TreeRoot {
  scope: string
  target: string
  count: number
  levelView: string | null
  views: string[]
  children: TreeNode[]
}

export interface TreeData {
  generation: number
  roots: TreeRoot[]
}

export interface Diagnostic {
  rule: string
  severity: string
  subjects?: string[]
  message: string
  reasoning?: string
  lifecycle?: string
  triage?: string | null
  prompt?: { question: string; options?: { label: string; edit?: unknown; answer?: string }[]; freeform?: boolean }
  answer?: { status?: string; text?: string } | null
  created?: string
  updated?: string
}

// The observable evidence one requirement is met (docs/compiler/model/criterion.md).
export interface Criterion {
  requirement: string
  statement: string
  given?: string
  when?: string
  then?: string
  // positive | negative | boundary | quality
  kind?: string
  measure?: string
  entities?: string[]
  source?: SourceRef
  provenance?: Provenance
  stale?: boolean
  confidence?: number
  reasoning?: string
}

export interface Graph {
  generation: number
  entities: Record<string, Entity>
  requirements: Record<string, Requirement>
  // Absent from a server that predates criteria in the graph read.
  criteria?: Record<string, Criterion>
  views: Record<string, View>
  relationships: Record<string, Relationship>
  stateMachines: Record<string, StateMachine>
  diagnostics: Record<string, Diagnostic>
  redirects: Record<string, string>
}

export interface Status {
  version?: number
  generation: number
  verdict: Verdict
  spent: { sessions: number; rounds: number; tokens: number }
  parked: Goal[]
  failed?: { goal: Goal; reason: string }[]
  costs?: {
    sessions?: number
    tokens?: number
    by_kind?: Record<string, { sessions: number; tokens: number }>
    by_class?: Record<string, { sessions: number; tokens: number }>
  }
  board?: Counts
  counts: { entities: number; requirements: number; relationships: number; views?: number }
  coverage: { covered: number; total: number }
  diagnostics: Record<string, number>
}

export interface Project {
  root: string
  out: string
  docsGlob: string[]
  roots: string[]
  deliverable: string
  executors?: Record<string, string>
  budgets?: Record<string, number>
  llm: { model: string; baseUrl: string }
  version: string
}

export interface DocInfo {
  path: string
  contentHash: string
  graphHash: string | null
  stale: boolean
  diagnostics?: Record<string, number>
  // Open goals on this document, counted by kind.
  goals?: Record<string, number>
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
  // session | edit | align | gc | settle-diagnostics | checks | decree | dual-write
  // | ratify | triage | answer
  kind?: string
  batch?: string[]
  author?: string
  note?: string
  dirtied?: string[]
  mutations: Record<string, unknown>[]
  resolved_goals?: { goal: string; justification: string; evidence?: unknown }[]
  opened_goals?: { goal: string; cause: Cause }[]
  rounds: number
  tokens: number
}

// One line naming a journal entry: its kind, or the batch's goals.
export function entryLabel(e: JournalEntry): string {
  if (e.kind === 'session' && (e.batch ?? []).length > 0) return (e.batch ?? []).join(', ')
  if (e.kind === 'edit') return `edit ${(e.dirtied ?? [])[0] ?? ''} (human)`
  return e.kind || 'changeset'
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
  events?: { jobId: number; n?: number; event: TraceEvent }[]
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

export interface Medium {
  form: string
  produced: string
  toolchain?: string
  artifact?: string
}

export interface BuildSpec {
  run?: string
  cwd?: string
  produces?: string[]
  lastRun?: { at?: string; exitCode?: number; evidence?: string; [k: string]: unknown } | null
  [k: string]: unknown
}

export interface Deliverable {
  root: string
  files: DelivFileInfo[]
  // The recorded medium and build (docs/consumers/gen.md#the-medium-is-decided-once-before-anything-is-generated).
  medium?: Medium | null
  build?: BuildSpec | null
}

// One file under the out directory, as GET /api/out/list reports it.
export interface OutFileInfo {
  path: string
  size: number
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
