// The deliverable viewer in the center pane: read-only, every resolved ledger
// site a code lens above its line (opens the requirement in the inspector), and
// gutter marks against the file's generation baseline with a side-by-side diff
// toggle (docs/frontends/gui.md#deliverable-viewer).
import { useEffect, useMemo, useRef, useState } from 'react'
import { useParams, useSearchParams } from 'react-router'
import { useQuery } from '@tanstack/react-query'
import * as monaco from 'monaco-editor'
import '../ide/monaco-env'
import { get, type DelivOwners } from '../lib/api'
import { useDelivBaseline, useMatrix } from '../lib/queries'
import { useInspector } from '../lib/nav'
import { lineMarks } from '../lib/diff'
import '../routes/routes.css'

// A resolved site from the ledger: located against the current text (line is null
// when lost), never parsed out of the file itself.
export interface Site {
  line: number | null
  requirement: string
  kind: 'site' | 'test'
  located: 'exact' | 'moved' | 'lost'
  exists: boolean
}

export interface FileResp {
  path: string
  text?: string
  binary?: boolean
  size?: number
  sites?: Site[]
  owners: DelivOwners
}

// Same theme names MonacoHost defines; redefining with identical content is safe,
// and this covers the viewer being used before the editor ever mounted.
function applyTheme() {
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

function langFor(path: string): string {
  const ext = `.${path.split('.').pop() ?? ''}`
  for (const l of monaco.languages.getLanguages()) if (l.extensions?.includes(ext)) return l.id
  return 'plaintext'
}

// Self-contained read-only monaco viewer; reveal is handed up through a ref.
// Located sites render twice: a gutter decoration and a clickable code lens (the
// requirement id and its verification status) that opens the inspector. Models
// get a deliv:// uri so the global lens provider fires only for this viewer.
let delivSeq = 0

function ReadOnlyCode({
  path,
  text,
  baseline,
  diffMode,
  sites,
  status,
  onOpenNode,
  revealRef,
}: {
  path: string
  text: string
  baseline: string | null
  diffMode: boolean
  sites: Site[]
  status: (id: string) => string
  onOpenNode: (id: string) => void
  revealRef: React.MutableRefObject<((line: number) => void) | null>
}) {
  const divRef = useRef<HTMLDivElement>(null)
  const diffDivRef = useRef<HTMLDivElement>(null)
  const editorRef = useRef<monaco.editor.IStandaloneCodeEditor | null>(null)
  const diffEditorRef = useRef<monaco.editor.IStandaloneDiffEditor | null>(null)
  const sitesRef = useRef<Site[]>(sites)
  sitesRef.current = sites
  const statusRef = useRef(status)
  statusRef.current = status
  const openRef = useRef(onOpenNode)
  openRef.current = onOpenNode
  const lensFire = useRef<() => void>(() => {})

  useEffect(() => {
    const editor = monaco.editor.create(divRef.current!, {
      model: null,
      readOnly: true,
      automaticLayout: true,
      minimap: { enabled: false },
      fontSize: 13,
      scrollBeyondLastLine: false,
    })
    editorRef.current = editor
    const lensEmitter = new monaco.Emitter<monaco.languages.CodeLensProvider>()
    const lensProvider: monaco.languages.CodeLensProvider = {
      onDidChange: lensEmitter.event,
      provideCodeLenses: (model) => {
        if (model.uri.scheme !== 'deliv') return { lenses: [], dispose: () => {} }
        return {
          lenses: sitesRef.current
            .filter((s) => s.line !== null)
            .map((s) => ({
              range: new monaco.Range(s.line!, 1, s.line!, 1),
              command: {
                id: 'jazyk.deliverable.openNode',
                title: `${s.requirement} · ${statusRef.current(s.requirement)}`,
                arguments: [s.requirement],
              },
            })),
          dispose: () => {},
        }
      },
    }
    lensFire.current = () => lensEmitter.fire(lensProvider)
    const disposables: monaco.IDisposable[] = [
      lensEmitter,
      monaco.languages.registerCodeLensProvider('*', lensProvider),
      monaco.editor.registerCommand('jazyk.deliverable.openNode', (_accessor, id) => {
        if (typeof id === 'string') openRef.current(id)
      }),
    ]
    applyTheme()
    const mql = window.matchMedia('(prefers-color-scheme: dark)')
    mql.addEventListener('change', applyTheme)
    const mo = new MutationObserver(applyTheme)
    mo.observe(document.documentElement, { attributes: true, attributeFilter: ['data-theme'] })
    revealRef.current = (line) => {
      const model = editor.getModel()
      if (!model) return
      const l = Math.min(Math.max(line, 1), model.getLineCount())
      editor.revealLineInCenterIfOutsideViewport(l)
      editor.setPosition({ lineNumber: l, column: 1 })
    }
    return () => {
      mo.disconnect()
      mql.removeEventListener('change', applyTheme)
      revealRef.current = null
      lensFire.current = () => {}
      for (const d of disposables) d.dispose()
      diffEditorRef.current?.dispose()
      diffEditorRef.current = null
      editor.getModel()?.dispose()
      editor.dispose()
      editorRef.current = null
    }
  }, [revealRef])

  useEffect(() => {
    const editor = editorRef.current
    if (!editor) return
    const old = editor.getModel()
    const uri = monaco.Uri.parse(`deliv://f/${++delivSeq}/${path}`)
    const model = monaco.editor.createModel(text, langFor(path), uri)
    editor.setModel(model)
    old?.dispose()
    const decos: monaco.editor.IModelDeltaDecoration[] = sites
      .filter((s) => s.line !== null)
      .map((s) => ({
        range: new monaco.Range(s.line!, 1, s.line!, 1),
        options: {
          isWholeLine: true,
          linesDecorationsClassName:
            !s.exists || status(s.requirement).startsWith('stale') ? 'dmark-bad' : 'dmark-ok',
        },
      }))
    // Gutter marks against the generation baseline: what the last run changed.
    if (baseline !== null) {
      const marks = lineMarks(baseline, text)
      const lineCount = model.getLineCount()
      const push = (line: number, cls: string, tip: string) => {
        const l = Math.min(Math.max(line, 1), lineCount)
        decos.push({
          range: new monaco.Range(l, 1, l, 1),
          options: { linesDecorationsClassName: cls, linesDecorationsTooltip: tip },
        })
      }
      for (const l of marks.added) push(l, 'gd-add', 'added by the last generation')
      for (const l of marks.modified) push(l, 'gd-mod', 'changed by the last generation')
      for (const [l, n] of marks.deletedAbove) push(l, 'gd-del', `${n} line${n > 1 ? 's' : ''} removed here`)
    }
    model.deltaDecorations([], decos)
    lensFire.current()
  }, [path, text, sites, status, baseline])

  // The diff view: two throwaway read-only models, baseline against current.
  useEffect(() => {
    if (!diffMode || baseline === null) return
    if (!diffEditorRef.current && diffDivRef.current) {
      diffEditorRef.current = monaco.editor.createDiffEditor(diffDivRef.current, {
        automaticLayout: true,
        minimap: { enabled: false },
        fontSize: 13,
        scrollBeyondLastLine: false,
        renderSideBySide: true,
        readOnly: true,
        originalEditable: false,
      })
    }
    const de = diffEditorRef.current
    if (!de) return
    de.setModel({
      original: monaco.editor.createModel(baseline, langFor(path)),
      modified: monaco.editor.createModel(text, langFor(path)),
    })
    return () => {
      const m = de.getModel()
      de.setModel(null)
      m?.original.dispose()
      m?.modified.dispose()
    }
  }, [diffMode, baseline, text, path])

  return (
    <div className="ide-editor-host">
      <div ref={divRef} className="deliv-editor" style={diffMode ? { display: 'none' } : undefined} />
      <div ref={diffDivRef} className="deliv-editor" style={diffMode ? undefined : { display: 'none' }} />
    </div>
  )
}

export default function DelivFile() {
  const params = useParams()
  const filePath = params['*'] ?? ''
  const [searchParams, setSearchParams] = useSearchParams()
  const matrix = useMatrix()
  const { openNode } = useInspector()
  const [diffMode, setDiffMode] = useState(false)
  const rows = matrix.data?.rows ?? {}
  const statusOf = useMemo(() => {
    return (id: string) => rows[id]?.status ?? 'unverified'
  }, [rows])

  const fileQ = useQuery({
    queryKey: ['deliverable', 'file', filePath],
    queryFn: () => get<FileResp>(`/api/deliverable/file?path=${encodeURIComponent(filePath)}`),
    enabled: filePath !== '',
    staleTime: 5_000,
  })
  const baselineQ = useDelivBaseline(filePath)
  const revealRef = useRef<((line: number) => void) | null>(null)
  const open = fileQ.data

  // ?site=req:x reveals that requirement's first located site once per target.
  const site = searchParams.get('site')
  const revealedSite = useRef('')
  useEffect(() => {
    if (!site || !open || open.path !== filePath) return
    const key = `${filePath}|${site}`
    if (revealedSite.current === key) return
    const hit = (open.sites ?? []).find((s) => s.requirement === site && s.line !== null)
    if (!hit) return
    revealedSite.current = key
    requestAnimationFrame(() => revealRef.current?.(hit.line!))
  }, [site, open, filePath])

  // Reset the one-shot ?site= reveal when the target changes.
  useEffect(() => {
    if (!site) revealedSite.current = ''
  }, [site, filePath])

  // ?line=N reveals a line directly: where a requirement card's code or test link
  // lands (docs/frontends/gui.md#layout).
  const line = searchParams.get('line')
  const revealedLine = useRef('')
  useEffect(() => {
    if (!line) {
      revealedLine.current = ''
      return
    }
    if (!open || open.path !== filePath) return
    const key = `${filePath}|${line}`
    if (revealedLine.current === key) return
    const n = Number(line)
    if (!Number.isFinite(n) || n < 1) return
    revealedLine.current = key
    requestAnimationFrame(() => revealRef.current?.(n))
  }, [line, open, filePath])

  const baseline =
    baselineQ.data && !baselineQ.data.binary && typeof baselineQ.data.text === 'string'
      ? baselineQ.data.text
      : null

  if (!filePath) return <p className="muted deliv-pad">select a deliverable file</p>
  if (fileQ.error)
    return (
      <p className="error-inline deliv-pad">
        {fileQ.error.message}{' '}
        <a href="#retry" onClick={(e) => { e.preventDefault(); fileQ.refetch() }}>retry</a>
      </p>
    )
  if (!open) return <p className="muted deliv-pad">loading…</p>
  if (open.binary) return <p className="muted deliv-pad">binary file · {open.size ?? 0} bytes</p>

  return (
    <>
      <div className="ide-topbar">
        <span className="mono">{open.path}</span>
        <span className="muted">read-only</span>
        <div className="ide-topbar-right row">
          <button
            className={diffMode ? 'btn-on' : ''}
            disabled={baseline === null}
            title={
              baseline !== null
                ? 'what the last generation changed'
                : 'no baseline: generation has not rewritten this file'
            }
            onClick={() => setDiffMode((v) => !v)}
          >
            diff
          </button>
        </div>
      </div>
      <ReadOnlyCode
        path={open.path}
        text={open.text ?? ''}
        baseline={baseline}
        diffMode={diffMode && baseline !== null}
        sites={open.sites ?? []}
        status={statusOf}
        onOpenNode={(id) => {
          // Keep ?site= out of the way so the inspector's site links re-fire.
          if (searchParams.get('site')) {
            const next = new URLSearchParams(searchParams)
            next.delete('site')
            setSearchParams(next, { replace: true })
          }
          openNode(id)
        }}
        revealRef={revealRef}
      />
    </>
  )
}
