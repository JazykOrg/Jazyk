import { describe, expect, it } from 'vitest'
import { toolWords, workingLine } from './working'

describe('toolWords', () => {
  it('names the tool in words with what it is called on', () => {
    expect(toolWords('read_section', 'docs/backend.md')).toBe('Reading docs/backend.md…')
    expect(toolWords('search', 'Customer')).toBe('Searching Customer…')
    expect(toolWords('load', 'ent:shop')).toBe('Loading ent:shop…')
    expect(toolWords('done')).toBe('Finishing…')
  })

  it('falls back to the name', () => {
    expect(toolWords('frobnicate')).toBe('frobnicate…')
    expect(toolWords('')).toBe('Working…')
  })
})

describe('workingLine', () => {
  it('is the pending tool, else thinking', () => {
    expect(workingLine([])).toBe('Thinking…')
    expect(workingLine([{ kind: 'user', text: 'hi' }])).toBe('Thinking…')
    expect(workingLine([{ kind: 'tool', text: 'search Customer', status: 'pending' }])).toBe('Searching Customer…')
    expect(workingLine([{ kind: 'tool', text: 'search Customer', status: 'ok' }])).toBe('Thinking…')
  })

  it('gives way to reasoning as it streams', () => {
    expect(workingLine([{ kind: 'user', text: 'hi' }, { kind: 'thought', text: 'Let me look' }])).toBeNull()
  })
})
