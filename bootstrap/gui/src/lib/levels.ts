// Level navigation: the view id a rendered drill-down anchor names, and the
// breadcrumb chain from a scope root down to the level a view shows
// (docs/frontends/gui.md#graph, docs/compiler/concepts/levels.md#drill-down).
import type { TreeData, TreeNode, TreeRoot } from './api'

// A drill-down anchor in a rendered svg is `../<kind>/<slug>.svg`, relative under
// diagrams/ (docs/compiler/diagrams.md#drill-down); the view it names is
// `view:<kind>/<slug>`. Anything else is not a view link.
export function viewIdFromDiagramHref(href: string | null | undefined): string | null {
  if (!href) return null
  const m = href.match(/(?:^|\/)([a-z-]+)\/([^/?#]+)\.svg(?:[?#].*)?$/)
  return m ? `view:${m[1]}/${m[2]}` : null
}

export interface Crumb {
  // `scope:<scope>` for the scope root, the entity id otherwise.
  target: string
  label: string
  levelView: string | null
  views: string[]
  count: number
}

export interface LevelChain {
  // The scope root first, the level's node last.
  crumbs: Crumb[]
  // Set when the view is one of the last crumb's flow views rather than its
  // structural level view.
  flow: string | null
}

function crumbOf(n: TreeRoot | TreeNode): Crumb {
  return 'scope' in n
    ? { target: n.target, label: `scope:${n.scope}`, levelView: n.levelView, views: n.views, count: n.count }
    : { target: n.id, label: n.name, levelView: n.levelView, views: n.views, count: n.count }
}

// The chain from the scope root down to the level `viewId` shows; null when no level
// owns the view (a curated view, a state or object view).
export function levelChain(tree: TreeData | undefined, viewId: string): LevelChain | null {
  if (!tree || !viewId) return null
  const walk = (n: TreeRoot | TreeNode, path: Crumb[]): LevelChain | null => {
    const here = [...path, crumbOf(n)]
    if (n.levelView === viewId) return { crumbs: here, flow: null }
    if (n.views.includes(viewId)) return { crumbs: here, flow: viewId }
    for (const c of n.children) {
      const hit = walk(c, here)
      if (hit) return hit
    }
    return null
  }
  for (const r of tree.roots) {
    const hit = walk(r, [])
    if (hit) return hit
  }
  return null
}

// The targets from the scope root down to an entity, the entity last; empty when the
// tree does not hold it.
export function ancestorsOf(tree: TreeData | undefined, id: string): string[] {
  if (!tree || !id) return []
  const walk = (n: TreeNode, path: string[]): string[] | null => {
    const here = [...path, n.id]
    if (n.id === id) return here
    for (const c of n.children) {
      const hit = walk(c, here)
      if (hit) return hit
    }
    return null
  }
  for (const r of tree.roots) {
    for (const c of r.children) {
      const hit = walk(c, [r.target])
      if (hit) return hit
    }
  }
  return []
}
