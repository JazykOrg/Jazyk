// The markdown rendering: a file rendered as a page, its relative links and images
// resolved against the file's own path so the docsgen walk works inside the GUI
// (docs/frontends/gui.md#markdown-preview). A link to a page the GUI serves
// navigates in place; a link with a scheme opens a new tab; a fragment scrolls
// the page to the heading; a link to a document the project does not hold says
// so in place; an image under the out directory loads through GET /api/out/file,
// an .svg there inline with its anchors live (OutSvg).
//
// As the editor's Read (docs/frontends/gui.md#editor) it carries what Write draws:
// every block knows its source lines, so the read state and the build bands color
// it, a requirement marker sits in its margin, a question or proposal band follows
// it, and entity mentions are underlined and open the inspector.
import { forwardRef, useCallback, useEffect, useImperativeHandle, useMemo, useRef, type ReactNode } from 'react'
import { useNavigate } from 'react-router'
import Markdown, { type Components, type ExtraProps } from 'react-markdown'
import remarkGfm from 'remark-gfm'
import type { Element, ElementContent, Root, Text } from 'hast'
import { hasScheme, resolveRel, slug, splitHash, useLinkResolver } from '../lib/mdlinks'
import { findMentions, type MentionIndex } from '../lib/mentions'
import { useApp } from '../lib/store'
import type { InlineBand } from '../ide/CmHost'
import OutSvg from './OutSvg'
import './markdown.css'

// What Read draws over the blocks, by 1-based source line.
export interface ReadDecor {
  // The classes a block starting on the line takes (read state, build band).
  lineClass: (line: number) => string
  lineTitle: (line: number) => string | undefined
  // The requirement markers, by the line their quote starts on.
  marks: Map<number, { rid: string; cls: string; tip: string }>
  // The bands, by the line their sentence ends on.
  bands: Map<number, InlineBand[]>
}

export interface MarkdownViewHandle {
  // Scroll the block holding the 1-based line to the top.
  revealLine(line: number): void
  revealSlug(slug: string): void
}

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

// The blocks that carry their source lines. A list item carries its band inside;
// the rest carry it after.
const BLOCKS = new Set(['p', 'h1', 'h2', 'h3', 'h4', 'h5', 'h6', 'li', 'pre', 'blockquote', 'table', 'ul', 'ol', 'hr'])
// The blocks a marker or a band attaches to: the innermost that spans the line.
const LEAF_BLOCKS = new Set(['p', 'h1', 'h2', 'h3', 'h4', 'h5', 'h6', 'li', 'pre', 'blockquote', 'table'])

function lineSpan(el: Element): [number, number] | null {
  const p = el.position
  return p?.start?.line !== undefined && p.end?.line !== undefined ? [p.start.line, p.end.line] : null
}

// The rehype pass: source lines and decoration classes on the blocks, the marks
// and bands on the innermost block holding their line, and the mentions wrapped
// in the text. One tree walk, no dependency beyond the hast the renderer builds.
function decoratePlugin(mentions: MentionIndex | undefined, decor: ReadDecor | undefined) {
  return () => (tree: Root) => {
    const markLines = decor ? [...decor.marks.keys()] : []
    const bandLines = decor ? [...decor.bands.keys()] : []
    const claimedMark = new Set<number>()
    const claimedBand = new Set<number>()

    const wrapText = (node: Text): ElementContent[] | null => {
      if (!mentions || !mentions.re) return null
      const hits = findMentions(node.value, mentions)
      if (hits.length === 0) return null
      const out: ElementContent[] = []
      let last = 0
      for (const h of hits) {
        if (h.from > last) out.push({ type: 'text', value: node.value.slice(last, h.from) })
        out.push({
          type: 'element',
          tagName: 'span',
          properties: {
            className: ['entity-mark'],
            dataEntity: h.id,
            title: mentions.names.find((n) => n.id === h.id)?.name ?? h.name,
          },
          children: [{ type: 'text', value: node.value.slice(h.from, h.to) }],
        })
        last = h.to
      }
      if (last < node.value.length) out.push({ type: 'text', value: node.value.slice(last) })
      return out
    }

    const walk = (el: Element | Root, inCode: boolean) => {
      // Innermost first: a child claims a line before its parent can.
      for (const child of el.children) {
        if (child.type === 'element') walk(child, inCode || child.tagName === 'code' || child.tagName === 'pre')
      }
      if (el.type === 'element' && BLOCKS.has(el.tagName)) {
        const span = lineSpan(el)
        if (span) {
          const [start, end] = span
          const props = el.properties
          props.dataLine = String(start)
          props.dataLineEnd = String(end)
          if (decor) {
            const cls = decor.lineClass(start)
            if (cls) props.className = [...((props.className as string[] | undefined) ?? []), ...cls.split(' ')]
            const title = decor.lineTitle(start)
            if (title && !props.title) props.title = title
            if (LEAF_BLOCKS.has(el.tagName)) {
              const mark = markLines.find((l) => l >= start && l <= end && !claimedMark.has(l))
              if (mark !== undefined) {
                claimedMark.add(mark)
                props.dataMark = String(mark)
              }
              const band = bandLines.find((l) => l >= start && l <= end && !claimedBand.has(l))
              if (band !== undefined) {
                claimedBand.add(band)
                props.dataBand = String(band)
              }
            }
          }
        }
      }
      if (!inCode && el.type === 'element' && el.tagName !== 'code' && el.tagName !== 'pre') {
        const next: ElementContent[] = []
        let changed = false
        for (const child of el.children) {
          const wrapped = child.type === 'text' ? wrapText(child) : null
          if (wrapped) {
            next.push(...wrapped)
            changed = true
          } else next.push(child)
        }
        if (changed) el.children = next
      }
    }
    walk(tree, false)
  }
}

// One band's element, rendered by its owner into the DOM once per key.
function Band({ band }: { band: InlineBand }) {
  const ref = useRef<HTMLDivElement>(null)
  useEffect(() => {
    ref.current?.replaceChildren(band.render())
  }, [band])
  return <div className="jz-band md-band" ref={ref} />
}

const MarkdownView = forwardRef<
  MarkdownViewHandle,
  {
    text: string
    // The absolute path of the file the text came from; relative targets resolve
    // against its directory.
    baseAbs: string
    // A heading slug to reveal once the text is rendered (the route's `#fragment`).
    fragment?: string
    mentions?: MentionIndex
    decor?: ReadDecor
    onOpenNode?: (id: string) => void
    // A document this page links to opens in this mode (Read links open Read).
    docsOpenIn?: 'read' | 'write'
    // The 1-based line of the block at the top of the view, as the reader scrolls.
    onTopLine?: (line: number) => void
  }
>(function MarkdownView({ text, baseAbs, fragment, mentions, decor, onOpenNode, docsOpenIn, onTopLine }, ref) {
  const navigate = useNavigate()
  const resolver = useLinkResolver()
  const rootRef = useRef<HTMLDivElement>(null)
  const onTopLineRef = useRef(onTopLine)
  onTopLineRef.current = onTopLine

  useEffect(() => {
    if (fragment) requestAnimationFrame(() => scrollToSlug(rootRef.current, fragment))
  }, [fragment, text, baseAbs])

  const blockAtLine = useCallback((line: number): HTMLElement | null => {
    const root = rootRef.current
    if (!root) return null
    let best: HTMLElement | null = null
    let bestLine = Infinity
    for (const el of Array.from(root.querySelectorAll<HTMLElement>('[data-line]'))) {
      const start = Number(el.dataset.line)
      const end = Number(el.dataset.lineEnd ?? el.dataset.line)
      if (line >= start && line <= end) return el
      if (start >= line && start < bestLine) {
        best = el
        bestLine = start
      }
    }
    return best
  }, [])

  // A reveal scrolls the page itself; the scroll it causes is not the reader
  // moving, so the top of the view is not reported for a moment after it.
  const holdUntil = useRef(0)
  useImperativeHandle(
    ref,
    () => ({
      revealLine: (line) => {
        const el = blockAtLine(line)
        holdUntil.current = performance.now() + 600
        el?.scrollIntoView({ block: 'start' })
      },
      revealSlug: (s) => {
        holdUntil.current = performance.now() + 600
        scrollToSlug(rootRef.current, s)
      },
    }),
    [blockAtLine],
  )

  // The block at the top of the view, reported a frame after the scroll settles.
  const scrollFrame = useRef<number | null>(null)
  const reportTop = useCallback(() => {
    if (scrollFrame.current !== null) return
    scrollFrame.current = requestAnimationFrame(() => {
      scrollFrame.current = null
      const root = rootRef.current
      if (!root || !onTopLineRef.current || performance.now() < holdUntil.current) return
      const top = root.getBoundingClientRect().top
      for (const el of Array.from(root.querySelectorAll<HTMLElement>('[data-line]'))) {
        if (el.getBoundingClientRect().bottom > top + 2) {
          onTopLineRef.current(Number(el.dataset.line))
          return
        }
      }
    })
  }, [])
  useEffect(() => {
    reportTop()
  }, [text, reportTop])
  useEffect(
    () => () => {
      if (scrollFrame.current !== null) cancelAnimationFrame(scrollFrame.current)
    },
    [],
  )

  const plugins = useMemo(() => [decoratePlugin(mentions, decor)], [mentions, decor])

  const components = useMemo<Components>(() => {
    // A block with its lines: the marker in its margin, the band after it (inside a
    // list item, where a sibling would break the list).
    const block = (Tag: 'p' | 'h1' | 'h2' | 'h3' | 'h4' | 'h5' | 'h6' | 'li' | 'pre' | 'blockquote' | 'table' | 'ul' | 'ol' | 'hr') =>
      function Block(props: ExtraProps & { children?: ReactNode; className?: string; title?: string }) {
        const { node: _node, children, ...rest } = props
        const p = rest as Record<string, unknown>
        const markLine = p['data-mark'] as string | undefined
        const bandLine = p['data-band'] as string | undefined
        const mark = markLine !== undefined ? decor?.marks.get(Number(markLine)) : undefined
        const bands = bandLine !== undefined ? decor?.bands.get(Number(bandLine)) : undefined
        const extra = /^h[1-6]$/.test(Tag) ? { 'data-slug': slug(textOf(children)) } : {}
        const marker = mark ? (
          <span
            className={`md-req jz-req ${mark.cls}`}
            title={mark.tip}
            role="button"
            aria-label={`requirement ${mark.rid}`}
            onClick={(e) => {
              e.stopPropagation()
              onOpenNode?.(mark.rid)
            }}
          />
        ) : null
        const bandEls = bands ? bands.map((b) => <Band key={b.key} band={b} />) : null
        if (Tag === 'hr') return <hr {...(rest as object)} />
        if (Tag === 'li')
          return (
            <li {...(rest as object)}>
              {marker}
              {children}
              {bandEls}
            </li>
          )
        const El = Tag
        return (
          <>
            <El {...(rest as object)} {...extra}>
              {marker}
              {children}
            </El>
            {bandEls}
          </>
        )
      }
    return {
      p: block('p'),
      h1: block('h1'),
      h2: block('h2'),
      h3: block('h3'),
      h4: block('h4'),
      h5: block('h5'),
      h6: block('h6'),
      li: block('li'),
      pre: block('pre'),
      blockquote: block('blockquote'),
      table: block('table'),
      ul: block('ul'),
      ol: block('ol'),
      hr: block('hr'),
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
          // A fragment on this page: scroll the page, not the window.
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
        const abs = resolveRel(baseAbs, path)
        const route = resolver.route(abs, hash)
        if (route === null)
          return (
            <span className="md-dead" title={target}>
              {children}
              <span className="md-dead-note">not in this project</span>
            </span>
          )
        return (
          <a
            href={route}
            onClick={(e) => {
              // Plain click stays in the app; modified clicks keep the browser's meaning.
              if (e.metaKey || e.ctrlKey || e.shiftKey || e.altKey || e.button !== 0) return
              e.preventDefault()
              const at = resolver.locate(abs)
              if (docsOpenIn && at?.tree === 'docs') useApp.getState().setDocMode(at.rel, docsOpenIn)
              navigate(route)
            }}
          >
            {children}
          </a>
        )
      },
      img({ src, alt, title }) {
        const target = typeof src === 'string' ? src : ''
        if (target !== '' && !hasScheme(target)) {
          // A rendering under the out directory goes inline, its anchors live.
          const rel = resolver.outRel(resolveRel(baseAbs, target))
          if (rel !== null && /\.svg$/i.test(rel)) return <OutSvg rel={rel} alt={alt} title={title} />
        }
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
  }, [baseAbs, navigate, resolver, decor, onOpenNode, docsOpenIn])

  // A click on an underlined mention opens the entity; inside a link, the link wins.
  const onClick = (e: React.MouseEvent) => {
    if (!onOpenNode || e.metaKey || e.ctrlKey || e.shiftKey || e.altKey || e.button !== 0) return
    const el = e.target as HTMLElement
    const mark = el.closest('[data-entity]') as HTMLElement | null
    if (!mark || el.closest('a')) return
    const id = mark.dataset.entity
    if (id) onOpenNode(id)
  }

  return (
    <div ref={rootRef} className={`md-preview${decor ? ' md-read' : ''}`} onScroll={reportTop} onClick={onClick}>
      <Markdown remarkPlugins={[remarkGfm]} rehypePlugins={plugins} components={components}>
        {text}
      </Markdown>
    </div>
  )
})

export default MarkdownView
