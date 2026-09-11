// A rendered view picture with its drill-down anchors kept inside the app: a click
// on the renderer's `../<kind>/<slug>.svg` anchor overlays that level view on the
// map instead of leaving the page (docs/compiler/diagrams.md#drill-down,
// docs/frontends/gui.md#graph).
import { useLocation, useNavigate } from 'react-router'
import { viewIdFromDiagramHref } from '../lib/levels'

export default function DiagramSvg({ svg, className }: { svg: string; className?: string }) {
  const navigate = useNavigate()
  const loc = useLocation()
  const onClick = (e: React.MouseEvent<HTMLDivElement>) => {
    const a = (e.target as Element).closest('a')
    if (!a) return
    // Any anchor in the picture stays in the app; only a level link goes somewhere.
    e.preventDefault()
    const id = viewIdFromDiagramHref(a.getAttribute('href') ?? a.getAttribute('xlink:href'))
    if (!id) return
    const next = new URLSearchParams(loc.pathname.startsWith('/graph') ? loc.search : '')
    next.set('view', id)
    next.set('node', id)
    next.delete('focus')
    next.delete('detail')
    navigate(`/graph?${next.toString()}`)
  }
  return <div className={className ?? 'view-svg'} onClick={onClick} dangerouslySetInnerHTML={{ __html: svg }} />
}
