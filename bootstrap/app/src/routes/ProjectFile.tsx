// Any other project file (jazyk.toml, a README outside the docs glob, a file the
// product folder does not hold), as text, read only (docs/frontends/gui.md#workspace).
import { useParams } from 'react-router'
import { useProjectFileText } from '../lib/queries'

export default function ProjectFile() {
  const params = useParams()
  const path = params['*'] ?? ''
  const q = useProjectFileText(path)
  return (
    <div className="ws-scroll project-file">
      {q.isLoading && <p className="muted ide-pad">loading…</p>}
      {q.isError && <p className="error-inline ide-pad">could not read {path}: {(q.error as Error).message}</p>}
      {q.data !== undefined && <pre className="project-file-text mono">{q.data}</pre>}
    </div>
  )
}
