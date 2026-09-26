// The projects a tab opens and where their changes go: a demo (fetched fresh, or its
// edited copy from browser storage), a project started in the browser, or a folder on
// the computer with the changes it has not taken kept in browser storage. Home and the
// top bar dispatch events; this listens, opens, and publishes the state the top bar
// renders. See docs/frontends/wasm.md#browser-storage and #a-folder-on-disk.
import {
  BufferSink,
  deleteFiles,
  deleteKept,
  describeStoreError,
  hasFiles,
  keptWriter,
  overlay,
  readFiles,
  readKept,
  requestPersistence,
  storageAvailable,
  StoredProject,
  writeFiles,
} from './browser-store'
import {
  describeFolderError,
  ensurePermission,
  folderIsEmpty,
  FolderMirror,
  pickFolder,
  readFolder,
  writeTree,
  type DirHandle,
} from './folder'
import {
  folderHandle,
  readRecent,
  rememberFolder,
  removeRecent,
  setRecentMode,
  touchRecent,
  updateRecent,
  type FolderMode,
} from './recent'
import { useApp } from './store'
import type { WasmBackend, WasmProject } from './wasm-client'

export interface LocalDeps {
  backend: WasmBackend
  // Seeds the backend with the project and moves to the documents.
  opened: (p: WasmProject, stay?: boolean) => Promise<void>
  // Nothing is open anymore: back to Home.
  closed: () => void
  // The descriptor changed: the persistence moved.
  refresh: () => void
  fetchDemo: (id: string) => Promise<{ files: Record<string, string>; name: string }>
  demoName: (id: string) => string
}

type Current =
  | { kind: 'folder'; id: string; name: string; mirror: FolderMirror }
  // `store` is null when the browser has no storage: the changes stay in the tab.
  | { kind: 'stored'; id: string; name: string; demo: string | null; store: StoredProject | null }

// The two files a new project starts with (docs/frontends/wasm.md#a-project-in-the-browser).
export function starterFiles(name: string): Record<string, string> {
  return {
    'jazyk.toml': [
      '# A directory with jazyk.toml is a jazyk project. Globs resolve relative to it.',
      '',
      '[docs]',
      'glob = ["docs/**/*.md"]',
      '',
      '[roots]',
      'files = ["docs/README.md"]',
      '',
    ].join('\n'),
    'docs/README.md': `# ${name}\n\nDescribe what this project is in plain sentences: what it is for, who uses it, and what it must do.\n`,
  }
}

const slug = (name: string) =>
  name
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-+|-+$/g, '') || 'project'

export function installLocalProjects(d: LocalDeps) {
  let current: Current | null = null

  const publish = () => {
    const s = useApp.getState()
    const c = current
    if (c?.kind === 'folder') {
      const m = c.mirror
      s.setFolder({ name: c.name, mode: m.currentMode, held: m.keptCount, error: m.error, writing: m.busy, storeError: m.storeError })
      s.setStored(null)
      updateRecent(c.id, { kept: m.keptCount })
      return
    }
    s.setFolder(null)
    if (c?.kind === 'stored') {
      const w = c.store?.writer
      s.setStored({
        id: c.id,
        kind: c.demo ? 'demo' : 'browser',
        name: c.name,
        edited: c.store?.edited ?? false,
        error: w?.error ?? null,
        unsaved: w?.error ? w.pendingCount : 0,
        available: !!c.store,
      })
      return
    }
    s.setStored(null)
  }

  const fail = (what: string, err: unknown) => {
    console.warn(`[jazyk ${what}]`, err)
    window.alert(err instanceof Error ? err.message : String(err))
  }

  // What the project being left still has pending lands before the next one is read,
  // so reopening the same project reads its latest state.
  const leave = async () => {
    const c = current
    if (c?.kind === 'stored') await c.store?.writer.flush()
    if (c?.kind === 'folder') await c.mirror.persist()
  }

  const openStored = async (c: Extract<Current, { kind: 'stored' }>, files: WasmProject['files'], stay = false) => {
    if (c.store) c.store.writer.onState = publish
    current = c
    publish()
    await d.opened({ files, name: c.name, demo: !!c.demo, sink: c.store, persistence: c.store ? 'browser' : 'memory' }, stay)
  }

  // A demo opens its edited copy when browser storage has one, and the fixture
  // otherwise; the fixture's first mutation stores the copy
  // (docs/frontends/wasm.md#an-edited-demo).
  const openDemo = async (demo: string, stay = false) => {
    await leave()
    const id = `demo:${demo}`
    const name = d.demoName(demo)
    if (await hasFiles(id)) {
      const files = await readFiles(id)
      touchRecent({ id, kind: 'demo', name, edited: true })
      return openStored({ kind: 'stored', id, name, demo, store: new StoredProject(id) }, files, stay)
    }
    const p = await d.fetchDemo(demo)
    const store = storageAvailable() ? new StoredProject(id, p.files) : null
    if (store)
      store.onFirstChange = () => {
        updateRecent(id, { edited: true })
        publish()
      }
    touchRecent({ id, kind: 'demo', name, edited: false })
    await openStored({ kind: 'stored', id, name, demo, store }, p.files, stay)
  }

  const openBrowserProject = async (id: string, name: string) => {
    await leave()
    const files = await readFiles(id)
    if (!('jazyk.toml' in files)) {
      window.alert(`${name} is no longer in this browser's storage. Clearing the site's data deletes it.`)
      removeRecent(id)
      return
    }
    touchRecent({ id, kind: 'browser', name })
    await openStored({ kind: 'stored', id, name, demo: null, store: new StoredProject(id) }, files)
  }

  const newProject = async (raw: string) => {
    const name = raw.trim() || 'Untitled project'
    if (!storageAvailable()) {
      window.alert('This browser offers no storage to this page (a private window may not), so a project started here would not survive a reload.')
      return
    }
    const id = `browser:${crypto.randomUUID()}`
    try {
      await writeFiles(id, starterFiles(name))
    } catch (e) {
      window.alert(`Could not create ${name} in this browser: ${describeStoreError(e)}.`)
      return
    }
    requestPersistence()
    touchRecent({ id, kind: 'browser', name })
    await openBrowserProject(id, name)
  }

  // A folder opens with the changes it has not taken laid over it, so the tab shows
  // the work as it was left (docs/frontends/wasm.md#when-a-folder-cannot-be-written).
  const openFolder = async (id: string, handle: DirHandle, mode: FolderMode, stay = false) => {
    await leave()
    const disk = await readFolder(handle)
    const kept = await readKept(id).catch(() => ({ changes: [], seq: 0 }))
    const files = overlay(disk, kept.changes)
    if (!('jazyk.toml' in files)) {
      window.alert(`${handle.name} has no jazyk.toml at its top; pick a project directory.`)
      return
    }
    const mirror = new FolderMirror(handle, '/project', mode, keptWriter(id), kept)
    mirror.onChange = publish
    current = { kind: 'folder', id, name: handle.name, mirror }
    touchRecent({ id, kind: 'folder', name: handle.name, mode, kept: kept.changes.length })
    publish()
    await d.opened({ files, name: handle.name, demo: false, sink: mirror, persistence: 'folder' }, stay)
  }

  // The open project's stored data is gone: nothing is open anymore.
  const closeCurrent = () => {
    const c = current
    if (c?.kind === 'stored') c.store?.writer.close()
    if (c?.kind === 'folder') c.mirror.discard()
    current = null
    d.backend.close()
    publish()
    d.closed()
  }

  const on = (name: string, fn: (detail: unknown) => Promise<void> | void, what = 'project') =>
    window.addEventListener(name, (e) => {
      Promise.resolve(fn((e as CustomEvent).detail)).catch((err) => fail(what, err))
    })

  on('jazyk:open-demo', (detail) => openDemo(String(detail ?? '')), 'demo')

  on('jazyk:new-project', (detail) => newProject(String((detail as { name?: string })?.name ?? '')))

  // `detail.mode` is the read-only choice beside the button.
  on(
    'jazyk:pick-folder',
    async (detail) => {
      const mode: FolderMode = (detail as { mode?: string })?.mode === 'write' ? 'write' : 'read'
      let handle: DirHandle
      try {
        handle = await pickFolder(mode)
      } catch (err) {
        // The picker was dismissed, or refused: nothing to do.
        console.info('[jazyk folder]', err)
        return
      }
      await openFolder(await rememberFolder(handle), handle, mode)
    },
    'folder',
  )

  // A row of the recent list. The click is the gesture the permission prompt needs.
  on('jazyk:open-recent', async (detail) => {
    const id = String(detail ?? '')
    const entry = readRecent().find((r) => r.id === id)
    if (!entry) return
    if (entry.kind === 'demo') return openDemo(id.slice('demo:'.length))
    if (entry.kind === 'browser') return openBrowserProject(id, entry.name)
    const handle = (await folderHandle(id)) as DirHandle | undefined
    if (!handle) {
      window.alert(`${entry.name} is no longer remembered by this browser; open it with Open folder.`)
      return
    }
    const mode = entry.mode ?? 'read'
    if (!(await ensurePermission(handle, mode))) return
    await openFolder(id, handle, mode)
  })

  // Forgetting an entry deletes what the browser stored for it, so it asks first.
  on('jazyk:forget-recent', async (detail) => {
    const id = String(detail ?? '')
    const r = readRecent().find((x) => x.id === id)
    if (!r) return
    const kept = r.kept ?? 0
    const question =
      r.kind === 'browser'
        ? `Delete ${r.name}? It is stored only in this browser, so this cannot be undone.`
        : r.kind === 'demo' && r.edited
          ? `Forget ${r.name} and delete your edits to it?`
          : r.kind === 'folder' && kept > 0
            ? `Forget ${r.name}? The ${kept} change${kept === 1 ? '' : 's'} kept in this browser for it ${kept === 1 ? 'is' : 'are'} deleted; the folder itself is not touched.`
            : null
    if (question && !window.confirm(question)) return
    if (question && current?.id === id) closeCurrent()
    if (r.kind === 'folder') await deleteKept(id)
    else await deleteFiles(id).catch(() => {})
    removeRecent(id)
  })

  // Reset to the original: the demo's copy is deleted; an open demo reopens from
  // its fixture.
  on('jazyk:reset-demo', async (detail) => {
    const id = String(detail ?? '')
    const demo = id.slice('demo:'.length)
    const name = d.demoName(demo)
    if (!window.confirm(`Reset ${name} to the original? Your edits to it in this browser are deleted.`)) return
    const open = current?.kind === 'stored' && current.id === id
    if (open && current?.kind === 'stored') current.store?.writer.close()
    await deleteFiles(id)
    updateRecent(id, { edited: false })
    if (open) {
      current = null
      await openDemo(demo, true)
    }
  })

  // The top bar's switch and the editor's prompt. Writing asks for write access,
  // then lands everything held.
  on(
    'jazyk:folder-mode',
    async (detail) => {
      const mode: FolderMode = detail === 'write' ? 'write' : 'read'
      if (current?.kind !== 'folder') return
      const { id, mirror } = current
      if (mode === 'write') {
        if (!(await ensurePermission(mirror.root, 'write'))) return
        await mirror.startWriting()
      } else {
        mirror.readOnly()
      }
      setRecentMode(id, mode)
      publish()
    },
    'folder',
  )

  // Retry, and writing the changes kept from an earlier visit: the browser is asked
  // for write access again if it needs to be.
  on(
    'jazyk:folder-retry',
    async () => {
      if (current?.kind !== 'folder') return
      const { mirror } = current
      if (!(await ensurePermission(mirror.root, 'write'))) {
        mirror.error = 'the browser did not grant write access'
        publish()
        return
      }
      await mirror.writeKept()
      publish()
    },
    'folder',
  )

  // Discard the kept changes and reopen the folder as it is on disk.
  on(
    'jazyk:folder-discard',
    async () => {
      if (current?.kind !== 'folder') return
      const { id, name, mirror } = current
      const n = mirror.keptCount
      if (!window.confirm(`Discard the ${n} change${n === 1 ? '' : 's'} kept in this browser and reopen ${name} as it is on disk?`)) return
      mirror.discard()
      await deleteKept(id)
      updateRecent(id, { kept: 0 })
      await openFolder(id, mirror.root as DirHandle, mirror.currentMode, true)
    },
    'folder',
  )

  // A stored project's refused writes, tried again.
  on('jazyk:store-retry', async () => {
    if (current?.kind !== 'stored') return
    await current.store?.writer.flush()
    publish()
  })

  // Save to a folder…: every file of the tree into the picked folder; from then on
  // the project is that folder, in writing mode
  // (docs/frontends/wasm.md#a-project-in-the-browser).
  on(
    'jazyk:save-to-folder',
    async () => {
      if (current?.kind !== 'stored') return
      const from = current
      let handle: DirHandle
      try {
        handle = await pickFolder('write')
      } catch (err) {
        console.info('[jazyk folder]', err)
        return
      }
      if (!(await folderIsEmpty(handle))) {
        const ok = window.confirm(`${handle.name} is not empty. Write ${from.name} into it anyway? Files with the same names are replaced.`)
        if (!ok) return
      }
      if (!(await ensurePermission(handle, 'write'))) return
      // Mutations that arrive while the tree is written wait here, then follow it.
      const buffer = new BufferSink()
      const back = { sink: from.store, persistence: from.store ? ('browser' as const) : ('memory' as const), demo: !!from.demo, name: from.name }
      d.backend.moveTo({ ...back, sink: buffer })
      try {
        await from.store?.writer.flush()
        await writeTree(handle, await d.backend.dump())
      } catch (e) {
        d.backend.moveTo(back)
        if (from.store) buffer.replay(from.store)
        window.alert(`Could not save ${from.name} to ${handle.name}: ${describeFolderError(e)}. The project stays in this browser.`)
        return
      }
      const id = await rememberFolder(handle)
      await deleteKept(id)
      const mirror = new FolderMirror(handle, '/project', 'write', keptWriter(id))
      mirror.onChange = publish
      buffer.replay(mirror)
      d.backend.moveTo({ sink: mirror, persistence: 'folder', demo: false, name: handle.name })
      from.store?.writer.close()
      await deleteFiles(from.id).catch(() => {})
      if (from.demo) updateRecent(from.id, { edited: false })
      else removeRecent(from.id)
      touchRecent({ id, kind: 'folder', name: handle.name, mode: 'write', kept: 0 })
      current = { kind: 'folder', id, name: handle.name, mirror }
      publish()
      d.refresh()
    },
    'folder',
  )

  // Download: the whole tree as one .zip named after the project.
  on('jazyk:download', async () => {
    if (!current) return
    const top = slug(current.name)
    const files = await d.backend.dump()
    const { zipSync } = await import('fflate')
    const zip = zipSync(Object.fromEntries(Object.entries(files).map(([p, b]) => [`${top}/${p}`, b])))
    const url = URL.createObjectURL(new Blob([zip as BlobPart], { type: 'application/zip' }))
    const a = document.createElement('a')
    a.href = url
    a.download = `${top}.zip`
    a.click()
    window.setTimeout(() => URL.revokeObjectURL(url), 10_000)
  })

  // Leaving the page stores what is pending at once.
  const persistNow = () => {
    if (current?.kind === 'stored') void current.store?.writer.flush()
    if (current?.kind === 'folder') void current.mirror.persist()
  }
  document.addEventListener('visibilitychange', () => {
    if (document.visibilityState === 'hidden') persistNow()
  })
  window.addEventListener('pagehide', persistNow)

  // Changes the folder has not taken are kept in browser storage, and closing the tab
  // still asks first; so does a project whose storage refused a write.
  window.addEventListener('beforeunload', (e) => {
    if (current?.kind === 'folder' && current.mirror.keptCount > 0) e.preventDefault()
    if (current?.kind === 'stored' && current.store?.writer.error) e.preventDefault()
  })
}
