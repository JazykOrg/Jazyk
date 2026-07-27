// One entity's generation task package, as the model would receive it.
import { useParams } from 'react-router'
import { useQuery } from '@tanstack/react-query'
import { get, post } from '../lib/api'
import { useApp } from '../lib/store'
import NodeLink, { linkifyIds } from '../components/NodeLink'
import './routes.css'

interface TaskReq {
  id: string
  ears: string
  quote: string
  testName: string
  hash: string
  criteriaPath: string
}

interface TaskPackage {
  instructions: string
  name: string
  context: string
  requirementGroups: TaskReq[][]
  changed: string[]
  generatedFiles: string[]
  deliverable: string
  factHash: string
}

export default function GenTask() {
  const { id: raw } = useParams()
  const id = decodeURIComponent(raw ?? '')
  const jobs = useApp((a) => a.jobs)
  const busy = Object.values(jobs).some((j) => j.state === 'running' || j.state === 'queued')
  const q = useQuery({
    queryKey: ['pending', 'task', id],
    queryFn: () => get<TaskPackage>(`/api/gen/task/${encodeURIComponent(id)}`),
    enabled: id.length > 0,
  })

  if (q.error)
    return (
      <p className="error-inline">
        {q.error.message}{' '}
        <a href="#retry" onClick={(e) => { e.preventDefault(); q.refetch() }}>retry</a>
      </p>
    )
  if (!q.data) return <p className="muted">loading…</p>
  const t = q.data

  return (
    <div>
      <h1>
        {t.name} <NodeLink id={id} />
      </h1>
      <div className="actionrow">
        <button disabled={busy} onClick={() => post('/api/jobs', { kind: 'gen', entities: [id] })}>
          run generation
        </button>
        <span className="muted mono">deliverable {t.deliverable}</span>
        <span className="muted mono">fact {t.factHash}</span>
      </div>

      <details>
        <summary>instructions</summary>
        <pre className="pack">{t.instructions}</pre>
      </details>

      <h2>context</h2>
      <pre className="pack">{t.context}</pre>

      <h2>requirements</h2>
      {t.requirementGroups.map((group, gi) => (
        <div key={gi} className="card">
          <p className="muted mono" style={{ margin: '0 0 4px' }}>group {gi + 1}</p>
          {group.map((r) => (
            <p key={r.id} style={{ margin: '2px 0' }}>
              <NodeLink id={r.id} /> {r.ears}{' '}
              <span className="muted mono">→ {r.testName}</span>
            </p>
          ))}
        </div>
      ))}

      {t.changed.length > 0 && (
        <>
          <h2>changed</h2>
          <p>{linkifyIds(t.changed.join(' '))}</p>
        </>
      )}

      <h2>generated files</h2>
      {t.generatedFiles.length === 0 && <p className="muted">nothing generated yet</p>}
      {t.generatedFiles.map((f) => (
        <p key={f} className="mono" style={{ margin: '2px 0' }}>{f}</p>
      ))}
    </div>
  )
}
