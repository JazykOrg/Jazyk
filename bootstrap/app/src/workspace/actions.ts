// The workspace's moves over the items store, by item rather than by exact path: a
// path with another query shows the open item for the same page, retargeted
// (docs/frontends/gui.md#workspace-experimental).
import { useWorkspace, type Item } from './items'
import { itemKey } from './paths'

export function findItem(path: string): Item | undefined {
  const key = itemKey(path)
  return useWorkspace.getState().items.find((i) => itemKey(i.path) === key)
}

// Show a path: focus (and retarget) its open item, or open it.
export function showPath(path: string, opts: { preview?: boolean; from?: string; background?: boolean } = {}) {
  const ws = useWorkspace.getState()
  const open = findItem(path)
  if (!open) {
    ws.openItem(path, opts)
    return
  }
  if (opts.background) return
  if (open.path !== path) useWorkspace.setState((s) => ({ items: s.items.map((i) => (i === open ? { ...i, path } : i)) }))
  // focusItem remembers the retargeted list with the focus.
  ws.focusItem(path)
  if (!opts.preview && open.preview) ws.pinItem(path)
}

// Keep an item: a double click, an edit, or the tab's menu.
export function pinPath(path: string) {
  const open = findItem(path)
  if (open) useWorkspace.getState().pinItem(open.path)
  else useWorkspace.getState().openItem(path)
}

export function closePath(path: string) {
  const open = findItem(path)
  if (open) useWorkspace.getState().closeItem(open.path)
}
