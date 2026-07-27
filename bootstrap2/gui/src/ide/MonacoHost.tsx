// Monaco host: one editor instance, one model per open document. LSP features are
// wired straight onto the hand-rolled client; providers translate between LSP
// 0-based positions and monaco 1-based ones.
import { forwardRef, useEffect, useImperativeHandle, useRef } from 'react'
import * as monaco from 'monaco-editor'
import EditorWorker from 'monaco-editor/esm/vs/editor/editor.worker?worker'
import type { DocRecord } from '../lib/api'
import type { LspClient, LspDiagnostic, LspRange } from './lsp-client'

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
  onDirty: (dirty: boolean) => void
  onNavigate: (path: string, line?: number) => void
  onMarkers: (markers: monaco.editor.IMarker[]) => void
  onSave: () => void
}

const MonacoHost = forwardRef<MonacoHandle, Props>(function MonacoHost(props, ref) {
  const divRef = useRef<HTMLDivElement>(null)
  const editorRef = useRef<monaco.editor.IStandaloneCodeEditor | null>(null)
  // Per-uri last loaded or saved text; dirty means the model diverged from it.
  const baselines = useRef(new Map<string, string>())
  const created = useRef(new Map<string, { model: monaco.editor.ITextModel; unsub: () => void }>())
  const covDecorations = useRef(new Map<string, string[]>())
  const quoteDecorations = useRef<string[]>([])
  const propsRef = useRef(props)
  propsRef.current = props

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

    disposables.push(
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
          const rel = uriToPath(propsRef.current.root, resource.toString())
          if (!rel) return false
          propsRef.current.onNavigate(rel)
          return true
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
      for (const d of disposables) d.dispose()
      for (const [uri, e] of created.current) {
        e.unsub()
        propsRef.current.lsp.didClose(uri)
        e.model.dispose()
      }
      created.current.clear()
      baselines.current.clear()
      covDecorations.current.clear()
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
      })
      const unsubDiag = propsRef.current.lsp.onDiagnostics(uri, (diags) => {
        monaco.editor.setModelMarkers(model, 'jazyk', diags.map(toMarker))
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
  }, [props.root, props.path])

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
        const cov = props.record.coverage[secRef]
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

  return <div ref={divRef} className="ide-editor" />
})

export default MonacoHost
