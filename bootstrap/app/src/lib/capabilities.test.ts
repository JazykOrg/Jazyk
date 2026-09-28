import { describe, expect, it } from 'vitest'
import { belowBuildTier, needsBuildModel, SERVER_CAPABILITIES, smallBuildModel, tabJobs, whyNot, type Capabilities } from './capabilities'

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

describe('the Build sheet', () => {
  it('runs a build-tier model plainly', () => {
    const caps = tab('in-page', 'build')
    expect(needsBuildModel(caps)).toBe(false)
    expect(smallBuildModel(caps)).toBe(false)
    expect(whyNot(caps, 'compile')).toBeNull()
  })

  it('runs a chat-tier model and says it is small', () => {
    const caps = tab('in-page', 'chat')
    expect(needsBuildModel(caps)).toBe(false)
    expect(belowBuildTier(caps)).toBe(false)
    expect(smallBuildModel(caps)).toBe(true)
    expect(whyNot(caps, 'compile')).toBeNull()
  })

  it('shows the chooser for a reader-tier model', () => {
    const caps = tab('in-page', 'reader')
    expect(needsBuildModel(caps)).toBe(true)
    expect(belowBuildTier(caps)).toBe(true)
    expect(smallBuildModel(caps)).toBe(false)
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
    expect(smallBuildModel({ ...SERVER_CAPABILITIES, modelTier: 'chat' })).toBe(false)
  })
})
