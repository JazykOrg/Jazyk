// CodeMirror host: one editor view, one cached state per open document, markdown
// rendered inline while editing (markdown-live.ts). LSP features are wired straight
// onto the hand-rolled client; positions translate between LSP 0-based and
// CodeMirror offsets. Mirrors docs/frontends/gui.md#editor.
import { forwardRef, useEffect, useImperativeHandle, useRef } from 'react'
import {
  EditorState,
  RangeSet,
  StateEffect,
  StateField,
  type Extension,
} from '@codemirror/state'
import {
  Decoration,
  type DecorationSet,
  EditorView,
  GutterMarker,
  WidgetType,
  gutter,
  hoverTooltip,
  keymap,
  lineNumbers,
  drawSelection,
  highlightSpecialChars,
  rectangularSelection,
} from '@codemirror/view'
import { defaultKeymap, history, historyKeymap, indentWithTab } from '@codemirror/commands'
import { bracketMatching, syntaxHighlighting } from '@codemirror/language'
import { classHighlighter } from '@lezer/highlight'
import { markdown, markdownLanguage } from '@codemirror/lang-markdown'
import { languages } from '@codemirror/language-data'
import { autocompletion, type CompletionContext, type CompletionResult } from '@codemirror/autocomplete'
import { setDiagnostics, type Diagnostic } from '@codemirror/lint'
import { MergeView } from '@codemirror/merge'
import type { DocRecord } from '../lib/api'
import { lineMarks } from '../lib/diff'
import { markdownLive } from './markdown-live'
import type { LspClient, LspDiagnostic, LspRange } from './lsp-client'

export function docUri(root: string, path: string): string {
  return `file://${root}/${path}`
}

function uriToPath(root: string, uri: string): string | null {
  const prefix = `file://${root}/`
  return uri.startsWith(prefix) ? uri.slice(prefix.length) : null
}

// A link click resolved to what the app routes on: the absolute path, the 1-based
// line an `#L<n>` fragment names, and the graph node a `?req=` query names (the
// requirement card's own link). See docs/frontends/lsp.md#capabilities.
export interface LinkTarget {
  path: string
  line?: number
  node?: string
}

// A diagnostic for the problems panel, editor-neutral.
export interface Marker {
  severity: 'error' | 'warning' | 'info' | 'hint'
  message: string
  source?: string
  line: number // 1-based
  column: number // 1-based
}

function lineFromFragment(fragment: string): number | undefined {
  const m = /^L?(\d+)$/.exec(fragment)
  return m ? Number(m[1]) : undefined
}

function queryParam(query: string, key: string): string | undefined {
  for (const part of query.split('&')) {
    const i = part.indexOf('=')
    if (i > 0 && decodeURIComponent(part.slice(0, i)) === key)
      return decodeURIComponent(part.slice(i + 1))
  }
  return undefined
}

// A file:// href from a document link or the hover card, resolved to a LinkTarget.
function fileTarget(href: string): LinkTarget | null {
  let u: URL
  try {
    u = new URL(href)
  } catch {
    return null
  }
  if (u.protocol !== 'file:') return null
  return {
    path: decodeURIComponent(u.pathname),
    line: lineFromFragment(u.hash.replace(/^#/, '')),
    node: queryParam(u.search.replace(/^\?/, ''), 'req'),
  }
}

function clampLine(state: EditorState, line: number): number {
  return Math.min(Math.max(line, 1), state.doc.lines)
}

function lspToPos(state: EditorState, p: { line: number; character: number }): number {
  const line = state.doc.line(clampLine(state, p.line + 1))
  return Math.min(line.from + p.character, line.to)
}

function lspRange(state: EditorState, r: LspRange): { from: number; to: number } {
  return { from: lspToPos(state, r.start), to: lspToPos(state, r.end) }
}

function posToLsp(state: EditorState, pos: number): { line: number; character: number } {
  const line = state.doc.lineAt(pos)
  return { line: line.number - 1, character: pos - line.from }
}

const SEV: Record<number, Marker['severity']> = { 1: 'error', 2: 'warning', 3: 'info', 4: 'hint' }

// ---------------------------------------------------------------------------
// Gutter strips (coverage, baseline changes, build state) and overlay fields.
// Every input arrives as full replacement ranges; positions map through edits.

class Strip extends GutterMarker {
  constructor(
    readonly cls: string,
    readonly tip: string | null,
  ) {
    super()
  }
  override eq(o: Strip) {
    return o.cls === this.cls && o.tip === this.tip
  }
  toDOM() {
    const s = document.createElement('div')
    s.className = this.cls
    if (this.tip) s.title = this.tip
    return s
  }
}

interface StripSpec {
  line: number // 1-based
  cls: string
  tip?: string
}

function stripSet(state: EditorState, specs: StripSpec[]): RangeSet<Strip> {
  const ranges = specs
    .map((s) => {
      const from = state.doc.line(clampLine(state, s.line)).from
      return new Strip(s.cls, s.tip ?? null).range(from)
    })
    .sort((a, b) => a.from - b.from)
  return RangeSet.of(ranges)
}

function stripField(): [StateField<RangeSet<Strip>>, ReturnType<typeof StateEffect.define<StripSpec[]>>] {
  const effect = StateEffect.define<StripSpec[]>()
  const field = StateField.define<RangeSet<Strip>>({
    create: () => RangeSet.empty,
    update(value, tr) {
      value = value.map(tr.changes)
      for (const e of tr.effects) if (e.is(effect)) value = stripSet(tr.state, e.value)
      return value
    },
  })
  return [field, effect]
}

const [covField, setCov] = stripField()
const [chgField, setChg] = stripField()
const [bldField, setBld] = stripField()

function decoField(): [StateField<DecorationSet>, ReturnType<typeof StateEffect.define<DecorationSet>>] {
  const effect = StateEffect.define<DecorationSet>()
  const field = StateField.define<DecorationSet>({
    create: () => Decoration.none,
    update(value, tr) {
      value = value.map(tr.changes)
      for (const e of tr.effects) if (e.is(effect)) value = e.value
      return value
    },
    provide: (f) => EditorView.decorations.from(f),
  })
  return [field, effect]
}

const [bandField, setBands] = decoField() // build bands: whole-line backgrounds
const [linkField, setLinks] = decoField() // entity marks over LSP document links
const [lensField, setLenses] = decoField() // requirement lenses above their quotes
const [quoteField, setQuote] = decoField() // the ?quote= highlight

class LensWidget extends WidgetType {
  constructor(
    readonly title: string,
    readonly rid: string,
    readonly open: (rid: string) => void,
  ) {
    super()
  }
  override eq(o: LensWidget) {
    return o.title === this.title && o.rid === this.rid
  }
  toDOM() {
    const d = document.createElement('div')
    d.className = 'jz-lens'
    d.textContent = this.title
    d.onmousedown = (e) => {
      e.preventDefault()
      this.open(this.rid)
    }
    return d
  }
  override ignoreEvent() {
    return true
  }
}

// ---------------------------------------------------------------------------
// The hover card: the language server hands back markdown (links, bold, code,
// lists, rules); render just that subset into DOM, links routed by the host.

function inlineMd(text: string, into: HTMLElement, onHref: (href: string) => void) {
  const re = /\[([^\]]+)\]\(([^)\s]+)\)|`([^`]+)`|\*\*([^*]+)\*\*/g
  let last = 0
  let m: RegExpExecArray | null
  while ((m = re.exec(text))) {
    if (m.index > last) into.appendChild(document.createTextNode(text.slice(last, m.index)))
    if (m[1] !== undefined) {
      const href = m[2]
      const a = document.createElement('a')
      a.textContent = m[1]
      a.dataset.href = href
      a.onmousedown = (e) => {
        e.preventDefault()
        onHref(href)
      }
      into.appendChild(a)
    } else if (m[3] !== undefined) {
      const c = document.createElement('code')
      c.textContent = m[3]
      into.appendChild(c)
    } else {
      const b = document.createElement('strong')
      b.textContent = m[4]
      into.appendChild(b)
    }
    last = m.index + m[0].length
  }
  if (last < text.length) into.appendChild(document.createTextNode(text.slice(last)))
}

function renderMd(value: string, onHref: (href: string) => void): HTMLElement {
  const root = document.createElement('div')
  root.className = 'jz-hover'
  let quote: HTMLElement | null = null
  for (const raw of value.split('\n')) {
    const line = raw.trimEnd()
    if (/^-{3,}$/.test(line.trim())) {
      root.appendChild(document.createElement('hr'))
      quote = null
      continue
    }
    if (line.startsWith('>')) {
      if (!quote) {
        quote = document.createElement('blockquote')
        root.appendChild(quote)
      }
      const p = document.createElement('div')
      inlineMd(line.replace(/^>\s?/, ''), p, onHref)
      quote.appendChild(p)
      continue
    }
    quote = null
    const p = document.createElement('div')
    if (/^\s*[-*]\s+/.test(line)) {
      p.className = 'jz-hover-li'
      inlineMd(line.replace(/^\s*[-*]\s+/, '• '), p, onHref)
    } else if (line === '') {
      p.className = 'jz-hover-gap'
    } else {
      inlineMd(line, p, onHref)
    }
    root.appendChild(p)
  }
  return root
}

export interface EditorHandle {
  getText(): string
  setText(text: string): void // replaces content and resets the dirty baseline
  markSaved(): void // current content becomes the baseline
  revealLine(line: number): void
  revealSection(startLine: number, quote?: string): void
}

// What a running build is doing to this document, drawn over the text
// (docs/frontends/gui.md#editor).
export interface BuildBands {
  label: string
  state: 'queued' | 'running' | 'done' | 'failed'
  sections: string[]
  active: string | null
  result: string | null
}

interface Props {
  root: string
  path: string
  initialText: string
  lsp: LspClient
  record?: DocRecord
  build?: BuildBands
  onHoverLine?: (line: number | null) => void
  baseline?: string | null
  diffMode?: boolean
  onDirty: (dirty: boolean) => void
  onNavigate: (path: string, line?: number) => void
  onOpenLink: (target: LinkTarget) => void
  onOpenNode?: (id: string) => void
  onMarkers: (markers: Marker[]) => void
  onSave: () => void
}

// Whitespace-insensitive quote search, preferring a match at or after fromPos.
function findQuote(state: EditorState, quoteText: string, fromPos: number): { from: number; to: number } | null {
  const words = quoteText.trim().split(/\s+/).filter((w) => w !== '')
  if (words.length === 0) return null
  const re = new RegExp(words.map((w) => w.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')).join('\\s+'))
  const text = state.doc.toString()
  let m = re.exec(text.slice(fromPos))
  if (m) return { from: fromPos + m.index, to: fromPos + m.index + m[0].length }
  m = re.exec(text)
  return m ? { from: m.index, to: m.index + m[0].length } : null
}

const CmHost = forwardRef<EditorHandle, Props>(function CmHost(props, ref) {
  const divRef = useRef<HTMLDivElement>(null)
  const diffDivRef = useRef<HTMLDivElement>(null)
  const viewRef = useRef<EditorView | null>(null)
  const mergeRef = useRef<MergeView | null>(null)
  // Per-uri cached editor state (undo history survives switching documents) and
  // the last loaded or saved text; dirty means the doc diverged from it.
  const states = useRef(new Map<string, EditorState>())
  const baselines = useRef(new Map<string, string>())
  const openedUri = useRef<string | null>(null)
  const diagUnsubs = useRef(new Map<string, () => void>())
  const lastDiags = useRef(new Map<string, LspDiagnostic[]>())
  const diffTimer = useRef<number | null>(null)
  const propsRef = useRef(props)
  propsRef.current = props

  const activeUri = () => (openedUri.current ? openedUri.current : null)

  const routeHref = (href: string) => {
    const t = fileTarget(href)
    if (t) propsRef.current.onOpenLink(t)
    else if (/^https?:/.test(href)) window.open(href, '_blank', 'noopener')
    else if (href.endsWith('.md') || href.includes('.md#')) {
      // A relative markdown link inside a document: resolve against the doc's dir.
      const p = propsRef.current
      const dir = p.path.includes('/') ? p.path.slice(0, p.path.lastIndexOf('/') + 1) : ''
      const [rel] = href.split('#')
      const parts: string[] = []
      for (const seg of (dir + rel).split('/')) {
        if (seg === '' || seg === '.') continue
        if (seg === '..') parts.pop()
        else parts.push(seg)
      }
      p.onNavigate(parts.join('/'))
    }
  }

  // ---- overlays recomputed from props ----

  const pushDiagnostics = (view: EditorView, diags: LspDiagnostic[]) => {
    const cmDiags: Diagnostic[] = diags.map((d) => {
      const r = lspRange(view.state, d.range)
      const sev = SEV[d.severity ?? 3] ?? 'info'
      return {
        from: r.from,
        to: Math.max(r.to, r.from),
        severity: sev === 'hint' ? 'info' : sev,
        message: d.message,
        source: d.source,
      }
    })
    view.dispatch(setDiagnostics(view.state, cmDiags))
    propsRef.current.onMarkers(
      diags.map((d) => ({
        severity: SEV[d.severity ?? 3] ?? 'info',
        message: d.message,
        source: d.source,
        line: d.range.start.line + 1,
        column: d.range.start.character + 1,
      })),
    )
  }

  const refreshLinks = async (view: EditorView, uri: string) => {
    try {
      const links = await propsRef.current.lsp.documentLinks(uri)
      if (activeUri() !== uri || viewRef.current !== view) return
      const decos = links
        .map((l) => {
          const r = lspRange(view.state, l.range)
          return Decoration.mark({
            class: 'entity-mark',
            attributes: l.target
              ? { 'data-lsp-target': l.target, title: l.tooltip ?? '' }
              : undefined,
          }).range(r.from, r.to)
        })
        .filter((d) => d.from < d.to)
        .sort((a, b) => a.from - b.from)
      view.dispatch({ effects: setLinks.of(Decoration.set(decos, true)) })
    } catch {
      // offline; marks stay as they were
    }
  }

  const refreshLenses = async (view: EditorView, uri: string) => {
    try {
      const lenses = await propsRef.current.lsp.codeLens(uri)
      if (activeUri() !== uri || viewRef.current !== view) return
      const open = (rid: string) => propsRef.current.onOpenNode?.(rid)
      const decos = lenses
        .filter((l) => l.command)
        .map((l) => {
          const line = view.state.doc.line(clampLine(view.state, l.range.start.line + 1))
          const rid = (l.command!.arguments?.[0] as string) ?? ''
          return Decoration.widget({
            widget: new LensWidget(l.command!.title, rid, open),
            side: -1,
            block: true,
          }).range(line.from)
        })
        .sort((a, b) => a.from - b.from)
      view.dispatch({ effects: setLenses.of(Decoration.set(decos, true)) })
    } catch {
      // offline; lenses stay as they were
    }
  }

  const applyCoverage = (view: EditorView) => {
    const rec = propsRef.current.record
    const specs: StripSpec[] = []
    if (rec) {
      for (const [secRef, sec] of Object.entries(rec.sections)) {
        const cov = (rec.coverage ?? {})[secRef]
        const cls = cov ? (cov.state === 'covered' ? 'cov-covered' : 'cov-nonnorm') : 'cov-unprocessed'
        const [start, end] = sec.lines
        for (let l = start; l <= Math.min(end, view.state.doc.lines); l++)
          specs.push({ line: l, cls, tip: cov?.note ?? undefined })
      }
    }
    view.dispatch({ effects: setCov.of(specs) })
  }

  const applyBuild = (view: EditorView) => {
    const p = propsRef.current
    const b = p.build
    const bands: { from: number; deco: Decoration }[] = []
    const specs: StripSpec[] = []
    if (b && p.record) {
      const done = b.state === 'done' || b.state === 'failed'
      const outcome = b.state === 'failed' ? 'build-failed' : 'build-done'
      for (const secRef of b.sections) {
        const sec = p.record.sections[secRef]
        if (!sec) continue
        const [start, end] = sec.lines
        if (start > view.state.doc.lines) continue
        const active = secRef === b.active
        const cls = done ? outcome : active ? 'build-active' : 'build-queued'
        const tip = done
          ? `${b.label}: ${b.result ?? (b.state === 'failed' ? 'parked' : 'committed')}`
          : `${b.label}: ${active ? 'reconciling this section' : 'queued for this build'}`
        for (let l = start; l <= Math.min(Math.max(end, start), view.state.doc.lines); l++)
          bands.push({
            from: view.state.doc.line(l).from,
            deco: Decoration.line({ class: cls, attributes: { title: tip } }),
          })
        specs.push({ line: start, cls: `${cls}-gutter`, tip })
      }
    }
    const decos = bands.sort((a, b2) => a.from - b2.from).map((x) => x.deco.range(x.from))
    view.dispatch({ effects: [setBands.of(Decoration.set(decos, true)), setBld.of(specs)] })
  }

  const applyDiffMarks = (view: EditorView) => {
    const p = propsRef.current
    const specs: StripSpec[] = []
    if (p.baseline !== null && p.baseline !== undefined) {
      const marks = lineMarks(p.baseline, view.state.doc.toString())
      for (const l of marks.added) specs.push({ line: l, cls: 'gd-add', tip: 'added since the last reconcile' })
      for (const l of marks.modified)
        specs.push({ line: l, cls: 'gd-mod', tip: 'changed since the last reconcile' })
      for (const [l, n] of marks.deletedAbove)
        specs.push({ line: l, cls: 'gd-del', tip: `${n} line${n > 1 ? 's' : ''} deleted here` })
    }
    view.dispatch({ effects: setChg.of(specs) })
  }

  const scheduleDiffMarks = () => {
    if (diffTimer.current !== null) window.clearTimeout(diffTimer.current)
    diffTimer.current = window.setTimeout(() => {
      const v = viewRef.current
      if (v) applyDiffMarks(v)
    }, 250)
  }

  // ---- extensions (per document, closures over the component refs) ----

  const buildExtensions = (uri: string): Extension[] => {
    const completionSource = async (ctx: CompletionContext): Promise<CompletionResult | null> => {
      // Triggered inside a backtick or link opener; the trigger character itself
      // stays, only the partial word after it is matched and replaced.
      const trigger = ctx.matchBefore(/[`[][^\s`\]]*$/)
      const word = ctx.matchBefore(/[\w-]+$/)
      if (!trigger && !ctx.explicit) return null
      try {
        const items = await propsRef.current.lsp.completion(uri, posToLsp(ctx.state, ctx.pos))
        if (items.length === 0) return null
        let from = trigger ? trigger.from + 1 : word ? word.from : ctx.pos
        const first = items.find((it) => it.textEdit)
        if (first?.textEdit) from = lspRange(ctx.state, first.textEdit.range).from
        return {
          from,
          options: items.map((it) => ({
            label: it.label,
            detail: it.detail,
            info: typeof it.documentation === 'string' ? it.documentation : it.documentation?.value,
            apply: it.textEdit?.newText ?? it.insertText ?? it.label,
          })),
          filter: true,
        }
      } catch {
        return null
      }
    }

    const hover = hoverTooltip(async (view, pos) => {
      try {
        const h = await propsRef.current.lsp.hover(uri, posToLsp(view.state, pos))
        if (!h) return null
        const value = typeof h.contents === 'string' ? h.contents : h.contents.value
        if (!value) return null
        const r = h.range ? lspRange(view.state, h.range) : { from: pos, to: pos }
        return {
          pos: r.from,
          end: r.to,
          above: true,
          create: () => ({ dom: renderMd(value, routeHref) }),
        }
      } catch {
        return null
      }
    })

    const clicks = EditorView.domEventHandlers({
      mousedown: (e, view) => {
        const el = e.target as HTMLElement
        const md = el.closest('[data-md-href]') as HTMLElement | null
        const lspLink = el.closest('[data-lsp-target]') as HTMLElement | null
        const mod = e.metaKey || e.ctrlKey
        if (mod && md?.dataset.mdHref) {
          e.preventDefault()
          routeHref(md.dataset.mdHref)
          return true
        }
        if (mod && lspLink?.dataset.lspTarget) {
          e.preventDefault()
          routeHref(lspLink.dataset.lspTarget)
          return true
        }
        if (mod) {
          const pos = view.posAtCoords({ x: e.clientX, y: e.clientY })
          if (pos === null) return false
          const method = e.shiftKey ? 'references' : 'definition'
          void (async () => {
            try {
              const locs = await propsRef.current.lsp[method](uri, posToLsp(view.state, pos))
              const loc = locs[0]
              if (!loc) return
              const rel = uriToPath(propsRef.current.root, loc.uri)
              if (rel) propsRef.current.onNavigate(rel, loc.range.start.line + 1)
              else {
                const t = fileTarget(loc.uri)
                if (t) propsRef.current.onOpenLink({ ...t, line: loc.range.start.line + 1 })
              }
            } catch {
              // offline
            }
          })()
          e.preventDefault()
          return true
        }
        return false
      },
      mousemove: (e, view) => {
        const pos = view.posAtCoords({ x: e.clientX, y: e.clientY })
        propsRef.current.onHoverLine?.(pos === null ? null : view.state.doc.lineAt(pos).number)
        return false
      },
      mouseleave: () => {
        propsRef.current.onHoverLine?.(null)
        return false
      },
    })

    const listener = EditorView.updateListener.of((u) => {
      if (!u.docChanged) return
      const text = u.state.doc.toString()
      propsRef.current.lsp.didChange(uri, text)
      propsRef.current.onDirty(text !== baselines.current.get(uri))
      scheduleDiffMarks()
    })

    return [
      lineNumbers(),
      highlightSpecialChars(),
      history(),
      drawSelection(),
      rectangularSelection(),
      bracketMatching(),
      EditorState.allowMultipleSelections.of(true),
      EditorView.lineWrapping,
      keymap.of([
        {
          key: 'Mod-s',
          run: () => {
            propsRef.current.onSave()
            return true
          },
        },
        ...defaultKeymap,
        ...historyKeymap,
        indentWithTab,
      ]),
      markdown({ base: markdownLanguage, codeLanguages: languages }),
      syntaxHighlighting(classHighlighter),
      markdownLive(),
      autocompletion({ override: [completionSource] }),
      hover,
      clicks,
      listener,
      // Gutter order: coverage strip, change marks, build dot, then line numbers.
      gutter({ class: 'jz-gutter-cov', markers: (v) => v.state.field(covField) }),
      gutter({ class: 'jz-gutter-chg', markers: (v) => v.state.field(chgField) }),
      gutter({ class: 'jz-gutter-bld', markers: (v) => v.state.field(bldField) }),
      covField,
      chgField,
      bldField,
      bandField,
      linkField,
      lensField,
      quoteField,
    ]
  }

  const refreshOverlays = (view: EditorView, uri: string) => {
    applyCoverage(view)
    applyBuild(view)
    applyDiffMarks(view)
    const diags = lastDiags.current.get(uri)
    if (diags) pushDiagnostics(view, diags)
    else propsRef.current.onMarkers([])
    void refreshLinks(view, uri)
    void refreshLenses(view, uri)
  }

  // The view lives for the component's lifetime; documents swap states in and out.
  useEffect(() => {
    const view = new EditorView({ parent: divRef.current! })
    viewRef.current = view
    return () => {
      for (const [uri, unsub] of diagUnsubs.current) {
        unsub()
        propsRef.current.lsp.didClose(uri)
      }
      diagUnsubs.current.clear()
      states.current.clear()
      baselines.current.clear()
      mergeRef.current?.destroy()
      mergeRef.current = null
      view.destroy()
      viewRef.current = null
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  // Open or switch documents: cache the outgoing state, restore or create the
  // incoming one (with an LSP didOpen and a diagnostics subscription per uri).
  useEffect(() => {
    const view = viewRef.current
    if (!view || !props.path) return
    const uri = docUri(props.root, props.path)
    if (openedUri.current === uri) return
    if (openedUri.current) states.current.set(openedUri.current, view.state)
    let state = states.current.get(uri)
    if (!state) {
      state = EditorState.create({ doc: props.initialText, extensions: buildExtensions(uri) })
      baselines.current.set(uri, props.initialText)
      propsRef.current.lsp.didOpen(uri, props.initialText)
      const unsub = propsRef.current.lsp.onDiagnostics(uri, (diags) => {
        lastDiags.current.set(uri, diags)
        const v = viewRef.current
        if (v && activeUri() === uri) {
          pushDiagnostics(v, diags)
          // The server republishes on generation bumps; entity marks and
          // requirement lenses refresh with it.
          void refreshLinks(v, uri)
          void refreshLenses(v, uri)
        }
      })
      diagUnsubs.current.set(uri, unsub)
    }
    openedUri.current = uri
    view.setState(state)
    propsRef.current.onDirty(view.state.doc.toString() !== baselines.current.get(uri))
    refreshOverlays(view, uri)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [props.root, props.path])

  // Overlays follow their inputs.
  useEffect(() => {
    const view = viewRef.current
    if (view) applyCoverage(view)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [props.record, props.path])
  useEffect(() => {
    const view = viewRef.current
    if (view) applyBuild(view)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [props.build, props.record, props.path])
  useEffect(() => {
    const view = viewRef.current
    if (view) applyDiffMarks(view)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [props.baseline, props.path])

  // The diff view: side-by-side, baseline read-only, the current side editable.
  // Edits forward into the hidden main view, so the document stays one source of
  // truth and the dirty state and save path are unchanged.
  useEffect(() => {
    const parent = diffDivRef.current
    mergeRef.current?.destroy()
    mergeRef.current = null
    if (!props.diffMode || !parent) return
    const view = viewRef.current
    if (!view || props.baseline === null || props.baseline === undefined) return
    const forward = EditorView.updateListener.of((u) => {
      if (u.docChanged) viewRef.current?.dispatch({ changes: u.changes })
    })
    mergeRef.current = new MergeView({
      parent,
      a: {
        doc: props.baseline,
        extensions: [
          lineNumbers(),
          EditorView.editable.of(false),
          EditorState.readOnly.of(true),
          EditorView.lineWrapping,
          markdown({ base: markdownLanguage, codeLanguages: languages }),
          syntaxHighlighting(classHighlighter),
        ],
      },
      b: {
        doc: view.state.doc.toString(),
        extensions: [
          lineNumbers(),
          history(),
          EditorView.lineWrapping,
          keymap.of([
            {
              key: 'Mod-s',
              run: () => {
                propsRef.current.onSave()
                return true
              },
            },
            ...defaultKeymap,
            ...historyKeymap,
          ]),
          markdown({ base: markdownLanguage, codeLanguages: languages }),
          syntaxHighlighting(classHighlighter),
          forward,
        ],
      },
    })
    return () => {
      mergeRef.current?.destroy()
      mergeRef.current = null
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [props.diffMode, props.baseline, props.path])

  useEffect(
    () => () => {
      if (diffTimer.current !== null) window.clearTimeout(diffTimer.current)
    },
    [],
  )

  useImperativeHandle(
    ref,
    () => ({
      getText: () => viewRef.current?.state.doc.toString() ?? '',
      setText: (text) => {
        const view = viewRef.current
        const uri = activeUri()
        if (!view || !uri) return
        baselines.current.set(uri, text)
        if (view.state.doc.toString() !== text)
          view.dispatch({ changes: { from: 0, to: view.state.doc.length, insert: text } })
        propsRef.current.onDirty(false)
      },
      markSaved: () => {
        const view = viewRef.current
        const uri = activeUri()
        if (!view || !uri) return
        baselines.current.set(uri, view.state.doc.toString())
        propsRef.current.onDirty(false)
      },
      revealLine: (line) => {
        const view = viewRef.current
        if (!view) return
        const l = view.state.doc.line(clampLine(view.state, line))
        view.dispatch({
          selection: { anchor: l.from },
          effects: EditorView.scrollIntoView(l.from, { y: 'center' }),
        })
        view.focus()
      },
      revealSection: (startLine, quoteText) => {
        const view = viewRef.current
        if (!view) return
        const l = view.state.doc.line(clampLine(view.state, startLine))
        let effects = [EditorView.scrollIntoView(l.from, { y: 'start', yMargin: 40 })]
        let decos: DecorationSet = Decoration.none
        if (quoteText) {
          const hit = findQuote(view.state, quoteText, l.from)
          if (hit) {
            decos = Decoration.set([Decoration.mark({ class: 'quote-hit' }).range(hit.from, hit.to)])
            effects = [EditorView.scrollIntoView(hit.from, { y: 'center' })]
          }
        }
        view.dispatch({ selection: { anchor: l.from }, effects: [...effects, setQuote.of(decos)] })
      },
    }),
    [],
  )

  return (
    <div className="ide-editor-host">
      <div ref={divRef} className="ide-editor" style={props.diffMode ? { display: 'none' } : undefined} />
      <div ref={diffDivRef} className="ide-editor" style={props.diffMode ? undefined : { display: 'none' }} />
    </div>
  )
})

export default CmHost
