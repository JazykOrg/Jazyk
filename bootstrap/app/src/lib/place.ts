// Where the open project's changes go, in one line, from the backend and its
// persistence (docs/frontends/gui.md#backends-and-capabilities). Said once, in the
// project menu, and on Home's Open now card.
import type { Capabilities } from './capabilities'
import type { FolderState, StoredState } from './store'

export function whereChangesGo(caps: Capabilities | undefined, folder: FolderState | null, stored: StoredState | null): string {
  if (caps?.backend === 'server') return 'Saved on this computer by jazyk'
  if (folder) return `Saved to ${folder.name} (${folder.mode === 'write' ? 'writing' : 'read-only'})`
  if (stored?.kind === 'demo') return stored.available ? 'Demo: edits are saved in this browser' : 'Demo: changes stay in this tab'
  if (stored?.kind === 'browser') return stored.available ? 'Saved in this browser only' : 'Changes stay in this tab'
  switch (caps?.persistence) {
    case 'disk':
      return 'Saved on this computer by jazyk'
    case 'folder':
      return 'Saved to a folder on this computer'
    case 'browser':
      return caps.backend === 'demo' ? 'Demo: edits are saved in this browser' : 'Saved in this browser only'
    case 'memory':
      return caps.backend === 'demo' ? 'Demo: changes stay in this tab' : 'Changes stay in this tab'
    default:
      return ''
  }
}

// The count of changes a project has not written, for the warning beside the name
// in the top bar: a folder's kept changes once a write failed (a refused grant
// included), or a stored project's refused ones. Changes held by choice in
// read-only mode are a state, not a problem, and stay in the menu.
export function unwrittenCount(folder: FolderState | null, stored: StoredState | null): number {
  if (folder) return folder.error ? folder.held : 0
  if (stored?.error) return stored.unsaved
  return 0
}
