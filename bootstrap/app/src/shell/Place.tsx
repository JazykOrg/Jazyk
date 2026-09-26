// Where the open project's changes go, in the top bar (docs/frontends/gui.md#home):
// a folder's mode and what it has not taken, a demo's edits, a project kept in this
// browser. A failure says so plainly with the way to recover beside it
// (docs/frontends/wasm.md#browser-storage).
import { useEffect, useRef, useState } from 'react'
import { folderPickerAvailable } from '../lib/folder'
import { useApp, type FolderState, type StoredState } from '../lib/store'

const send = (name: string, detail?: unknown) => window.dispatchEvent(new CustomEvent(name, { detail }))
const changes = (n: number) => `${n} change${n === 1 ? '' : 's'}`

function Folder({ f }: { f: FolderState }) {
  const kept = f.held
  const notStored = f.storeError ? ` (this browser could not store them either: ${f.storeError})` : ''
  if (f.writing && kept > 0)
    return (
      <span className="ws-folder held" role="status">
        Writing {changes(kept)} to {f.name}…
      </span>
    )
  if (f.mode === 'write' && kept > 0 && f.error)
    return (
      <span className="ws-folder failed" role="status" title={`${f.error}${notStored}`}>
        Could not write to {f.name}: {changes(kept)} kept in this browser
        <button onClick={() => send('jazyk:folder-retry')} title="write the kept changes again, in order">
          Retry
        </button>
      </span>
    )
  if (f.mode === 'write' && kept > 0)
    return (
      <span className="ws-folder held" title={`${f.name} has not taken these yet${notStored}`}>
        {changes(kept)} kept in this browser
        <button onClick={() => send('jazyk:folder-retry')} title={`write them to ${f.name}, in order`}>
          Write to folder
        </button>
        <button onClick={() => send('jazyk:folder-discard')} title={`forget them and reopen ${f.name} as it is on disk`}>
          Discard
        </button>
      </span>
    )
  if (f.mode === 'write')
    return (
      <span className="ws-folder" title={`${f.name}: every change is written to the folder`}>
        writing
        <button onClick={() => send('jazyk:folder-mode', 'read')} title="keep changes in this browser instead of writing them">
          read-only
        </button>
      </span>
    )
  return (
    <span className={`ws-folder read${kept > 0 ? ' held' : ''}`} title={`${f.name}: changes stay in this browser until written${notStored}`}>
      read-only{kept > 0 ? ` · ${kept} unwritten` : ''}
      <button onClick={() => send('jazyk:folder-mode', 'write')} title="write what this browser keeps to the folder, and keep writing">
        {kept > 0 ? 'write to disk' : 'writing'}
      </button>
      {kept > 0 && (
        <button onClick={() => send('jazyk:folder-discard')} title={`forget them and reopen ${f.name} as it is on disk`}>
          discard
        </button>
      )}
    </span>
  )
}

// A small menu under the chip: what the place means and what can be done with it.
function Menu({ label, className, children }: { label: string; className?: string; children: (close: () => void) => React.ReactNode }) {
  const [open, setOpen] = useState(false)
  const [at, setAt] = useState<{ top: number; right: number }>({ top: 44, right: 12 })
  const ref = useRef<HTMLSpanElement>(null)
  useEffect(() => {
    if (!open) return
    const away = (e: MouseEvent | KeyboardEvent) => {
      if (e instanceof KeyboardEvent ? e.key === 'Escape' : !ref.current?.contains(e.target as Node)) setOpen(false)
    }
    document.addEventListener('mousedown', away)
    document.addEventListener('keydown', away)
    return () => {
      document.removeEventListener('mousedown', away)
      document.removeEventListener('keydown', away)
    }
  }, [open])
  return (
    <span className={`ws-place${className ? ` ${className}` : ''}`} ref={ref}>
      <button
        className="ws-place-chip"
        aria-expanded={open}
        onClick={(e) => {
          const r = e.currentTarget.getBoundingClientRect()
          setAt({ top: r.bottom + 6, right: Math.max(8, window.innerWidth - r.right) })
          setOpen((v) => !v)
        }}
      >
        {label} <span aria-hidden>▾</span>
      </button>
      {open && (
        <div className="ws-place-menu" style={{ top: at.top, right: at.right }}>
          {children(() => setOpen(false))}
        </div>
      )}
    </span>
  )
}

function Stored({ s }: { s: StoredState }) {
  if (s.error)
    return (
      <span className="ws-folder failed" role="status" title={s.error}>
        Could not save to this browser: {s.unsaved > 0 ? `${changes(s.unsaved)} in this tab only` : s.error}
        <button onClick={() => send('jazyk:store-retry')} title="store them again">
          Retry
        </button>
        <button onClick={() => send('jazyk:download')} title="download the project as a .zip">
          Download
        </button>
      </span>
    )
  const picker = folderPickerAvailable()
  const label = !s.available ? 'in this tab only' : s.kind === 'browser' ? 'in this browser' : s.edited ? 'demo · your edits' : 'demo'
  const what = !s.available
    ? `This browser offers no storage to this page: a reload starts ${s.name} again. Download it to keep your changes.`
    : s.kind === 'browser'
      ? `${s.name} is stored in this browser only. Clearing this site's data deletes it; save it to a folder or download it to keep a copy.`
      : s.edited
        ? `Your edits to ${s.name} are kept in this browser. Reset it to go back to the demo as shipped.`
        : `Your first edit keeps a copy of ${s.name} in this browser, so a reload does not lose it.`
  return (
    <Menu label={label} className={s.kind}>
      {(close) => (
        <>
          <p>{what}</p>
          <div className="ws-place-actions">
            {s.kind === 'demo' && s.edited && (
              <button
                onClick={() => {
                  close()
                  send('jazyk:reset-demo', s.id)
                }}
              >
                Reset to the original
              </button>
            )}
            {picker && s.available && (
              <button
                onClick={() => {
                  close()
                  send('jazyk:save-to-folder')
                }}
                title="write every file into a folder on this computer; from then on the project is that folder"
              >
                Save to a folder…
              </button>
            )}
            <button
              onClick={() => {
                close()
                send('jazyk:download')
              }}
              title="the whole project as one .zip"
            >
              Download .zip
            </button>
          </div>
        </>
      )}
    </Menu>
  )
}

export default function Place() {
  const folder = useApp((a) => a.folder)
  const stored = useApp((a) => a.stored)
  if (folder) return <Folder f={folder} />
  if (stored) return <Stored s={stored} />
  return null
}
