// The file tree a code editor has (docs/frontends/gui.md#docs): folders collapsible
// with a chevron and remembered open or closed per project, folders first then
// files by name, the open file highlighted and kept in view, the keyboard walking
// it the standard way, and the actions on the rows: New document and New folder on
// a folder, Rename and Delete on a file, the same as a right-click menu. A new
// document is typed in place under its folder; a delete asks for a second click on
// the row. The rows carry whatever badges the owner gives them.
import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from 'react'
import { Link, useNavigate } from 'react-router'
import * as ContextMenu from '@radix-ui/react-context-menu'
import { ancestors, buildTree, dirOf, readClosedFolders, visibleRows, writeClosedFolders, type TreeFile, type TreeRow } from '../lib/tree'
import './filetree.css'

export interface TreeActions {
  // The new document's path, as typed under its folder.
  create: (path: string) => Promise<void>
  rename: (from: string, to: string) => Promise<void>
  remove: (path: string) => Promise<void>
}

interface Props<T extends { path: string }> {
  // The head this tree stands under; its open state is remembered by it.
  head: string
  // What identifies the project when the browser remembers no open id (a server).
  projectKey: string
  items: T[]
  current: string
  hrefFor: (file: TreeFile<T>) => string
  // A file outside the glob: drawn dimmed.
  dim?: (file: TreeFile<T>) => boolean
  // The linkage tint (docs/frontends/gui.md#lenses).
  related?: Set<string>
  badges?: (file: TreeFile<T>) => ReactNode
  onHover?: (path: string, held: boolean) => void
  // A class for the name, so the owner's tests and styles find it.
  nameClass?: string
  actions?: TreeActions
  placeholder?: string
  // Folders start closed and the opened ones are remembered (the project tree);
  // otherwise folders start open and the closed ones are remembered.
  defaultClosed?: boolean
  // A row's kind: a class on the row and a glyph before the name.
  fileClass?: (file: TreeFile<T>) => string
  fileGlyph?: (file: TreeFile<T>) => ReactNode
  folderClass?: (dir: string) => string
  folderBadge?: (dir: string) => ReactNode
  // Which folders and files the actions (create, rename, delete) apply to.
  actionsFor?: (path: string) => boolean
}

function InlineInput({
  initial,
  placeholder,
  onSubmit,
  onCancel,
}: {
  initial?: string
  placeholder?: string
  onSubmit: (v: string) => void
  onCancel: () => void
}) {
  const [v, setV] = useState(initial ?? '')
  const done = useRef(false)
  return (
    <input
      className="ft-input mono"
      type="text"
      value={v}
      placeholder={placeholder}
      autoFocus
      aria-label={placeholder ?? 'name'}
      onFocus={(e) => {
        // The name is what gets renamed; the extension stays put.
        const dot = e.target.value.lastIndexOf('.')
        e.target.setSelectionRange(0, dot > 0 ? dot : e.target.value.length)
      }}
      onChange={(e) => setV(e.target.value)}
      onBlur={() => {
        if (!done.current) onCancel()
      }}
      onKeyDown={(e) => {
        if (e.key === 'Enter') {
          done.current = true
          onSubmit(v)
        } else if (e.key === 'Escape') {
          done.current = true
          onCancel()
        }
        e.stopPropagation()
      }}
    />
  )
}

type Edit = { mode: 'create' | 'folder'; dir: string } | { mode: 'rename'; path: string }

const CONFIRM_MS = 4000

export default function FileTree<T extends { path: string }>(props: Props<T>) {
  const { head: headProp, projectKey, items, current, hrefFor, dim, related, badges, onHover, nameClass, actions: allActions, placeholder, defaultClosed, fileClass, fileGlyph, folderClass, folderBadge, actionsFor } = props
  // With folders closed by default, the remembered set is the opened folders.
  const head = defaultClosed ? `${headProp}#open` : headProp
  const navigate = useNavigate()
  const [closed, setClosed] = useState<Set<string>>(() => readClosedFolders(head, projectKey))
  useEffect(() => setClosed(readClosedFolders(head, projectKey)), [head, projectKey])
  const actions = allActions
  const actionsAt = (path: string) => (allActions && (!actionsFor || actionsFor(path)) ? allActions : undefined)
  // Folders typed in place that hold no file yet.
  const [phantoms, setPhantoms] = useState<string[]>([])
  const [focused, setFocused] = useState<string | null>(null)
  const [edit, setEdit] = useState<Edit | null>(null)
  const [err, setErr] = useState<string | null>(null)
  const [confirm, setConfirm] = useState<string | null>(null)
  const confirmTimer = useRef<number | null>(null)
  const rowRefs = useRef(new Map<string, HTMLDivElement>())

  const nodes = useMemo(() => buildTree(items, phantoms), [items, phantoms])
  // A phantom folder that got its first file is a real one now.
  useEffect(() => {
    setPhantoms((ps) => {
      const keep = ps.filter((p) => !items.some((it) => it.path.startsWith(`${p}/`)))
      return keep.length === ps.length ? ps : keep
    })
  }, [items])
  const isOpen = useCallback((dir: string) => (defaultClosed ? closed.has(dir) : !closed.has(dir)), [closed, defaultClosed])
  const rows = useMemo(() => visibleRows(nodes, isOpen), [nodes, isOpen])

  const setOpen = useCallback(
    (dir: string, open: boolean) => {
      setClosed((c) => {
        // The set holds the opened folders when they start closed, else the closed.
        const add = defaultClosed ? open : !open
        if (add ? c.has(dir) : !c.has(dir)) return c
        const next = new Set(c)
        if (add) next.add(dir)
        else next.delete(dir)
        writeClosedFolders(head, projectKey, next)
        return next
      })
    },
    [head, projectKey, defaultClosed],
  )

  // The open file stays in view: its folders open, its row scrolls into view.
  useEffect(() => {
    if (!current) return
    const dirs = ancestors(current)
    setClosed((c) => {
      const shut = (d: string) => (defaultClosed ? !c.has(d) : c.has(d))
      if (!dirs.some(shut)) return c
      const next = new Set(c)
      for (const d of dirs) {
        if (defaultClosed) next.add(d)
        else next.delete(d)
      }
      writeClosedFolders(head, projectKey, next)
      return next
    })
    setFocused(current)
  }, [current, head, projectKey, defaultClosed])
  useEffect(() => {
    if (!current) return
    const el = rowRefs.current.get(current)
    el?.scrollIntoView({ block: 'nearest' })
  }, [current, rows])

  const cancel = useCallback(() => {
    setEdit(null)
    setErr(null)
  }, [])

  const arm = useCallback((path: string) => {
    setConfirm(path)
    if (confirmTimer.current !== null) window.clearTimeout(confirmTimer.current)
    confirmTimer.current = window.setTimeout(() => setConfirm(null), CONFIRM_MS)
  }, [])
  useEffect(
    () => () => {
      if (confirmTimer.current !== null) window.clearTimeout(confirmTimer.current)
    },
    [],
  )

  const run = useCallback(async (f: () => Promise<void>) => {
    try {
      await f()
      setErr(null)
      setEdit(null)
    } catch (e) {
      setErr((e as Error).message)
    }
  }, [])

  const startCreate = useCallback(
    (dir: string) => {
      if (!actions) return
      setOpen(dir, true)
      setEdit({ mode: 'create', dir })
      setErr(null)
    },
    [actions, setOpen],
  )
  const startFolder = useCallback(
    (dir: string) => {
      if (!actions) return
      setOpen(dir, true)
      setEdit({ mode: 'folder', dir })
      setErr(null)
    },
    [actions, setOpen],
  )
  const startRename = useCallback(
    (path: string) => {
      if (!actions) return
      setEdit({ mode: 'rename', path })
      setErr(null)
    },
    [actions],
  )
  const remove = useCallback(
    (path: string) => {
      if (!actions) return
      if (confirm !== path) {
        arm(path)
        return
      }
      setConfirm(null)
      void run(() => actions.remove(path))
    },
    [actions, arm, confirm, run],
  )

  const submitCreate = useCallback(
    (dir: string, name: string) => {
      const n = name.trim()
      if (!n || !actions) {
        cancel()
        return
      }
      const withExt = /\.[A-Za-z0-9]+$/.test(n) ? n : `${n}.md`
      void run(() => actions.create(dir ? `${dir}/${withExt}` : withExt))
    },
    [actions, cancel, run],
  )
  const submitFolder = useCallback(
    (dir: string, name: string) => {
      const n = name.trim().replace(/^\/+|\/+$/g, '')
      if (!n) {
        cancel()
        return
      }
      const path = dir ? `${dir}/${n}` : n
      setPhantoms((ps) => (ps.includes(path) ? ps : [...ps, path]))
      setOpen(path, true)
      // The folder exists once a document is in it: the next row is that document.
      setEdit({ mode: 'create', dir: path })
    },
    [cancel, setOpen],
  )
  const submitRename = useCallback(
    (from: string, name: string) => {
      const n = name.trim()
      if (!n || !actions) {
        cancel()
        return
      }
      const dir = dirOf(from)
      const to = n.includes('/') ? n : dir ? `${dir}/${n}` : n
      if (to === from) {
        cancel()
        return
      }
      void run(() => actions.rename(from, to))
    },
    [actions, cancel, run],
  )

  const focusRow = useCallback((path: string) => {
    setFocused(path)
    rowRefs.current.get(path)?.focus()
  }, [])

  const onKeyDown = (e: React.KeyboardEvent) => {
    if ((e.target as HTMLElement).tagName === 'INPUT') return
    const i = rows.findIndex((r) => r.node.path === focused)
    const row = i >= 0 ? rows[i] : null
    const go = (j: number) => {
      const r = rows[Math.max(0, Math.min(rows.length - 1, j))]
      if (r) focusRow(r.node.path)
    }
    switch (e.key) {
      case 'ArrowDown':
        go(i < 0 ? 0 : i + 1)
        break
      case 'ArrowUp':
        go(i < 0 ? 0 : i - 1)
        break
      case 'Home':
        go(0)
        break
      case 'End':
        go(rows.length - 1)
        break
      case 'ArrowRight':
        if (!row) return
        if (row.node.kind === 'dir') {
          if (!isOpen(row.node.path)) setOpen(row.node.path, true)
          else if (row.node.children.length > 0) go(i + 1)
        }
        break
      case 'ArrowLeft':
        if (!row) return
        if (row.node.kind === 'dir' && isOpen(row.node.path)) setOpen(row.node.path, false)
        else if (row.parent) focusRow(row.parent)
        break
      case 'Enter':
        if (!row) return
        if (row.node.kind === 'dir') setOpen(row.node.path, !isOpen(row.node.path))
        else navigate(hrefFor(row.node))
        break
      case 'F2':
        if (row?.node.kind === 'file') startRename(row.node.path)
        break
      case 'Delete':
      case 'Backspace':
        if (row?.node.kind === 'file') remove(row.node.path)
        break
      case 'Escape':
        setConfirm(null)
        cancel()
        break
      default:
        return
    }
    e.preventDefault()
  }

  const rowRef = (path: string) => (el: HTMLDivElement | null) => {
    if (el) rowRefs.current.set(path, el)
    else rowRefs.current.delete(path)
  }

  const editRow = (dir: string, depth: number) =>
    edit && edit.mode !== 'rename' && edit.dir === dir ? (
      <div key={`edit:${dir}`} className="ft-row ft-edit" style={{ paddingLeft: 8 + depth * 12 }}>
        <span className="ft-chev" aria-hidden="true">
          {edit.mode === 'folder' ? '▸' : ''}
        </span>
        <InlineInput
          placeholder={edit.mode === 'folder' ? 'folder name' : (placeholder ?? 'name.md')}
          onSubmit={(v) => (edit.mode === 'folder' ? submitFolder(dir, v) : submitCreate(dir, v))}
          onCancel={cancel}
        />
      </div>
    ) : null

  const renderRow = (r: TreeRow<T>) => {
    const n = r.node
    const pad = 8 + r.depth * 12
    const isFocused = focused === n.path
    if (n.kind === 'dir') {
      const open = isOpen(n.path)
      const row = (
        <div
          ref={rowRef(n.path)}
          role="treeitem"
          aria-level={r.depth + 1}
          aria-expanded={open}
          tabIndex={isFocused ? 0 : -1}
          className={`ft-row ft-dir${isFocused ? ' focused' : ''}${folderClass ? ` ${folderClass(n.path)}` : ''}`}
          data-path={n.path}
          style={{ paddingLeft: pad }}
          onFocus={() => setFocused(n.path)}
          onClick={() => {
            setFocused(n.path)
            setOpen(n.path, !open)
          }}
        >
          <span className="ft-chev" aria-hidden="true">
            {open ? '▾' : '▸'}
          </span>
          <span className="ft-name ft-dirname">{n.name}</span>
          {folderBadge?.(n.path)}
          {actionsAt(n.path) && (
            <span className="ft-actions" onClick={(e) => e.stopPropagation()}>
              <button className="ft-mini" title="New document" aria-label={`New document in ${n.name}`} onClick={() => startCreate(n.path)}>
                +
              </button>
              <button className="ft-mini" title="New folder" aria-label={`New folder in ${n.name}`} onClick={() => startFolder(n.path)}>
                +/
              </button>
            </span>
          )}
        </div>
      )
      return (
        <div key={n.path}>
          {actions ? (
            <ContextMenu.Root>
              <ContextMenu.Trigger asChild>{row}</ContextMenu.Trigger>
              <ContextMenu.Portal>
                <ContextMenu.Content className="ws-menu ft-menu" collisionPadding={8}>
                  <ContextMenu.Item className="ws-menu-item" onSelect={() => startCreate(n.path)}>
                    New document
                  </ContextMenu.Item>
                  <ContextMenu.Item className="ws-menu-item" onSelect={() => startFolder(n.path)}>
                    New folder
                  </ContextMenu.Item>
                </ContextMenu.Content>
              </ContextMenu.Portal>
            </ContextMenu.Root>
          ) : (
            row
          )}
          {open && editRow(n.path, r.depth + 1)}
        </div>
      )
    }
    const f = n
    const active = f.path === current
    const armed = confirm === f.path
    if (edit?.mode === 'rename' && edit.path === f.path) {
      return (
        <div key={f.path} className="ft-row ft-edit" style={{ paddingLeft: pad }}>
          <span className="ft-chev" aria-hidden="true" />
          <InlineInput initial={f.name} onSubmit={(v) => submitRename(f.path, v)} onCancel={cancel} />
        </div>
      )
    }
    const row = (
      <div
        ref={rowRef(f.path)}
        role="treeitem"
        aria-level={r.depth + 1}
        aria-selected={active}
        tabIndex={isFocused ? 0 : -1}
data-path={f.path}
                className={`ft-row ft-file${active ? ' active' : ''}${related?.has(f.path) ? ' related' : ''}${dim?.(f) ? ' dim' : ''}${isFocused ? ' focused' : ''}${fileClass ? ` ${fileClass(f)}` : ''}`}
        style={{ paddingLeft: pad }}
        onFocus={() => setFocused(f.path)}
        onClick={() => setFocused(f.path)}
      >
        <span className="ft-chev" aria-hidden="true">{fileGlyph?.(f)}</span>
        <Link to={hrefFor(f)} className="ft-link" tabIndex={-1}>
          <span className={`ft-name${nameClass ? ` ${nameClass}` : ''}`}>{f.name}</span>
          {badges?.(f)}
        </Link>
        {actionsAt(f.path) && (
          <span className="ft-actions">
            <button className="ft-mini" title="Rename" aria-label={`Rename ${f.name}`} onClick={() => startRename(f.path)}>
              ✎
            </button>
            <button
              className={`ft-mini${armed ? ' ft-confirm' : ''}`}
              title={armed ? 'click again to delete' : 'Delete'}
              aria-label={armed ? `Delete ${f.name}, click again to confirm` : `Delete ${f.name}`}
              onClick={() => remove(f.path)}
            >
              {armed ? 'delete?' : '✕'}
            </button>
          </span>
        )}
      </div>
    )
    return (
      <div key={f.path} onMouseEnter={() => onHover?.(f.path, true)} onMouseLeave={() => onHover?.(f.path, false)}>
        {actions ? (
          <ContextMenu.Root>
            <ContextMenu.Trigger asChild>{row}</ContextMenu.Trigger>
            <ContextMenu.Portal>
              <ContextMenu.Content className="ws-menu ft-menu" collisionPadding={8}>
                <ContextMenu.Item className="ws-menu-item" onSelect={() => startRename(f.path)}>
                  Rename
                </ContextMenu.Item>
                <ContextMenu.Item className="ws-menu-item" onSelect={() => arm(f.path)}>
                  Delete
                </ContextMenu.Item>
              </ContextMenu.Content>
            </ContextMenu.Portal>
          </ContextMenu.Root>
        ) : (
          row
        )}
      </div>
    )
  }

  return (
    <div className="ft" role="tree" onKeyDown={onKeyDown}>
      {editRow('', 0)}
      {rows.map(renderRow)}
      {err && <p className="error-inline ft-err">{err}</p>}
    </div>
  )
}
