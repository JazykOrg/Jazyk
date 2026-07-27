// The whole cross-linking model: any node id anywhere renders through this and
// routes to the universal resolver at /n/:id. Redirects resolve client-side.
import { Link } from 'react-router'
import { useGraph } from '../lib/queries'

export function useResolveId(id: string): string {
  const { data: graph } = useGraph()
  let cur = id
  const seen = new Set<string>()
  while (graph?.redirects[cur] && !seen.has(cur)) {
    seen.add(cur)
    cur = graph.redirects[cur]
  }
  return cur
}

export default function NodeLink({ id, children }: { id: string; children?: React.ReactNode }) {
  const resolved = useResolveId(id)
  return (
    <Link className="id mono" to={`/n/${encodeURIComponent(resolved)}`}>
      {children ?? resolved}
    </Link>
  )
}

// Linkify node ids appearing inside a plain string (trace args, reasoning, ops).
export function linkifyIds(text: string): React.ReactNode[] {
  const re = /\b((?:ent|req|rel|diag):[a-z0-9-]+)\b/g
  const out: React.ReactNode[] = []
  let last = 0
  let m: RegExpExecArray | null
  let k = 0
  while ((m = re.exec(text)) !== null) {
    if (m.index > last) out.push(text.slice(last, m.index))
    out.push(<NodeLink key={k++} id={m[1]} />)
    last = m.index + m[1].length
  }
  if (last < text.length) out.push(text.slice(last))
  return out
}
