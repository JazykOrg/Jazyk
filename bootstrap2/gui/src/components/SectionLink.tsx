// A source reference: opens the editor at the section (and quote, when given).
import { Link } from 'react-router'

export default function SectionLink({
  doc,
  section,
  quote,
  children,
}: {
  doc: string
  section?: string
  quote?: string
  children?: React.ReactNode
}) {
  const params = new URLSearchParams()
  if (section) params.set('section', section)
  if (quote) params.set('quote', quote)
  const qs = params.toString()
  return (
    <Link className="id mono" to={`/docs/${doc}${qs ? `?${qs}` : ''}`}>
      {children ?? (section ? `${doc}#${section}` : doc)}
    </Link>
  )
}
