// The capability descriptor the app renders against
// (docs/frontends/gui.md#backends-and-capabilities). The server answers with
// everything it serves; a demo or browser backend answers with what it can do. A
// server from before the descriptor existed answers 404, which reads as the full
// server set.
import { useQuery } from '@tanstack/react-query'
import { get } from './api'
import type { ModelTier } from './webllm'

export type JobKind = 'compile' | 'gen' | 'verify' | 'audit' | 'decompile' | 'benchmark'

export interface Capabilities {
  backend: 'server' | 'demo' | 'wasm'
  projects: 'list' | 'bundled' | 'folder-picker'
  docs: 'read' | 'write'
  events: 'stream' | 'pushed' | 'refetch'
  lsp: boolean
  jobs: JobKind[]
  agent: 'embedded' | 'external' | 'none'
  model: 'configured' | 'in-page' | 'loadable' | 'needs-key' | 'none'
  // The loaded model's tier (docs/frontends/wasm.md#model-sources), null with no
  // model. An endpoint and the server's model count as build.
  modelTier?: ModelTier | null
  rendering: 'server' | 'none'
  persistence: 'disk' | 'folder' | 'browser' | 'memory'
  dev?: boolean
}

export const SERVER_CAPABILITIES: Capabilities = {
  backend: 'server',
  projects: 'list',
  docs: 'write',
  events: 'stream',
  lsp: true,
  jobs: ['compile', 'gen', 'verify', 'audit', 'decompile', 'benchmark'],
  agent: 'embedded',
  model: 'configured',
  modelTier: 'build',
  rendering: 'server',
  persistence: 'disk',
}

export function useCapabilities() {
  return useQuery({
    queryKey: ['capabilities'],
    queryFn: async () => {
      try {
        return await get<Capabilities>('/api/capabilities')
      } catch (e) {
        if ((e as { status?: number }).status === 404) return SERVER_CAPABILITIES
        throw e
      }
    },
    staleTime: Infinity,
    retry: 0,
  })
}

// A tab's builds and chat run the page's model: until it is ready, the Build sheet and
// the chat pane show the model chooser (docs/frontends/wasm.md#model-sources).
export function needsPageModel(caps: Capabilities | undefined): boolean {
  return !!caps && caps.backend !== 'server' && caps.model !== 'in-page'
}

// Whether a tier builds: the build tier well, the chat tier with rough results
// (docs/frontends/wasm.md#model-sources).
const builds = (t: Capabilities['modelTier']) => t === 'build' || t === 'chat'

// The jobs a tab runs with its model's tier, null with no model ready: a build on a
// model that builds, nothing else, since a tab has no shell.
export function tabJobs(tier: ModelTier | null): JobKind[] {
  return builds(tier) ? ['compile'] : []
}

// A build needs a model of the build or chat tier: with none, or a reader, the Build
// sheet shows the chooser in place of the run (docs/frontends/gui.md#the-action-button).
// Chat runs on any tier.
export function needsBuildModel(caps: Capabilities | undefined): boolean {
  return needsPageModel(caps) || (!!caps && caps.backend !== 'server' && !builds(caps.modelTier))
}

// The loaded model is of the reader tier: it answers questions and does not build.
export function belowBuildTier(caps: Capabilities | undefined): boolean {
  return !!caps && caps.model === 'in-page' && !!caps.modelTier && !builds(caps.modelTier)
}

// The loaded model builds with rough results: the sheet runs it and says so, with the
// chooser a click away.
export function smallBuildModel(caps: Capabilities | undefined): boolean {
  return !!caps && caps.backend !== 'server' && caps.model === 'in-page' && caps.modelTier === 'chat'
}

// Whether the backend runs a job kind: a page hides the action that would start it
// otherwise (docs/frontends/wasm.md#what-a-tab-does-not-do). Unknown yet reads as yes.
export function useCanRun(): (need: JobKind) => boolean {
  const caps = useCapabilities()
  return (need) => !caps.data || caps.data.jobs.includes(need)
}

// The sentence an empty state gives when a capability is missing.
export function whyNot(caps: Capabilities | undefined, need: JobKind | 'agent' | 'docs-write'): string | null {
  if (!caps) return null
  if (need === 'agent') {
    return caps.agent === 'none' ? 'This backend has no agent to talk to. Chat needs the server or an attached agent.' : null
  }
  if (need === 'docs-write') {
    return caps.docs === 'write' ? null : 'This backend reads the documents but cannot write them.'
  }
  // A tab that can load its model offers the load in the Build sheet.
  if (need === 'compile' && caps.model === 'loadable') return 'Build runs a model in this browser. Load it first.'
  if (need === 'compile' && belowBuildTier(caps)) return 'A build needs a model of 3B or more, or an endpoint.'
  if (!caps.jobs.includes(need)) {
    return `This backend cannot run ${need === 'compile' ? 'a build' : need === 'gen' ? 'a generate run' : need === 'verify' || need === 'audit' ? 'a review' : need === 'decompile' ? 'Document' : need}.`
  }
  if (need === 'compile' && caps.model === 'needs-key') return 'Build needs a model. Add an API key in Settings.'
  if (need === 'compile' && caps.model === 'none') return 'Build needs a model. Configure one in Settings.'
  return null
}
