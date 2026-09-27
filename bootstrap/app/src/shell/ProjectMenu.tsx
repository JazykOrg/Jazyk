// The project menu (docs/frontends/gui.md#the-project-menu): the top bar's first
// item is the open project's name, and it is the one place that says where the
// project's changes go and offers what to do with them. A folder that could not be
// written also shows a short warning beside the name, the one exception, since it
// is a problem, not a state; clicking it opens the menu.
import * as DropdownMenu from '@radix-ui/react-dropdown-menu'
import { useState } from 'react'
import { Link, useNavigate } from 'react-router'
import { useCapabilities } from '../lib/capabilities'
import { folderPickerAvailable } from '../lib/folder'
import { unwrittenCount, whereChangesGo } from '../lib/place'
import { useApp, type FolderState, type StoredState } from '../lib/store'

const send = (name: string, detail?: unknown) => window.dispatchEvent(new CustomEvent(name, { detail }))
const changes = (n: number) => `${n} change${n === 1 ? '' : 's'}`

function Item({ onSelect, children, title, disabled }: { onSelect: () => void; children: React.ReactNode; title?: string; disabled?: boolean }) {
  return (
    <DropdownMenu.Item className="ws-menu-item" onSelect={onSelect} title={title} disabled={disabled}>
      {children}
    </DropdownMenu.Item>
  )
}

// A folder's mode and the switch, and what it has not taken
// (docs/frontends/wasm.md#when-a-folder-cannot-be-written).
function FolderItems({ f }: { f: FolderState }) {
  const kept = f.held
  const notStored = f.storeError ? ` This browser could not store them either: ${f.storeError}.` : ''
  return (
    <>
      {f.error && (
        <p className="ws-menu-note failed" role="status">
          Could not write to {f.name}: {changes(kept)} kept in this browser ({f.error}).{notStored}
        </p>
      )}
      {f.writing && kept > 0 && (
        <p className="ws-menu-note" role="status">
          Writing {changes(kept)} to {f.name}…
        </p>
      )}
      {kept > 0 && !f.writing && f.mode === 'read' && (
        <Item onSelect={() => send('jazyk:folder-mode', 'write')} title={`write them to ${f.name}, in order, and keep writing`}>
          Write {changes(kept)} to the folder
        </Item>
      )}
      {kept > 0 && !f.writing && f.mode === 'write' && (
        <Item onSelect={() => send('jazyk:folder-retry')} title={`write them to ${f.name}, in order`}>
          {f.error ? 'Retry' : `Write ${changes(kept)} to the folder`}
        </Item>
      )}
      {kept > 0 && !f.writing && (
        <Item onSelect={() => send('jazyk:folder-discard')} title={`forget them and reopen ${f.name} as it is on disk`}>
          Discard
        </Item>
      )}
      {f.mode === 'write' ? (
        <Item onSelect={() => send('jazyk:folder-mode', 'read')} title="keep changes in this browser instead of writing them">
          Switch to read-only
        </Item>
      ) : (
        <Item onSelect={() => send('jazyk:folder-mode', 'write')} title="write what this browser keeps to the folder, and keep writing">
          Switch to writing
        </Item>
      )}
    </>
  )
}

// A browser project or a demo: where it can go (docs/frontends/wasm.md#a-project-in-the-browser).
function StoredItems({ s }: { s: StoredState }) {
  const picker = folderPickerAvailable()
  return (
    <>
      {s.error && (
        <p className="ws-menu-note failed" role="status" title={s.error}>
          Could not save to this browser: {s.unsaved > 0 ? `${changes(s.unsaved)} in this tab only` : s.error}.
        </p>
      )}
      {s.error && (
        <Item onSelect={() => send('jazyk:store-retry')} title="store them again">
          Retry
        </Item>
      )}
      {picker && s.available && (
        <Item onSelect={() => send('jazyk:save-to-folder')} title="write every file into a folder on this computer; from then on the project is that folder">
          Save to a folder on this computer…
        </Item>
      )}
      <Item onSelect={() => send('jazyk:download')} title="the whole project as one .zip">
        Download as .zip
      </Item>
      {s.kind === 'demo' && s.edited && (
        <Item onSelect={() => send('jazyk:reset-demo', s.id)} title="discard your edits and go back to the demo as shipped">
          Reset to the original
        </Item>
      )}
    </>
  )
}

export default function ProjectMenu({ open, name, root }: { open: boolean; name: string; root?: string }) {
  const caps = useCapabilities()
  const folder = useApp((a) => a.folder)
  const stored = useApp((a) => a.stored)
  const navigate = useNavigate()
  const [menuOpen, setMenuOpen] = useState(false)

  if (!open)
    return (
      <Link to="/" className="ws-project" title="choose a project">
        <span className="ws-mark">J</span>
        <span>jazyk</span>
      </Link>
    )

  const server = caps.data?.backend === 'server'
  const unwritten = unwrittenCount(folder, stored)
  return (
    <DropdownMenu.Root open={menuOpen} onOpenChange={setMenuOpen}>
      <DropdownMenu.Trigger asChild>
        <button className="ws-project" title={root ?? name}>
          <span className="ws-mark">J</span>
          <span className="ws-project-name">{name}</span>
          <span aria-hidden className="ws-project-caret">
            ▾
          </span>
        </button>
      </DropdownMenu.Trigger>
      {unwritten > 0 && (
        <button className="ws-unwritten" role="status" onClick={() => setMenuOpen(true)} title="what happened and what to do">
          {changes(unwritten)} not written
        </button>
      )}
      <DropdownMenu.Portal>
        <DropdownMenu.Content className="ws-menu" align="start" sideOffset={6} collisionPadding={8}>
          <DropdownMenu.Label className="ws-menu-where">{whereChangesGo(caps.data, folder, stored)}</DropdownMenu.Label>
          {folder && <FolderItems f={folder} />}
          {!folder && stored && <StoredItems s={stored} />}
          <DropdownMenu.Separator className="ws-menu-sep" />
          <Item onSelect={() => navigate('/')}>Home</Item>
          {!server && (
            <Item onSelect={() => send('jazyk:close-project')} title="leave the project; nothing is deleted">
              Close project
            </Item>
          )}
        </DropdownMenu.Content>
      </DropdownMenu.Portal>
    </DropdownMenu.Root>
  )
}
