// The files sidebar: one explorer over both trees, documents and deliverable,
// with the linkage tint between them (docs/frontends/gui.md#files). Selecting a
// document highlights the deliverable files bound to its requirements; selecting
// a deliverable file highlights the documents on the other side of the join.
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { Link, useLocation, useNavigate } from 'react-router'
import { useQueryClient } from '@tanstack/react-query'
import { post, put, tokenParam, type DelivFileInfo, type DocInfo } from '../lib/api'
import { useDeliverable, useDocs, useMatrix } from '../lib/queries'
import { useDocDelivLinks } from '../lib/links'
import { useApp, type TurnProgress } from '../lib/store'
import '../ide/ide.css'
import '../routes/routes.css'

function contentPath(path: string): string {
  return `/api/docs/content?path=${encodeURIComponent(path)}`
}

async function deleteDoc(path: string): Promise<void> {
  const qs = tokenParam()
  const res = await fetch(`${contentPath(path)}${qs ? `&${qs}` : ''}`, { method: 'DELETE' })
  if (!res.ok) {
    let msg = `${res.status}`
    try {
      const j = (await res.json()) as { error?: string }
      if (j.error) msg = j.error
    } catch {
      // no body
    }
    throw new Error(msg)
  }
}

interface Tree<T> {
  dirs: Record<string, Tree<T>>
  files: T[]
}

function buildTree<T extends { path: string }>(items: T[]): Tree<T> {
  const root: Tree<T> = { dirs: {}, files: [] }
  for (const d of [...items].sort((a, b) => a.path.localeCompare(b.path))) {
    const parts = d.path.split('/')
    let node = root
    for (const p of parts.slice(0, -1)) node = node.dirs[p] ??= { dirs: {}, files: [] }
    node.files.push(d)
  }
  return root
}

function DiagBadge({ doc }: { doc: DocInfo }) {
  const diag = doc.diagnostics
  if (!diag) return null
  const total = Object.values(diag).reduce((a, b) => a + b, 0)
  if (total === 0) return null
  const cls = diag.error ? 'sev-error' : diag.warning ? 'sev-warning' : diag.info ? 'sev-info' : 'sev-none'
  return <span className={`ide-diag mono ${cls}`}>{total}</span>
}

// Open goals on the document, as a count badge (docs/frontends/gui.md#files).
function GoalBadge({ doc }: { doc: DocInfo }) {
  const total = Object.values(doc.goals ?? {}).reduce((a, b) => a + b, 0)
  if (total === 0) return null
  const title = Object.entries(doc.goals ?? {})
    .map(([k, n]) => `${n} ${k}`)
    .join(', ')
  return (
    <span className="ide-diag mono sev-info" title={title}>
      {total}g
    </span>
  )
}

function InlineInput({
  initial,
  placeholder,
  onSubmit,
  onCancel,
}: {
  initial?: string
  placeholder?: string
  onSubmit: (v: string) => void
  onCancel: () => void
}) {
  const [v, setV] = useState(initial ?? '')
  return (
    <input
      className="mono"
      type="text"
      value={v}
      placeholder={placeholder}
      autoFocus
      onChange={(e) => setV(e.target.value)}
      onKeyDown={(e) => {
        if (e.key === 'Enter') onSubmit(v)
        else if (e.key === 'Escape') onCancel()
      }}
    />
  )
}

interface TreeOps {
  renamePath: string | null
  confirmDelete: string | null
  startRename: (p: string) => void
  armDelete: (p: string) => void
  doDelete: (p: string) => void
  doRename: (from: string, to: string) => void
  cancel: () => void
  // Hold this document's build result in place while the pointer is on its row.
  // Keyed by path, not by turn label: the row is hovered before the turn exists.
  hold: (path: string, held: boolean) => void
}

// What the build is doing to this document, under its row. A running turn shows
// the section it reached; a finished one shows what it staged, until it fades
// (docs/frontends/gui.md#files).
function BuildMark({ p }: { p: TurnProgress }) {
  const cls =
    p.state === 'running'
      ? 'v-stale'
      : p.state === 'failed'
        ? 'v-bad'
        : p.state === 'done'
          ? 'v-ok'
          : 'muted'
  const mark = p.state === 'running' ? '▶' : p.state === 'failed' ? '✗' : p.state === 'done' ? '✓' : '◦'
  const detail =
    p.state === 'running'
      ? (p.active ?? 'reading the document')
      : p.state === 'queued'
        ? 'queued'
        : (p.result ?? '')
  return (
    <div className={`ide-build ${cls}${p.state === 'running' ? ' running' : ''}`} title={`${p.label}\n${detail}`}>
      <span className="ide-build-mark">{mark}</span>
      <span className="ide-build-text mono">{detail}</span>
      {p.sections.length > 0 && (
        <span className="ide-build-count mono">
          {p.touched.length}/{p.sections.length}
        </span>
      )}
    </div>
  )
}

function DocLevel({
  node,
  depth,
  current,
  related,
  ops,
  progress,
}: {
  node: Tree<DocInfo>
  depth: number
  current: string
  related: Set<string>
  ops: TreeOps
  progress: Map<string, TurnProgress>
}) {
  return (
    <>
      {node.files.map((d) => {
        const p = progress.get(d.path)
        return ops.renamePath === d.path ? (
          <div key={d.path} className="ide-row" style={{ paddingLeft: depth * 12 }}>
            <InlineInput
              initial={d.path}
              onSubmit={(v) => ops.doRename(d.path, v)}
              onCancel={ops.cancel}
            />
          </div>
        ) : (
          // The pointer on the row holds a finished turn's result in place.
          <div
            key={d.path}
            onMouseEnter={() => ops.hold(d.path, true)}
            onMouseLeave={() => ops.hold(d.path, false)}
          >
            <div
              className={`ide-row${d.path === current ? ' active' : ''}${related.has(d.path) ? ' related' : ''}`}
              style={{ paddingLeft: depth * 12 }}
            >
              <Link to={`/files/docs/${d.path}`} className="ide-doc">
                <span className="ide-doc-name">{d.path.split('/').pop()}</span>
                <DiagBadge doc={d} />
                <GoalBadge doc={d} />
                {d.stale && <span className="dot-stale" title="stale against the graph" />}
              </Link>
              <span className="ide-row-actions">
                <button className="ide-mini" title="rename" onClick={() => ops.startRename(d.path)}>
                  ✎
                </button>
                {ops.confirmDelete === d.path ? (
                  <button className="ide-mini ide-confirm" onClick={() => ops.doDelete(d.path)}>
                    delete?
                  </button>
                ) : (
                  <button className="ide-mini" title="delete" onClick={() => ops.armDelete(d.path)}>
                    ✕
                  </button>
                )}
              </span>
            </div>
            {p && <div style={{ paddingLeft: depth * 12 }}>{<BuildMark p={p} />}</div>}
          </div>
        )
      })}
      {Object.entries(node.dirs).map(([name, child]) => (
        <div key={name}>
          <div className="ide-dir mono" style={{ paddingLeft: 12 + depth * 12 }}>
            {name}/
          </div>
          <DocLevel
            node={child}
            depth={depth + 1}
            current={current}
            related={related}
            ops={ops}
            progress={progress}
          />
        </div>
      ))}
    </>
  )
}

function DelivLevel({
  node,
  depth,
  current,
  related,
  staleFor,
}: {
  node: Tree<DelivFileInfo>
  depth: number
  current: string
  related: Set<string>
  staleFor: (f: DelivFileInfo) => boolean
}) {
  return (
    <>
      {node.files.map((f) => {
        const owners = f.owners.entities.length + f.owners.requirements.length
        return (
          <div
            key={f.path}
            className={`deliv-row${f.path === current ? ' active' : ''}${related.has(f.path) ? ' related' : ''}`}
            style={{ paddingLeft: depth * 12 }}
          >
            <Link to={`/files/deliverable/${f.path}`} className="deliv-file">
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
          <DelivLevel node={child} depth={depth + 1} current={current} related={related} staleFor={staleFor} />
        </div>
      ))}
    </>
  )
}

const EMPTY_SET = new Set<string>()

export default function Explorer() {
  const loc = useLocation()
  const navigate = useNavigate()
  const qc = useQueryClient()
  const docsQ = useDocs()
  const delivQ = useDeliverable()
  const matrix = useMatrix()
  const links = useDocDelivLinks()
  const editorDirty = useApp((a) => a.editorDirty)
  const turns = useApp((a) => a.turns)
  const turnHold = useApp((a) => a.turnHold)

  // The pointer may already be on the row when the turn appears under it, so the
  // label is resolved at call time, not captured at render.
  const holdDoc = useCallback(
    (path: string, held: boolean) => {
      const t = Object.values(useApp.getState().turns).find((x) => x.doc === path)
      if (t) turnHold(t.label, held)
    },
    [turnHold],
  )

  // The running build, per document. One turn per document at a time; a retry
  // replaces the entry under the same key.
  const docProgress = useMemo(() => {
    const m = new Map<string, TurnProgress>()
    for (const t of Object.values(turns)) if (t.doc) m.set(t.doc, t)
    return m
  }, [turns])

  const docPath = loc.pathname.startsWith('/files/docs/')
    ? decodeURIComponent(loc.pathname.slice('/files/docs/'.length))
    : ''
  const delivPath = loc.pathname.startsWith('/files/deliverable/')
    ? decodeURIComponent(loc.pathname.slice('/files/deliverable/'.length))
    : ''

  // The linkage tint: whichever side is open lights the other side up.
  const relatedFiles = docPath ? (links.docToFiles.get(docPath) ?? EMPTY_SET) : EMPTY_SET
  const relatedDocs = delivPath ? (links.fileToDocs.get(delivPath) ?? EMPTY_SET) : EMPTY_SET

  const [treeEdit, setTreeEdit] = useState<{ mode: 'create' } | { mode: 'rename'; path: string } | null>(null)
  const [treeErr, setTreeErr] = useState<string | null>(null)
  const [confirmDelete, setConfirmDelete] = useState<string | null>(null)
  const deleteTimer = useRef<number | null>(null)

  const cancelTreeEdit = useCallback(() => {
    setTreeEdit(null)
    setTreeErr(null)
  }, [])

  const doCreate = useCallback(
    async (path: string) => {
      const p = path.trim()
      if (!p) {
        cancelTreeEdit()
        return
      }
      const name = (p.split('/').pop() ?? p).replace(/\.md$/, '').replace(/[-_]+/g, ' ')
      const title = name ? name.charAt(0).toUpperCase() + name.slice(1) : ''
      try {
        await put(contentPath(p), { text: `# ${title}\n`, baseHash: null })
        void qc.invalidateQueries({ queryKey: ['docs'] })
        cancelTreeEdit()
        navigate(`/files/docs/${p}`)
      } catch (e) {
        setTreeErr((e as Error).message)
      }
    },
    [qc, navigate, cancelTreeEdit],
  )

  const doRename = useCallback(
    async (from: string, to: string) => {
      const t = to.trim()
      if (!t || t === from) {
        cancelTreeEdit()
        return
      }
      if (from === docPath && editorDirty) {
        setTreeErr('save the open document first')
        return
      }
      try {
        await post('/api/docs/rename', { from, to: t })
        void qc.invalidateQueries({ queryKey: ['docs'] })
        cancelTreeEdit()
        if (from === docPath) navigate(`/files/docs/${t}`)
      } catch (e) {
        setTreeErr((e as Error).message)
      }
    },
    [qc, navigate, cancelTreeEdit, docPath, editorDirty],
  )

  const armDelete = useCallback((path: string) => {
    setConfirmDelete(path)
    if (deleteTimer.current !== null) window.clearTimeout(deleteTimer.current)
    deleteTimer.current = window.setTimeout(() => setConfirmDelete(null), 4000)
  }, [])
  useEffect(
    () => () => {
      if (deleteTimer.current !== null) window.clearTimeout(deleteTimer.current)
    },
    [],
  )

  const doDelete = useCallback(
    async (path: string) => {
      setConfirmDelete(null)
      try {
        await deleteDoc(path)
        void qc.invalidateQueries({ queryKey: ['docs'] })
        setTreeErr(null)
        if (path === docPath) navigate('/files')
      } catch (e) {
        setTreeErr((e as Error).message)
      }
    },
    [qc, navigate, docPath],
  )

  const ops: TreeOps = {
    renamePath: treeEdit?.mode === 'rename' ? treeEdit.path : null,
    confirmDelete,
    startRename: (p) => {
      setTreeEdit({ mode: 'rename', path: p })
      setTreeErr(null)
    },
    armDelete,
    doDelete: (p) => void doDelete(p),
    doRename: (from, to) => void doRename(from, to),
    cancel: cancelTreeEdit,
    hold: holdDoc,
  }

  const rows = matrix.data?.rows ?? {}
  const staleFor = (f: DelivFileInfo) =>
    [...f.owners.requirements, ...f.owners.tests].some((r) => rows[r]?.status.startsWith('stale'))

  const files = delivQ.data?.files ?? []

  return (
    <>
      <div className="wb-explorer-section">
        <div className="wb-explorer-label">
          documents
          {treeEdit?.mode !== 'create' && (
            <button
              className="ide-mini"
              title="new document"
              onClick={() => {
                setTreeEdit({ mode: 'create' })
                setTreeErr(null)
              }}
            >
              +
            </button>
          )}
        </div>
        {treeEdit?.mode === 'create' && (
          <div className="ide-row" style={{ padding: '0 8px' }}>
            <InlineInput
              placeholder="path/to/doc.md"
              onSubmit={(v) => void doCreate(v)}
              onCancel={cancelTreeEdit}
            />
          </div>
        )}
        {treeErr && <p className="error-inline ide-tree-err">{treeErr}</p>}
        {docsQ.isLoading && <p className="muted ide-pad">loading…</p>}
        {docsQ.isError && <p className="muted ide-pad">could not load the document list</p>}
        {docsQ.data && (
          <DocLevel
            node={buildTree(docsQ.data.docs)}
            depth={1}
            current={docPath}
            related={relatedDocs}
            ops={ops}
            progress={docProgress}
          />
        )}
      </div>

      <div className="wb-explorer-section">
        <div className="wb-explorer-label">deliverable</div>
        {delivQ.isError && <p className="muted ide-pad">could not load the deliverable</p>}
        {delivQ.data && files.length === 0 && (
          <p className="muted" style={{ padding: '0 12px', fontSize: 12 }}>
            nothing generated yet
          </p>
        )}
        {files.length > 0 && (
          <DelivLevel
            node={buildTree(files)}
            depth={1}
            current={delivPath}
            related={relatedFiles}
            staleFor={staleFor}
          />
        )}
      </div>
    </>
  )
}
