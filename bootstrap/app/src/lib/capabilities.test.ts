import { describe, expect, it } from 'vitest'
import { belowBuildTier, needsBuildModel, SERVER_CAPABILITIES, tabJobs, whyNot, type Capabilities } from './capabilities'

// A tab with its model in the given state (docs/frontends/wasm.md#model-sources).
function tab(model: Capabilities['model'], modelTier: Capabilities['modelTier']): Capabilities {
  return { ...SERVER_CAPABILITIES, backend: 'demo', model, modelTier, jobs: model === 'in-page' ? tabJobs(modelTier ?? null) : [] }
}

describe('tabJobs', () => {
  it('builds on a model of the build or chat tier, and on nothing else', () => {
    expect(tabJobs('build')).toEqual(['compile'])
    expect(tabJobs('chat')).toEqual(['compile'])
    expect(tabJobs('reader')).toEqual([])
    expect(tabJobs(null)).toEqual([])
  })
})

describe('the action button', () => {
  it('runs a build-tier model plainly', () => {
    const caps = tab('in-page', 'build')
    expect(needsBuildModel(caps)).toBe(false)
    expect(whyNot(caps, 'compile')).toBeNull()
  })

  it('runs a chat-tier model', () => {
    const caps = tab('in-page', 'chat')
    expect(needsBuildModel(caps)).toBe(false)
    expect(belowBuildTier(caps)).toBe(false)
    expect(whyNot(caps, 'compile')).toBeNull()
  })

  it('shows the chooser for a reader-tier model', () => {
    const caps = tab('in-page', 'reader')
    expect(needsBuildModel(caps)).toBe(true)
    expect(belowBuildTier(caps)).toBe(true)
    expect(whyNot(caps, 'compile')).toBe('A build needs a model of 3B or more, or an endpoint.')
  })

  it('shows the chooser with no model ready', () => {
    const caps = tab('loadable', null)
    expect(needsBuildModel(caps)).toBe(true)
    expect(belowBuildTier(caps)).toBe(false)
    expect(whyNot(caps, 'compile')).toBe('Build runs a model in this browser. Load it first.')
  })

  it('never shows it on a server', () => {
    expect(needsBuildModel(SERVER_CAPABILITIES)).toBe(false)
  })
})

describe('the work a build runs', () => {
  it('leaves the ledger kinds out', async () => {
    const { buildWork } = await import('../shell/ActionButton')
    expect(buildWork(undefined)).toBe(0)
    expect(buildWork({ open: 5 })).toBe(5)
    expect(buildWork({ open: 190, by_kind: { bind: 184, generate: 3, 'reconcile-section': 2, verify: 1 } })).toBe(2)
  })
})

describe('a diagram box link', () => {
  it('opens the entity for a card and the page otherwise', async () => {
    const { diagramLinkTarget } = await import('./levels')
    expect(diagramLinkTarget('../../docsgen/entities/quay.md')).toEqual({ kind: 'entity', id: 'ent:quay' })
    expect(diagramLinkTarget('../../docsgen/diagrams/class/funds.md#x')).toEqual({ kind: 'page', path: 'docsgen/diagrams/class/funds.md' })
    expect(diagramLinkTarget('https://example.com/docsgen/x.md')).toBeNull()
    expect(diagramLinkTarget('../other.md')).toBeNull()
  })
})
