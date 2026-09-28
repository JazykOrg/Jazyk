// A rendered view picture with its drill-down anchors kept inside the app: a click
// on the renderer's `../<kind>/<slug>.svg` anchor overlays that level view on the
// map instead of leaving the page (docs/compiler/diagrams.md#drill-down,
// docs/frontends/gui.md#graph).
import { useEffect, useRef } from 'react'
import { useLocation, useNavigate } from 'react-router'
import { diagramLinkTarget, viewIdFromDiagramHref } from '../lib/levels'

export default function DiagramSvg({ svg, className }: { svg: string; className?: string }) {
  const navigate = useNavigate()
  const loc = useLocation()
  const ref = useRef<HTMLDivElement>(null)
  // The picture keeps its rendered size: the hard width and height were dropped so
  // the anchors stay live, so the viewBox gives the natural width back, capped at
  // the panel (docs/frontends/gui.md#markdown-preview).
  useEffect(() => {
    const el = ref.current?.querySelector('svg')
    if (!el) return
    const vb = (el.getAttribute('viewBox') ?? '').split(/[\s,]+/).map(Number)
    if (vb.length === 4 && vb[2] > 0) {
      el.style.width = `${vb[2]}px`
      el.style.maxWidth = '100%'
      el.style.height = 'auto'
    }
  }, [svg])
  const onClick = (e: React.MouseEvent<HTMLDivElement>) => {
    const a = (e.target as Element).closest('a')
    if (!a) return
    // Any anchor in the picture stays in the app; only a level link goes somewhere.
    e.preventDefault()
    const href = a.getAttribute('href') ?? a.getAttribute('xlink:href')
    const id = viewIdFromDiagramHref(href)
    if (!id) {
      // A box links to its entity's card (`../../docsgen/entities/<slug>.md`): the
      // entity opens in the inspector; another generated page opens as itself.
      const target = diagramLinkTarget(href)
      if (target?.kind === 'entity') {
        const next = new URLSearchParams(loc.search)
        next.set('node', target.id)
        navigate(`${loc.pathname}?${next.toString()}`)
      } else if (target?.kind === 'page') navigate(`/product/generated/${target.path}`)
      return
    }
    const next = new URLSearchParams(loc.pathname.startsWith('/dev/map') ? loc.search : '')
    next.set('view', id)
    next.set('node', id)
    next.delete('focus')
    next.delete('detail')
    navigate(`/dev/map?${next.toString()}`)
  }
  return <div ref={ref} className={className ?? 'view-svg'} onClick={onClick} dangerouslySetInnerHTML={{ __html: svg }} />
}
