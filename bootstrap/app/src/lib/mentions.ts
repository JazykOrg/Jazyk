// Entity mentions in a document's text (docs/frontends/gui.md#editor): every
// occurrence of an entity's name or alias, matched case-insensitively on word
// boundaries, the longest name winning where two overlap. The editor underlines
// them in Write and in Read from the graph alone, no language server needed. Also
// the quote locator the Read rendering shares with the editor: whitespace-insensitive,
// the way provenance quotes are located.
import type { Entity } from './api'

export interface MentionIndex {
  // Every name, longest first, so the alternation prefers the longer entity.
  names: { name: string; id: string }[]
  // The whole index as one expression, or null when no entity has a name.
  re: RegExp | null
  byName: Map<string, string>
}

export interface Mention {
  from: number
  to: number
  id: string
  name: string
}

function escapeRe(s: string): string {
  return s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
}

export function buildMentionIndex(entities: Record<string, Entity>): MentionIndex {
  const byName = new Map<string, string>()
  for (const [id, e] of Object.entries(entities)) {
    for (const raw of [e.name, ...(e.aliases ?? [])]) {
      const name = (raw ?? '').trim()
      // One-character names match everywhere; they are noise, not mentions.
      if (name.length < 2) continue
      const key = name.toLowerCase()
      // The entity's own name outranks another's alias for the same string.
      if (!byName.has(key) || raw === e.name) byName.set(key, id)
    }
  }
  const names = [...byName.entries()]
    .map(([name, id]) => ({ name, id }))
    .sort((a, b) => b.name.length - a.name.length || a.name.localeCompare(b.name))
  if (names.length === 0) return { names, re: null, byName }
  // Word boundaries by letters and digits, so a name inside a longer word is not a
  // mention; a name that starts or ends with punctuation still bounds by what is
  // around it.
  const alt = names.map((n) => escapeRe(n.name).replace(/\s+/g, '\\s+')).join('|')
  const re = new RegExp(`(?<![\\p{L}\\p{N}_])(?:${alt})(?![\\p{L}\\p{N}_])`, 'giu')
  return { names, re, byName }
}

// Every mention in the text, in order, none overlapping. The alternation is longest
// first, so at one position the longer name wins; a match consumes its text, so a
// shorter name inside it is never a second mention.
export function findMentions(text: string, index: MentionIndex): Mention[] {
  const out: Mention[] = []
  if (!index.re) return out
  // A link's destination (`](./user.md)`) is a path, not prose: no mention in it.
  const skip: { from: number; to: number }[] = []
  for (const u of text.matchAll(/\]\([^)\n]*\)/g)) skip.push({ from: u.index + 1, to: u.index + u[0].length })
  index.re.lastIndex = 0
  let m: RegExpExecArray | null
  while ((m = index.re.exec(text))) {
    const from = m.index
    const key = m[0].toLowerCase().replace(/\s+/g, ' ')
    const id = index.byName.get(key)
    if (id && !skip.some((s) => from >= s.from && from < s.to)) out.push({ from, to: from + m[0].length, id, name: m[0] })
    if (m[0].length === 0) index.re.lastIndex++
  }
  return out
}

// A quote located in the text whitespace-insensitively, preferring a hit at or
// after `fromPos`, then anywhere. The same rule the editor's margin uses.
export function findQuoteIn(text: string, quote: string, fromPos = 0): { from: number; to: number } | null {
  const words = quote.trim().split(/\s+/).filter((w) => w !== '')
  if (words.length === 0) return null
  const re = new RegExp(words.map(escapeRe).join('\\s+'))
  let m = re.exec(text.slice(fromPos))
  if (m) return { from: fromPos + m.index, to: fromPos + m.index + m[0].length }
  m = re.exec(text)
  return m ? { from: m.index, to: m.index + m[0].length } : null
}

// The 1-based line an offset falls on.
export function lineOf(text: string, pos: number): number {
  let line = 1
  for (let i = 0; i < pos && i < text.length; i++) if (text.charCodeAt(i) === 10) line++
  return line
}

// The 1-based line a quote starts on, or null when the text does not hold it.
export function quoteLine(text: string, quote: string): number | null {
  const hit = findQuoteIn(text, quote)
  return hit ? lineOf(text, hit.from) : null
}
