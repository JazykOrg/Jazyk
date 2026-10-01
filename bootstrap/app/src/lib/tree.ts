// The file tree the navigator draws (docs/frontends/gui.md#docs): folders first,
// then files, each level sorted by name; which folders stand open, remembered per
// project; and the glob the documents are matched by, so a file outside it dims.
import { useApp } from './store'
import { readOpenId } from './open'

export interface TreeFile<T> {
  kind: 'file'
  name: string
  path: string
  item: T
}

export interface TreeDir<T> {
  kind: 'dir'
  name: string
  path: string
  children: TreeNode<T>[]
}

export type TreeNode<T> = TreeFile<T> | TreeDir<T>

function byName(a: { name: string }, b: { name: string }): number {
  return a.name.localeCompare(b.name, undefined, { numeric: true, sensitivity: 'base' })
}

// Folders before files at every level, each group by name. `extraDirs` are folders
// that hold no file yet (a folder typed in place, waiting for its first document).
export function buildTree<T extends { path: string }>(items: T[], extraDirs: string[] = []): TreeNode<T>[] {
  interface Level {
    dirs: Map<string, Level>
    files: TreeFile<T>[]
  }
  const root: Level = { dirs: new Map(), files: [] }
  const dirAt = (parts: string[]): Level => {
    let node = root
    for (const p of parts) {
      let next = node.dirs.get(p)
      if (!next) {
        next = { dirs: new Map(), files: [] }
        node.dirs.set(p, next)
      }
      node = next
    }
    return node
  }
  for (const d of extraDirs) {
    const parts = d.split('/').filter((p) => p !== '')
    if (parts.length > 0) dirAt(parts)
  }
  for (const it of items) {
    const parts = it.path.split('/')
    const name = parts.pop() ?? it.path
    dirAt(parts).files.push({ kind: 'file', name, path: it.path, item: it })
  }
  const emit = (level: Level, prefix: string): TreeNode<T>[] => {
    const dirs: TreeDir<T>[] = [...level.dirs.entries()]
      .map(([name, child]) => {
        const path = prefix ? `${prefix}/${name}` : name
        return { kind: 'dir' as const, name, path, children: emit(child, path) }
      })
      .sort(byName)
    const files = [...level.files].sort(byName)
    return [...dirs, ...files]
  }
  return emit(root, '')
}

// The tree as rows, top to bottom, with only the open folders' children shown:
// what the keyboard walks.
export interface TreeRow<T> {
  node: TreeNode<T>
  depth: number
  parent: string | null
}

export function visibleRows<T>(nodes: TreeNode<T>[], isOpen: (dir: string) => boolean): TreeRow<T>[] {
  const out: TreeRow<T>[] = []
  const walk = (list: TreeNode<T>[], depth: number, parent: string | null) => {
    for (const n of list) {
      out.push({ node: n, depth, parent })
      if (n.kind === 'dir' && isOpen(n.path)) walk(n.children, depth + 1, n.path)
    }
  }
  walk(nodes, 0, null)
  return out
}

// The folders on the way to a path (`a/b/c.md` → `a`, `a/b`).
export function ancestors(path: string): string[] {
  const parts = path.split('/')
  const out: string[] = []
  for (let i = 1; i < parts.length; i++) out.push(parts.slice(0, i).join('/'))
  return out
}

export function dirOf(path: string): string {
  const i = path.lastIndexOf('/')
  return i < 0 ? '' : path.slice(0, i)
}

// The docs glob (docs/compiler/project-settings.md): `**` spans directories, `*`
// and `?` stay within one, a leading `!` negates, the last matching pattern wins.
// Mirrors the compiler's `glob_included`.
export function globToRegExp(pattern: string): RegExp {
  let re = '^'
  for (let i = 0; i < pattern.length; i++) {
    const c = pattern[i]
    if (c === '*') {
      if (pattern[i + 1] === '*') {
        // `**/` matches zero or more whole segments; a bare `**` matches the rest.
        if (pattern[i + 2] === '/') {
          re += '(?:.*/)?'
          i += 2
        } else {
          re += '.*'
          i += 1
        }
      } else re += '[^/]*'
    } else if (c === '?') re += '[^/]'
    else re += c.replace(/[.+^${}()|[\]\\]/g, '\\$&')
  }
  return new RegExp(`${re}$`)
}

export function matchesGlob(path: string, patterns: string[]): boolean {
  let included = false
  for (const pat of patterns) {
    const neg = pat.startsWith('!')
    const p = neg ? pat.slice(1) : pat
    if (globToRegExp(p).test(path)) included = !neg
  }
  return included
}

// Which folders a project's tree has closed, per project and per tree head. Open
// is the default: a first visit shows everything.
const CLOSED_KEY = 'jazyk.tree.closed'

// What identifies the project: the recent entry the browser remembers as open (a
// demo, a browser project, a folder), else what the backend reports (a server's
// root).
export function treeProjectKey(fallback: string): string {
  return readOpenId() ?? fallback
}

// The same key, live: a component using it follows a project switch at once.
export function useProjectKey(fallback: string): string {
  const id = useApp((a) => a.openId)
  return id ?? fallback
}

export function readClosedFolders(head: string, projectKey: string): Set<string> {
  try {
    const raw = localStorage.getItem(`${CLOSED_KEY}:${projectKey}:${head}`)
    const list = raw ? (JSON.parse(raw) as unknown) : []
    return new Set(Array.isArray(list) ? list.filter((x): x is string => typeof x === 'string') : [])
  } catch {
    return new Set()
  }
}

export function writeClosedFolders(head: string, projectKey: string, closed: Set<string>) {
  try {
    localStorage.setItem(`${CLOSED_KEY}:${projectKey}:${head}`, JSON.stringify([...closed]))
  } catch {
    // not remembered
  }
}
