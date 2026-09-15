// Inline markdown rendering while editing: headings take their size, emphasis and
// code take their style, links show their text, bullets and rules draw as marks.
// The markup tokens are hidden except where the selection touches their node, so
// the document reads like a page and edits like text. Decorations only: the text
// underneath stays plain markdown, byte for byte, because the docs are compiler
// input and provenance quotes locate against the exact characters.
// Mirrors docs/frontends/gui.md#editor.
import { syntaxTree } from '@codemirror/language'
import type { EditorState, Extension, Range } from '@codemirror/state'
import {
  Decoration,
  type DecorationSet,
  EditorView,
  ViewPlugin,
  type ViewUpdate,
  WidgetType,
} from '@codemirror/view'
import type { SyntaxNode } from '@lezer/common'

class BulletWidget extends WidgetType {
  toDOM() {
    const s = document.createElement('span')
    s.className = 'md-bullet'
    s.textContent = '•'
    return s
  }
  override eq() {
    return true
  }
}

class RuleWidget extends WidgetType {
  toDOM() {
    const s = document.createElement('span')
    s.className = 'md-hr'
    return s
  }
  override eq() {
    return true
  }
}

class TaskWidget extends WidgetType {
  constructor(readonly checked: boolean) {
    super()
  }
  toDOM() {
    const s = document.createElement('span')
    s.className = `md-task${this.checked ? ' md-task-done' : ''}`
    s.textContent = this.checked ? '☑' : '☐'
    return s
  }
  override eq(o: TaskWidget) {
    return o.checked === this.checked
  }
}

const bullet = Decoration.replace({ widget: new BulletWidget() })
const rule = Decoration.replace({ widget: new RuleWidget() })
const hide = Decoration.replace({})

function touches(state: EditorState, from: number, to: number): boolean {
  return state.selection.ranges.some((r) => r.from <= to && r.to >= from)
}

// Line decorations for every line a node spans.
function eachLine(
  state: EditorState,
  from: number,
  to: number,
  push: (lineFrom: number) => void,
) {
  let line = state.doc.lineAt(from)
  for (;;) {
    push(line.from)
    if (line.to >= to || line.to >= state.doc.length) break
    line = state.doc.lineAt(line.to + 1)
  }
}

function build(view: EditorView): DecorationSet {
  const decos: Range<Decoration>[] = []
  const state = view.state
  const doc = state.doc
  const line = (pos: number, cls: string) => decos.push(Decoration.line({ class: cls }).range(pos))
  const mark = (from: number, to: number, cls: string, attrs?: Record<string, string>) => {
    if (from < to)
      decos.push(Decoration.mark({ class: cls, attributes: attrs }).range(from, to))
  }
  const conceal = (from: number, to: number, deco: Decoration = hide) => {
    if (from < to) decos.push(deco.range(from, to))
  }

  for (const range of view.visibleRanges) {
    syntaxTree(state).iterate({
      from: range.from,
      to: range.to,
      enter(node) {
        const name = node.name
        const hot = touches(state, node.from, node.to)

        if (name.startsWith('ATXHeading')) {
          const level = Number(name.slice('ATXHeading'.length)) || 1
          line(doc.lineAt(node.from).from, `md-h md-h${level}`)
          if (!hot) {
            const m = node.node.getChild('HeaderMark')
            if (m) {
              let end = m.to
              if (doc.sliceString(end, end + 1) === ' ') end += 1
              conceal(m.from, end)
            }
          }
          return
        }
        if (name === 'SetextHeading1' || name === 'SetextHeading2') {
          line(doc.lineAt(node.from).from, `md-h md-h${name.endsWith('1') ? 1 : 2}`)
          const m = node.node.getChild('HeaderMark')
          if (m) mark(m.from, m.to, 'md-mark')
          return
        }

        if (name === 'Emphasis' || name === 'StrongEmphasis' || name === 'Strikethrough') {
          const cls =
            name === 'Emphasis' ? 'md-em' : name === 'StrongEmphasis' ? 'md-strong' : 'md-strike'
          mark(node.from, node.to, cls)
          if (!hot)
            for (const m of marksOf(node.node, ['EmphasisMark', 'StrikethroughMark']))
              conceal(m.from, m.to)
          return
        }

        if (name === 'InlineCode') {
          mark(node.from, node.to, 'md-code')
          if (!hot) for (const m of marksOf(node.node, ['CodeMark'])) conceal(m.from, m.to)
          return
        }

        if (name === 'Link' || name === 'Image') {
          const url = node.node.getChild('URL')
          const target = url ? doc.sliceString(url.from, url.to) : ''
          const marks = marksOf(node.node, ['LinkMark'])
          // Content is what sits between the opening `[` and the closing `]`.
          const open = marks[0]
          const close = marks.find((m) => doc.sliceString(m.from, m.to).startsWith(']'))
          if (open && close && close.from > open.to)
            mark(open.to, close.from, 'md-link', target ? { 'data-md-href': target } : undefined)
          if (!hot) {
            for (const m of marks) conceal(m.from, m.to)
            if (url) conceal(url.from, url.to)
            const title = node.node.getChild('LinkTitle')
            if (title) conceal(title.from, title.to)
          } else if (url) {
            mark(url.from, url.to, 'md-url')
          }
          return
        }
        if (name === 'Autolink' || name === 'URL') {
          // A URL inside a Link or Image is that node's business, handled above.
          const parent = node.node.parent?.name
          if (parent === 'Link' || parent === 'Image') return
          const target = doc.sliceString(node.from, node.to)
          mark(node.from, node.to, 'md-link', { 'data-md-href': target })
          return
        }

        if (name === 'ListMark') {
          const text = doc.sliceString(node.from, node.to)
          const itemHot = touches(state, doc.lineAt(node.from).from, doc.lineAt(node.from).to)
          if (/^[-*+]$/.test(text)) {
            if (!itemHot) conceal(node.from, node.to, bullet)
            else mark(node.from, node.to, 'md-mark')
          } else {
            mark(node.from, node.to, 'md-list-num')
          }
          return
        }
        if (name === 'TaskMarker') {
          const itemHot = touches(state, doc.lineAt(node.from).from, doc.lineAt(node.from).to)
          const checked = doc.sliceString(node.from, node.to).toLowerCase().includes('x')
          if (!itemHot)
            conceal(node.from, node.to, Decoration.replace({ widget: new TaskWidget(checked) }))
          return
        }

        if (name === 'Blockquote') {
          eachLine(state, node.from, node.to, (lf) => line(lf, 'md-quote'))
          return
        }
        if (name === 'QuoteMark') {
          mark(node.from, node.to, 'md-mark')
          return
        }

        if (name === 'FencedCode') {
          const first = doc.lineAt(node.from)
          const last = doc.lineAt(node.to)
          eachLine(state, node.from, node.to, (lf) => line(lf, 'md-codeblock'))
          line(first.from, 'md-fence')
          if (last.from !== first.from) line(last.from, 'md-fence')
          return
        }
        if (name === 'CodeInfo') {
          mark(node.from, node.to, 'md-mark')
          return
        }

        if (name === 'HorizontalRule') {
          if (!hot) conceal(node.from, node.to, rule)
          else mark(node.from, node.to, 'md-mark')
          return
        }

        if (name === 'Table') {
          eachLine(state, node.from, node.to, (lf) => line(lf, 'md-table'))
          return
        }
        if (name === 'TableDelimiter') {
          mark(node.from, node.to, 'md-mark')
          return
        }
        if (name === 'TableHeader') {
          mark(node.from, node.to, 'md-strong')
          return
        }
      },
    })
  }
  return Decoration.set(decos, true)
}

function marksOf(node: SyntaxNode, names: string[]): SyntaxNode[] {
  const out: SyntaxNode[] = []
  for (let c = node.firstChild; c; c = c.nextSibling) if (names.includes(c.name)) out.push(c)
  return out
}

// Recompute on edits, selection moves (marks reveal around the cursor), and scroll.
export function markdownLive(): Extension {
  return ViewPlugin.fromClass(
    class {
      decorations: DecorationSet
      constructor(view: EditorView) {
        this.decorations = build(view)
      }
      update(u: ViewUpdate) {
        if (u.docChanged || u.selectionSet || u.viewportChanged) this.decorations = build(u.view)
      }
    },
    { decorations: (v) => v.decorations },
  )
}
