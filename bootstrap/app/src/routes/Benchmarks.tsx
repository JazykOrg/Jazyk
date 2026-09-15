// Benchmarks: the merged model comparison table (embedded results, the machine-wide
// history, this project's own grades), one row per model and codec, and a run form
// that starts a benchmark job (docs/frontends/gui.md#benchmarks).
import { useState } from 'react'
import { post, type BenchmarkReport, type BenchmarkResult } from '../lib/api'
import { useBenchmarks, useBenchmarkModels } from '../lib/queries'
import { useApp } from '../lib/store'
import './routes.css'

// The palette's state classes, on the 0..1 scale the reports use.
const scoreClass = (v: number) => (v >= 0.9 ? 'v-ok' : v >= 0.5 ? 'v-stale' : 'v-bad')

// Workflow verdicts: compilation grades review > extraction > not-capable, the
// other two are capable or not (docs/benchmark/benchmark.md#report).
function verdictClass(v: string): string {
  if (v === 'review' || v === 'capable') return 'v-ok'
  if (v === 'extraction') return 'v-stale'
  if (v === 'not-capable') return 'v-bad'
  return 'v-none'
}

const sourceClass = (s: string) => (s === 'project' ? 'v-ok' : s === 'history' ? 'sev-info' : 'sev-none')

function age(unixSecs: number): string {
  if (!unixSecs) return ''
  const s = Math.max(0, Date.now() / 1000 - unixSecs)
  if (s < 3600) return `${Math.round(s / 60)}m`
  if (s < 86400) return `${Math.round(s / 3600)}h`
  if (s < 86400 * 60) return `${Math.round(s / 86400)}d`
  return `${Math.round(s / (86400 * 30))}mo`
}

interface TableRow {
  key: string
  result: BenchmarkResult
  codec: string
  report: BenchmarkReport
}

// The run form: endpoint and model resolve to the LLM settings when left empty.
function RunForm() {
  const [url, setUrl] = useState('')
  // The endpoint asked about models: committed on blur or enter, never per keystroke
  // (an unreachable URL holds the probe for its full timeout).
  const [probedUrl, setProbedUrl] = useState('')
  const [model, setModel] = useState('')
  const models = useBenchmarkModels(probedUrl)
  const jobs = useApp((a) => a.jobs)
  const busy = Object.values(jobs).some((j) => j.state === 'running' || j.state === 'queued')

  const run = () => {
    const body: Record<string, unknown> = { kind: 'benchmark' }
    if (url.trim()) body.baseUrl = url.trim()
    if (model.trim()) body.model = model.trim()
    post('/api/jobs', body)
  }

  return (
    <div className="actionrow">
      <input
        type="text"
        placeholder={models.data?.baseUrl || 'endpoint url'}
        value={url}
        onChange={(e) => setUrl(e.target.value)}
        onBlur={() => setProbedUrl(url.trim())}
        onKeyDown={(e) => e.key === 'Enter' && setProbedUrl(url.trim())}
        style={{ maxWidth: 320 }}
        title="the OpenAI-compatible endpoint to grade; empty runs the configured one"
      />
      <input
        type="text"
        list="bench-models"
        placeholder="model (from settings)"
        value={model}
        onChange={(e) => setModel(e.target.value)}
        style={{ maxWidth: 240 }}
        title="pick from the endpoint's listing or type any model name"
      />
      <datalist id="bench-models">
        {(models.data?.models ?? []).map((m) => (
          <option key={m} value={m} />
        ))}
      </datalist>
      <button disabled={busy} onClick={run} title="grade the model against the benchmark cases (spends LLM budget)">
        run benchmark ▸
      </button>
      {models.data?.error && (
        <span className="muted" title={models.data.error}>
          model listing unavailable, type a name
        </span>
      )}
    </div>
  )
}

// Live progress: the benchmark run's note lines, straight from the job trace.
function LiveProgress() {
  const jobs = useApp((a) => a.jobs)
  const trace = useApp((a) => a.trace)
  const job = Object.values(jobs).find(
    (j) => (j.state === 'running' || j.state === 'queued') && j.kind.kind === 'benchmark',
  )
  if (!job) return null
  const lines = trace.filter(
    (r) => r.jobId === job.id && r.event.kind === 'note' && r.event.label === 'benchmark',
  )
  return (
    <div className="card turn-active">
      <p className="row" style={{ margin: '2px 0' }}>
        <b>benchmark</b>
        <span className="chip v-stale">{job.state}</span>
        <button onClick={() => post(`/api/jobs/${job.id}/cancel`)}>cancel</button>
      </p>
      {lines.length === 0 && <p className="muted" style={{ margin: '2px 0' }}>waiting for the first case…</p>}
      {lines.map((r) => (
        <div key={r.seq} className="trace-row t-muted">
          {String(r.event.text ?? '')}
        </div>
      ))}
    </div>
  )
}

// One graded case per line: score, rounds against par, tokens, the first failing check.
function CaseDetail({ report }: { report: BenchmarkReport }) {
  return (
    <>
      {Object.entries(report.cases).map(([name, c]) => (
        <div key={name} className={`trace-row ${c.fail ? 't-err' : 't-ok'}`}>
          {c.fail ? '✗' : '✓'} {name} · <span className={scoreClass(c.score)}>{c.score.toFixed(2)}</span> ·{' '}
          {c.checks} checks · {c.rounds} rounds (par {c.parRounds}) · {c.tokens} tok
          {c.fail ? ` · ${c.fail}` : ''}
        </div>
      ))}
    </>
  )
}

const COLS = 14

export default function Benchmarks() {
  const table = useBenchmarks()
  const [openRow, setOpenRow] = useState<string | null>(null)

  if (table.error)
    return (
      <p className="error-inline">
        {table.error.message}{' '}
        <a href="#retry" onClick={(e) => { e.preventDefault(); table.refetch() }}>retry</a>
      </p>
    )
  if (!table.data) return <p className="muted">loading…</p>

  const rows: TableRow[] = table.data.results.flatMap((result) =>
    (['native', 'text'] as const).flatMap((codec) => {
      const report = result.codecs[codec]
      return report
        ? [{ key: `${result.source}:${result.model}:${result.caseSetHash}:${codec}`, result, codec, report }]
        : []
    }),
  )
  // Grades on this binary's case set first, then newest first.
  rows.sort(
    (a, b) => Number(b.result.current) - Number(a.result.current) || b.result.gradedAt - a.result.gradedAt,
  )

  return (
    <div>
      <h1>Benchmarks</h1>
      <p className="muted">
        Turn capability per model and codec, graded by deterministic checks. Case set{' '}
        <span className="mono">{table.data.caseSetHash.slice(0, 8)}</span>; grades taken on an older case set are
        marked stale.
      </p>

      <RunForm />
      <LiveProgress />

      {rows.length === 0 && <p className="empty">no benchmark results yet</p>}
      {rows.length > 0 && (
        <table>
          <thead>
            <tr>
              <th>model</th>
              <th>codec</th>
              <th>source</th>
              <th title="workflow verdict: review, extraction, or not-capable">compilation</th>
              <th title="workflow verdict">generation</th>
              <th title="workflow verdict">verification</th>
              <th title="tier score, 0..1">extr</th>
              <th title="tier score, 0..1">rev</th>
              <th title="tier score, 0..1">gen</th>
              <th title="tier score, 0..1">ver</th>
              <th title="rounds vs par, 0..1">eff</th>
              <th>tokens</th>
              <th>tok/s</th>
              <th>age</th>
            </tr>
          </thead>
          <tbody>
            {rows.map((row) => {
              const { result: r, report: rep } = row
              const open = openRow === row.key
              return (
                <FragmentRow
                  key={row.key}
                  row={row}
                  r={r}
                  rep={rep}
                  open={open}
                  toggle={() => setOpenRow(open ? null : row.key)}
                />
              )
            })}
          </tbody>
        </table>
      )}
    </div>
  )
}

function FragmentRow({
  row,
  r,
  rep,
  open,
  toggle,
}: {
  row: TableRow
  r: BenchmarkResult
  rep: BenchmarkReport
  open: boolean
  toggle: () => void
}) {
  return (
    <>
      <tr onClick={toggle} style={{ cursor: 'pointer' }} title="per-case detail">
        <td>
          <span className="mono">{r.model}</span>
          {!r.current && <span className="chip v-none" style={{ marginLeft: 6 }}>stale case set</span>}
        </td>
        <td className="mono">{row.codec}</td>
        <td>
          <span className={`chip ${sourceClass(r.source)}`}>{r.source}</span>
        </td>
        <td className={verdictClass(rep.verdicts.compilation)}>{rep.verdicts.compilation}</td>
        <td className={verdictClass(rep.verdicts.generation)}>{rep.verdicts.generation}</td>
        <td className={verdictClass(rep.verdicts.verification)}>{rep.verdicts.verification}</td>
        <td className={`mono ${scoreClass(rep.scores.extraction)}`}>{rep.scores.extraction.toFixed(2)}</td>
        <td className={`mono ${scoreClass(rep.scores.review)}`}>{rep.scores.review.toFixed(2)}</td>
        <td className={`mono ${scoreClass(rep.scores.generation)}`}>{rep.scores.generation.toFixed(2)}</td>
        <td className={`mono ${scoreClass(rep.scores.verification)}`}>{rep.scores.verification.toFixed(2)}</td>
        <td className="mono">{rep.efficiency.toFixed(2)}</td>
        <td className="mono">{rep.tokens}</td>
        <td className="mono">{rep.throughput}</td>
        <td className="mono muted" title={r.gradedAt ? new Date(r.gradedAt * 1000).toLocaleString() : ''}>
          {age(r.gradedAt)}
        </td>
      </tr>
      {open && (
        <tr>
          <td colSpan={COLS} style={{ padding: '6px 10px 10px' }}>
            <div className="muted" style={{ fontSize: 12, marginBottom: 4 }}>
              {rep.checks} checks
              {r.baseUrl ? ' · ' : ''}
              {r.baseUrl && <span className="mono">{r.baseUrl}</span>}
              {' · case set '}
              <span className="mono">{r.caseSetHash.slice(0, 8)}</span>
            </div>
            <CaseDetail report={rep} />
          </td>
        </tr>
      )}
    </>
  )
}
