// Read, Write, or Diff for every document at once (docs/frontends/gui.md#editor): the
// switch in the editor's bar and in a board window's bar.
import { useApp, type DocMode } from '../lib/store'

// The document mode in effect: the session's choice, else the open folder's default.
export function useDocMode(): DocMode {
  const chosen = useApp((a) => a.docMode)
  const folderMode = useApp((a) => a.folder?.mode)
  return chosen ?? (folderMode === 'read' ? 'read' : 'write')
}

export default function DocModeSwitch({ canDiff }: { canDiff: boolean }) {
  const mode = useDocMode()
  const setDocMode = useApp((a) => a.setDocMode)
  const shown = mode === 'diff' && !canDiff ? 'write' : mode
  const item = (m: DocMode, label: string, title: string, disabled = false) => (
    <button className={shown === m ? 'btn-on' : ''} aria-pressed={shown === m} title={title} disabled={disabled} onClick={() => setDocMode(m)}>
      {label}
    </button>
  )
  return (
    <span className="ide-mode nodrag" role="group" aria-label="Read, Write, or Diff, for every document">
      {item('read', 'Read', 'every document as its rendered page, with the same marks, nothing editable')}
      {item('write', 'Write', 'every document in the editor')}
      {item('diff', 'Diff', canDiff ? 'every document side by side against its last-built text' : 'this document never built: nothing to diff against', !canDiff)}
    </span>
  )
}
