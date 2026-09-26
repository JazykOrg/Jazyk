// The capability descriptor the app renders against
// (docs/frontends/gui.md#backends-and-capabilities). The server answers with
// everything it serves; a demo or browser backend answers with what it can do. A
// server from before the descriptor existed answers 404, which reads as the full
// server set.
import { useQuery } from '@tanstack/react-query'
import { get } from './api'

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

// A tab's syncs and chat run the page's model: until it is ready, the Sync sheet and
// the chat pane show the model chooser (docs/frontends/wasm.md#model-sources).
export function needsPageModel(caps: Capabilities | undefined): boolean {
  return !!caps && caps.backend !== 'server' && caps.model !== 'in-page'
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
  // A tab that can load its model offers the load in the Sync sheet.
  if (need === 'compile' && caps.model === 'loadable') return 'Sync runs a model in this browser. Load it first.'
  if (!caps.jobs.includes(need)) {
    return `This backend cannot run ${need === 'compile' ? 'a sync' : need === 'gen' ? 'a build' : need === 'verify' ? 'a check' : need}.`
  }
  if (need === 'compile' && caps.model === 'needs-key') return 'Sync needs a model. Add an API key in Settings.'
  if (need === 'compile' && caps.model === 'none') return 'Sync needs a model. Configure one in Settings.'
  return null
}
