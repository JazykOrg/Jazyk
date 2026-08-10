// The deliverable viewer in the center pane: read-only, every resolved ledger
// site a code lens above its line (opens the requirement in the inspector), and
// gutter marks against the file's generation baseline with a side-by-side diff
// toggle (docs/frontends/gui.md#deliverable-viewer).
import { useEffect, useMemo, useRef, useState } from 'react'
import { useParams, useSearchParams } from 'react-router'
import { useQuery } from '@tanstack/react-query'
import { EditorState, RangeSet, StateEffect, StateField, type Extension } from '@codemirror/state'
import {
  Decoration,
  type DecorationSet,
  EditorView,
  GutterMarker,
  WidgetType,
  gutter,
  lineNumbers,
} from '@codemirror/view'
import { syntaxHighlighting } from '@codemirror/language'
import { classHighlighter } from '@lezer/highlight'
import { languages } from '@codemirror/language-data'
import { MergeView } from '@codemirror/merge'
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

// A site lens above its line: the requirement id and its verification status,
// clickable into the inspector.
class SiteLens extends WidgetType {
  constructor(
    readonly title: string,
    readonly rid: string,
    readonly open: (rid: string) => void,
  ) {
    super()
  }
  override eq(o: SiteLens) {
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

const setStrips = StateEffect.define<RangeSet<Strip>>()
const stripField = StateField.define<RangeSet<Strip>>({
  create: () => RangeSet.empty,
  update(value, tr) {
    value = value.map(tr.changes)
    for (const e of tr.effects) if (e.is(setStrips)) value = e.value
    return value
  },
})
const setLenses = StateEffect.define<DecorationSet>()
const lensField = StateField.define<DecorationSet>({
  create: () => Decoration.none,
  update(value, tr) {
    value = value.map(tr.changes)
    for (const e of tr.effects) if (e.is(setLenses)) value = e.value
    return value
  },
  provide: (f) => EditorView.decorations.from(f),
})

// The language by filename, loaded on demand from language-data; plain text until
// (and unless) it resolves.
function languageFor(path: string) {
  return languages.find((l) => l.extensions.some((e) => path.endsWith(`.${e}`)) || l.filename?.test(path))
}

// Self-contained read-only CodeMirror viewer; reveal is handed up through a ref.
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
  const viewRef = useRef<EditorView | null>(null)
  const mergeRef = useRef<MergeView | null>(null)
  const openRef = useRef(onOpenNode)
  openRef.current = onOpenNode

  useEffect(() => {
    const view = new EditorView({ parent: divRef.current! })
    viewRef.current = view
    revealRef.current = (line) => {
      const l = view.state.doc.line(Math.min(Math.max(line, 1), view.state.doc.lines))
      view.dispatch({
        selection: { anchor: l.from },
        effects: EditorView.scrollIntoView(l.from, { y: 'center' }),
      })
    }
    return () => {
      revealRef.current = null
      mergeRef.current?.destroy()
      mergeRef.current = null
      view.destroy()
      viewRef.current = null
    }
  }, [revealRef])

  // Content, language, site lenses, and baseline gutter marks reset together.
  useEffect(() => {
    const view = viewRef.current
    if (!view) return
    let cancelled = false
    const base: Extension[] = [
      lineNumbers(),
      EditorView.editable.of(false),
      EditorState.readOnly.of(true),
      EditorView.lineWrapping,
      syntaxHighlighting(classHighlighter),
      gutter({ class: 'jz-gutter-chg', markers: (v) => v.state.field(stripField) }),
      stripField,
      lensField,
    ]
    const finish = (exts: Extension[]) => {
      if (cancelled || viewRef.current !== view) return
      view.setState(EditorState.create({ doc: text, extensions: exts }))
      const state = view.state
      const clamp = (l: number) => Math.min(Math.max(l, 1), state.doc.lines)
      const lenses = sites
        .filter((s) => s.line !== null)
        .map((s) =>
          Decoration.widget({
            widget: new SiteLens(
              `${s.requirement} · ${status(s.requirement)}`,
              s.requirement,
              (id) => openRef.current(id),
            ),
            side: -1,
            block: true,
          }).range(state.doc.line(clamp(s.line!)).from),
        )
        .sort((a, b) => a.from - b.from)
      const strips: { from: number; marker: Strip }[] = []
      for (const s of sites.filter((s) => s.line !== null)) {
        const bad = !s.exists || status(s.requirement).startsWith('stale')
        strips.push({
          from: state.doc.line(clamp(s.line!)).from,
          marker: new Strip(bad ? 'dmark-bad' : 'dmark-ok', s.requirement),
        })
      }
      if (baseline !== null) {
        const marks = lineMarks(baseline, text)
        const push = (l: number, cls: string, tip: string) =>
          strips.push({ from: state.doc.line(clamp(l)).from, marker: new Strip(cls, tip) })
        for (const l of marks.added) push(l, 'gd-add', 'added by the last generation')
        for (const l of marks.modified) push(l, 'gd-mod', 'changed by the last generation')
        for (const [l, n] of marks.deletedAbove)
          push(l, 'gd-del', `${n} line${n > 1 ? 's' : ''} removed here`)
      }
      view.dispatch({
        effects: [
          setLenses.of(Decoration.set(lenses, true)),
          setStrips.of(
            RangeSet.of(strips.sort((a, b) => a.from - b.from).map((s) => s.marker.range(s.from))),
          ),
        ],
      })
    }
    const desc = languageFor(path)
    if (desc)
      desc.load().then(
        (lang) => finish([...base, lang]),
        () => finish(base),
      )
    else finish(base)
    return () => {
      cancelled = true
    }
  }, [path, text, sites, status, baseline])

  // The diff view: baseline against current, both read-only.
  useEffect(() => {
    const parent = diffDivRef.current
    mergeRef.current?.destroy()
    mergeRef.current = null
    if (!diffMode || baseline === null || !parent) return
    const ro: Extension[] = [
      lineNumbers(),
      EditorView.editable.of(false),
      EditorState.readOnly.of(true),
      EditorView.lineWrapping,
      syntaxHighlighting(classHighlighter),
    ]
    mergeRef.current = new MergeView({
      parent,
      a: { doc: baseline, extensions: ro },
      b: { doc: text, extensions: ro },
    })
    return () => {
      mergeRef.current?.destroy()
      mergeRef.current = null
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
