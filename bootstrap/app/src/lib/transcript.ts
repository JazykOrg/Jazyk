// The chat column's transcript rows: a chat session's updates and a build thread's
// trace both normalize onto one row model the pane renders
// (docs/frontends/gui.md#chat).
import type { ChatUpdateRow } from './store'
import { failureLine, failureWhy, noClues, noteClue, parkedLine, parseGoalId, targetWords, type Clues, type GoalRef } from './failure'

// ---- transcript row model: both session kinds normalize onto this ----

export interface Row {
  key: string
  // why: the line under a batch that did not land, saying why.
  kind: 'user' | 'agent' | 'thought' | 'tool' | 'marker' | 'why'
  text: string
  // Tool rows: pending | ok | failed.
  status?: string
  detail?: string
}

// Chat sessions: coalesce streamed chunks, one row per tool call.
// A tool payload one field per line (docs/compiler/tools.md): a string that already
// holds pretty JSON stays as it is, anything else is pretty-printed.
function pretty(v: unknown): string {
  if (typeof v === 'string') return v
  try {
    return JSON.stringify(v, null, 2)
  } catch {
    return String(v)
  }
}

export function chatRows(updates: ChatUpdateRow[]): Row[] {
  const rows: Row[] = []
  const toolRow: Record<string, number> = {}
  const push = (kind: Row['kind'], text: string, key: string) => rows.push({ key, kind, text })
  const appendText = (kind: 'agent' | 'thought', text: string, key: string) => {
    const last = rows[rows.length - 1]
    if (last && last.kind === kind) last.text += text
    else push(kind, text, key)
  }
  for (const { n, update } of updates) {
    const u = update as Record<string, never> & Record<string, unknown>
    const k = String(u.sessionUpdate ?? '')
    const content = (u.content ?? {}) as { type?: string; text?: string }
    switch (k) {
      case 'user_message':
        push('user', String(u.text ?? ''), `u${n}`)
        break
      case 'agent_message':
        push('agent', String(u.text ?? ''), `a${n}`)
        break
      case 'agent_message_chunk':
        appendText('agent', content.text ?? '', `a${n}`)
        break
      case 'agent_thought_chunk':
        appendText('thought', content.text ?? '', `t${n}`)
        break
      case 'tool_call': {
        const id = String(u.toolCallId ?? n)
        toolRow[id] = rows.length
        rows.push({
          key: `c${n}`,
          kind: 'tool',
          text: String(u.title ?? 'tool'),
          status: 'pending',
          detail: u.rawInput !== undefined ? pretty(u.rawInput) : undefined,
        })
        break
      }
      case 'tool_call_update': {
        const id = String(u.toolCallId ?? '')
        const fields = (u.fields ?? u) as { status?: string; rawOutput?: unknown; title?: string }
        const at = toolRow[id]
        if (at !== undefined && rows[at]) {
          const status = fields.status === 'failed' ? 'failed' : fields.status === 'completed' ? 'ok' : 'pending'
          rows[at] = {
            ...rows[at],
            status,
            detail: fields.rawOutput !== undefined ? pretty(fields.rawOutput) : rows[at].detail,
          }
        }
        break
      }
      case 'plan': {
        const entries = (u.entries as { content?: string; status?: string }[]) ?? []
        push('marker', entries.map((e) => `[${e.status}] ${e.content}`).join('\n'), `p${n}`)
        break
      }
      case 'turn_end':
        push('marker', u.error ? `turn ended: ${u.error}` : `· ${String(u.stop ?? 'end')} ·`, `e${n}`)
        break
      default:
        break
    }
  }
  return rows
}

// A batch in plain words: what the session does and to what
// (docs/frontends/gui.md#chat). The Developer toggle keeps the goal ids beside it.
export function batchWords(goals: { kind?: string; target?: string }[], dev: boolean): string {
  if (goals.length === 0) return 'working'
  const first = goals[0]
  const kind = first.kind ?? ''
  const target = first.target ?? ''
  const doc = target.includes('#') ? target.slice(0, target.indexOf('#')) : target
  const name = (t: string) => t.replace(/^(ent|req|view):/, '')
  let text: string
  switch (kind) {
    case 'reconcile-section':
      text = `Reading ${doc}${goals.length > 1 ? `, ${goals.length} sections` : ''}`
      break
    case 'place-anchors':
      text = `Re-anchoring ${doc}`
      break
    case 'rejudge-pair':
      text = `Comparing ${goals.length} pair${goals.length === 1 ? '' : 's'} of requirements`
      break
    case 'review-entity':
      text = `Reviewing ${name(target)}${goals.length > 1 ? ` and ${goals.length - 1} more` : ''}`
      break
    case 'retrace':
      text = `Re-tracing ${name(target)}`
      break
    case 'conform-instance':
      text = `Checking ${name(target)} against its type`
      break
    case 'bind':
      text = `Binding ${goals.length} requirement${goals.length === 1 ? '' : 's'} to the product`
      break
    case 'generate':
      text = `Generating ${name(target)}${goals.length > 1 ? ` and ${goals.length - 1} more` : ''}`
      break
    case 'verify':
      text = `Checking ${goals.length} requirement${goals.length === 1 ? '' : 's'}`
      break
    case 'declare-edges':
    case 'dedupe-candidates':
    case 'curate-view':
    case 'split-view':
    case 'abstract-entity':
      text = `Tidying the model: ${kind.replace('-', ' ')} on ${name(target)}`
      break
    default:
      text = `${kind} ${target}`
  }
  return dev ? `${text} (${goals.map((g) => `${g.kind} ${g.target}`).join(' · ')})` : text
}

// Follow entries: the running build's trace rows, filtered by job.
export function followRows(trace: { jobId: number; seq: number; event: Record<string, unknown> }[], jobId: number, dev: boolean): Row[] {
  const rows: Row[] = []
  // Per session label: the batch's goals, what explains an end, and the last
  // failure's why, so a failed batch and a goal left for later say why in words
  // (docs/frontends/gui.md#chat).
  const goalsOf = new Map<string, GoalRef[]>()
  const cluesOf = new Map<string, Clues>()
  const whyOf = new Map<string, string>()
  for (const { jobId: j, seq, event } of trace) {
    if (j !== jobId) continue
    const kind = String(event.kind ?? '')
    const label = String(event.label ?? '')
    cluesOf.set(label, noteClue(cluesOf.get(label) ?? noClues(), event as never))
    switch (kind) {
      case 'batchStart': {
        const goals = Array.isArray(event.goals)
          ? (event.goals as { kind?: string; target?: string }[])
          : []
        goalsOf.set(label, goals)
        whyOf.delete(label)
        rows.push({
          key: `b${seq}`,
          kind: 'marker',
          text: `▷ ${batchWords(goals, dev)}${dev ? ` [${label}]` : ''}`,
        })
        break
      }
      case 'sessionStart':
        if (dev) rows.push({ key: `s${seq}`, kind: 'marker', text: `▶ ${label}` })
        break
      case 'goal': {
        const goal = String(event.goal ?? '')
        const state = String(event.event ?? '')
        const tail = String(event.justification ?? event.reason ?? '')
        // Goal ids are Developer words; the default mode names what the goal is on.
        if (dev) {
          rows.push({
            key: `g${seq}`,
            kind: 'marker',
            text: `${state === 'resolved' ? '✓' : '◈'} ${state} ${goal}${tail ? `: ${tail}` : ''}`,
          })
        } else if (state === 'parked') {
          rows.push({ key: `g${seq}`, kind: 'why', text: `◈ ${parkedLine(goal, tail, whyOf.get(label) ?? null)}` })
        } else if (state === 'resolved' || state === 'failed') {
          const on = targetWords(parseGoalId(goal).target ?? goal)
          rows.push({
            key: `g${seq}`,
            kind: 'marker',
            text: state === 'resolved' ? `✓ ${on}${tail ? `: ${tail}` : ''}` : `✗ ${on} failed${tail ? `: ${tail}` : ''}`,
          })
        }
        break
      }
      case 'modelText':
        // The model's reasoning and prose, as it streams (docs/frontends/gui.md#chat).
        rows.push({ key: `m${seq}`, kind: 'thought', text: String(event.text ?? '') })
        break
      case 'toolCall':
        rows.push({
          key: `c${seq}`,
          kind: 'tool',
          text: `${label} → ${String(event.name ?? '')}`,
          status: 'pending',
          detail: String(event.summary ?? ''),
        })
        break
      case 'toolResult': {
        // The result closes the last pending tool row of the same label.
        for (let i = rows.length - 1; i >= 0; i--) {
          if (rows[i].kind === 'tool' && rows[i].status === 'pending' && rows[i].text.startsWith(label)) {
            rows[i] = { ...rows[i], status: 'ok', detail: String(event.summary ?? '') }
            break
          }
        }
        break
      }
      case 'toolError':
        rows.push({
          key: `x${seq}`,
          kind: 'tool',
          text: `${label} ✗ ${String(event.rule ?? '')}`,
          status: 'failed',
          detail: String(event.message ?? ''),
        })
        break
      case 'genEntityStart':
        rows.push({ key: `ge${seq}`, kind: 'marker', text: `▷ Generating ${String(event.entity ?? '').replace(/^ent:/, '')}${event.stage ? ` (${String(event.stage)})` : ''}` })
        break
      case 'genEntityDone':
        rows.push({
          key: `gd${seq}`,
          kind: 'marker',
          text: `✓ Generated ${String(event.entity ?? '').replace(/^ent:/, '')}${Array.isArray(event.files) ? `: ${(event.files as string[]).join(', ')}` : ''}`,
        })
        break
      case 'genEntitySkipped':
        rows.push({ key: `gs${seq}`, kind: 'marker', text: `· Skipped ${String(event.entity ?? '').replace(/^ent:/, '')}${event.reason ? `: ${String(event.reason)}` : ''}` })
        break
      case 'genEntityFailed':
        rows.push({ key: `gf${seq}`, kind: 'marker', text: `✗ ${String(event.entity ?? '').replace(/^ent:/, '')}: ${String(event.error ?? event.reason ?? 'failed')}` })
        break
      case 'verifyRowStart':
        rows.push({ key: `vs${seq}`, kind: 'tool', text: `Checking ${String(event.requirement ?? '')}`, status: 'pending', detail: String(event.test ?? '') })
        break
      case 'verifyRowDone': {
        const rid = String(event.requirement ?? '')
        const pass = String(event.verdict ?? '') === 'pass'
        let closed = false
        for (let i = rows.length - 1; i >= 0; i--) {
          if (rows[i].kind === 'tool' && rows[i].status === 'pending' && rows[i].text === `Checking ${rid}`) {
            rows[i] = { ...rows[i], text: `${pass ? 'Verified' : 'Failing'} ${rid}`, status: pass ? 'ok' : 'failed', detail: String(event.evidence ?? event.run ?? '') }
            closed = true
            break
          }
        }
        if (!closed) rows.push({ key: `vd${seq}`, kind: 'tool', text: `${pass ? 'Verified' : 'Failing'} ${rid}`, status: pass ? 'ok' : 'failed', detail: String(event.evidence ?? '') })
        break
      }
      case 'verifyRowStale':
        rows.push({ key: `vt${seq}`, kind: 'tool', text: `Stale ${String(event.requirement ?? '')}`, status: 'pending', detail: `${String(event.status ?? '')}: ${String(event.reason ?? '')}` })
        break
      case 'verifyRowError':
        rows.push({ key: `ve${seq}`, kind: 'tool', text: `Could not check ${String(event.requirement ?? '')}`, status: 'failed', detail: String(event.error ?? event.reason ?? '') })
        break
      case 'sessionDone':
        rows.push({ key: `d${seq}`, kind: 'marker', text: dev ? `✓ ${label}` : `✓ done${event.summary ? `: ${String(event.summary)}` : ''}` })
        break
      case 'sessionFailed':
      case 'sessionEnded': {
        const stop = String(event.error ?? event.reason ?? '')
        // A ledger batch returned for its ledger to judge is no failure of its own.
        if (kind === 'sessionEnded' && !/cancel/i.test(stop)) {
          if (dev) rows.push({ key: `f${seq}`, kind: 'marker', text: `■ ${label}: ${stop}` })
          break
        }
        const clues = cluesOf.get(label) ?? noClues()
        whyOf.set(label, failureWhy(clues, stop))
        const goals = goalsOf.get(label) ?? (Array.isArray(event.goals) ? (event.goals as string[]).map(parseGoalId) : [])
        const attempt = typeof event.attempt === 'number' ? event.attempt : undefined
        rows.push({
          key: `f${seq}`,
          kind: 'why',
          text: `✗ ${failureLine(goals, clues, stop, attempt)}`,
          detail: dev ? `${label}: ${stop}` : undefined,
        })
        break
      }
      default:
        break
    }
  }
  return rows.slice(-500)
}
