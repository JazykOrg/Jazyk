// The GUI's presentation words (docs/frontends/gui.md#presentation): what a status,
// a relation, a kind, or a document reference reads as for someone who does not
// know jazyk. Developer mode keeps the compiler's word beside these where it helps.
import { useCallback } from 'react'
import type { Graph } from './api'
import { useCoverage, useGraph } from './queries'

// A trace status in words, with the dot class the chips use.
export function statusWord(status?: string | null): { word: string; cls: string } {
  switch (status) {
    case 'verified':
      return { word: 'Passing', cls: 'v-ok' }
    case 'failing':
      return { word: 'Failing', cls: 'v-bad' }
    case 'partial':
      return { word: 'Partly passing', cls: 'v-bad' }
    case 'unimplemented':
      return { word: 'Not generated yet', cls: 'v-none' }
    case 'stale-requirement':
      return { word: 'Outdated: the requirement changed', cls: 'v-stale' }
    case 'stale-test':
    case 'stale-code':
      return { word: 'Needs a rerun', cls: 'v-stale' }
    default:
      return { word: 'No test yet', cls: 'v-none' }
  }
}

// A relationship type read from one side (docs/compiler/model/relationship.md): `a`
// acts on `b`, so the entity at `a` reads the outgoing verb.
export function relationPhrase(type: string | undefined, outgoing: boolean): string {
  switch (type) {
    case 'generalization':
      return outgoing ? 'is a kind of' : 'has the kind'
    case 'realization':
      return outgoing ? 'implements' : 'is implemented by'
    case 'composition':
      return outgoing ? 'is made of' : 'is part of'
    case 'aggregation':
      return outgoing ? 'has' : 'belongs to'
    case 'association':
      return 'is related to'
    default:
      return outgoing ? 'uses' : 'is used by'
  }
}

export function criterionKindWord(kind?: string): string {
  switch (kind) {
    case 'negative':
      return 'Failure case'
    case 'boundary':
      return 'Edge case'
    case 'quality':
      return 'Measure'
    default:
      return 'Example'
  }
}

// A stereotype as a word: `«component»` reads as Component.
export function kindWord(stereotype?: string | null): string {
  if (!stereotype) return ''
  const w = stereotype.replace(/[-_]/g, ' ')
  // A short stereotype is an acronym: cli, api, ui.
  if (w.length <= 3) return w.toUpperCase()
  return w.charAt(0).toUpperCase() + w.slice(1)
}

export function shorten(s: string, n: number): string {
  return s.length > n ? `${s.slice(0, n - 1).trimEnd()}…` : s
}

// What a node reads as: an entity's name, a requirement's or a criterion's
// statement, a diagram's title, a problem's message.
export function nodeName(g: Graph | undefined, id: string): string {
  if (!g) return id
  if (g.entities[id]) return g.entities[id].name
  if (g.requirements[id]) return g.requirements[id].statement
  if (g.criteria?.[id]) return g.criteria[id].statement
  if (g.views[id]) return g.views[id].title ?? id
  if (g.diagnostics[id]) return g.diagnostics[id].message
  const sm = g.stateMachines[id]
  if (sm) return `States of ${g.entities[sm.subject]?.name ?? sm.subject}`
  const rel = g.relationships[id]
  if (rel) return rel.members.map((m) => g.entities[m]?.name ?? m).join(' and ')
  return id
}

export function useNodeName(): (id: string) => string {
  const { data: g } = useGraph()
  return useCallback((id: string) => nodeName(g, id), [g])
}

// A document reference as the file name and the section's title:
// `backend.md › Database` rather than `docs/backend.md#/backend/database`.
export function useSectionLabel(): (doc: string, section?: string) => string {
  const { data: cov } = useCoverage()
  return useCallback(
    (doc: string, section?: string) => {
      const file = doc.split('/').pop() ?? doc
      if (!section) return file
      const title = cov?.[doc]?.sections?.[section]?.title
      return title ? `${file} › ${title}` : file
    },
    [cov],
  )
}
