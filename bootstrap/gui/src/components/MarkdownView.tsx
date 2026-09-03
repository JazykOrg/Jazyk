// The markdown preview: a file rendered as a page, its relative links and images
// resolved against the file's own path so the docsgen walk works inside the GUI
// (docs/frontends/gui.md#markdown-preview). A link to a page the GUI serves
// navigates in place; a link with a scheme opens a new tab; a fragment scrolls
// the preview to the heading; an image under the out directory loads through
// GET /api/out/file.
import { useEffect, useMemo, useRef, type ReactNode } from 'react'
import { useNavigate } from 'react-router'
import Markdown, { type Components } from 'react-markdown'
import remarkGfm from 'remark-gfm'
import { hasScheme, resolveRel, slug, splitHash, useLinkResolver } from '../lib/mdlinks'
import './markdown.css'

// The visible text of a heading, for its slug.
function textOf(node: ReactNode): string {
  if (node === null || node === undefined || typeof node === 'boolean') return ''
  if (typeof node === 'string' || typeof node === 'number') return String(node)
  if (Array.isArray(node)) return node.map(textOf).join('')
  if (typeof node === 'object' && 'props' in node)
    return textOf((node as { props: { children?: ReactNode } }).props.children)
  return ''
}

function scrollToSlug(root: HTMLElement | null, s: string) {
  if (!root || !s) return
  const el = root.querySelector<HTMLElement>(`[data-slug="${CSS.escape(s)}"]`)
  el?.scrollIntoView({ block: 'start' })
}

export default function MarkdownView({
  text,
  baseAbs,
  fragment,
}: {
  text: string
  // The absolute path of the file the text came from; relative targets resolve
  // against its directory.
  baseAbs: string
  // A heading slug to reveal once the text is rendered (the route's `#fragment`).
  fragment?: string
}) {
  const navigate = useNavigate()
  const resolver = useLinkResolver()
  const rootRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    if (fragment) requestAnimationFrame(() => scrollToSlug(rootRef.current, fragment))
  }, [fragment, text, baseAbs])

  const components = useMemo<Components>(() => {
    const heading = (Tag: 'h1' | 'h2' | 'h3' | 'h4' | 'h5' | 'h6') =>
      function Heading({ children }: { children?: ReactNode }) {
        return <Tag data-slug={slug(textOf(children))}>{children}</Tag>
      }
    return {
      h1: heading('h1'),
      h2: heading('h2'),
      h3: heading('h3'),
      h4: heading('h4'),
      h5: heading('h5'),
      h6: heading('h6'),
      a({ href, children }) {
        const target = href ?? ''
        if (target === '') return <span>{children}</span>
        if (hasScheme(target))
          return (
            <a href={target} target="_blank" rel="noopener noreferrer">
              {children}
            </a>
          )
        const { path, hash } = splitHash(target)
        if (path === '') {
          // A fragment on this page: scroll the preview, not the window.
          return (
            <a
              href={`#${hash}`}
              onClick={(e) => {
                e.preventDefault()
                scrollToSlug(rootRef.current, hash)
              }}
            >
              {children}
            </a>
          )
        }
        const route = resolver.route(resolveRel(baseAbs, path), hash)
        if (route === null) return <span className="md-dead" title={target}>{children}</span>
        return (
          <a
            href={route}
            onClick={(e) => {
              // Plain click stays in the app; modified clicks keep the browser's meaning.
              if (e.metaKey || e.ctrlKey || e.shiftKey || e.altKey || e.button !== 0) return
              e.preventDefault()
              navigate(route)
            }}
          >
            {children}
          </a>
        )
      },
      img({ src, alt, title }) {
        const target = typeof src === 'string' ? src : ''
        const url = target === '' ? null : hasScheme(target) ? target : resolver.image(resolveRel(baseAbs, target))
        if (url === null)
          return (
            <span className="md-noimg mono" title={target}>
              {alt || target}
            </span>
          )
        return <img src={url} alt={alt ?? ''} title={title} loading="lazy" />
      },
    }
  }, [baseAbs, navigate, resolver])

  return (
    <div ref={rootRef} className="md-preview">
      <Markdown remarkPlugins={[remarkGfm]} components={components}>
        {text}
      </Markdown>
    </div>
  )
}
