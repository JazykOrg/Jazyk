// Deliverable: the generated product, browsable beside the prose that produced
// it. File tree with ownership badges, a read-only viewer with traceability
// marker decorations, and a ties sidebar back to the graph.
import { useEffect, useMemo, useRef } from 'react'
import { Link, useParams } from 'react-router'
import { keepPreviousData, useQuery } from '@tanstack/react-query'
import * as monaco from 'monaco-editor'
import EditorWorker from 'monaco-editor/esm/vs/editor/editor.worker?worker&inline'
import { get, type VerifyRow } from '../lib/api'
import { useMatrix } from '../lib/queries'
import NodeLink from '../components/NodeLink'
import { VerifyChip } from '../components/Chip'
import './routes.css'

// The Docs tab's MonacoHost sets this at module load; guard for the case where
// this chunk evaluates first.
const env = self as unknown as { MonacoEnvironment?: monaco.Environment }
if (!env.MonacoEnvironment) env.MonacoEnvironment = { getWorker: () => new EditorWorker() }

interface Owners {
  entities: string[]
  requirements: string[]
  tests: string[]
}

interface DFile {
  path: string
  size: number
  owners: Owners
}

interface Listing {
  root: string
  files: DFile[]
}

interface Marker {
  line: number
  requirement: string
  hash: string
  exists: boolean
  stale: boolean
}

interface FileResp {
  path: string
  text?: string
  binary?: boolean
  size?: number
  markers?: Marker[]
  owners: Owners
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
function ReadOnlyCode({
  path,
  text,
  markers,
  revealRef,
}: {
  path: string
  text: string
  markers: Marker[]
  revealRef: React.MutableRefObject<((line: number) => void) | null>
}) {
  const divRef = useRef<HTMLDivElement>(null)
  const editorRef = useRef<monaco.editor.IStandaloneCodeEditor | null>(null)

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
      editor.getModel()?.dispose()
      editor.dispose()
      editorRef.current = null
    }
  }, [revealRef])

  useEffect(() => {
    const editor = editorRef.current
    if (!editor) return
    const old = editor.getModel()
    const model = monaco.editor.createModel(text, langFor(path))
    editor.setModel(model)
    old?.dispose()
    model.deltaDecorations(
      [],
      markers.map((m) => ({
        range: new monaco.Range(m.line, 1, m.line, 1),
        options: {
          isWholeLine: true,
          linesDecorationsClassName: m.stale || !m.exists ? 'dmark-bad' : 'dmark-ok',
        },
      })),
    )
  }, [path, text, markers])

  return <div ref={divRef} className="deliv-editor" />
}

interface Tree {
  dirs: Record<string, Tree>
  files: DFile[]
}

function buildTree(files: DFile[]): Tree {
  const root: Tree = { dirs: {}, files: [] }
  for (const f of [...files].sort((a, b) => a.path.localeCompare(b.path))) {
    const parts = f.path.split('/')
    let node = root
    for (const p of parts.slice(0, -1)) node = node.dirs[p] ??= { dirs: {}, files: [] }
    node.files.push(f)
  }
  return root
}

function TreeLevel({
  node,
  depth,
  current,
  staleFor,
}: {
  node: Tree
  depth: number
  current: string
  staleFor: (f: DFile) => boolean
}) {
  return (
    <>
      {node.files.map((f) => {
        const owners = f.owners.entities.length + f.owners.requirements.length
        return (
          <div
            key={f.path}
            className={`deliv-row${f.path === current ? ' active' : ''}`}
            style={{ paddingLeft: depth * 12 }}
          >
            <Link to={`/deliverable/${f.path}`} className="deliv-file">
              <span className="deliv-name">{f.path.split('/').pop()}</span>
              {owners > 0 && <span className="deliv-count mono">{owners}</span>}
              {staleFor(f) && <span className="dot-stale" title="a bound requirement is stale" />}
            </Link>
          </div>
        )
      })}
      {Object.entries(node.dirs).map(([name, child]) => (
        <div key={name}>
          <div className="deliv-dir mono" style={{ paddingLeft: 12 + depth * 12 }}>
            {name}/
          </div>
          <TreeLevel node={child} depth={depth + 1} current={current} staleFor={staleFor} />
        </div>
      ))}
    </>
  )
}

function Ties({
  owners,
  markers,
  rows,
  reveal,
}: {
  owners: Owners
  markers: Marker[]
  rows: Record<string, VerifyRow>
  reveal: (line: number) => void
}) {
  return (
    <>
      <h3>entities</h3>
      {owners.entities.length === 0 && <p className="muted">none</p>}
      {owners.entities.map((id) => (
        <p key={id} style={{ margin: '2px 0' }}>
          <NodeLink id={id} />
        </p>
      ))}
      <h3>requirements</h3>
      {owners.requirements.length === 0 && <p className="muted">none</p>}
      {owners.requirements.map((id) => (
        <div key={id} style={{ margin: '4px 0' }}>
          <NodeLink id={id} /> <VerifyChip status={rows[id]?.status ?? 'unverified'} />
          {markers
            .filter((m) => m.requirement === id)
            .map((m, i) => (
              <p key={i} style={{ margin: '1px 0 1px 10px' }}>
                <a
                  href="#line"
                  className="mono"
                  onClick={(e) => {
                    e.preventDefault()
                    reveal(m.line)
                  }}
                >
                  line {m.line}
                </a>
                {m.stale && <span className="v-stale"> ⚠ stale marker</span>}
                {!m.exists && <span className="v-bad"> (gone)</span>}
              </p>
            ))}
        </div>
      ))}
      <h3>tests</h3>
      {owners.tests.length === 0 && <p className="muted">none</p>}
      {owners.tests.map((id) => (
        <p key={id} style={{ margin: '2px 0' }}>
          <NodeLink id={id} /> <VerifyChip status={rows[id]?.status ?? 'unverified'} />
        </p>
      ))}
    </>
  )
}

export default function Deliverable() {
  const params = useParams()
  const filePath = params['*'] ?? ''
  const matrix = useMatrix()
  const rows = matrix.data?.rows ?? {}

  const listQ = useQuery({
    queryKey: ['deliverable'],
    queryFn: () => get<Listing>('/api/deliverable'),
    placeholderData: keepPreviousData,
    staleTime: 5_000,
  })
  const fileQ = useQuery({
    queryKey: ['deliverable', 'file', filePath],
    queryFn: () => get<FileResp>(`/api/deliverable/file?path=${encodeURIComponent(filePath)}`),
    enabled: filePath !== '',
    staleTime: 5_000,
  })
  const revealRef = useRef<((line: number) => void) | null>(null)

  const staleFor = useMemo(() => {
    return (f: DFile) =>
      [...f.owners.requirements, ...f.owners.tests].some((r) => rows[r]?.status.startsWith('stale'))
  }, [rows])

  const files = listQ.data?.files ?? []
  const bound = files.filter(
    (f) => f.owners.entities.length + f.owners.requirements.length + f.owners.tests.length > 0,
  )
  const unbound = files.filter((f) => !bound.includes(f))
  const open = fileQ.data

  return (
    <div className="deliv">
      <div className="deliv-tree">
        {listQ.error && (
          <p className="error-inline deliv-pad">
            {listQ.error.message}{' '}
            <a href="#retry" onClick={(e) => { e.preventDefault(); listQ.refetch() }}>retry</a>
          </p>
        )}
        {!listQ.data && !listQ.error && <p className="muted deliv-pad">loading…</p>}
        {listQ.data && files.length === 0 && (
          <p className="muted deliv-pad">
            no deliverable yet; generate from the <Link to="/work">Work tab</Link>
          </p>
        )}
        {files.length > 0 && (
          <TreeLevel node={buildTree(files)} depth={0} current={filePath} staleFor={staleFor} />
        )}
      </div>

      <div className="deliv-main">
        {!filePath ? (
          listQ.data && files.length === 0 ? (
            <p className="muted deliv-pad">
              no deliverable yet; generate from the <Link to="/work">Work tab</Link>
            </p>
          ) : (
            <div className="deliv-pad">
              <p>
                {files.length} files · {bound.length} bound to the graph · {unbound.length} unbound
              </p>
              {unbound.length > 0 && (
                <>
                  {unbound.map((f) => (
                    <p key={f.path} className="mono muted" style={{ margin: '2px 0' }}>
                      <Link to={`/deliverable/${f.path}`}>{f.path}</Link>
                    </p>
                  ))}
                  <p className="muted">
                    unbound files are generated but unclaimed by the ledger; <span className="mono">jazyk test --audit</span> rebinds them
                  </p>
                </>
              )}
            </div>
          )
        ) : fileQ.error ? (
          <p className="error-inline deliv-pad">
            {fileQ.error.message}{' '}
            <a href="#retry" onClick={(e) => { e.preventDefault(); fileQ.refetch() }}>retry</a>
          </p>
        ) : !open ? (
          <p className="muted deliv-pad">loading…</p>
        ) : open.binary ? (
          <p className="muted deliv-pad">binary file · {open.size ?? 0} bytes</p>
        ) : (
          <>
            <div className="deliv-topbar mono">{open.path}</div>
            <ReadOnlyCode
              path={open.path}
              text={open.text ?? ''}
              markers={open.markers ?? []}
              revealRef={revealRef}
            />
          </>
        )}
      </div>

      <div className="deliv-side">
        {filePath && open ? (
          <Ties
            owners={open.owners}
            markers={open.markers ?? []}
            rows={rows}
            reveal={(l) => revealRef.current?.(l)}
          />
        ) : (
          <p className="muted">select a file to see its ties to the graph</p>
        )}
      </div>
    </div>
  )
}
