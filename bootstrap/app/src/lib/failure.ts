// Why a batch did not land, in product words (docs/frontends/gui.md#chat): one line
// under the batch naming what was not read and why, never `nothing committed` alone.
// The why is the stop when it was hard (the model's error, a context too small, the
// round budget, a cancel), else the last step a gate refused, else the model's last
// words, else the plain stop. The build thread, the files tree, and the editor's
// gutter all say it the same way.
import type { TraceEvent } from './api'

export interface Refusal {
  rule: string
  message: string
}

// What a session left on the trace that can explain its end: the last refused call
// and the model's last words.
export interface Clues {
  refusal: Refusal | null
  text: string | null
}

export interface GoalRef {
  kind?: string
  target?: string
}

export const noClues = (): Clues => ({ refusal: null, text: null })

// One event folded into the clues of its label. A new attempt starts over.
export function noteClue(c: Clues, ev: TraceEvent): Clues {
  switch (ev.kind) {
    case 'batchStart':
    case 'sessionStart':
      return noClues()
    case 'toolError':
      return { ...c, refusal: { rule: String(ev.rule ?? ''), message: String(ev.message ?? '') } }
    case 'modelText': {
      const text = String(ev.text ?? '').trim()
      return text ? { ...c, text } : c
    }
    default:
      return c
  }
}

// One line, whitespace collapsed, cut at `n` characters.
export function oneLine(s: string, n = 140): string {
  const flat = s.replace(/\s+/g, ' ').trim()
  return flat.length > n ? `${flat.slice(0, n - 1).trimEnd()}…` : flat
}

// A refusal's message: a jazyk refusal carries `{"error": {"rule", "message"}}`,
// which a native trace may keep whole.
export function refusalMessage(message: string): string {
  try {
    const v = JSON.parse(message) as { error?: { message?: unknown } | string }
    if (v && typeof v === 'object') {
      if (typeof v.error === 'string') return v.error
      if (v.error && typeof v.error.message === 'string') return v.error.message
    }
  } catch {
    // plain text
  }
  return message.replace(/^"|"$/g, '')
}

const has = (s: string, words: string[]) => words.some((w) => s.includes(w))

type StopKind = 'cancelled' | 'context' | 'limited' | 'rounds' | 'quiet' | 'error' | 'stopped' | 'other'

function stopKind(stop: string): StopKind {
  const s = stop.toLowerCase()
  if (!s.trim()) return 'stopped'
  if (has(s, ['cancel'])) return 'cancelled'
  if (has(s, ['too large', 'too long', 'context length', 'maximum context', 'exceeds the context', 'context window'])) return 'context'
  if (has(s, ['rate limit', 'rate_limit', '429', 'quota'])) return 'limited'
  if (has(s, ['ran out of rounds', 'max_turn_requests', 'round budget'])) return 'rounds'
  if (has(s, ['idle watchdog'])) return 'quiet'
  if (has(s, ['ended its turn', 'end_turn', 'did not land', 'nothing committed', 'without committing', 'batch still open'])) return 'stopped'
  if (has(s, ['model call failed', 'error', 'http ', 'failed', 'refused', 'endpoint'])) return 'error'
  return 'other'
}

// The runner's stop in words.
export function stopWords(stop: string): string {
  switch (stopKind(stop)) {
    case 'cancelled':
      return 'the build was cancelled'
    case 'context':
      return "the model's context is too small for it"
    case 'limited':
      return 'the model source is rate limited or out of quota'
    case 'rounds':
      return 'the model used up its rounds without finishing'
    case 'quiet':
      return 'the model went quiet'
    case 'stopped':
      return 'the model stopped without finishing'
    case 'error':
      return `the model failed (${oneLine(stop.replace(/^the model call failed:\s*/i, ''), 120)})`
    default:
      return oneLine(stop, 120)
  }
}

// A stop that explains itself: nothing the model said before it matters more.
function hardStop(stop: string): boolean {
  const k = stopKind(stop)
  return k !== 'stopped' && k !== 'other'
}

// Why a session did not land.
export function failureWhy(clues: Clues, stop: string): string {
  if (hardStop(stop)) return stopWords(stop)
  if (clues.refusal) {
    const msg = oneLine(refusalMessage(clues.refusal.message), 140)
    return `a step was refused (${clues.refusal.rule}${msg ? `: ${msg}` : ''})`
  }
  if (clues.text) return `the model said “${oneLine(clues.text, 120)}” and stopped`
  return stopWords(stop)
}

// A section ref's last part as a title: `/intro/hello-world` → `Hello world`.
export function sectionTitle(ref: string): string {
  const last = ref.split('/').filter(Boolean).pop() ?? ref
  const words = decodeURIComponent(last).replace(/[-_]+/g, ' ').trim()
  return words ? words.charAt(0).toUpperCase() + words.slice(1) : ref
}

// What a goal works on, in words: `docs/README.md › Hello world`, `docs/README.md`,
// or an entity's name.
export function targetWords(target: string): string {
  const at = target.indexOf('#')
  if (at >= 0) return `${target.slice(0, at)} › ${sectionTitle(target.slice(at + 1))}`
  return target.replace(/^(ent|req|view):/, '')
}

// A goal id's kind and target: `g:<kind>:<target>`.
export function parseGoalId(id: string): GoalRef {
  const m = /^g:([^:]+):(.*)$/.exec(id)
  return m ? { kind: m[1], target: m[2] } : { target: id }
}

// What a batch works on, in words.
export function batchSubject(goals: GoalRef[]): string {
  const sections = goals.filter((g) => g.kind === 'reconcile-section' && (g.target ?? '').includes('#'))
  if (sections.length === 1) return targetWords(sections[0].target ?? '')
  if (sections.length > 1) {
    const t = sections[0].target ?? ''
    return `${t.slice(0, t.indexOf('#'))} (${sections.length} sections)`
  }
  const first = goals[0]
  return first ? targetWords(first.target ?? '') : 'The batch'
}

function notDone(kind: string | undefined): string {
  switch (kind) {
    case 'reconcile-section':
      return 'was not read'
    case 'place-anchors':
      return 'was not re-anchored'
    case 'review-entity':
      return 'was not reviewed'
    case 'generate':
      return 'was not generated'
    case 'verify':
      return 'was not checked'
    default:
      return 'was not finished'
  }
}

// The line under a batch that did not land. The first failure is retried in the
// same build; after that the goal waits for the next one.
export function failureLine(goals: GoalRef[], clues: Clues, stop: string, attempt?: number): string {
  const subject = batchSubject(goals)
  const why = failureWhy(clues, stop)
  const next = attempt === 1 ? 'It will be retried.' : 'It will be retried on the next build.'
  return `${subject} ${notDone(goals[0]?.kind)}: ${why}. ${next}`
}

// A goal left for later, in words. `why` is what its batch's failure said, when
// the build saw one; the reconciler's own reason stands in otherwise.
export function parkedLine(goalId: string, reason: string, why: string | null): string {
  const g = parseGoalId(goalId)
  const because = why ?? (/left open after its retry/.test(reason) ? 'it was not finished after a retry' : stopWords(reason))
  return `${targetWords(g.target ?? goalId)} left for later: ${because}`
}
