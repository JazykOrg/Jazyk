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
import { useDeliverable, useDocs, useMatrix, useOutList, useProject, useProjectFiles } from '../lib/queries'
import { useDocDelivLinks } from '../lib/links'
import { useInbox } from '../lib/product'
import { outHref } from '../lib/nav'
import { matchesGlob, useProjectKey, type TreeFile } from '../lib/tree'
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

// What the build is doing to this document: a small icon on its row (waiting,
// running, read, not read yet) whose tooltip says what happened, fading a few
// seconds after the build and held under the pointer. No sentence sits in the tree
// (docs/frontends/gui.md#docs).
function buildMarkWords(p: TurnProgress): { state: 'waiting' | 'running' | 'read' | 'unread'; icon: string; tip: string } {
  const count = p.sections.length > 0 ? ` (${p.touched.length}/${p.sections.length} sections)` : ''
  switch (p.state) {
    case 'queued':
      return { state: 'waiting', icon: '○', tip: `Waiting to be read in this build${count}` }
    case 'running':
      return { state: 'running', icon: '●', tip: `Being read now${p.active ? `: ${p.active}` : ''}${count}` }
    case 'done':
      return { state: 'read', icon: '✓', tip: p.result || 'Read' }
    default:
      return { state: 'unread', icon: '!', tip: p.result || 'Not read yet' }
  }
}

function BuildMark({ p }: { p: TurnProgress }) {
  const m = buildMarkWords(p)
  const settled = p.state === 'done' || p.state === 'failed'
  return (
    <span
      className={`ide-build-icon ${m.state}${settled && !p.held ? ' fading' : ''}`}
      title={m.tip}
      aria-label={m.tip}
      role="img"
      data-state={m.state}
    >
      {m.icon}
    </span>
  )
}

// The document actions on the tree's rows. Everything goes through the documents
// API; the tree only types the names.
function useDocActions(docPath: string): TreeActions {
  const navigate = useNavigate()
  const qc = useQueryClient()
  const editorDirty = useApp((a) => a.editorDirty)
  return useMemo<TreeActions>(
    () => ({
      create: async (path) => {
        const name = (path.split('/').pop() ?? path).replace(/\.md$/, '').replace(/[-_]+/g, ' ')
        const title = name ? name.charAt(0).toUpperCase() + name.slice(1) : ''
        await put(contentPath(path), { text: `# ${title}\n`, baseHash: null })
        void qc.invalidateQueries({ queryKey: ['docs'] })
        void qc.invalidateQueries({ queryKey: ['project', 'files'] })
        navigate(`/docs/${path}`)
      },
      rename: async (from, to) => {
        if (from === docPath && editorDirty) throw new Error('save the open document first')
        await post('/api/docs/rename', { from, to })
        void qc.invalidateQueries({ queryKey: ['docs'] })
        void qc.invalidateQueries({ queryKey: ['project', 'files'] })
        if (from === docPath) navigate(`/docs/${to}`)
      },
      remove: async (path) => {
        await deleteDoc(path)
        void qc.invalidateQueries({ queryKey: ['docs'] })
        void qc.invalidateQueries({ queryKey: ['project', 'files'] })
        if (path === docPath) navigate('/docs')
      },
    }),
    [qc, navigate, docPath, editorDirty],
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
  const projectKey = useProjectKey(projectQ.data?.root ?? 'project')
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

  const docActions = useDocActions(docPath)

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
                  {docProgress.has(f.path) && <BuildMark p={docProgress.get(f.path)!} />}
                </>
              )}
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

// ---- the project tree (docs/frontends/gui.md#workspace) ----

type ProjectRow = { path: string; size: number }

// The static head of a docs glob pattern: `docs/**/*.md` names `docs`.
function globRoot(pattern: string): string {
  const parts = pattern.split('/')
  const out: string[] = []
  for (const p of parts) {
    if (/[*?[{]/.test(p)) break
    out.push(p)
  }
  return out.join('/')
}

const TEST_FILE = /(^|\/)(tests?|spec|__tests__)\//
const TEST_NAME = /\.(test|spec)\.[a-z0-9]+$/i

export type FileKind = 'doc' | 'file' | 'test' | 'generated' | 'project'

// What a project file is, and the page it opens as.
export function classifyFile(path: string, where: { deliverable: string | null; out: string | null; docsGlob: string[] }): { kind: FileKind; href: string } {
  const { deliverable, out, docsGlob } = where
  // The files the app has a page for open that page.
  if (path === 'jazyk.toml') return { kind: 'project', href: '/settings' }
  if (out && (path === out || path.startsWith(`${out}/`))) {
    const rel = path.slice(out.length + 1)
    const gen = /^journal\/g(\d+)\.yaml$/.exec(rel)
    if (gen) return { kind: 'generated', href: `/history/${gen[1]}` }
    if (rel === 'status.yaml') return { kind: 'generated', href: '/history' }
    if (rel === 'graph/diagnostics.yaml') return { kind: 'generated', href: '/inbox' }
    return { kind: 'generated', href: outHref(rel) }
  }
  if (docsGlob.length > 0 && matchesGlob(path, docsGlob)) return { kind: 'doc', href: `/docs/${path}` }
  const underProduct = deliverable !== null && (deliverable === '' || path.startsWith(`${deliverable}/`))
  if (underProduct && path !== 'jazyk.toml' && !path.split('/').some((c) => c.startsWith('.'))) {
    const rel = deliverable ? path.slice(deliverable.length + 1) : path
    return { kind: TEST_FILE.test(rel) || TEST_NAME.test(rel) ? 'test' : 'file', href: `/product/files/${rel}` }
  }
  return { kind: 'project', href: `/project/${path}` }
}

const GLYPH: Record<FileKind, string> = { doc: '¶', file: '▤', test: '✓', generated: '▦', project: '▫' }

export function ProjectExplorer() {
  const loc = useLocation()
  const projectQ = useProject()
  const filesQ = useProjectFiles()
  const docsQ = useDocs()
  const inbox = useInbox()
  const turns = useApp((a) => a.turns)
  const projectKey = useProjectKey(projectQ.data?.root ?? 'project')
  const docPath = loc.pathname.startsWith('/docs/') ? decodeURIComponent(loc.pathname.slice('/docs/'.length)) : ''
  const docActions = useDocActions(docPath)
  const pendingDocs = useMemo(() => new Set(inbox.data?.pending.docs ?? []), [inbox.data])
  const docInfo = useMemo(() => new Map((docsQ.data?.docs ?? []).map((d) => [d.path, d])), [docsQ.data])
  const docProgress = useMemo(() => {
    const m = new Map<string, TurnProgress>()
    for (const t of Object.values(turns)) if (t.doc) m.set(t.doc, t)
    return m
  }, [turns])

  // A backend that does not serve the project files (an older `jazyk gui`): the tree
  // from what it does list, the documents, the product files, the generated pages.
  const delivQ = useDeliverable()
  const outQ = useOutList('docsgen')
  const data = useMemo(() => {
    const served = filesQ.data && Array.isArray(filesQ.data.files)
    if (served) return filesQ.data
    if ((!filesQ.isError && !filesQ.data) || !projectQ.data) return undefined
    const root = projectQ.data.root.replace(/\/$/, '')
    const rel = (abs?: string) => (abs && (abs === root || abs.startsWith(`${root}/`)) ? abs.slice(root.length + 1) : null)
    const deliverable = rel((projectQ.data as { deliverable?: string }).deliverable)
    const out = rel(projectQ.data.out)
    const files: { path: string; size: number }[] = []
    for (const d of docsQ.data?.docs ?? []) files.push({ path: d.path, size: 0 })
    if (deliverable !== null) for (const f of delivQ.data?.files ?? []) files.push({ path: deliverable ? `${deliverable}/${f.path}` : f.path, size: 0 })
    if (out) for (const f of outQ.data?.files ?? []) files.push({ path: `${out}/${f.path}`, size: 0 })
    return { files, deliverable, out, docsGlob: projectQ.data.docsGlob ?? [] }
  }, [filesQ.data, filesQ.isError, projectQ.data, docsQ.data, delivQ.data, outQ.data])
  const where = useMemo(
    () => ({ deliverable: data?.deliverable ?? null, out: data?.out ?? null, docsGlob: data?.docsGlob ?? projectQ.data?.docsGlob ?? NO_GLOB }),
    [data, projectQ.data],
  )
  const kinds = useMemo(() => new Map((data?.files ?? []).map((f) => [f.path, classifyFile(f.path, where)])), [data, where])
  // A test's result, from the requirements it tests: pass, fail, or none yet.
  const matrix = useMatrix()
  const testResult = useMemo(() => {
    const rows = matrix.data?.rows ?? {}
    const byFile = new Map((delivQ.data?.files ?? []).map((f) => [f.path, f.owners.tests]))
    const out = new Map<string, 'pass' | 'fail' | 'none'>()
    for (const [path, k] of kinds) {
      if (k.kind !== 'test') continue
      const rel = where.deliverable ? path.slice(where.deliverable.length + 1) : path
      const st = (byFile.get(rel) ?? []).map((r) => rows[r]?.status)
      out.set(path, st.some((x) => x === 'failing' || x === 'partial') ? 'fail' : st.some((x) => x === 'verified') ? 'pass' : 'none')
    }
    return out
  }, [kinds, delivQ.data, matrix.data, where])
  const docRoots = useMemo(() => new Set(where.docsGlob.map(globRoot).filter(Boolean)), [where])

  // The open item's file, highlighted in the tree.
  const current = useMemo(() => {
    const p = decodeURIComponent(loc.pathname)
    for (const [path, k] of kinds) if (k.href === p) return path
    return ''
  }, [loc.pathname, kinds])

  const isDocPath = (p: string) => kinds.get(p)?.kind === 'doc' || [...docRoots].some((r) => p === r || p.startsWith(`${r}/`))

  return (
    <div className="wb-explorer-section project-tree">
      {filesQ.isLoading && <p className="muted ide-pad">reading the project…</p>}

      {data && (
        <FileTree<ProjectRow>
          head="project"
          projectKey={projectKey}
          items={data.files}
          current={current}
          defaultClosed
          hrefFor={(f) => kinds.get(f.path)?.href ?? `/project/${f.path}`}
          fileClass={(f) => `pf k-${kinds.get(f.path)?.kind ?? 'project'}${testResult.has(f.path) ? ` t-${testResult.get(f.path)}` : ''}`}
          fileGlyph={(f) => (
            <span
              className="pf-glyph"
              aria-hidden="true"
              title={testResult.has(f.path) ? { pass: 'its tests pass', fail: 'a test fails', none: 'no result yet' }[testResult.get(f.path)!] : undefined}
            >
              {GLYPH[kinds.get(f.path)?.kind ?? 'project']}
            </span>
          )}
          folderClass={(dir) =>
            docRoots.has(dir) ? 'pf-dir k-doc' : where.deliverable && dir === where.deliverable ? 'pf-dir k-file' : where.out && dir === where.out ? 'pf-dir k-generated' : ''
          }
          folderBadge={(dir) =>
            docRoots.has(dir) ? (
              <span className="pf-tag">documents</span>
            ) : where.deliverable && dir === where.deliverable ? (
              <span className="pf-tag">product</span>
            ) : where.out && dir === where.out ? (
              <span className="pf-tag">build output</span>
            ) : null
          }
          nameClass="ide-doc-name"
          placeholder="name.md"
          actions={docActions}
          actionsFor={isDocPath}
          alwaysFolders={[...docRoots]}
          badges={(f) => {
            const d = docInfo.get(f.path)
            if (!d) return null
            return (
              <>
                <DiagBadge doc={d} />
                <GoalBadge doc={d} />
                {d.stale && <span className="dot-stale" title="stale against the model" />}
                {pendingDocs.has(f.path) && (
                  <span className="ide-pending" title="edited since the last build">
                    ●
                  </span>
                )}
                {docProgress.has(f.path) && <BuildMark p={docProgress.get(f.path)!} />}
              </>
            )
          }}
        />
      )}
      {where.deliverable === '' && <p className="muted pf-note">The product is the whole project folder.</p>}
    </div>
  )
}
