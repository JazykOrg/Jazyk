// A rendering's text made safe and navigable for inline embedding
// (docs/frontends/gui.md#markdown-preview): scripts and event handlers dropped, the
// hard size dropped in favor of the viewBox, every anchor rewritten to the app route
// its target opens (or unlinked when the GUI serves no page for it).

export const ROUTE_ATTR = 'data-route'
export const DEAD_CLASS = 'md-dead'

// The natural size the renderer wrote, for the css that keeps it.
export interface PreparedSvg {
  html: string
  width: number | null
}

function parse(text: string): SVGSVGElement | null {
  const doc = new DOMParser().parseFromString(text, 'image/svg+xml')
  const root = doc.documentElement
  if (!root || root.localName !== 'svg' || doc.querySelector('parsererror')) return null
  return root as unknown as SVGSVGElement
}

// The width the viewBox names; null without one.
function viewBoxWidth(root: Element): number | null {
  const vb = root.getAttribute('viewBox')
  if (!vb) return null
  const parts = vb.trim().split(/[\s,]+/).map(Number)
  return parts.length === 4 && parts.every((n) => Number.isFinite(n)) && parts[2] > 0 ? parts[2] : null
}

function sanitize(root: Element) {
  for (const s of Array.from(root.querySelectorAll('script'))) s.remove()
  const walker = [root, ...Array.from(root.querySelectorAll('*'))]
  for (const el of walker) {
    for (const attr of Array.from(el.attributes)) {
      const name = attr.name.toLowerCase()
      if (name.startsWith('on')) el.removeAttribute(attr.name)
      else if ((name === 'href' || name === 'xlink:href') && /^\s*javascript:/i.test(attr.value))
        el.removeAttribute(attr.name)
    }
  }
}

// Drop the hard size when a viewBox can carry it: the element then follows the
// css width, its height from the aspect ratio.
function unsize(root: Element) {
  if (!root.getAttribute('viewBox')) return
  root.removeAttribute('width')
  root.removeAttribute('height')
  const style = root.getAttribute('style')
  if (style) {
    const kept = style
      .split(';')
      .map((s) => s.trim())
      .filter((s) => s !== '' && !/^(width|height)\s*:/i.test(s))
    if (kept.length) root.setAttribute('style', `${kept.join(';')};`)
    else root.removeAttribute('style')
  }
}

// Every anchor: `route(href)` names the app route the target opens, or null when
// nothing is served there. A scheme href is left to the browser.
function relink(root: Element, route: (href: string) => string | null) {
  for (const a of Array.from(root.querySelectorAll('a'))) {
    const href = a.getAttribute('href') ?? a.getAttribute('xlink:href')
    a.removeAttribute('xlink:href')
    if (!href || /^[a-z][a-z0-9+.-]*:/i.test(href)) continue
    const to = route(href)
    if (to === null) {
      a.removeAttribute('href')
      a.setAttribute('class', `${a.getAttribute('class') ?? ''} ${DEAD_CLASS}`.trim())
      continue
    }
    a.setAttribute('href', to)
    a.setAttribute(ROUTE_ATTR, to)
  }
}

// Null when the text is not an svg document.
export function prepareSvg(text: string, route: (href: string) => string | null): PreparedSvg | null {
  const root = parse(text)
  if (!root) return null
  sanitize(root)
  const width = viewBoxWidth(root)
  unsize(root)
  relink(root, route)
  return { html: new XMLSerializer().serializeToString(root), width }
}
