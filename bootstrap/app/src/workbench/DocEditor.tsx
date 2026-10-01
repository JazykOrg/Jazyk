// The document editor in the center pane. Content loads under the ['docs', ...]
// query prefix so docs.changed invalidation refetches it: a clean editor reloads
// silently, a dirty one gets the conflict bar. The gutter marks changes against
// the reconciled baseline; the diff toggle shows baseline against current side by
// side. One toggle, Read or Write: Write is the editor, Read the rendered page in
// its place with the same markers, underlines, bands, and read states
// (docs/frontends/gui.md#editor).
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useLocation, useNavigate, useParams, useSearchParams } from 'react-router'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { get, put } from '../lib/api'
import { useCoverage, useDocBaseline, useDocs, useGraph, useMatrix, useProject } from '../lib/queries'
import { useInlineBands, useReqMarks } from '../ide/overlays'
import { useApp } from '../lib/store'
import DocModeSwitch, { useDocMode } from '../components/DocModeSwitch'
import { ArrangeButton } from '../workspace/ItemControls'
import { useCapabilities } from '../lib/capabilities'
import { delivHref, outHref, useInspector } from '../lib/nav'
import { relTo } from '../lib/mdlinks'
import { buildMentionIndex, findQuoteIn, lineOf, quoteLine } from '../lib/mentions'
import { sectionBySlug, sectionLines } from '../lib/sections'
import CmHost, { buildBar, docUri, type EditorHandle, type FlashLines, type LinkTarget, type Marker } from '../ide/CmHost'
import { LspClient } from '../ide/lsp-client'
import { tokenParam } from '../lib/api'
import { serverBase } from '../lib/backend'
import MarkdownView, { type MarkdownViewHandle, type ReadDecor } from '../components/MarkdownView'
import { REVEAL_EVENT } from '../shell/Outline'
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

function sevClass(sev: Marker['severity']): string {
  return sev === 'error' ? 'sev-error' : sev === 'warning' ? 'sev-warning' : sev === 'info' ? 'sev-info' : 'sev-none'
}

const lastRevealed = new Map<string, string>()

export default function DocEditor() {
  const params = useParams()
  const docPath = params['*'] ?? ''
  const navigate = useNavigate()
  const [searchParams] = useSearchParams()
  const loc = useLocation()
  const qc = useQueryClient()
  const projectQ = useProject()
  const docsQ = useDocs()
  const coverageQ = useCoverage()
  const baselineQ = useDocBaseline(docPath)
  const { openNode } = useInspector()
  const setEditorDirty = useApp((a) => a.setEditorDirty)
  const graphQ = useGraph()
  const matrixQ = useMatrix()
  const marks = useReqMarks(graphQ.data, matrixQ.data?.rows ?? {}, docPath)
  const bands = useInlineBands(graphQ.data, docPath)
  const mentions = useMemo(() => (graphQ.data ? buildMentionIndex(graphQ.data.entities) : undefined), [graphQ.data])
  // The text Read renders: what the editor holds, a beat behind the keystrokes.
  const [readText, setReadText] = useState('')
  const previewTimer = useRef<number | null>(null)
  // Read or Write: the last choice for this document, else the default. A folder
  // opened read-only reads; everything else writes (docs/frontends/gui.md#editor).
  const mode = useDocMode()
  const reading = mode === 'read'
  const mdRef = useRef<MarkdownViewHandle>(null)
  // Where the reader is, for the outline: the cursor's line in Write, the top of
  // the view in Read, a beat behind the movement.
  const setEditorLine = useApp((a) => a.setEditorLine)
  const lineTimer = useRef<number | null>(null)
  const reportLine = useCallback(
    (line: number) => {
      if (lineTimer.current !== null) window.clearTimeout(lineTimer.current)
      lineTimer.current = window.setTimeout(() => {
        lineTimer.current = null
        setEditorLine(docPath, line)
      }, 120)
    },
    [docPath, setEditorLine],
  )
  useEffect(
    () => () => {
      if (lineTimer.current !== null) window.clearTimeout(lineTimer.current)
    },
    [],
  )
  // The entity under the caret, for the board's highlight; cleared when the
  // document closes.
  const setCaret = useApp((a) => a.setCaret)
  const reportCaret = useCallback((entity: string | null, reqs: string[]) => setCaret(docPath, entity, reqs), [docPath, setCaret])
  useEffect(() => () => setCaret(docPath, null, []), [docPath, setCaret])
  // Autosave: in the browser, where a write lands without asking (the demo, and a
  // folder in writing mode), the editor saves a beat after typing stops and when the
  // document is left. A read-only folder saves on request and asks whether to write
  // (docs/frontends/gui.md#editor).
  const caps = useCapabilities()
  const folder = useApp((a) => a.folder)
  const autosave = !!caps.data && caps.data.backend !== 'server' && (!folder || folder.mode === 'write')
  const autosaveRef = useRef(autosave)
  autosaveRef.current = autosave
  const saveTimer = useRef<number | null>(null)
  const saveRef = useRef<() => Promise<void>>(async () => {})
  const lastText = useRef<string | null>(null)
  const [askWrite, setAskWrite] = useState(false)
  const onText = useCallback((text: string) => {
    lastText.current = text
    if (autosaveRef.current) {
      if (saveTimer.current !== null) window.clearTimeout(saveTimer.current)
      saveTimer.current = window.setTimeout(() => {
        saveTimer.current = null
        void saveRef.current()
      }, 800)
    }
    if (previewTimer.current !== null) window.clearTimeout(previewTimer.current)
    previewTimer.current = window.setTimeout(() => {
      previewTimer.current = null
      setReadText(text)
    }, 120)
  }, [])
  useEffect(
    () => () => {
      if (previewTimer.current !== null) window.clearTimeout(previewTimer.current)
    },
    [],
  )

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
            held: progress.held,
          }
        : undefined,
    [progress],
  )

  const lsp = useMemo(() => new LspClient(), [])
  // Only a backend that serves `/lsp` gets a connection; without one the editor
  // derives its decorations from the graph (docs/frontends/gui.md#editor).
  const hasLsp = caps.data?.lsp === true
  useEffect(() => {
    if (!hasLsp) return
    // The socket is the connected server's (`ws://127.0.0.1:<port>/lsp`), the page
    // being on another origin (docs/frontends/gui.md#connecting-to-a-local-jazyk).
    // Read the base and the token per dial: a replacement token entered after a
    // server restart must reach the re-dial, not the URL captured at mount.
    lsp.connect(() => {
      const base = serverBase()
      if (!base) return ''
      const qs = tokenParam()
      return `${base.replace(/^http/, 'ws')}/lsp${qs ? `?${qs}` : ''}`
    })
    return () => lsp.dispose()
  }, [lsp, hasLsp])

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
  // The target last revealed in each document, kept across editors: an editor that
  // mounts again (a board window going live) does not jump back to it.
  const revealedKey = useMemo(() => ({ get current() { return lastRevealed.get(docPath) ?? '' }, set current(v: string) { lastRevealed.set(docPath, v) } }), [docPath])

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

  // Disk changed under us (docs.changed refetch, or a write this page made
  // elsewhere): when clean, the new text lands as the smallest edit, the scroll and
  // the selection kept, and the changed lines are marked until the mark fades; when
  // dirty, the conflict bar rises (docs/frontends/gui.md#editor).
  const addFlash = useApp((a) => a.addFlash)
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
    const lines = hostRef.current?.applyExternal(data.text) ?? null
    setLoaded({ path: docPath, text: data.text, hash: data.hash })
    setConflict(false)
    if (!lines) return
    // An applied option already named the text it landed; the lines add nothing.
    const app = useApp.getState()
    const named = app.flashes.some((f) => f.doc === docPath && f.text && !f.fading)
    if (!named) addFlash({ doc: docPath, label: app.changeLabel(), lines })
  }, [contentQ.data, docPath, loaded, dirty, addFlash])

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
    // Leaving the document can unmount the editor before its last save runs; the
    // latest text is kept for that.
    const text = host?.getText() ?? lastText.current
    if (!target || text === null || savingRef.current || !dirtyRef.current) return
    savingRef.current = true
    try {
      const r = await put<{ path: string; hash: string }>(contentPath(target), {
        text,
        baseHash: baseHashes.current.get(target),
      })
      afterWrite(target, text, r.hash)
      if (!autosaveRef.current && useApp.getState().folder?.mode === 'read') setAskWrite(true)
    } catch (e) {
      if ((e as { conflict?: boolean }).conflict) setConflict(true)
      else setSaveErr((e as Error).message)
    } finally {
      savingRef.current = false
    }
  }, [afterWrite])

  saveRef.current = save

  // Leaving a document (another one, or the Docs lens) saves what autosave had not
  // reached yet.
  useEffect(
    () => () => {
      if (saveTimer.current !== null) {
        window.clearTimeout(saveTimer.current)
        saveTimer.current = null
      }
      if (autosaveRef.current && dirtyRef.current) void saveRef.current()
    },
    [docPath],
  )
  useEffect(() => setAskWrite(false), [docPath])

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
      navigate(`/docs/${path}`)
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

  // Scroll to a section: the editor to its first line (the quote highlighted when
  // one is named), the Read page to the block holding it.
  const readingRef = useRef(reading)
  readingRef.current = reading
  const reveal = useCallback(
    (ref: string, quoteText?: string) => {
      const sec = coverageQ.data?.[docPathRef.current]?.sections[ref]
      if (!sec) return
      const [start] = sectionLines(sec)
      requestAnimationFrame(() => {
        if (readingRef.current) {
          mdRef.current?.revealLine(start)
          // The reader is at the section now, whether or not the page could
          // scroll it to the top.
          if (lineTimer.current !== null) window.clearTimeout(lineTimer.current)
          lineTimer.current = null
          setEditorLine(docPathRef.current, start)
        } else hostRef.current?.revealSection(start, quoteText)
      })
    },
    [coverageQ.data, setEditorLine],
  )

  // ?section=&quote=: once per distinct target.
  const section = searchParams.get('section')
  const quote = searchParams.get('quote')
  useEffect(() => {
    if (!section || !loaded || loaded.path !== docPath) return
    if (!coverageQ.data?.[docPath]?.sections[section]) return
    const key = `${docPath}|${section}|${quote ?? ''}`
    if (revealedKey.current === key) return
    revealedKey.current = key
    reveal(section, quote ?? undefined)
  }, [section, quote, coverageQ.data, loaded, docPath, reveal])

  // The outline's click, even when the route already names the section.
  useEffect(() => {
    const onReveal = (e: Event) => {
      const d = (e as CustomEvent<{ doc: string; section: string }>).detail
      if (d && d.doc === docPathRef.current) reveal(d.section)
    }
    window.addEventListener(REVEAL_EVENT, onReveal)
    return () => window.removeEventListener(REVEAL_EVENT, onReveal)
  }, [reveal])

  // A link's `#fragment`: the heading with that slug (docs/frontends/gui.md#markdown-preview).
  const fragment = loc.hash.replace(/^#/, '')
  useEffect(() => {
    if (!fragment || !loaded || loaded.path !== docPath) return
    const key = `${docPath}#${fragment}`
    if (revealedKey.current === key) return
    revealedKey.current = key
    if (readingRef.current) {
      requestAnimationFrame(() => mdRef.current?.revealSlug(fragment))
      return
    }
    const sections = coverageQ.data?.[docPath]?.sections
    const ref = sections ? sectionBySlug(sections, fragment) : null
    if (ref) reveal(ref)
  }, [fragment, loaded, docPath, coverageQ.data, reveal])

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

  // The marks over this document's changes, as line ranges in the text it holds.
  // Showing the document starts their hold and fade.
  const allFlashes = useApp((a) => a.flashes)
  const armFlashes = useApp((a) => a.armFlashes)
  useEffect(() => {
    if (docPath && allFlashes.some((f) => f.doc === docPath && !f.armed)) armFlashes(docPath)
  }, [allFlashes, docPath, armFlashes])
  const flashLines = useMemo<FlashLines[]>(() => {
    const out: FlashLines[] = []
    for (const f of allFlashes) {
      if (f.doc !== docPath) continue
      let range = f.lines ?? null
      if (f.text) {
        const hit = findQuoteIn(readText, f.text)
        range = hit ? [lineOf(readText, hit.from), lineOf(readText, hit.to)] : null
      }
      if (range) out.push({ key: String(f.id), from: range[0], to: range[1], label: f.label, fading: f.fading })
    }
    return out
  }, [allFlashes, docPath, readText])

  // What Read draws over the blocks: the read state and the build band of the
  // section a block starts in, the requirement markers by their quote's line, and
  // the bands under their sentences (docs/frontends/gui.md#editor).
  const decor = useMemo<ReadDecor | undefined>(() => {
    if (!reading) return undefined
    const sections = record?.sections ?? {}
    const coverage = record?.coverage ?? {}
    const secAt = (line: number) => {
      let best: string | null = null
      let bestStart = -1
      for (const [ref, sec] of Object.entries(sections)) {
        const [start, end] = sectionLines(sec)
        if (line >= start && line <= end && start > bestStart) {
          best = ref
          bestStart = start
        }
      }
      return best
    }
    const lineClass = (line: number) => {
      const ref = secAt(line)
      if (!ref) return ''
      const cov = coverage[ref]
      const cls = [cov ? (cov.state === 'covered' ? 'md-cov-covered' : 'md-cov-nonnorm') : 'md-cov-unprocessed']
      // The build's bar beside the block, never a tint over it.
      if (build && build.sections.includes(ref)) cls.push(buildBar(build, ref).cls)
      return cls.join(' ')
    }
    const lineTitle = (line: number) => {
      const ref = secAt(line)
      if (!ref) return undefined
      if (build && build.sections.includes(ref)) return buildBar(build, ref).tip
      return coverage[ref]?.note
    }
    const markMap = new Map<number, { rid: string; cls: string; tip: string }>()
    for (const m of marks) {
      const line = quoteLine(readText, m.quote)
      if (line !== null && !markMap.has(line)) markMap.set(line, { rid: m.rid, cls: m.cls, tip: m.tip })
    }
    const bandMap = new Map<number, typeof bands>()
    for (const b of bands) {
      let line: number | null = null
      if (b.quote) {
        const hit = findQuoteIn(readText, b.quote)
        if (hit) line = lineOf(readText, hit.to)
      }
      if (line === null && b.line !== undefined) line = b.line
      if (line === null) continue
      bandMap.set(line, [...(bandMap.get(line) ?? []), b])
    }
    const flash = (start: number, end: number) => {
      const f = flashLines.find((x) => x.from <= end && x.to >= start)
      return f ? { cls: `md-flash${f.fading ? ' fading' : ''}`, title: f.label } : null
    }
    return { lineClass, lineTitle, marks: markMap, bands: bandMap, flash }
  }, [reading, record, build, marks, bands, readText, flashLines])

  const baseline = baselineQ.data ? baselineQ.data.text : baselineQ.data === null ? null : undefined
  const hasBaseline = baseline !== null && baseline !== undefined

  // A requirement's line on the board reveals its quote here: the caret moves to
  // it in Write, the view scrolls to it in Read (docs/frontends/gui.md#the-linkage).
  const pendingReveal = useApp((a) => a.reveal)
  const revealState = useRef({ reading, readText })
  revealState.current = { reading, readText }
  useEffect(() => {
    if (!pendingReveal || pendingReveal.doc !== docPath || loaded?.path !== docPath) return
    const quote = pendingReveal.quote
    useApp.getState().takeReveal(pendingReveal.n)
    // A beat for the editor to take the text and its marks; taking the reveal must
    // not cancel it.
    window.setTimeout(() => {
      const { reading: r, readText: t } = revealState.current
      if (r) {
        const line = quoteLine(t, quote)
        if (line) mdRef.current?.revealLine(line)
      } else hostRef.current?.revealQuote(quote)
    }, 60)
  }, [pendingReveal, docPath, loaded])

  if (!docPath) return <p className="empty ide-pad">select a document to edit</p>
  if (contentQ.isError && !loaded)
    return <p className="error-inline ide-pad">could not load {docPath}</p>
  if (!root || !loaded) return <p className="empty ide-pad">loading…</p>

  return (
    <>
      {(
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
            <DocModeSwitch canDiff={hasBaseline} />
            {autosave ? (
              <span className="muted" title="changes save on their own a beat after typing stops">
                {dirty ? 'saving…' : 'saved'}
              </span>
            ) : (
              <button onClick={() => void save()} disabled={!dirty || conflict}>
                save
              </button>
            )}
            <ArrangeButton />
          </div>
        </div>
      )}
      {askWrite && folder?.mode === 'read' && (
        <div className="ide-conflict ide-askwrite">
          <span>
            Saved in this browser. {folder.name} is read-only{folder.held > 0 ? `: ${folder.held} unwritten` : ''}.
          </span>
          <button
            onClick={() => {
              setAskWrite(false)
              window.dispatchEvent(new CustomEvent('jazyk:folder-mode', { detail: 'write' }))
            }}
            title="write everything this tab holds to the folder, and keep writing"
          >
            write to disk
          </button>
          <button onClick={() => setAskWrite(false)}>not now</button>
        </div>
      )}
      {conflict && (
        <div className="ide-conflict">
          <span>the document changed on disk</span>
          <button onClick={() => void reloadDiscard()}>reload (discard mine)</button>
          <button onClick={() => void overwrite()}>overwrite</button>
        </div>
      )}
      <div className={`ide-split${reading ? ' read' : ''}`}>
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
          diffMode={mode === 'diff' && hasBaseline}
          onDirty={onDirty}
          onText={onText}
          onNavigate={onNavigate}
          onOpenLink={onOpenLink}
          onOpenNode={openNode}
          onMarkers={setMarkers}
          onSave={() => void save()}
          marks={marks}
          bands={bands}
          mentions={mentions}
          onCursorLine={reportLine}
          onCaretContext={reportCaret}
          flashes={flashLines}
        />
        {reading && (
          <MarkdownView
            ref={mdRef}
            text={readText}
            baseAbs={`${root}/${loaded.path}`}
            mentions={mentions}
            decor={decor}
            onOpenNode={openNode}
            docsOpenIn="read"
            onTopLine={reportLine}
          />
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
