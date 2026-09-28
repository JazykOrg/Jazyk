import { describe, expect, it } from 'vitest'
import { chatRows, followRows } from './transcript'

const GOAL = 'g:reconcile-section:docs/README.md#/hello-world'

// The build the owner watched on a weak model: two attempts on one section, each
// ending with nothing landed, then the goal left for later.
function weakBuild(withClues: boolean) {
  const events: Record<string, unknown>[] = []
  for (const attempt of [1, 2]) {
    events.push({ kind: 'batchStart', label: 'b2-1', goals: [{ kind: 'reconcile-section', target: 'docs/README.md#/hello-world' }] })
    events.push({ kind: 'sessionStart', label: 'b2-1', goals: [GOAL] })
    if (withClues && attempt === 1) events.push({ kind: 'modelText', label: 'b2-1', text: 'The section says hello.' })
    if (withClues && attempt === 2) {
      events.push({ kind: 'toolCall', label: 'b2-1', name: 'upsert_requirement', summary: '{}' })
      events.push({ kind: 'toolError', label: 'b2-1', rule: 'quote-not-found', message: 'the quote is not in the section' })
    }
    events.push({ kind: 'sessionFailed', label: 'b2-1', goals: [GOAL], attempt, error: 'the model ended its turn with the batch still open' })
  }
  events.push({ kind: 'goal', label: 'b2-1', goal: GOAL, event: 'parked', reason: 'left open after its retry in session b2-1' })
  return events.map((event, seq) => ({ jobId: 1, seq, event }))
}

describe('followRows', () => {
  it('says why a batch did not land, never nothing committed', () => {
    const rows = followRows(weakBuild(true), 1, false)
    const why = rows.filter((r) => r.kind === 'why').map((r) => r.text)
    expect(why).toEqual([
      '✗ docs/README.md › Hello world was not read: the model said “The section says hello.” and stopped. It will be retried.',
      '✗ docs/README.md › Hello world was not read: a step was refused (quote-not-found: the quote is not in the section). It will be retried on the next build.',
      '◈ docs/README.md › Hello world left for later: a step was refused (quote-not-found: the quote is not in the section)',
    ])
    const all = rows.map((r) => r.text).join('\n')
    expect(all).not.toContain('nothing committed')
    // Goal ids are Developer words.
    expect(all).not.toContain('g:reconcile-section')
    // The model's words show as reasoning in the default mode.
    expect(rows.filter((r) => r.kind === 'thought').map((r) => r.text)).toEqual(['The section says hello.'])
  })

  it('falls back to the stop in words', () => {
    const rows = followRows(weakBuild(false), 1, false)
    expect(rows.filter((r) => r.kind === 'why')[0].text).toBe(
      '✗ docs/README.md › Hello world was not read: the model stopped without finishing. It will be retried.',
    )
  })

  it('keeps the ids in Developer mode', () => {
    const rows = followRows(weakBuild(true), 1, true)
    const all = rows.map((r) => r.text).join('\n')
    expect(all).toContain(`parked ${GOAL}`)
    expect(rows.find((r) => r.kind === 'why')?.detail).toContain('b2-1')
  })
})

describe('chatRows', () => {
  it('keeps reasoning as its own rows beside the answer', () => {
    const rows = chatRows([
      { n: 0, update: { sessionUpdate: 'user_message', text: 'hi' } },
      { n: 1, update: { sessionUpdate: 'agent_thought_chunk', content: { type: 'text', text: 'Let me look.' } } },
      { n: 2, update: { sessionUpdate: 'agent_message_chunk', content: { type: 'text', text: 'Hello.' } } },
    ])
    expect(rows.map((r) => r.kind)).toEqual(['user', 'thought', 'agent'])
  })
})
