// The Product lens's overview (docs/frontends/gui.md#overview): the product itself.
// A written product reads as one document with its print action, a built product
// shows its artifacts (a deck as slides, an image inline, a PDF in the browser's
// viewer, anything else as a download), a program its README, how to run it, and
// its test summary. A project with no product yet says so.
import { lazy, Suspense, useEffect, useState } from 'react'
import { createPortal, flushSync } from 'react-dom'
import { Link } from 'react-router'
import { productBytes, type ProductOverview } from '../lib/api'
import { useWords } from '../lib/words'
import { verifyClass } from '../components/Chip'
import MarkdownView from '../components/MarkdownView'
import './product.css'

// The PowerPoint renderer loads only when a deck is shown, so the page's first
// script does not carry it (docs/frontends/wasm.md#building).
const PptxSlides = lazy(() => import('../components/PptxSlides'))

export function sizeLabel(n: number | null | undefined): string {
  if (n === null || n === undefined) return ''
  if (n < 1024) return `${n} bytes`
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`
  return `${(n / 1024 / 1024).toFixed(1)} MB`
}

const baseName = (p: string) => p.split('/').pop() || p

// Saves bytes as a file the way a download link does.
function save(bytes: Uint8Array, contentType: string, name: string) {
  const url = URL.createObjectURL(new Blob([bytes as BlobPart], { type: contentType }))
  const a = document.createElement('a')
  a.href = url
  a.download = name
  document.body.appendChild(a)
  a.click()
  a.remove()
  setTimeout(() => URL.revokeObjectURL(url), 10_000)
}

function DownloadButton({ path }: { path: string }) {
  const [busy, setBusy] = useState(false)
  const [err, setErr] = useState<string | null>(null)
  return (
    <>
      <button
        disabled={busy}
        onClick={async () => {
          setBusy(true)
          setErr(null)
          try {
            const { bytes, contentType } = await productBytes(path)
            save(bytes, contentType, baseName(path))
          } catch (e) {
            setErr(e instanceof Error ? e.message : String(e))
          } finally {
            setBusy(false)
          }
        }}
      >
        Download
      </button>
      {err && <span className="v-bad">{err}</span>}
    </>
  )
}

// The artifact's bytes, read once per path.
function useBytes(path: string) {
  const [state, setState] = useState<{ bytes: Uint8Array; contentType: string } | { error: string } | null>(null)
  useEffect(() => {
    let live = true
    setState(null)
    productBytes(path).then(
      (r) => live && setState(r),
      (e) => live && setState({ error: e instanceof Error ? e.message : String(e) }),
    )
    return () => {
      live = false
    }
  }, [path])
  return state
}

// An image or a PDF shown from its bytes, through an object URL.
function ObjectView({ path, kind }: { path: string; kind: 'image' | 'pdf' }) {
  const got = useBytes(path)
  const [url, setUrl] = useState<string | null>(null)
  useEffect(() => {
    if (!got || 'error' in got) return
    const u = URL.createObjectURL(new Blob([got.bytes as BlobPart], { type: got.contentType }))
    setUrl(u)
    return () => URL.revokeObjectURL(u)
  }, [got])
  if (got && 'error' in got) return <p className="v-bad">{got.error}</p>
  if (!url) return <p className="muted">Loading {baseName(path)}…</p>
  if (kind === 'image') return <img className="product-image" src={url} alt={baseName(path)} />
  return <iframe className="product-pdf" src={url} title={baseName(path)} />
}

function DeckView({ path }: { path: string }) {
  const got = useBytes(path)
  if (got && 'error' in got) return <p className="v-bad">{got.error}</p>
  if (!got) return <p className="muted">Loading {baseName(path)}…</p>
  return (
    <Suspense fallback={<p className="muted">Loading the slide renderer…</p>}>
      <PptxSlides bytes={got.bytes} />
    </Suspense>
  )
}

const IMAGE = /\.(png|jpe?g|gif|webp|svg)$/i

function Built({ ov }: { ov: ProductOverview }) {
  const arts = ov.artifacts ?? []
  const run = ov.build?.run
  if (arts.length === 0)
    return (
      <div className="empty-state">
        <b>No artifact recorded</b>
        The medium is built, but neither a build nor an artifact is recorded yet.
      </div>
    )
  return (
    <>
      {arts.map((a) => (
        <section key={a.path} className="product-artifact">
          <p className="row" style={{ margin: '4px 0' }}>
            <b className="mono">{a.path}</b>
            {a.exists ? <span className="muted">{sizeLabel(a.size)}</span> : <span className="v-bad">not built</span>}
            {a.exists && <DownloadButton path={a.path} />}
          </p>
          {!a.exists && (
            <p className="muted">
              The deliverable does not hold it yet. {run ? <>The build makes it: <span className="mono">{run}</span>.</> : 'No build command is recorded.'}
            </p>
          )}
          {a.exists && /\.pptx$/i.test(a.path) && <DeckView path={a.path} />}
          {a.exists && IMAGE.test(a.path) && <ObjectView path={a.path} kind="image" />}
          {a.exists && /\.pdf$/i.test(a.path) && <ObjectView path={a.path} kind="pdf" />}
        </section>
      ))}
    </>
  )
}

// The written product as one document: every part rendered like Read.
function Parts({ parts, root }: { parts: { path: string; text: string }[]; root: string }) {
  return (
    <>
      {parts.map((p) => (
        <section key={p.path} className="product-part" data-path={p.path}>
          <MarkdownView text={p.text} baseAbs={`${root}/${p.path}`} />
        </section>
      ))}
    </>
  )
}

function Written({ ov, root }: { ov: ProductOverview; root: string }) {
  const parts = ov.parts ?? []
  // The print copy exists only while printing: the browser's print, with the
  // stylesheet leaving out everything but it (docs/frontends/gui.md#overview).
  const [printing, setPrinting] = useState(false)
  useEffect(() => {
    const before = () => flushSync(() => setPrinting(true))
    const after = () => setPrinting(false)
    window.addEventListener('beforeprint', before)
    window.addEventListener('afterprint', after)
    return () => {
      window.removeEventListener('beforeprint', before)
      window.removeEventListener('afterprint', after)
    }
  }, [])
  return (
    <>
      <div className="row product-doc-head">
        <span className="muted">
          {parts.length} part{parts.length === 1 ? '' : 's'}, in {ov.order === 'index' && ov.index ? `the order ${ov.index} gives` : 'file name order'}
        </span>
        <button
          onClick={() => {
            flushSync(() => setPrinting(true))
            window.print()
          }}
        >
          Print / Save as PDF
        </button>
      </div>
      <div className="product-doc">
        <Parts parts={parts} root={root} />
      </div>
      {printing &&
        createPortal(
          <div className="print-root">
            <Parts parts={parts} root={root} />
          </div>,
          document.body,
        )}
    </>
  )
}

function Program({ ov, readmeAbs, counts }: { ov: ProductOverview; readmeAbs: string; counts: Record<string, number> }) {
  const w = useWords()
  const m = ov.medium
  const b = ov.build
  const readme = ov.readme
  const total = Object.values(counts).reduce((a, n) => a + n, 0)
  return (
    <>
      <h2>How to run it</h2>
      <div className="card">
        {m && (
          <p className="row" style={{ margin: '2px 0' }}>
            <b>form</b> <span>{m.form}</span>
            {m.toolchain && <span className="mono muted">{m.toolchain}</span>}
          </p>
        )}
        {b?.run && (
          <p className="row" style={{ margin: '2px 0' }}>
            <b>build</b> <span className="mono">{b.run}</span>
            {b.cwd && b.cwd !== '.' && <span className="muted mono">in {b.cwd}</span>}
          </p>
        )}
        {ov.commands.map((c) => (
          <p key={`${c.cwd} ${c.run}`} className="row" style={{ margin: '2px 0' }}>
            <b>test</b> <span className="mono">{c.run}</span>
            {c.cwd && c.cwd !== '.' && <span className="muted mono">in {c.cwd}</span>}
          </p>
        ))}
        {ov.moreCommands > 0 && <p className="muted" style={{ margin: '2px 0' }}>and {ov.moreCommands} more test command{ov.moreCommands === 1 ? '' : 's'}</p>}
        {!m && !b?.run && ov.commands.length === 0 && <p className="muted" style={{ margin: '2px 0' }}>No form, build, or test command is recorded yet.</p>}
      </div>
      <h2>Tests</h2>
      <p className="product-tests">
        {total === 0 && <span className="muted">No tests yet. </span>}
        {Object.entries(counts).map(([st, n]) => (
          <span key={st} className={verifyClass(st)} style={{ marginRight: 12 }}>
            {n} {st}
          </span>
        ))}
        <Link to="/product/trace">open the {w.trace.toLowerCase()} →</Link>
      </p>
      <h2>README</h2>
      {readme ? (
        <>
          <p className="muted">
            {readme.tree === 'deliverable' ? (
              <span className="mono">{readme.path}</span>
            ) : (
              <>
                The product has no README, so its root document stands in: <span className="mono">{readme.path}</span>
              </>
            )}
          </p>
          <div className="product-doc product-readme">
            <section className="product-part">
              <MarkdownView text={readme.text} baseAbs={readmeAbs} />
            </section>
          </div>
        </>
      ) : (
        <p className="muted">No README and no document to stand in for one.</p>
      )}
    </>
  )
}

export default function Overview({
  ov,
  root,
  projectRoot,
  counts,
}: {
  ov: ProductOverview
  // The deliverable's and the project's absolute paths, what links resolve against.
  root: string
  projectRoot: string
  counts: Record<string, number>
}) {
  const w = useWords()
  if (ov.kind === 'none')
    return (
      <div className="empty-state">
        <b>No product yet</b>
        Nothing is in the product folder yet. {w.generate} produces it from the documents.
        <div className="actions">
          <Link to="/product/build" className="btn">
            {w.generate}
          </Link>
          <Link to="/settings" className="btn">
            Settings
          </Link>
        </div>
      </div>
    )
  if (ov.kind === 'written') return <Written ov={ov} root={root} />
  if (ov.kind === 'built') return <Built ov={ov} />
  const readmeAbs = `${ov.readme?.tree === 'docs' ? projectRoot : root}/${ov.readme?.path ?? ''}`
  return <Program ov={ov} readmeAbs={readmeAbs} counts={counts} />
}
