// Monaco host: one editor instance, one model per open document. LSP features are
// wired straight onto the hand-rolled client; providers translate between LSP
// 0-based positions and monaco 1-based ones.
import { forwardRef, useEffect, useImperativeHandle, useRef } from 'react'
import * as monaco from 'monaco-editor'
// Inline the worker as a blob: Firefox refuses cross-context worker URLs from the
// bundled chunk (it mangles the base to https://[ff00::]/), and the main-thread
// fallback stutters on large docs.
import EditorWorker from 'monaco-editor/esm/vs/editor/editor.worker?worker&inline'
import type { DocRecord } from '../lib/api'
import { lineMarks } from '../lib/diff'
import type { LspClient, LspCodeLens, LspDiagnostic, LspDocumentLink, LspRange } from './lsp-client'

;(self as unknown as { MonacoEnvironment: monaco.Environment }).MonacoEnvironment = {
  getWorker: () => new EditorWorker(),
}

export function docUri(root: string, path: string): string {
  return `file://${root}/${path}`
}

function uriToPath(root: string, uri: string): string | null {
  const prefix = `file://${root}/`
  return uri.startsWith(prefix) ? uri.slice(prefix.length) : null
}

function toLspPos(p: monaco.Position) {
  return { line: p.lineNumber - 1, character: p.column - 1 }
}

function toMonacoRange(r: LspRange): monaco.Range {
  return new monaco.Range(r.start.line + 1, r.start.character + 1, r.end.line + 1, r.end.character + 1)
}

const SEVERITY: Record<number, monaco.MarkerSeverity> = {
  1: monaco.MarkerSeverity.Error,
  2: monaco.MarkerSeverity.Warning,
  3: monaco.MarkerSeverity.Info,
  4: monaco.MarkerSeverity.Hint,
}

function toMarker(d: LspDiagnostic): monaco.editor.IMarkerData {
  return {
    severity: SEVERITY[d.severity ?? 3] ?? monaco.MarkerSeverity.Info,
    message: d.message,
    source: d.source,
    code: d.code !== undefined ? String(d.code) : undefined,
    startLineNumber: d.range.start.line + 1,
    startColumn: d.range.start.character + 1,
    endLineNumber: d.range.end.line + 1,
    endColumn: d.range.end.character + 1,
  }
}

// Two named themes over the vs bases, editor background taken from the app palette.
function applyMonacoTheme() {
  const explicit = document.documentElement.dataset.theme
  const dark = explicit ? explicit === 'dark' : window.matchMedia('(prefers-color-scheme: dark)').matches
  const bg = getComputedStyle(document.documentElement).getPropertyValue('--panel').trim()
  const name = dark ? 'jazyk-dark' : 'jazyk-light'
  monaco.editor.defineTheme(name, {
    base: dark ? 'vs-dark' : 'vs',
    inherit: true,
    rules: [],
    colors: bg ? { 'editor.background': bg } : {},
  })
  monaco.editor.setTheme(name)
}

function escapeRegExp(s: string): string {
  return s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
}

// Whitespace-insensitive quote search: any whitespace run matches any other.
// Prefers a match at or after fromLine, falls back to the first in the document.
function findQuote(model: monaco.editor.ITextModel, quote: string, fromLine: number): monaco.Range | null {
  const words = quote.trim().split(/\s+/).filter((w) => w !== '')
  if (words.length === 0) return null
  const re = new RegExp(words.map(escapeRegExp).join('\\s+'))
  const text = model.getValue()
  const fromOffset = model.getOffsetAt({ lineNumber: Math.min(fromLine, model.getLineCount()), column: 1 })
  let m = re.exec(text.slice(fromOffset))
  let start: number
  if (m) start = fromOffset + m.index
  else {
    m = re.exec(text)
    if (!m) return null
    start = m.index
  }
  const s = model.getPositionAt(start)
  const e = model.getPositionAt(start + m[0].length)
  return new monaco.Range(s.lineNumber, s.column, e.lineNumber, e.column)
}

export interface MonacoHandle {
  getText(): string
  setText(text: string): void // replaces content and resets the dirty baseline
  markSaved(): void // current content becomes the baseline
  revealLine(line: number): void
  revealSection(startLine: number, quote?: string): void
}

interface Props {
  root: string
  path: string
  initialText: string
  lsp: LspClient
  record?: DocRecord
  // The reconciled baseline text: gutter marks show what the next compile sees as
  // dirty; null means the document never reconciled (no marks, no diff).
  baseline?: string | null
  // Swap the editor for a side-by-side diff of baseline against current text; the
  // current side stays the same shared model, so edits and undo survive.
  diffMode?: boolean
  onDirty: (dirty: boolean) => void
  onNavigate: (path: string, line?: number) => void
  // Route a node id (e.g. a docsgen link's entity) to its app page.
  onOpenNode?: (id: string) => void
  onMarkers: (markers: monaco.editor.IMarker[]) => void
  onSave: () => void
}

const MonacoHost = forwardRef<MonacoHandle, Props>(function MonacoHost(props, ref) {
  const divRef = useRef<HTMLDivElement>(null)
  const diffDivRef = useRef<HTMLDivElement>(null)
  const editorRef = useRef<monaco.editor.IStandaloneCodeEditor | null>(null)
  const diffEditorRef = useRef<monaco.editor.IStandaloneDiffEditor | null>(null)
  // Per-uri last loaded or saved text; dirty means the model diverged from it.
  const baselines = useRef(new Map<string, string>())
  const created = useRef(new Map<string, { model: monaco.editor.ITextModel; unsub: () => void }>())
  const covDecorations = useRef(new Map<string, string[]>())
  const linkDecorations = useRef(new Map<string, string[]>())
  const diffDecorations = useRef(new Map<string, string[]>())
  const diffTimer = useRef<number | null>(null)
  const quoteDecorations = useRef<string[]>([])
  const propsRef = useRef(props)
  propsRef.current = props

  // Gutter marks against the reconciled baseline: added, modified, and a strip
  // where baseline lines were deleted. Recomputed on load and (debounced) on edit.
  const recomputeDiffMarks = (model: monaco.editor.ITextModel) => {
    if (model.isDisposed()) return
    const p = propsRef.current
    const uri = model.uri.toString()
    if (uri !== docUri(p.root, p.path)) return
    const old = diffDecorations.current.get(uri) ?? []
    if (p.baseline === null || p.baseline === undefined) {
      diffDecorations.current.set(uri, model.deltaDecorations(old, []))
      return
    }
    const marks = lineMarks(p.baseline, model.getValue())
    const lineCount = model.getLineCount()
    const next: monaco.editor.IModelDeltaDecoration[] = []
    const push = (line: number, cls: string, tip: string) => {
      const l = Math.min(Math.max(line, 1), lineCount)
      next.push({
        range: new monaco.Range(l, 1, l, 1),
        options: { linesDecorationsClassName: cls, linesDecorationsTooltip: tip },
      })
    }
    for (const l of marks.added) push(l, 'gd-add', 'added since the last reconcile')
    for (const l of marks.modified) push(l, 'gd-mod', 'changed since the last reconcile')
    for (const [l, n] of marks.deletedAbove) push(l, 'gd-del', `${n} line${n > 1 ? 's' : ''} deleted here`)
    diffDecorations.current.set(uri, model.deltaDecorations(old, next))
  }
  const scheduleDiffMarks = (model: monaco.editor.ITextModel) => {
    if (diffTimer.current !== null) window.clearTimeout(diffTimer.current)
    diffTimer.current = window.setTimeout(() => recomputeDiffMarks(model), 250)
  }
  // Re-queries requirement lenses; wired to the code lens provider's change event so
  // a committed build refreshes lens titles (verification status) without an edit.
  const lensRefresh = useRef<() => void>(() => {})

  // Always-visible entity marks over LSP document link ranges; kept apart from the
  // coverage gutter so the two never clobber each other.
  const decorateLinks = (model: monaco.editor.ITextModel, links: LspDocumentLink[]) => {
    if (model.isDisposed()) return
    const uri = model.uri.toString()
    const old = linkDecorations.current.get(uri) ?? []
    const next = links.map((l) => ({
      range: toMonacoRange(l.range),
      options: { inlineClassName: 'entity-mark' },
    }))
    linkDecorations.current.set(uri, model.deltaDecorations(old, next))
  }
  const refreshLinkMarks = async (model: monaco.editor.ITextModel) => {
    try {
      decorateLinks(model, await propsRef.current.lsp.documentLinks(model.uri.toString()))
    } catch {
      // offline; marks stay as they were
    }
  }

  // Editor, providers, and theme watcher live for the component's lifetime.
  useEffect(() => {
    const editor = monaco.editor.create(divRef.current!, {
      model: null,
      automaticLayout: true,
      minimap: { enabled: false },
      wordWrap: 'on',
      fontSize: 13,
      scrollBeyondLastLine: false,
      fixedOverflowWidgets: true,
    })
    editorRef.current = editor
    editor.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyCode.KeyS, () => propsRef.current.onSave())

    const lspUri = (model: monaco.editor.ITextModel) => model.uri.toString()
    const disposables: monaco.IDisposable[] = []

    // Requirement attachments as code lenses over their quotes. The lens command
    // routes to the requirement's node page, the richer view inside the app.
    const lensEmitter = new monaco.Emitter<monaco.languages.CodeLensProvider>()
    const lensProvider: monaco.languages.CodeLensProvider = {
      onDidChange: lensEmitter.event,
      provideCodeLenses: async (model) => {
        try {
          const lenses: LspCodeLens[] = await propsRef.current.lsp.codeLens(lspUri(model))
          return {
            lenses: lenses.map((l) => ({
              range: toMonacoRange(l.range),
              command: l.command
                ? { id: l.command.command, title: l.command.title, arguments: l.command.arguments }
                : undefined,
            })),
            dispose: () => {},
          }
        } catch {
          return { lenses: [], dispose: () => {} }
        }
      },
    }
    lensRefresh.current = () => lensEmitter.fire(lensProvider)

    disposables.push(
      lensEmitter,
      monaco.languages.registerCodeLensProvider('markdown', lensProvider),
      monaco.editor.registerCommand('jazyk.openRequirement', (_accessor, rid) => {
        if (typeof rid === 'string') propsRef.current.onOpenNode?.(rid)
      }),
      monaco.languages.registerHoverProvider('markdown', {
        provideHover: async (model, position) => {
          try {
            const h = await propsRef.current.lsp.hover(lspUri(model), toLspPos(position))
            if (!h) return null
            const value = typeof h.contents === 'string' ? h.contents : h.contents.value
            return { contents: [{ value }], range: h.range ? toMonacoRange(h.range) : undefined }
          } catch {
            return null
          }
        },
      }),
      monaco.languages.registerDefinitionProvider('markdown', {
        provideDefinition: async (model, position) => {
          try {
            const locs = await propsRef.current.lsp.definition(lspUri(model), toLspPos(position))
            return locs.map((l) => ({ uri: monaco.Uri.parse(l.uri), range: toMonacoRange(l.range) }))
          } catch {
            return null
          }
        },
      }),
      monaco.languages.registerReferenceProvider('markdown', {
        provideReferences: async (model, position) => {
          try {
            const locs = await propsRef.current.lsp.references(lspUri(model), toLspPos(position))
            return locs.map((l) => ({ uri: monaco.Uri.parse(l.uri), range: toMonacoRange(l.range) }))
          } catch {
            return null
          }
        },
      }),
      monaco.languages.registerCompletionItemProvider('markdown', {
        triggerCharacters: ['`', '['],
        provideCompletionItems: async (model, position) => {
          try {
            const items = await propsRef.current.lsp.completion(lspUri(model), toLspPos(position))
            const word = model.getWordUntilPosition(position)
            const wordRange = new monaco.Range(
              position.lineNumber,
              word.startColumn,
              position.lineNumber,
              word.endColumn,
            )
            return {
              suggestions: items.map((it) => ({
                label: it.label,
                kind: monaco.languages.CompletionItemKind.Text,
                detail: it.detail,
                documentation: typeof it.documentation === 'string' ? it.documentation : it.documentation?.value,
                insertText: it.textEdit?.newText ?? it.insertText ?? it.label,
                filterText: it.filterText,
                range: it.textEdit ? toMonacoRange(it.textEdit.range) : wordRange,
              })),
            }
          } catch {
            return { suggestions: [] }
          }
        },
      }),
      monaco.languages.registerLinkProvider('markdown', {
        provideLinks: async (model) => {
          try {
            const links = await propsRef.current.lsp.documentLinks(lspUri(model))
            decorateLinks(model, links)
            return {
              links: links.map((l) => ({
                range: toMonacoRange(l.range),
                url: l.target,
                tooltip: l.tooltip,
              })),
            }
          } catch {
            return { links: [] }
          }
        },
      }),
      // Cross-document definition and reference targets route through the app.
      monaco.editor.registerEditorOpener({
        openCodeEditor: (_source, resource, selectionOrPosition) => {
          const rel = uriToPath(propsRef.current.root, resource.toString())
          if (!rel) return false
          let line: number | undefined
          if (selectionOrPosition) {
            line =
              'startLineNumber' in selectionOrPosition
                ? selectionOrPosition.startLineNumber
                : selectionOrPosition.lineNumber
          }
          propsRef.current.onNavigate(rel, line)
          return true
        },
      }),
      // Document links target files under the root; the route owner decides whether
      // the path is an editable document.
      monaco.editor.registerLinkOpener({
        open: (resource) => {
          const target = resource.toString()
          const rel = uriToPath(propsRef.current.root, target)
          if (rel) {
            propsRef.current.onNavigate(rel)
            return true
          }
          // A docsgen link names an entity's requirements document; land on the
          // entity page instead of a file the browser cannot open.
          const docsgen = target.match(/\/docsgen\/([a-z0-9-]+)\.md$/)
          if (docsgen && propsRef.current.onOpenNode) {
            propsRef.current.onOpenNode(`ent:${docsgen[1]}`)
            return true
          }
          // Never hand a file:// target to the browser; it is blocked and noisy.
          return target.startsWith('file://')
        },
      }),
      monaco.editor.onDidChangeMarkers((uris) => {
        const model = editorRef.current?.getModel()
        if (model && uris.some((u) => u.toString() === model.uri.toString()))
          propsRef.current.onMarkers(monaco.editor.getModelMarkers({ resource: model.uri }))
      }),
    )

    applyMonacoTheme()
    const mql = window.matchMedia('(prefers-color-scheme: dark)')
    mql.addEventListener('change', applyMonacoTheme)
    const mo = new MutationObserver(applyMonacoTheme)
    mo.observe(document.documentElement, { attributes: true, attributeFilter: ['data-theme'] })

    return () => {
      mo.disconnect()
      mql.removeEventListener('change', applyMonacoTheme)
      lensRefresh.current = () => {}
      for (const d of disposables) d.dispose()
      // The diff editor must let go of the shared models before they are disposed.
      diffEditorRef.current?.dispose()
      diffEditorRef.current = null
      for (const [uri, e] of created.current) {
        e.unsub()
        propsRef.current.lsp.didClose(uri)
        e.model.dispose()
      }
      created.current.clear()
      baselines.current.clear()
      covDecorations.current.clear()
      linkDecorations.current.clear()
      diffDecorations.current.clear()
      for (const m of monaco.editor.getModels())
        if (m.uri.scheme === 'jazyk-base') m.dispose()
      editor.dispose()
      editorRef.current = null
    }
  }, [])

  // Model per document: created on first open (with an LSP didOpen), reused after,
  // so unsaved edits and undo history survive switching documents.
  useEffect(() => {
    const editor = editorRef.current
    if (!editor || !props.path) return
    const uri = docUri(props.root, props.path)
    const muri = monaco.Uri.parse(uri)
    let entry = created.current.get(uri)
    if (!entry) {
      const model = monaco.editor.getModel(muri) ?? monaco.editor.createModel(props.initialText, 'markdown', muri)
      baselines.current.set(uri, props.initialText)
      propsRef.current.lsp.didOpen(uri, model.getValue())
      const sub = model.onDidChangeContent(() => {
        const text = model.getValue()
        propsRef.current.lsp.didChange(uri, text)
        propsRef.current.onDirty(text !== baselines.current.get(uri))
        scheduleDiffMarks(model)
      })
      const unsubDiag = propsRef.current.lsp.onDiagnostics(uri, (diags) => {
        monaco.editor.setModelMarkers(model, 'jazyk', diags.map(toMarker))
        // The server republishes on generation bumps; entity marks and requirement
        // lenses refresh with it.
        void refreshLinkMarks(model)
        lensRefresh.current()
      })
      entry = {
        model,
        unsub: () => {
          sub.dispose()
          unsubDiag()
        },
      }
      created.current.set(uri, entry)
    }
    editor.setModel(entry.model)
    propsRef.current.onDirty(entry.model.getValue() !== baselines.current.get(uri))
    propsRef.current.onMarkers(monaco.editor.getModelMarkers({ resource: muri }))
    void refreshLinkMarks(entry.model)
  }, [props.root, props.path])

  // Reconciled-baseline gutter marks follow the baseline and the open document.
  useEffect(() => {
    const model = editorRef.current?.getModel()
    if (model) recomputeDiffMarks(model)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [props.baseline, props.path])

  // The diff view: lazily created, sharing the live model on the modified side.
  useEffect(() => {
    if (!props.diffMode) return
    if (!diffEditorRef.current && diffDivRef.current) {
      const de = monaco.editor.createDiffEditor(diffDivRef.current, {
        automaticLayout: true,
        minimap: { enabled: false },
        fontSize: 13,
        scrollBeyondLastLine: false,
        renderSideBySide: true,
        originalEditable: false,
        fixedOverflowWidgets: true,
      })
      de.getModifiedEditor().addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyCode.KeyS, () =>
        propsRef.current.onSave(),
      )
      diffEditorRef.current = de
    }
    const de = diffEditorRef.current
    const entry = created.current.get(docUri(props.root, props.path))
    if (!de || !entry || props.baseline === null || props.baseline === undefined) return
    const buri = monaco.Uri.parse(`jazyk-base:///${props.path}`)
    let base = monaco.editor.getModel(buri)
    if (!base) base = monaco.editor.createModel(props.baseline, 'markdown', buri)
    else if (base.getValue() !== props.baseline) base.setValue(props.baseline)
    de.setModel({ original: base, modified: entry.model })
  }, [props.diffMode, props.baseline, props.root, props.path])

  useEffect(
    () => () => {
      if (diffTimer.current !== null) window.clearTimeout(diffTimer.current)
    },
    [],
  )

  // Coverage gutter from the section tree. Section lines refer to the last
  // reconciled text: when the editor text hash differs from graphHash the gutter
  // can drift against edits; acceptable, it heals when the next build commits.
  useEffect(() => {
    const model = editorRef.current?.getModel()
    if (!model) return
    const uri = model.uri.toString()
    const next: monaco.editor.IModelDeltaDecoration[] = []
    if (props.record) {
      const lineCount = model.getLineCount()
      for (const [secRef, sec] of Object.entries(props.record.sections)) {
        // A reconciled doc with nothing covered serializes without a coverage map.
        const cov = (props.record.coverage ?? {})[secRef]
        const cls = cov ? (cov.state === 'covered' ? 'cov-covered' : 'cov-nonnorm') : 'cov-unprocessed'
        const [start, end] = sec.lines
        if (start > lineCount) continue
        next.push({
          range: new monaco.Range(start, 1, Math.min(end, lineCount), 1),
          options: {
            linesDecorationsClassName: cls,
            linesDecorationsTooltip: cov?.note ?? null,
          },
        })
      }
    }
    const old = covDecorations.current.get(uri) ?? []
    covDecorations.current.set(uri, model.deltaDecorations(old, next))
  }, [props.record, props.path])

  useImperativeHandle(
    ref,
    () => ({
      getText: () => editorRef.current?.getModel()?.getValue() ?? '',
      setText: (text) => {
        const model = editorRef.current?.getModel()
        if (!model) return
        baselines.current.set(model.uri.toString(), text)
        if (model.getValue() !== text) model.setValue(text)
        propsRef.current.onDirty(false)
      },
      markSaved: () => {
        const model = editorRef.current?.getModel()
        if (!model) return
        baselines.current.set(model.uri.toString(), model.getValue())
        propsRef.current.onDirty(false)
      },
      revealLine: (line) => {
        const editor = editorRef.current
        const model = editor?.getModel()
        if (!editor || !model) return
        const l = Math.min(Math.max(line, 1), model.getLineCount())
        editor.revealLineInCenterIfOutsideViewport(l)
        editor.setPosition({ lineNumber: l, column: 1 })
        editor.focus()
      },
      revealSection: (startLine, quote) => {
        const editor = editorRef.current
        const model = editor?.getModel()
        if (!editor || !model) return
        const line = Math.min(Math.max(startLine, 1), model.getLineCount())
        editor.revealLineNearTop(line)
        editor.setPosition({ lineNumber: line, column: 1 })
        let next: monaco.editor.IModelDeltaDecoration[] = []
        if (quote) {
          const range = findQuote(model, quote, line)
          if (range) {
            next = [{ range, options: { className: 'quote-hit' } }]
            editor.revealRangeInCenterIfOutsideViewport(range)
          }
        }
        quoteDecorations.current = model.deltaDecorations(quoteDecorations.current, next)
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

export default MonacoHost
