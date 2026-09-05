// The preview pane: the next session's prompt exactly as the model receives it,
// with the batch's toolset and the executor it resolves to. In compile: manual the
// pane opens before a release, and its release button records the release and runs
// the build. Mirrors docs/frontends/gui.md#preview.
import { useState } from 'react'
import { post } from '../lib/api'
import { usePreview } from '../lib/queries'
import { useApp } from '../lib/store'

export default function PreviewPane({ target, onClose }: { target: string; onClose: () => void }) {
  const preview = usePreview(target)
  const watchMode = useApp((a) => a.watchMode)
  const jobs = useApp((a) => a.jobs)
  const running = Object.values(jobs).some((j) => j.state === 'running')
  const [busy, setBusy] = useState(false)
  const manual = watchMode !== 'watch'

  const releaseAndRun = async () => {
    setBusy(true)
    try {
      // The release records the approval; the job runs the build. An attached
      // agent's watcher fires from the same release.
      await post('/api/release', { stage: 'compile' })
      await post('/api/jobs', { kind: 'compile' })
      onClose()
    } finally {
      setBusy(false)
    }
  }

  const p = preview.data
  return (
    <div className="card" style={{ margin: '8px 0' }}>
      <div className="row">
        <b>preview</b>
        {p?.batch && <span className="mono">{p.batch.id}</span>}
        {p?.batch?.tier != null && <span className="chip sev-none">tier {p.batch.tier}</span>}
        {p?.gated && (
          <span className="chip sev-info" title="the batch the release forms; nothing runs until it is released">
            awaiting release
          </span>
        )}
        {p?.executor && <span className="chip sev-info" title="the executor this batch resolves to">{p.executor}</span>}
        {p?.executorError && <span className="v-bad">{p.executorError}</span>}
        <span className="bar-right" />
        {manual && (
          <button disabled={busy || running || !p?.batch} onClick={() => void releaseAndRun()}>
            release and compile ▸
          </button>
        )}
        {!manual && (
          <button disabled={busy || running} onClick={() => void post('/api/jobs', { kind: 'compile' })}>
            compile ▸
          </button>
        )}
        <button onClick={onClose} title="close">✕</button>
      </div>
      {p?.batch && (
        <p className="muted mono" style={{ margin: '2px 0' }}>
          {p.batch.goals.map((g) => `${g.kind} ${g.target}`).join(' · ')}
        </p>
      )}
      {(p?.toolset ?? []).length > 0 && (
        <p style={{ margin: '4px 0' }}>
          {(p?.toolset ?? []).map((t) => (
            <span key={t} className="chip sev-none" style={{ marginRight: 4 }}>
              {t}
            </span>
          ))}
        </p>
      )}
      {preview.isLoading && <p className="muted">assembling the prompt…</p>}
      {preview.error && <p className="error-inline">{preview.error.message}</p>}
      {p && !p.prompt && <p className="muted">{p.note ?? 'no ready batch'}</p>}
      {p?.prompt && (
        <>
          <p className="muted" style={{ margin: '4px 0 2px' }}>
            the prompt, assembled by the same code that runs the session; read-only, no LLM call
          </p>
          <pre className="pack" style={{ maxHeight: 360, overflow: 'auto' }}>{p.prompt}</pre>
        </>
      )}
    </div>
  )
}
