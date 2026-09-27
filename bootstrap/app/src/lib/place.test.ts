import { describe, expect, it } from 'vitest'
import type { Capabilities } from './capabilities'
import { unwrittenCount, whereChangesGo } from './place'

const caps = (over: Partial<Capabilities>): Capabilities => ({
  backend: 'demo',
  projects: 'bundled',
  docs: 'write',
  events: 'pushed',
  lsp: false,
  jobs: [],
  agent: 'embedded',
  model: 'none',
  rendering: 'none',
  persistence: 'browser',
  ...over,
})

const folder = (mode: 'read' | 'write', held = 0, error: string | null = null) => ({
  name: 'proj',
  mode,
  held,
  error,
  writing: false,
  storeError: null,
})

const stored = (kind: 'demo' | 'browser', available = true, error: string | null = null, unsaved = 0) => ({
  id: kind === 'demo' ? 'demo:shop' : 'browser:1',
  kind,
  name: 'The shop',
  edited: false,
  error,
  unsaved,
  available,
})

describe('whereChangesGo', () => {
  it('says the exact phrase per backend and persistence', () => {
    expect(whereChangesGo(caps({ backend: 'server', persistence: 'disk' }), null, null)).toBe('Saved on this computer by jazyk')
    expect(whereChangesGo(caps({ backend: 'wasm', persistence: 'folder' }), folder('write'), null)).toBe('Saved to proj (writing)')
    expect(whereChangesGo(caps({ backend: 'wasm', persistence: 'folder' }), folder('read'), null)).toBe('Saved to proj (read-only)')
    expect(whereChangesGo(caps({ backend: 'wasm', persistence: 'browser' }), null, stored('browser'))).toBe('Saved in this browser only')
    expect(whereChangesGo(caps({ backend: 'demo', persistence: 'browser' }), null, stored('demo'))).toBe('Demo: edits are saved in this browser')
    expect(whereChangesGo(caps({ backend: 'demo', persistence: 'memory' }), null, stored('demo', false))).toBe('Demo: changes stay in this tab')
  })

  it('falls back to the descriptor before the project state is published', () => {
    expect(whereChangesGo(caps({ backend: 'demo', persistence: 'browser' }), null, null)).toBe('Demo: edits are saved in this browser')
    expect(whereChangesGo(caps({ backend: 'demo', persistence: 'memory' }), null, null)).toBe('Demo: changes stay in this tab')
    expect(whereChangesGo(caps({ backend: 'wasm', persistence: 'browser' }), null, null)).toBe('Saved in this browser only')
    expect(whereChangesGo(undefined, null, null)).toBe('')
  })
})

describe('unwrittenCount', () => {
  it('counts a writing folder’s kept changes only when a write failed', () => {
    expect(unwrittenCount(folder('write', 3, 'the disk refused'), null)).toBe(3)
    expect(unwrittenCount(folder('write', 3), null)).toBe(0)
    expect(unwrittenCount(folder('read', 3), null)).toBe(0)
  })

  it('counts a stored project’s refused writes', () => {
    expect(unwrittenCount(null, stored('browser', true, 'full', 2))).toBe(2)
    expect(unwrittenCount(null, stored('browser'))).toBe(0)
    expect(unwrittenCount(null, null)).toBe(0)
  })
})
