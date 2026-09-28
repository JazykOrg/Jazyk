// The navigator's trees (docs/frontends/gui.md#docs): the documents as the file
// tree a code editor has, the product's files and the generated pages as trees of
// the same kind under their own heads, with the linkage tint between documents and
// product files (docs/frontends/gui.md#lenses). Selecting a document highlights the
// product files bound to its requirements; selecting a product file highlights the
// documents on the other side of the join.
import { useCallback, useMemo } from 'react'
import { useLocation, useNavigate } from 'react-router'
import { useQueryClient } from '@tanstack/react-query'
import { del, post, put, type DelivFileInfo, type DocInfo, type OutFileInfo } from '../lib/api'
import { useDeliverable, useDocs, useMatrix, useOutList, useProject } from '../lib/queries'
import { useDocDelivLinks } from '../lib/links'
import { useInbox } from '../lib/product'
import { outHref } from '../lib/nav'
import { matchesGlob, treeProjectKey, type TreeFile } from '../lib/tree'
import { useApp, type TurnProgress } from '../lib/store'
import FileTree, { type TreeActions } from './FileTree'
import '../ide/ide.css'
import '../routes/routes.css'

function contentPath(path: string): string {
  return `/api/docs/content?path=${encodeURIComponent(path)}`
}

// Through the transport seam like every other write, so it reaches the backend
// that has the project (a server on this computer or the one in the tab).
async function deleteDoc(path: string): Promise<void> {
  await del(contentPath(path))
}

function DiagBadge({ doc }: { doc: DocInfo }) {
  const diag = doc.diagnostics
  if (!diag) return null
  const total = Object.values(diag).reduce((a, b) => a + b, 0)
  if (total === 0) return null
  const cls = diag.error ? 'sev-error' : diag.warning ? 'sev-warning' : diag.info ? 'sev-info' : 'sev-none'
  return <span className={`ide-diag mono ${cls}`}>{total}</span>
}

// Open goals on the document, as a count badge (docs/frontends/gui.md#docs).
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

// What the build is doing to this document, under its row. A running turn shows
// the section it reached; a finished one shows what it staged, until it fades
// (docs/frontends/gui.md#docs).
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

const EMPTY_SET = new Set<string>()
const NO_GLOB: string[] = []

export type ExplorerSection = 'docs' | 'deliverable' | 'generated'

export default function Explorer({ sections = ['docs', 'deliverable', 'generated'] }: { sections?: ExplorerSection[] }) {
  const show = (x: ExplorerSection) => sections.includes(x)
  const loc = useLocation()
  const navigate = useNavigate()
  const qc = useQueryClient()
  const projectQ = useProject()
  const docsQ = useDocs()
  const delivQ = useDeliverable()
  const outQ = useOutList('docsgen')
  const matrix = useMatrix()
  const links = useDocDelivLinks()
  const inbox = useInbox()
  const editorDirty = useApp((a) => a.editorDirty)
  const turns = useApp((a) => a.turns)
  const turnHold = useApp((a) => a.turnHold)
  const projectKey = treeProjectKey(projectQ.data?.root ?? 'project')
  // Edited since its last build: the pending mark (docs/frontends/gui.md#docs).
  const pendingDocs = useMemo(() => new Set(inbox.data?.pending.docs ?? []), [inbox.data])
  const docsGlob = projectQ.data?.docsGlob ?? NO_GLOB

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

  const docPath = loc.pathname.startsWith('/docs/') ? decodeURIComponent(loc.pathname.slice('/docs/'.length)) : ''
  const delivPath = loc.pathname.startsWith('/product/files/')
    ? decodeURIComponent(loc.pathname.slice('/product/files/'.length))
    : ''
  const outPath = loc.pathname.startsWith('/product/generated/')
    ? decodeURIComponent(loc.pathname.slice('/product/generated/'.length))
    : ''

  // The linkage tint: whichever side is open lights the other side up.
  const relatedFiles = docPath ? (links.docToFiles.get(docPath) ?? EMPTY_SET) : EMPTY_SET
  const relatedDocs = delivPath ? (links.fileToDocs.get(delivPath) ?? EMPTY_SET) : EMPTY_SET

  // Everything goes through the documents API; the tree only types the names.
  const docActions = useMemo<TreeActions>(
    () => ({
      create: async (path) => {
        const name = (path.split('/').pop() ?? path).replace(/\.md$/, '').replace(/[-_]+/g, ' ')
        const title = name ? name.charAt(0).toUpperCase() + name.slice(1) : ''
        await put(contentPath(path), { text: `# ${title}\n`, baseHash: null })
        void qc.invalidateQueries({ queryKey: ['docs'] })
        navigate(`/docs/${path}`)
      },
      rename: async (from, to) => {
        if (from === docPath && editorDirty) throw new Error('save the open document first')
        await post('/api/docs/rename', { from, to })
        void qc.invalidateQueries({ queryKey: ['docs'] })
        if (from === docPath) navigate(`/docs/${to}`)
      },
      remove: async (path) => {
        await deleteDoc(path)
        void qc.invalidateQueries({ queryKey: ['docs'] })
        if (path === docPath) navigate('/docs')
      },
    }),
    [qc, navigate, docPath, editorDirty],
  )

  const rows = matrix.data?.rows ?? {}
  const staleFor = (f: DelivFileInfo) =>
    [...f.owners.requirements, ...f.owners.tests].some((r) => rows[r]?.status.startsWith('stale'))

  const files = delivQ.data?.files ?? []
  // The docsgen pages, markdown only, keyed under docsgen/ so the top-level pages
  // sit at the head; the rows route to /product/generated/docsgen/<path>.
  const pages = useMemo(
    () =>
      (outQ.data?.files ?? [])
        .filter((f) => f.path.endsWith('.md') && f.path.startsWith('docsgen/'))
        .map((f) => ({ ...f, path: f.path.slice('docsgen/'.length) })),
    [outQ.data],
  )

  return (
    <>
      {show('docs') && (
        <div className="wb-explorer-section">
          <div className="wb-explorer-label">documents</div>
          {docsQ.isLoading && <p className="muted ide-pad">reading the document list…</p>}
          {docsQ.isError && <p className="muted ide-pad">could not load the document list</p>}
          {docsQ.data && (
            <FileTree<DocInfo>
              head="docs"
              projectKey={projectKey}
              items={docsQ.data.docs}
              current={docPath}
              hrefFor={(f) => `/docs/${f.path}`}
              dim={(f) => docsGlob.length > 0 && !matchesGlob(f.path, docsGlob)}
              related={relatedDocs}
              nameClass="ide-doc-name"
              placeholder="name.md"
              actions={docActions}
              onHover={holdDoc}
              badges={(f: TreeFile<DocInfo>) => (
                <>
                  <DiagBadge doc={f.item} />
                  <GoalBadge doc={f.item} />
                  {f.item.stale && <span className="dot-stale" title="stale against the model" />}
                  {pendingDocs.has(f.path) && (
                    <span className="ide-pending" title="edited since the last build">
                      ●
                    </span>
                  )}
                </>
              )}
              under={(f) => {
                const p = docProgress.get(f.path)
                return p ? <BuildMark p={p} /> : null
              }}
            />
          )}
        </div>
      )}

      {show('deliverable') && (
        <div className="wb-explorer-section">
          <div className="wb-explorer-label">product files</div>
          {delivQ.isError && <p className="muted ide-pad">could not load the deliverable</p>}
          {delivQ.data && files.length === 0 && (
            <p className="muted" style={{ padding: '0 12px', fontSize: 12 }}>
              nothing generated yet
            </p>
          )}
          {files.length > 0 && (
            <FileTree<DelivFileInfo>
              head="product"
              projectKey={projectKey}
              items={files}
              current={delivPath}
              hrefFor={(f) => `/product/files/${f.path}`}
              related={relatedFiles}
              nameClass="deliv-name"
              badges={(f) => {
                const owners = f.item.owners.entities.length + f.item.owners.requirements.length
                return (
                  <>
                    {owners > 0 && <span className="deliv-count mono">{owners}</span>}
                    {staleFor(f.item) && <span className="dot-stale" title="a bound requirement is stale" />}
                  </>
                )
              }}
            />
          )}
        </div>
      )}

      {show('generated') && (
        <div className="wb-explorer-section">
          <div className="wb-explorer-label">generated</div>
          {outQ.isError && <p className="muted ide-pad">could not list the generated pages</p>}
          {outQ.data && pages.length === 0 && (
            <p className="muted" style={{ padding: '0 12px', fontSize: 12 }}>
              no pages yet: a build writes them
            </p>
          )}
          {pages.length > 0 && (
            <FileTree<OutFileInfo>
              head="generated"
              projectKey={projectKey}
              items={pages}
              current={outPath.startsWith('docsgen/') ? outPath.slice('docsgen/'.length) : ''}
              hrefFor={(f) => outHref(`docsgen/${f.path}`)}
              nameClass="deliv-name"
            />
          )}
        </div>
      )}
    </>
  )
}
