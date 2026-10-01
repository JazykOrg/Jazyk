// What each kind of item looks like and whether it is a file
// (docs/frontends/gui.md#workspace): one glyph and one color per kind, on its tab,
// its window's bars, and its cards. A file stays open until closed; any other item
// is held (items.ts).
import { itemKind, type ItemKind } from './items'

export interface KindMeta {
  glyph: string
  label: string
  // The plural, for a links bar's group.
  plural: string
  file: boolean
}

export const KINDS: Record<ItemKind, KindMeta> = {
  doc: { glyph: '¶', label: 'Document', plural: 'documents', file: true },
  file: { glyph: '▤', label: 'Product file', plural: 'product files', file: true },
  test: { glyph: '✓', label: 'Test', plural: 'tests', file: true },
  generated: { glyph: '▦', label: 'Generated page', plural: 'generated pages', file: true },
  product: { glyph: '▣', label: 'Product', plural: 'product pages', file: false },
  entity: { glyph: '◆', label: 'Entity', plural: 'entities', file: false },
  requirement: { glyph: '≡', label: 'Requirement', plural: 'requirements', file: false },
  diagram: { glyph: '◇', label: 'Diagram', plural: 'diagrams', file: false },
  inbox: { glyph: '✉', label: 'Inbox', plural: 'inbox pages', file: false },
  history: { glyph: '↺', label: 'History', plural: 'history pages', file: false },
  other: { glyph: '▢', label: 'Page', plural: 'pages', file: false },
}

export const isFile = (path: string) => KINDS[itemKind(path)].file
