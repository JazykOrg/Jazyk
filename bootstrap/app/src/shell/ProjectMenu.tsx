// The project menu (docs/frontends/gui.md#the-project-menu): the top bar's first
// item is the open project's name, and it is a menu laid out like a desktop
// editor's File menu: the projects open now (this tab's, checked, and the ones in
// this browser's other tabs), then the ways to open another, then what to do with
// the one in front. It is the one place that says where the project's changes go.
// A folder that could not be written also shows a short warning beside the name,
// the one exception, since it is a problem, not a state; clicking it opens the
// menu. With no project open the trigger reads `jazyk` and the menu holds the ways
// to open one.
import * as DropdownMenu from '@radix-ui/react-dropdown-menu'
import { useEffect, useRef, useState } from 'react'
import { useNavigate } from 'react-router'
import { storageAvailable } from '../lib/browser-store'
import { useCapabilities } from '../lib/capabilities'
import { DEFAULT_PORT, useConnection } from '../lib/connect'
import { DEMOS } from '../lib/demos'
import { folderPickerAvailable } from '../lib/folder'
import { unwrittenCount, whereChangesGo } from '../lib/place'
import { kindWords, setReadOnlyPref, useReadOnlyPref, useRecent } from '../lib/recent'
import { useApp, type FolderState, type StoredState } from '../lib/store'
import { tabPresence, useOtherTabs, type TabProject } from '../lib/tabs'

const send = (name: string, detail?: unknown) => window.dispatchEvent(new CustomEvent(name, { detail }))
const changes = (n: number) => `${n} change${n === 1 ? '' : 's'}`

const NO_PICKER = 'Opening a folder needs Chrome, Edge, or another Chromium browser; to work on a folder from this browser, connect to jazyk on this computer'
const NO_STORAGE = 'This browser offers no storage to this page (a private window may not), so a project started here could not survive a reload'

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

// A submenu: its trigger, and its rows in a panel beside the menu.
function Sub({ label, title, disabled, children }: { label: string; title?: string; disabled?: boolean; children: React.ReactNode }) {
  return (
    <DropdownMenu.Sub>
      <DropdownMenu.SubTrigger className="ws-menu-item ws-menu-subtrigger" title={title} disabled={disabled}>
        {label}
      </DropdownMenu.SubTrigger>
      <DropdownMenu.Portal>
        <DropdownMenu.SubContent className="ws-menu-sub" sideOffset={4} collisionPadding={8}>
          {children}
        </DropdownMenu.SubContent>
      </DropdownMenu.Portal>
    </DropdownMenu.Sub>
  )
}

// The two items that ask for one thing first: a new project's name, a server's
// port. A small modal over the page; Escape or Cancel closes it.
function AskDialog({ ask, onClose }: { ask: 'new' | 'connect'; onClose: () => void }) {
  const ref = useRef<HTMLDialogElement>(null)
  const conn = useConnection()
  const [value, setValue] = useState(ask === 'connect' ? String(conn.state === 'idle' ? DEFAULT_PORT : conn.port) : '')
  const [asked, setAsked] = useState<number | null>(null)

  useEffect(() => {
    const d = ref.current
    if (d && !d.open) d.showModal()
  }, [])

  // A connect closes the dialog once the server answered; one that did not answer
  // says so here.
  useEffect(() => {
    if (asked !== null && conn.state === 'connected' && conn.port === asked) onClose()
  }, [asked, conn, onClose])

  const port = Number(value)
  const validPort = Number.isInteger(port) && port > 0 && port < 65536
  const busy = conn.state === 'connecting'

  return (
    <dialog ref={ref} className="ws-ask" onClose={onClose} aria-label={ask === 'new' ? 'New project in this browser' : 'Connect to jazyk on this computer'}>
      <form
        onSubmit={(e) => {
          e.preventDefault()
          if (ask === 'new') {
            send('jazyk:new-project', { name: value.trim() || 'Untitled project' })
            ref.current?.close()
            return
          }
          if (!validPort || busy) return
          setAsked(port)
          send('jazyk:connect', { port })
        }}
      >
        {ask === 'new' ? (
          <>
            <h2>New project in this browser</h2>
            <p className="muted">
              A <span className="mono">jazyk.toml</span> and a <span className="mono">docs/README.md</span> to write in, kept in this browser only.
            </p>
            <input type="text" value={value} onChange={(e) => setValue(e.target.value)} placeholder="Project name, e.g. Garden planner" aria-label="Project name" autoFocus />
          </>
        ) : (
          <>
            <h2>Connect to jazyk on this computer</h2>
            <p className="muted">
              Run <span className="mono">jazyk gui</span> in the project's folder; it prints the port it serves on.
            </p>
            <input type="number" inputMode="numeric" min={1} max={65535} value={value} onChange={(e) => setValue(e.target.value)} aria-label="Port" className="mono" autoFocus />
            {asked !== null && conn.state === 'unreachable' && conn.port === asked && (
              <p className="v-stale" role="status">
                Nothing answered on port {conn.port}.
              </p>
            )}
          </>
        )}
        <div className="ws-ask-actions">
          <button type="button" onClick={() => ref.current?.close()}>
            Cancel
          </button>
          <button type="submit" className="primary" disabled={ask === 'connect' && (!validPort || busy)}>
            {ask === 'new' ? 'Create' : busy ? 'Connecting…' : 'Connect'}
          </button>
        </div>
      </form>
    </dialog>
  )
}

export default function ProjectMenu({ open, name, root }: { open: boolean; name: string; root?: string }) {
  const caps = useCapabilities()
  const folder = useApp((a) => a.folder)
  const stored = useApp((a) => a.stored)
  const conn = useConnection()
  const recent = useRecent()
  const readOnly = useReadOnlyPref()
  const navigate = useNavigate()
  const [menuOpen, setMenuOpen] = useState(false)
  const [ask, setAsk] = useState<'new' | 'connect' | null>(null)
  // Set while an item opens the dialog, so the closing menu leaves the focus there.
  const asking = useRef(false)

  // This tab's project as the other tabs see it: its recent entry's id.
  const mine: TabProject | null = !open
    ? null
    : folder
      ? { entryId: folder.id, name: folder.name, kind: 'folder' }
      : stored
        ? { entryId: stored.id, name: stored.name, kind: stored.kind }
        : conn.state === 'connected'
          ? { entryId: `server:${conn.port}`, name: conn.name, kind: 'server' }
          : null
  const others = useOtherTabs(mine)
  const byId = new Map(recent.map((r) => [r.id, r]))
  const words = (p: TabProject) => kindWords(byId.get(p.entryId) ?? { id: p.entryId, kind: p.kind })

  const server = caps.data?.backend === 'server'
  const unwritten = open ? unwrittenCount(folder, stored) : 0
  const picker = folderPickerAvailable()
  const canStore = storageAvailable()
  const shown = open ? name : 'jazyk'

  const openAsk = (a: 'new' | 'connect') => {
    asking.current = true
    setAsk(a)
  }

  return (
    <>
      <DropdownMenu.Root
        open={menuOpen}
        onOpenChange={(v) => {
          setMenuOpen(v)
          // Asks the other tabs again; one that does not answer leaves the list.
          if (v) tabPresence()?.ping()
        }}
      >
        <DropdownMenu.Trigger asChild>
          <button className="ws-project" title={open ? (root ?? name) : 'open a project'}>
            <span className="ws-mark">J</span>
            <span className="ws-project-name">{shown}</span>
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
          <DropdownMenu.Content
            className="ws-menu"
            align="start"
            sideOffset={6}
            collisionPadding={8}
            onCloseAutoFocus={(e) => {
              if (!asking.current) return
              asking.current = false
              e.preventDefault()
            }}
          >
            {open && (
              <DropdownMenu.CheckboxItem className="ws-menu-item ws-menu-check ws-menu-tab" checked title="the project in this tab">
                <DropdownMenu.ItemIndicator className="ws-menu-tick" aria-hidden>
                  ✓
                </DropdownMenu.ItemIndicator>
                <span className="name">{name}</span>
                {mine && <span className="state">{words(mine)}</span>}
              </DropdownMenu.CheckboxItem>
            )}
            {others.map((t) => (
              <DropdownMenu.CheckboxItem
                key={t.tabId}
                className="ws-menu-item ws-menu-check ws-menu-tab"
                checked={false}
                onSelect={() => send('jazyk:open-recent', t.entryId)}
                title="open in another tab of this browser; choose it to open it here too"
              >
                <span className="name">{t.name}</span>
                <span className="state">{words(t)} · in another tab</span>
              </DropdownMenu.CheckboxItem>
            ))}
            {(open || others.length > 0) && <DropdownMenu.Separator className="ws-menu-sep" />}

            <Sub label="Open recent" disabled={recent.length === 0} title={recent.length === 0 ? 'nothing opened in this browser yet' : undefined}>
              {recent.map((r) => (
                <DropdownMenu.Item key={r.id} className="ws-menu-item" onSelect={() => send('jazyk:open-recent', r.id)}>
                  <span className="name">{r.name}</span>
                  <span className="state">{kindWords(r)}</span>
                </DropdownMenu.Item>
              ))}
              <DropdownMenu.Separator className="ws-menu-sep" />
              <Item onSelect={() => send('jazyk:clear-recent')} title="forget every entry, and what this browser stored for them">
                Clear recent…
              </Item>
            </Sub>
            <Item
              onSelect={() => send('jazyk:pick-folder', { mode: readOnly ? 'read' : 'write' })}
              disabled={!picker}
              title={!picker ? NO_PICKER : readOnly ? 'changes stay in this browser until you write them' : 'edits and builds write back into the folder'}
            >
              Open folder…
            </Item>
            <DropdownMenu.CheckboxItem
              className="ws-menu-item ws-menu-check"
              checked={readOnly}
              disabled={!picker}
              onCheckedChange={(v) => setReadOnlyPref(v === true)}
              // The choice is made in place; the menu stays open for Open folder.
              onSelect={(e) => e.preventDefault()}
              title={!picker ? NO_PICKER : 'keep every change in this browser until you choose to write it'}
            >
              <DropdownMenu.ItemIndicator className="ws-menu-tick" aria-hidden>
                  ✓
                </DropdownMenu.ItemIndicator>
              read-only
            </DropdownMenu.CheckboxItem>
            <Sub label="Open demo">
              {DEMOS.map((d) => (
                <DropdownMenu.Item key={d.id} className="ws-menu-item" onSelect={() => send('jazyk:open-demo', d.id)}>
                  <span className="name">{d.name}</span>
                  <span className="state">{d.blurb}</span>
                </DropdownMenu.Item>
              ))}
            </Sub>
            <Item onSelect={() => openAsk('new')} disabled={!canStore} title={canStore ? 'a project kept in this browser only' : NO_STORAGE}>
              New project in this browser…
            </Item>
            <Item onSelect={() => openAsk('connect')} title="reach a jazyk gui server running on this computer by its port">
              Connect to jazyk on this computer…
              {conn.state === 'connected' && <span className="state">connected · port {conn.port}</span>}
            </Item>

            {open && (
              <>
                <DropdownMenu.Separator className="ws-menu-sep" />
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
              </>
            )}
          </DropdownMenu.Content>
        </DropdownMenu.Portal>
      </DropdownMenu.Root>
      {ask && <AskDialog key={ask} ask={ask} onClose={() => setAsk(null)} />}
    </>
  )
}
