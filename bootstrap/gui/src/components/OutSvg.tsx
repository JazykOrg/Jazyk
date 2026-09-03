// A rendering under the out directory inline as svg, its anchors live
// (docs/frontends/gui.md#markdown-preview): the text comes through GET /api/out/file,
// is sanitized and relinked against the file's own path, and a click on a box
// navigates in the app the way a link in a page does. The map never uses this: it
// draws from the graph (docs/frontends/gui.md#graph).
import { useMemo } from 'react'
import { useNavigate } from 'react-router'
import { outFileUrl, resolveRel, splitHash, useLinkResolver } from '../lib/mdlinks'
import { useOutText } from '../lib/queries'
import { prepareSvg, ROUTE_ATTR } from '../lib/svg'

export default function OutSvg({ rel, alt, title }: { rel: string; alt?: string; title?: string }) {
  const navigate = useNavigate()
  const resolver = useLinkResolver()
  const textQ = useOutText(rel)
  const base = resolver.outAbs(rel)

  const prepared = useMemo(() => {
    if (textQ.data === undefined || base === null) return null
    return prepareSvg(textQ.data, (href) => {
      const { path, hash } = splitHash(href)
      if (path === '') return null
      return resolver.route(resolveRel(base, path), hash)
    })
  }, [textQ.data, base, resolver])

  const onClick = (e: React.MouseEvent<HTMLSpanElement>) => {
    const a = (e.target as Element).closest('a')
    if (!a) return
    const to = a.getAttribute(ROUTE_ATTR)
    if (to === null) {
      // Unlinked, or a scheme the browser handles.
      if (!a.getAttribute('href')) e.preventDefault()
      return
    }
    // Plain click stays in the app; modified clicks keep the browser's meaning.
    if (e.metaKey || e.ctrlKey || e.shiftKey || e.altKey || e.button !== 0) return
    e.preventDefault()
    navigate(to)
  }

  if (textQ.isError)
    return (
      <span className="md-noimg mono" title={textQ.error.message}>
        {alt || rel}
      </span>
    )
  // Spans, displayed as blocks: the preview places an image inside a paragraph.
  if (textQ.data === undefined) return <span className="md-svg md-svg-loading" />
  // Not an svg document after all: the browser shows what it can.
  if (prepared === null) return <img src={outFileUrl(rel)} alt={alt ?? ''} title={title} />
  return (
    <span className="md-svg" title={title} onClick={onClick}>
      <span
        className="md-svg-canvas"
        style={prepared.width === null ? undefined : { width: prepared.width }}
        dangerouslySetInnerHTML={{ __html: prepared.html }}
      />
    </span>
  )
}
