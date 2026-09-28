// The section tree of a document as the outline and the editor read it
// (docs/frontends/gui.md#docs): the sections in document order, the section a line
// falls in, and the heading a link's fragment names.
import type { Section } from './api'
import { slug } from './mdlinks'

export type SectionEntry = [string, Section]

// Every section in document order: the start line puts a parent before its
// children and one tree before the next; `order` breaks ties among siblings.
export function sortedSections(sections: Record<string, Section>): SectionEntry[] {
  return Object.entries(sections).sort(([, a], [, b]) => (a.lines?.[0] ?? 0) - (b.lines?.[0] ?? 0) || a.order - b.order)
}

// A section record's `lines` are 0-based, the end exclusive; the editor counts
// from 1. The 1-based lines a section spans, inclusive.
export function sectionLines(s: Section): [number, number] {
  const [start, end] = s.lines
  return [start + 1, Math.max(end, start + 1)]
}

// The deepest section holding the line (1-based), or null outside every section.
// Only the sections that are headings of the outline count: a list item or a code
// block inside a section belongs to that section's row.
export function sectionAtLine(sections: Record<string, Section>, line: number): string | null {
  let best: string | null = null
  let bestStart = -1
  let bestDepth = -1
  for (const [ref, s] of Object.entries(sections)) {
    if (!outlined(s)) continue
    const [start, end] = sectionLines(s)
    if (line < start || line > end) continue
    const depth = ref.split('/').length
    if (start > bestStart || (start === bestStart && depth > bestDepth)) {
      best = ref
      bestStart = start
      bestDepth = depth
    }
  }
  return best
}

// The outline lists the headings (the preamble and the root are headings' kin);
// list items, code blocks, block quotes, and diagrams are inside them.
export function outlined(s: Section): boolean {
  return s.kind === 'heading' || s.kind === 'root' || s.kind === 'preamble'
}

// The section whose heading a `#fragment` names, under the compiler's slug rule.
export function sectionBySlug(sections: Record<string, Section>, fragment: string): string | null {
  const want = fragment.toLowerCase()
  for (const [ref, s] of sortedSections(sections)) {
    if (!outlined(s)) continue
    if (slug(s.title) === want) return ref
  }
  return null
}
