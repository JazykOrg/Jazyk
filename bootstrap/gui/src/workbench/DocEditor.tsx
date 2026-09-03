// The document editor in the center pane. Content loads under the ['docs', ...]
// query prefix so docs.changed invalidation refetches it: a clean editor reloads
// silently, a dirty one gets the conflict bar. The gutter marks changes against
// the reconciled baseline; the diff toggle shows baseline against current side by
// side (docs/frontends/gui.md#editor).
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useNavigate, useParams, useSearchParams } from 'react-router'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { get, put } from '../lib/api'
import { useCoverage, useDocBaseline, useDocs, useProject } from '../lib/queries'
import { useApp } from '../lib/store'
import { delivHref, outHref, useInspector } from '../lib/nav'
import { relTo } from '../lib/mdlinks'
import CmHost, { docUri, type EditorHandle, type LinkTarget, type Marker } from '../ide/CmHost'
import { LspClient } from '../ide/lsp-client'
import { tokenParam } from '../lib/api'
import MarkdownView from '../components/MarkdownView'
import '../ide/ide.css'
import '../components/markdown.css'

interface DocContent {
  path: string
  text: string
  hash: string
}

function contentPath(path: string): string {
  return `/api/docs/content?path=${encodeURIComponent(path)}`
}

// The preview toggle is a browser-wide preference, remembered across documents
// (docs/frontends/gui.md#markdown-preview).
const PREVIEW_KEY = 'jazyk-doc-preview'

function readPreviewPref(): boolean {
  try {
    return localStorage.getItem(PREVIEW_KEY) === '1'
  } catch {
    return false
  }
}

function sevClass(sev: Marker['severity']): string {
  return sev === 'error' ? 'sev-error' : sev === 'warning' ? 'sev-warning' : sev === 'info' ? 'sev-info' : 'sev-none'
}

export default function DocEditor() {
  const params = useParams()
  const docPath = params['*'] ?? ''
  const navigate = useNavigate()
  const [searchParams] = useSearchParams()
  const qc = useQueryClient()
  const projectQ = useProject()
  const docsQ = useDocs()
  const coverageQ = useCoverage()
  const baselineQ = useDocBaseline(docPath)
  const { openNode } = useInspector()
  const setEditorDirty = useApp((a) => a.setEditorDirty)
  const [diffMode, setDiffMode] = useState(false)
  const [preview, setPreview] = useState(readPreviewPref)
  // The text the preview renders: what the editor holds, a beat behind the keystrokes.
  const [previewText, setPreviewText] = useState('')
  const previewTimer = useRef<number | null>(null)
  const onText = useCallback((text: string) => {
    if (previewTimer.current !== null) window.clearTimeout(previewTimer.current)
    previewTimer.current = window.setTimeout(() => {
      previewTimer.current = null
      setPreviewText(text)
    }, 120)
  }, [])
  useEffect(
    () => () => {
      if (previewTimer.current !== null) window.clearTimeout(previewTimer.current)
    },
    [],
  )
  const togglePreview = useCallback(() => {
    setPreview((v) => {
      try {
        localStorage.setItem(PREVIEW_KEY, v ? '0' : '1')
      } catch {
        // a private window; the choice lives for this page only
      }
      return !v
    })
  }, [])

  // What the running build is doing to this document, if anything.
  const turns = useApp((a) => a.turns)
  const turnHold = useApp((a) => a.turnHold)
  const progress = useMemo(
    () => Object.values(turns).find((t) => t.doc === docPath),
    [turns, docPath],
  )
  const build = useMemo(
    () =>
      progress
        ? {
            label: progress.label,
            state: progress.state,
            sections: progress.sections,
            active: progress.active,
            result: progress.result,
          }
        : undefined,
    [progress],
  )

  const lsp = useMemo(() => new LspClient(), [])
  useEffect(() => {
    const proto = location.protocol === 'https:' ? 'wss' : 'ws'
    // Read the token per dial: a replacement token entered after a server
    // restart must reach the re-dial, not the URL captured at mount.
    lsp.connect(() => {
      const qs = tokenParam()
      return `${proto}://${location.host}/lsp${qs ? `?${qs}` : ''}`
    })
    return () => lsp.dispose()
  }, [lsp])

  const hostRef = useRef<EditorHandle>(null)
  const [loaded, setLoaded] = useState<DocContent | null>(null)
  const loadedRef = useRef(loaded)
  loadedRef.current = loaded
  // Per-document disk hash of the text the editor is based on; the save baseHash.
  const baseHashes = useRef(new Map<string, string>())
  const [dirty, setDirty] = useState(false)
  const dirtyRef = useRef(false)
  const [conflict, setConflict] = useState(false)
  const [saveErr, setSaveErr] = useState<string | null>(null)
  const [markers, setMarkers] = useState<Marker[]>([])
  const savingRef = useRef(false)
  const pendingLine = useRef<{ path: string; line: number } | null>(null)
  const revealedKey = useRef('')

  const rootRef = useRef<string | undefined>(undefined)
  rootRef.current = projectQ.data?.root
  const delivRef = useRef<string | undefined>(undefined)
  delivRef.current = projectQ.data?.deliverable
  const outRef = useRef<string | undefined>(undefined)
  outRef.current = projectQ.data?.out
  const docsRef = useRef<{ path: string }[]>([])
  docsRef.current = docsQ.data?.docs ?? []
  const docPathRef = useRef(docPath)
  docPathRef.current = docPath

  // The open doc's dirty state is global: tree ops in the explorer guard on it,
  // and it clears on unmount.
  useEffect(() => () => setEditorDirty(false), [setEditorDirty])

  const contentQ = useQuery({
    queryKey: ['docs', 'content', docPath],
    queryFn: () => get<DocContent>(contentPath(docPath)),
    enabled: docPath !== '',
    staleTime: 5_000,
  })

  const onDirty = useCallback(
    (d: boolean) => {
      dirtyRef.current = d
      setDirty(d)
      setEditorDirty(d)
    },
    [setEditorDirty],
  )

  // First load of a document. The baseHash is only seeded when absent: revisiting
  // a doc with unsaved edits must not silently adopt a newer disk hash.
  useEffect(() => {
    const data = contentQ.data
    if (!docPath || !data || loaded?.path === docPath) return
    setLoaded({ path: docPath, text: data.text, hash: data.hash })
    if (!baseHashes.current.has(docPath)) baseHashes.current.set(docPath, data.hash)
    setConflict(false)
    setSaveErr(null)
  }, [contentQ.data, docPath, loaded])

  // Disk changed under us (docs.changed refetch): reload silently when clean,
  // raise the conflict bar when dirty.
  useEffect(() => {
    const data = contentQ.data
    if (!docPath || !data || loaded?.path !== docPath) return
    const base = baseHashes.current.get(docPath)
    if (base === undefined || data.hash === base) return
    if (dirtyRef.current) {
      setConflict(true)
      return
    }
    baseHashes.current.set(docPath, data.hash)
    hostRef.current?.setText(data.text)
    setLoaded({ path: docPath, text: data.text, hash: data.hash })
    setConflict(false)
  }, [contentQ.data, docPath, loaded, dirty])

  const afterWrite = useCallback(
    (target: string, text: string, hash: string) => {
      baseHashes.current.set(target, hash)
      // Keep the cache in step so the reload effect does not see a stale hash.
      qc.setQueryData(['docs', 'content', target], { path: target, text, hash })
      hostRef.current?.markSaved()
      setConflict(false)
      setSaveErr(null)
      if (rootRef.current) lsp.didSave(docUri(rootRef.current, target))
    },
    [qc, lsp],
  )

  const save = useCallback(async () => {
    const target = loadedRef.current?.path
    const host = hostRef.current
    if (!target || !host || savingRef.current || !dirtyRef.current) return
    savingRef.current = true
    const text = host.getText()
    try {
      const r = await put<{ path: string; hash: string }>(contentPath(target), {
        text,
        baseHash: baseHashes.current.get(target),
      })
      afterWrite(target, text, r.hash)
    } catch (e) {
      if ((e as { conflict?: boolean }).conflict) setConflict(true)
      else setSaveErr((e as Error).message)
    } finally {
      savingRef.current = false
    }
  }, [afterWrite])

  // Overwrite: adopt the current disk hash, then write my text over it.
  const overwrite = useCallback(async () => {
    const target = loadedRef.current?.path
    const host = hostRef.current
    if (!target || !host) return
    const text = host.getText()
    try {
      const cur = await get<DocContent>(contentPath(target))
      const r = await put<{ path: string; hash: string }>(contentPath(target), {
        text,
        baseHash: cur.hash,
      })
      afterWrite(target, text, r.hash)
    } catch (e) {
      setSaveErr((e as Error).message)
    }
  }, [afterWrite])

  const reloadDiscard = useCallback(async () => {
    const target = loadedRef.current?.path
    if (!target) return
    try {
      const cur = await get<DocContent>(contentPath(target))
      baseHashes.current.set(target, cur.hash)
      qc.setQueryData(['docs', 'content', target], cur)
      hostRef.current?.setText(cur.text)
      setLoaded({ path: target, text: cur.text, hash: cur.hash })
      setConflict(false)
      setSaveErr(null)
    } catch (e) {
      setSaveErr((e as Error).message)
    }
  }, [qc])

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key === 's') {
        e.preventDefault()
        void save()
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [save])

  // Definition or link target in another document: switch route, ignore paths
  // that are not matched documents (e.g. generated docsgen output).
  const onNavigate = useCallback(
    (path: string, line?: number) => {
      if (path === docPathRef.current) {
        if (line !== undefined) hostRef.current?.revealLine(line)
        return
      }
      if (!docsRef.current.some((d) => d.path === path)) return
      if (line !== undefined) pendingLine.current = { path, line }
      navigate(`/files/docs/${path}`)
    },
    [navigate],
  )

  // A clicked link from a document link or the requirement card. The node wins (the
  // card's requirement link opens the inspector), then a matched document, then a
  // deliverable file at the line, then a file under the out directory on its
  // preview (docs/frontends/gui.md#editor).
  const onOpenLink = useCallback(
    (t: LinkTarget) => {
      if (t.node) {
        openNode(t.node)
        return
      }
      const root = rootRef.current
      const docRel = root ? relTo(root, t.path) : null
      if (docRel && docsRef.current.some((d) => d.path === docRel)) {
        onNavigate(docRel, t.line)
        return
      }
      const deliv = delivRef.current
      const delivRel = deliv ? relTo(deliv, t.path) : null
      if (delivRel) {
        navigate(delivHref(delivRel, undefined, t.line))
        return
      }
      const out = outRef.current
      const outRel = out ? relTo(out, t.path) : null
      if (outRel) navigate(outHref(outRel))
    },
    [navigate, onNavigate, openNode],
  )

  useEffect(() => {
    const p = pendingLine.current
    if (p && loaded?.path === p.path) {
      pendingLine.current = null
      requestAnimationFrame(() => hostRef.current?.revealLine(p.line))
    }
  }, [loaded])

  // ?section=&quote=: scroll to the section start, highlight the quote once per
  // distinct target.
  const section = searchParams.get('section')
  const quote = searchParams.get('quote')
  useEffect(() => {
    if (!section || !loaded || loaded.path !== docPath) return
    const sec = coverageQ.data?.[docPath]?.sections[section]
    if (!sec) return
    const key = `${docPath}|${section}|${quote ?? ''}`
    if (revealedKey.current === key) return
    revealedKey.current = key
    requestAnimationFrame(() => hostRef.current?.revealSection(sec.lines[0], quote ?? undefined))
  }, [section, quote, coverageQ.data, loaded, docPath])

  // The pointer inside a banded section holds that turn's result in place, the
  // same hold the files tree gives its rows (docs/frontends/gui.md#editor). The
  // pointer may already be inside when the turn ends, so the last hovered line is
  // kept and re-checked whenever the turn's state moves.
  const record = coverageQ.data?.[docPath]
  const hoverLine = useRef<number | null>(null)
  const evaluateHold = useCallback(() => {
    const t = Object.values(useApp.getState().turns).find((x) => x.doc === docPath)
    if (!t) return
    const line = hoverLine.current
    const inside =
      line !== null &&
      t.sections.some((ref) => {
        const sec = record?.sections[ref]
        return sec ? line >= sec.lines[0] && line <= sec.lines[1] : false
      })
    turnHold(t.label, inside)
  }, [docPath, record, turnHold])
  const onHoverLine = useCallback(
    (line: number | null) => {
      hoverLine.current = line
      evaluateHold()
    },
    [evaluateHold],
  )
  useEffect(() => {
    evaluateHold()
  }, [evaluateHold, progress?.label, progress?.state])

  const root = projectQ.data?.root
  const sortedMarkers = useMemo(() => [...markers].sort((a, b) => a.line - b.line), [markers])

  const baseline = baselineQ.data ? baselineQ.data.text : baselineQ.data === null ? null : undefined
  const hasBaseline = baseline !== null && baseline !== undefined

  if (!docPath) return <p className="empty ide-pad">select a document to edit</p>
  if (contentQ.isError && !loaded)
    return <p className="error-inline ide-pad">could not load {docPath}</p>
  if (!root || !loaded) return <p className="empty ide-pad">loading…</p>

  return (
    <>
      <div className="ide-topbar">
        <span className="mono">{loaded.path}</span>
        {dirty && (
          <span className="ide-dirty" title="unsaved changes">
            ●
          </span>
        )}
        {loaded.path !== docPath && <span className="muted">loading…</span>}
        {saveErr && <span className="error-inline ide-inline">{saveErr}</span>}
        <div className="ide-topbar-right row">
          <button
            className={preview ? 'btn-on' : ''}
            title={preview ? 'hide the rendered preview' : 'render the markdown beside the text, live'}
            onClick={togglePreview}
          >
            preview
          </button>
          <button
            className={diffMode ? 'btn-on' : ''}
            disabled={!hasBaseline}
            title={
              hasBaseline
                ? 'side-by-side diff against the last reconciled text'
                : 'no baseline: this document never reconciled'
            }
            onClick={() => setDiffMode((v) => !v)}
          >
            diff
          </button>
          <button onClick={() => void save()} disabled={!dirty || conflict}>
            save
          </button>
        </div>
      </div>
      {conflict && (
        <div className="ide-conflict">
          <span>the document changed on disk</span>
          <button onClick={() => void reloadDiscard()}>reload (discard mine)</button>
          <button onClick={() => void overwrite()}>overwrite</button>
        </div>
      )}
      <div className="ide-split">
        <CmHost
          ref={hostRef}
          root={root}
          path={loaded.path}
          initialText={loaded.text}
          lsp={lsp}
          record={coverageQ.data?.[loaded.path]}
          build={loaded.path === docPath ? build : undefined}
          onHoverLine={onHoverLine}
          baseline={baseline}
          diffMode={diffMode && hasBaseline}
          onDirty={onDirty}
          onText={onText}
          onNavigate={onNavigate}
          onOpenLink={onOpenLink}
          onOpenNode={openNode}
          onMarkers={setMarkers}
          onSave={() => void save()}
        />
        {preview && !diffMode && (
          <MarkdownView text={previewText} baseAbs={`${root}/${loaded.path}`} />
        )}
      </div>
      <div className="ide-problems">
        {sortedMarkers.length === 0 ? (
          <div className="ide-problem-none muted">no problems</div>
        ) : (
          sortedMarkers.map((m, i) => (
            <div key={i} className="ide-problem" onClick={() => hostRef.current?.revealLine(m.line)}>
              <span className={`mono ${sevClass(m.severity)}`}>{m.severity}</span>
              <span className="mono muted">
                {m.line}:{m.column}
              </span>
              <span className="ide-problem-msg">{m.message}</span>
            </div>
          ))
        )}
      </div>
    </>
  )
}
