// Pure helpers the workspace's arrangements share (docs/frontends/gui.md#workspace-experimental):
// what identifies an item among the open ones, the selection a URL carries, the page a
// node opens as, and the recently shown items that stay mounted.

// An item is its page: the path without its query or fragment. A link to another
// section of an open document retargets that item rather than opening a second one.
export function itemKey(path: string): string {
  return path.split(/[?#]/)[0]
}

// The selection a URL carries (`?node=`, with the explorer's `?entity=` beside it)
// and the item's own path without it, so a click on a mention selects without
// opening anything.
export function splitSelection(full: string): { path: string; node: string | null } {
  const hashAt = full.indexOf('#')
  const hash = hashAt >= 0 ? full.slice(hashAt) : ''
  const beforeHash = hashAt >= 0 ? full.slice(0, hashAt) : full
  const q = beforeHash.indexOf('?')
  if (q < 0) return { path: full, node: null }
  const pathname = beforeHash.slice(0, q)
  const sp = new URLSearchParams(beforeHash.slice(q + 1))
  const node = sp.get('node') ?? sp.get('entity')
  sp.delete('node')
  sp.delete('entity')
  const rest = sp.toString()
  return { path: `${pathname}${rest ? `?${rest}` : ''}${hash}`, node }
}

// Paths that are the shell, not an item: Home, the connect route, and the documents'
// empty state, which is what the center shows with nothing open.
export function isShellPath(path: string): boolean {
  const p = itemKey(path)
  return p === '/' || p === '' || p === '/connect' || p === '/docs'
}

// The page a node opens as, or null when it has none of its own.
export function nodePagePath(id: string): string | null {
  const enc = encodeURIComponent(id)
  if (id.startsWith('ent:')) return `/model/entities/${enc}`
  if (id.startsWith('req:')) return `/model/requirements/${enc}`
  if (id.startsWith('view:')) return `/model/diagrams/${enc}`
  if (id.startsWith('diag:')) return '/inbox'
  return null
}

// The most recently shown first, at most `max`: the items that stay mounted while
// hidden, so switching back to one keeps its scroll and its editor.
export function touchRecent(recent: string[], key: string, max: number): string[] {
  const next = [key, ...recent.filter((k) => k !== key)]
  return next.slice(0, max)
}

// A navigator href (`#/docs/a.md` under the hash router, or `/docs/a.md`) as a path.
export function hrefPath(href: string | null): string | null {
  if (!href) return null
  if (href.startsWith('#/')) return href.slice(1)
  if (href.startsWith('/')) return href
  try {
    const u = new URL(href, 'http://x')
    if (u.hash.startsWith('#/')) return u.hash.slice(1)
    return u.pathname + u.search
  } catch {
    return null
  }
}
