// The workspace's moves over the items store, by item rather than by exact path: a
// path with another query shows the open item for the same page, retargeted
// (docs/frontends/gui.md#workspace).
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

// The item a close-as-back is leaving: the history steps that belonged to it are
// skipped on the way back (Workspace.tsx).
export const closing: { key: string | null } = { key: null }

// The browser's position in the app's history (the router keeps it in the state).
function historyIndex(): number {
  const st = window.history.state as { idx?: number } | null
  return typeof st?.idx === 'number' ? st.idx : 0
}

// Close an item. Closing the focused item is a back: the item shown before it comes
// to the front. With no earlier step in the app, the next item takes the focus.
export function closePath(path: string) {
  const open = findItem(path)
  if (!open) return
  const ws = useWorkspace.getState()
  const focused = ws.focused !== null && itemKey(ws.focused) === itemKey(open.path)
  if (focused && historyIndex() > 0 && ws.items.length > 1) {
    closing.key = itemKey(open.path)
    ws.closeItem(open.path, { viaBack: true })
    window.history.back()
    return
  }
  ws.closeItem(open.path)
}

// Back while closing: 'skip' when the history step is the closed item's and the
// walk went one further back; 'other' when that step is the first one, so another
// open item takes the focus; 'none' otherwise.
export function skipClosed(path: string): 'skip' | 'other' | 'none' {
  if (!closing.key) return 'none'
  if (itemKey(path) !== closing.key) {
    closing.key = null
    return 'none'
  }
  if (historyIndex() > 0) {
    window.history.back()
    return 'skip'
  }
  closing.key = null
  return 'other'
}
