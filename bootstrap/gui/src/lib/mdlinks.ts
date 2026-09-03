// Where a relative link or image in a markdown file lands: resolved against the
// open file's path, then located in one of the trees the GUI serves (the out
// directory, the docs glob, the deliverable) and turned into an app route or an
// asset URL (docs/frontends/gui.md#markdown-preview).
import { useCallback } from 'react'
import { tokenParam, type Project } from './api'
import { useDocs, useProject } from './queries'
import { delivHref, docHref, outHref } from './nav'

// Lexical path normalization: the project reports directories as configured
// (`../product`), and link targets carry the same unresolved segments.
export function normPath(p: string): string {
  const parts: string[] = []
  for (const seg of p.split('/')) {
    if (seg === '' || seg === '.') continue
    if (seg === '..') {
      parts.pop()
      continue
    }
    parts.push(seg)
  }
  return `${p.startsWith('/') ? '/' : ''}${parts.join('/')}`
}

// The path relative to a directory, or null when it is not under it.
export function relTo(dir: string, path: string): string | null {
  const d = normPath(dir)
  const p = normPath(path)
  return p.startsWith(`${d}/`) ? p.slice(d.length + 1) : null
}

export function dirname(p: string): string {
  const i = p.lastIndexOf('/')
  return i < 0 ? '' : p.slice(0, i)
}

// A link's target split into the path part and the fragment.
export function splitHash(href: string): { path: string; hash: string } {
  const i = href.indexOf('#')
  return i < 0 ? { path: href, hash: '' } : { path: href.slice(0, i), hash: href.slice(i + 1) }
}

export function hasScheme(href: string): boolean {
  return /^[a-z][a-z0-9+.-]*:/i.test(href)
}

// Resolve a relative href against the absolute path of the file it appears in.
export function resolveRel(baseAbs: string, href: string): string {
  const path = href.startsWith('/') ? href : `${dirname(baseAbs)}/${href}`
  return normPath(decodeURIComponent(path))
}

// The heading slug docsgen writes its fragments with: the compiler's own rule
// (lowercase, alphanumeric runs kept, everything else one dash, trimmed).
export function slug(s: string): string {
  let out = ''
  let prevDash = false
  for (const c of s.trim().toLowerCase()) {
    if (/[\p{L}\p{N}]/u.test(c)) {
      out += c
      prevDash = false
    } else if (!prevDash) {
      out += '-'
      prevDash = true
    }
  }
  return out.replace(/^-+|-+$/g, '')
}

export type Located =
  | { tree: 'out'; rel: string }
  | { tree: 'docs'; rel: string }
  | { tree: 'deliverable'; rel: string }
  | null

// Which tree an absolute path falls in. The out directory wins (it usually sits
// under the root), then a matched document, then the deliverable (which may be
// the root itself).
export function locate(abs: string, project: Project, docs: { path: string }[]): Located {
  const out = relTo(project.out, abs)
  if (out !== null) return { tree: 'out', rel: out }
  const doc = relTo(project.root, abs)
  if (doc !== null && docs.some((d) => d.path === doc)) return { tree: 'docs', rel: doc }
  const deliv = relTo(project.deliverable, abs)
  if (deliv !== null) return { tree: 'deliverable', rel: deliv }
  return null
}

// The raw bytes of a file under the out directory: an image element sends no
// bearer header, so the token rides in the query.
export function outFileUrl(rel: string): string {
  const qs = tokenParam()
  return `/api/out/file?path=${encodeURIComponent(rel)}${qs ? `&${qs}` : ''}`
}

export interface LinkResolver {
  // The app route a resolved absolute path opens, or null when the GUI serves no
  // page for it.
  route(abs: string, hash?: string): string | null
  // The URL an image at the absolute path loads from, or null when it is not served.
  image(abs: string): string | null
}

export function useLinkResolver(): LinkResolver {
  const projectQ = useProject()
  const docsQ = useDocs()
  const project = projectQ.data
  const docs = docsQ.data?.docs ?? []
  const route = useCallback(
    (abs: string, hash?: string) => {
      if (!project) return null
      const at = locate(abs, project, docs)
      if (!at) return null
      const frag = hash ? `#${hash}` : ''
      if (at.tree === 'out') return `${outHref(at.rel)}${frag}`
      if (at.tree === 'docs') return `${docHref(at.rel)}${frag}`
      return `${delivHref(at.rel)}${frag}`
    },
    [project, docs],
  )
  const image = useCallback(
    (abs: string) => {
      if (!project) return null
      const at = locate(abs, project, docs)
      return at?.tree === 'out' ? outFileUrl(at.rel) : null
    },
    [project, docs],
  )
  return { route, image }
}
