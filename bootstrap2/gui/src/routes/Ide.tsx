// The Docs route: document tree, monaco editor, problems strip. Document content
// loads under the ['docs', ...] query prefix so docs.changed invalidation refetches
// it: a clean editor reloads silently, a dirty one gets the conflict bar.
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { Link, useNavigate, useParams, useSearchParams } from 'react-router'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import type * as monaco from 'monaco-editor'
import { get, put, tokenParam, type DocInfo } from '../lib/api'
import { useCoverage, useDocs, useProject } from '../lib/queries'
import MonacoHost, { docUri, type MonacoHandle } from '../ide/MonacoHost'
import { LspClient } from '../ide/lsp-client'
import '../ide/ide.css'

interface DocContent {
  path: string
  text: string
  hash: string
}

function contentPath(path: string): string {
  return `/api/docs/content?path=${encodeURIComponent(path)}`
}

// Marker severity values (monaco.MarkerSeverity), kept numeric so this file needs
// no runtime monaco import.
function sevName(sev: number): string {
  return sev === 8 ? 'error' : sev === 4 ? 'warning' : sev === 2 ? 'info' : 'hint'
}
function sevClass(sev: number): string {
  return sev === 8 ? 'sev-error' : sev === 4 ? 'sev-warning' : sev === 2 ? 'sev-info' : 'sev-none'
}

interface Tree {
  dirs: Record<string, Tree>
  files: DocInfo[]
}

function buildTree(docs: DocInfo[]): Tree {
  const root: Tree = { dirs: {}, files: [] }
  for (const d of [...docs].sort((a, b) => a.path.localeCompare(b.path))) {
    const parts = d.path.split('/')
    let node = root
    for (const p of parts.slice(0, -1)) node = node.dirs[p] ??= { dirs: {}, files: [] }
    node.files.push(d)
  }
  return root
}

function TreeLevel({ node, depth, current }: { node: Tree; depth: number; current: string }) {
  return (
    <>
      {node.files.map((d) => (
        <Link
          key={d.path}
          to={`/docs/${d.path}`}
          className={`ide-doc${d.path === current ? ' active' : ''}`}
          style={{ paddingLeft: 12 + depth * 12 }}
        >
          <span className="ide-doc-name">{d.path.split('/').pop()}</span>
          {d.stale && <span className="dot-stale" title="stale against the graph" />}
        </Link>
      ))}
      {Object.entries(node.dirs).map(([name, child]) => (
        <div key={name}>
          <div className="ide-dir mono" style={{ paddingLeft: 12 + depth * 12 }}>
            {name}/
          </div>
          <TreeLevel node={child} depth={depth + 1} current={current} />
        </div>
      ))}
    </>
  )
}

export default function Ide() {
  const params = useParams()
  const docPath = params['*'] ?? ''
  const navigate = useNavigate()
  const [searchParams] = useSearchParams()
  const qc = useQueryClient()
  const projectQ = useProject()
  const docsQ = useDocs()
  const coverageQ = useCoverage()

  const lsp = useMemo(() => new LspClient(), [])
  useEffect(() => {
    const proto = location.protocol === 'https:' ? 'wss' : 'ws'
    const qs = tokenParam()
    lsp.connect(`${proto}://${location.host}/lsp${qs ? `?${qs}` : ''}`)
    return () => lsp.dispose()
  }, [lsp])

  const hostRef = useRef<MonacoHandle>(null)
  const [loaded, setLoaded] = useState<DocContent | null>(null)
  const loadedRef = useRef(loaded)
  loadedRef.current = loaded
  // Per-document disk hash of the text the editor is based on; the save baseHash.
  const baseHashes = useRef(new Map<string, string>())
  const [dirty, setDirty] = useState(false)
  const dirtyRef = useRef(false)
  const [conflict, setConflict] = useState(false)
  const [saveErr, setSaveErr] = useState<string | null>(null)
  const [markers, setMarkers] = useState<monaco.editor.IMarker[]>([])
  const savingRef = useRef(false)
  const pendingLine = useRef<{ path: string; line: number } | null>(null)
  const revealedKey = useRef('')

  const rootRef = useRef<string | undefined>(undefined)
  rootRef.current = projectQ.data?.root
  const docsRef = useRef<DocInfo[]>([])
  docsRef.current = docsQ.data?.docs ?? []
  const docPathRef = useRef(docPath)
  docPathRef.current = docPath

  const contentQ = useQuery({
    queryKey: ['docs', 'content', docPath],
    queryFn: () => get<DocContent>(contentPath(docPath)),
    enabled: docPath !== '',
    staleTime: 5_000,
  })

  const onDirty = useCallback((d: boolean) => {
    dirtyRef.current = d
    setDirty(d)
  }, [])

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
      navigate(`/docs/${path}`)
    },
    [navigate],
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

  const root = projectQ.data?.root
  const sortedMarkers = useMemo(
    () => [...markers].sort((a, b) => a.startLineNumber - b.startLineNumber),
    [markers],
  )

  return (
    <div className="ide">
      <div className="ide-tree">
        {docsQ.isLoading && <p className="empty ide-pad">loading…</p>}
        {docsQ.isError && <p className="empty ide-pad">could not load the document list</p>}
        {docsQ.data && <TreeLevel node={buildTree(docsQ.data.docs)} depth={0} current={docPath} />}
      </div>
      <div className="ide-main">
        {!docPath ? (
          <p className="empty ide-pad">select a document to edit</p>
        ) : contentQ.isError && !loaded ? (
          <p className="error-inline ide-pad">could not load {docPath}</p>
        ) : !root || !loaded ? (
          <p className="empty ide-pad">loading…</p>
        ) : (
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
              <div className="ide-topbar-right">
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
            <MonacoHost
              ref={hostRef}
              root={root}
              path={loaded.path}
              initialText={loaded.text}
              lsp={lsp}
              record={coverageQ.data?.[loaded.path]}
              onDirty={onDirty}
              onNavigate={onNavigate}
              onMarkers={setMarkers}
              onSave={() => void save()}
            />
            <div className="ide-problems">
              {sortedMarkers.length === 0 ? (
                <div className="ide-problem-none muted">no problems</div>
              ) : (
                sortedMarkers.map((m, i) => (
                  <div
                    key={i}
                    className="ide-problem"
                    onClick={() => hostRef.current?.revealLine(m.startLineNumber)}
                  >
                    <span className={`mono ${sevClass(m.severity)}`}>{sevName(m.severity)}</span>
                    <span className="mono muted">
                      {m.startLineNumber}:{m.startColumn}
                    </span>
                    <span className="ide-problem-msg">{m.message}</span>
                  </div>
                ))
              )}
            </div>
          </>
        )}
      </div>
    </div>
  )
}
