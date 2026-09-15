// A source reference: opens the editor at the section (and quote, when given).
import { Link } from 'react-router'
import { docHref } from '../lib/nav'

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
  return (
    <Link className="id mono" to={docHref(doc, section, quote)}>
      {children ?? (section ? `${doc}#${section}` : doc)}
    </Link>
  )
}
