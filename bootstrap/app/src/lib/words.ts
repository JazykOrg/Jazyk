// The vocabulary: product words in the default mode, the compiler's words beside
// them under the Developer toggle (docs/frontends/gui.md#vocabulary). One module,
// so a label is never written twice.
import type { Verdict } from './api'
import { useApp } from './store'

export interface Words {
  dev: boolean
  sync: string
  build: string
  check: string
  document: string
  syncing: string
  building: string
  checking: string
  upToDate: string
  syncOnSave: string
  syncOnRequest: string
  buildAfterSync: string
  buildOnRequest: string
  pendingChanges: string
  waitingOnYou: string
  suggestions: string
  problem: string
  problems: string
  question: string
  questions: string
  proposal: string
  proposals: string
  drafts: string
  seen: string
  hidden: string
  wontFix: string
  read: string
  informational: string
  notReadYet: string
  quoted: string
  inferred: string
  yourEdit: string
  diagram: string
  diagrams: string
  relation: string
  relations: string
  states: string
  product: string
  productForm: string
  trace: string
  notDescribed: string
  history: string
  syncNumber: (n: number) => string
  whatThisCaused: string
  // A label with the compiler word beside it in Developer mode.
  both: (product: string, compiler: string) => string
}

function make(dev: boolean): Words {
  const both = (product: string, compiler: string) => (dev ? `${product} (${compiler})` : product)
  return {
    dev,
    sync: both('Sync', 'compile'),
    build: both('Build', 'generate'),
    check: both('Check', 'verify'),
    document: both('Document', 'decompile'),
    syncing: both('Syncing', 'compiling'),
    building: both('Building', 'generating'),
    checking: both('Checking', 'verifying'),
    upToDate: both('Up to date', 'converged'),
    syncOnSave: both('Sync on save', 'compile: auto'),
    syncOnRequest: both('Sync on request', 'compile: manual'),
    buildAfterSync: both('Build after sync', 'gen: auto'),
    buildOnRequest: both('Build on request', 'gen: manual'),
    pendingChanges: both('Pending changes', 'awaiting release'),
    waitingOnYou: both('Waiting on you', 'blocked'),
    suggestions: both('Suggestions', 'optional'),
    problem: both('Problem', 'diagnostic'),
    problems: both('Problems', 'diagnostics'),
    question: both('Question', 'prompted diagnostic'),
    questions: both('Questions', 'prompted diagnostics'),
    proposal: both('Proposal', 'ratification'),
    proposals: both('Proposals', 'ratification'),
    drafts: both('Drafts', 'unratified'),
    seen: both('Seen', 'acknowledged'),
    hidden: both('Hidden', 'suppressed'),
    wontFix: both("Won't fix", 'wontfix'),
    read: both('Read', 'covered'),
    informational: both('Informational', 'non-normative'),
    notReadYet: both('Not read yet', 'unprocessed'),
    quoted: both('Quoted', 'quote'),
    inferred: both('Inferred', 'derived'),
    yourEdit: both('Your edit', 'decree'),
    diagram: both('Diagram', 'view'),
    diagrams: both('Diagrams', 'views'),
    relation: both('Relation', 'relationship'),
    relations: both('Relations', 'relationships'),
    states: both('States', 'state machine'),
    product: both('Product', 'deliverable'),
    productForm: both('Product form', 'medium'),
    trace: both('Trace', 'verification matrix'),
    notDescribed: both('Not described', 'unclaimed'),
    history: both('History', 'journal'),
    syncNumber: (n) => (dev ? `Sync #${n} (g${n})` : `Sync #${n}`),
    whatThisCaused: both('What this caused', 'ripple'),
    both,
  }
}

const USER = make(false)
const DEV = make(true)

export function words(dev: boolean): Words {
  return dev ? DEV : USER
}

export function useWords(): Words {
  const dev = useApp((a) => a.dev)
  return words(dev)
}

// The verdict in product words; the compiler's line beside it in Developer mode.
export function verdictWords(v: Verdict | undefined | null, dev: boolean): string {
  if (!v || !v.state) return 'Not synced yet'
  const w = words(dev)
  const compiler = compilerVerdict(v)
  // The blocked count mixes questions, proposals, and gated work; the Inbox is
  // where "waiting on you" is counted, so the state line leaves it out.
  if (v.state === 'converged') {
    let s = w.upToDate.split(' (')[0]
    if (v.optional) s += `, ${v.optional} suggestion${v.optional === 1 ? '' : 's'}`
    return dev ? `${s} (${compiler})` : s
  }
  const left = (v.open ?? 0) + (v.failed ?? 0)
  let s = `Sync incomplete: ${left} item${left === 1 ? '' : 's'} left`
  if (v.failed) s += `, ${v.failed} failed`
  return dev ? `${s} (${compiler})` : s
}

export function compilerVerdict(v: Verdict): string {
  if (v.state === 'converged') {
    let s = 'converged'
    if (v.blocked) s += `, ${v.blocked} blocked`
    if (v.optional) s += `, ${v.optional} optional advised`
    return s
  }
  return `${v.state}: ${v.open ?? 0} open, ${v.failed ?? 0} failed, ${v.blocked ?? 0} blocked, ${v.optional ?? 0} optional advised`
}

// A project shape from GET /api/projects, in words.
export function shapeWords(shape: string | undefined): string {
  switch (shape) {
    case 'docs-and-product':
      return 'documents and product'
    case 'docs-only':
      return 'documents only'
    case 'product-only':
      return 'product only'
    case 'empty':
      return 'empty'
    default:
      return ''
  }
}

// The triage states in words.
export function triageWords(dev: boolean): { value: string; label: string }[] {
  const w = words(dev)
  return [
    { value: 'acknowledged', label: w.seen },
    { value: 'suppressed', label: w.hidden },
    { value: 'wontfix', label: w.wontFix },
  ]
}

// A coverage state in words.
export function coverageWord(state: string | undefined, dev: boolean): string {
  const w = words(dev)
  if (state === 'covered') return w.read
  if (state === 'non-normative') return w.informational
  return w.notReadYet
}

// A provenance kind in words.
export function provenanceWord(kind: 'quote' | 'derived' | 'decree' | string, dev: boolean): string {
  const w = words(dev)
  if (kind === 'quote') return w.quoted
  if (kind === 'derived') return w.inferred
  if (kind === 'decree') return w.yourEdit
  return kind
}
