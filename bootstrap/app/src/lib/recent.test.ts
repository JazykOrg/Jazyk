import 'fake-indexeddb/auto'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { clear as idbClear, get as idbGet } from 'idb-keyval'
import type { RecentEntry } from './recent'

// The module reads `jazyk.recent` once at import, so each test imports it fresh
// after arranging local storage.
async function fresh() {
  vi.resetModules()
  return import('./recent')
}

const entry = (id: string, extra: Partial<RecentEntry> = {}) => ({
  id,
  kind: id.split(':')[0] as RecentEntry['kind'],
  name: id,
  ...extra,
})

// A directory handle as the browser stores it: the same object comes back, and
// `isSameEntry` compares identity. The browser serializes real handles specially;
// here structuredClone passes a fake through untouched.
class FakeHandle {
  async isSameEntry(other: unknown): Promise<boolean> {
    return other === this
  }
}
const handle = () => new FakeHandle() as unknown as FileSystemDirectoryHandle
const nativeClone = globalThis.structuredClone

beforeEach(async () => {
  localStorage.clear()
  await idbClear()
  globalThis.structuredClone = ((v: unknown, o?: StructuredSerializeOptions) =>
    v instanceof FakeHandle ? v : nativeClone(v, o)) as typeof structuredClone
})

afterEach(() => {
  globalThis.structuredClone = nativeClone
  vi.restoreAllMocks()
})

describe('touchRecent', () => {
  it('puts the entry first and caps the list at 12', async () => {
    const { touchRecent, readRecent } = await fresh()
    for (let i = 1; i <= 13; i++) touchRecent(entry(`demo:${i}`))
    const ids = readRecent().map((r) => r.id)
    expect(ids).toHaveLength(12)
    expect(ids[0]).toBe('demo:13')
    expect(ids).not.toContain('demo:1')
  })

  it('remembers a server on this computer with its port and root, one row per port', async () => {
    const { touchRecent, readRecent } = await fresh()
    touchRecent({ id: 'server:4680', kind: 'server', name: 'garden', port: 4680, root: '/home/me/garden' })
    touchRecent(entry('demo:shop'))
    touchRecent({ id: 'server:4680', kind: 'server', name: 'garden', port: 4680, root: '/home/me/garden' })
    expect(readRecent().map((r) => r.id)).toEqual(['server:4680', 'demo:shop'])
    expect(readRecent()[0]).toMatchObject({ kind: 'server', name: 'garden', port: 4680, root: '/home/me/garden' })
    // A server entry holds nothing in the browser: it can be dropped to make room.
    const { holdsData } = await import('./recent')
    expect(holdsData(readRecent()[0])).toBe(false)
  })

  it('moves a touched entry to the top and keeps its other fields', async () => {
    const { touchRecent, readRecent } = await fresh()
    touchRecent(entry('folder:a', { mode: 'write' }))
    touchRecent(entry('demo:b'))
    touchRecent(entry('folder:a'))
    expect(readRecent().map((r) => r.id)).toEqual(['folder:a', 'demo:b'])
    expect(readRecent()[0].mode).toBe('write')
  })

  it('never drops a browser project, an edited demo, or a folder with kept changes', async () => {
    const { touchRecent, readRecent } = await fresh()
    touchRecent(entry('browser:p'))
    touchRecent(entry('demo:edited', { edited: true }))
    touchRecent(entry('folder:kept', { kept: 2 }))
    touchRecent(entry('demo:plain'))
    for (let i = 1; i <= 9; i++) touchRecent(entry(`demo:${i}`))
    const ids = readRecent().map((r) => r.id)
    expect(ids).toHaveLength(12)
    expect(ids).toContain('browser:p')
    expect(ids).toContain('demo:edited')
    expect(ids).toContain('folder:kept')
    // The oldest entry that holds no data went instead.
    expect(ids).not.toContain('demo:plain')
  })

  it('keeps more than 12 when every entry holds data', async () => {
    const { touchRecent, readRecent } = await fresh()
    for (let i = 1; i <= 14; i++) touchRecent(entry(`browser:${i}`))
    expect(readRecent()).toHaveLength(14)
  })

  it('forgets the handle of a dropped folder', async () => {
    const { touchRecent, rememberFolder, readRecent } = await fresh()
    const id = await rememberFolder(handle())
    touchRecent(entry(id))
    for (let i = 1; i <= 12; i++) touchRecent(entry(`demo:${i}`))
    expect(readRecent().map((r) => r.id)).not.toContain(id)
    await vi.waitFor(async () => expect(await idbGet(id)).toBeUndefined())
  })
})

describe('updateRecent', () => {
  it('does not write when the patch changes nothing', async () => {
    const { touchRecent, updateRecent } = await fresh()
    touchRecent(entry('folder:a', { mode: 'read', kept: 0 }))
    const setItem = vi.spyOn(Storage.prototype, 'setItem')
    const changed = vi.fn()
    window.addEventListener('jazyk:recent-changed', changed)
    updateRecent('folder:a', { mode: 'read', kept: 0 })
    updateRecent('folder:missing', { mode: 'write' })
    window.removeEventListener('jazyk:recent-changed', changed)
    expect(setItem).not.toHaveBeenCalled()
    expect(changed).not.toHaveBeenCalled()
  })

  it('changes the fields in place and keeps the position', async () => {
    const { touchRecent, updateRecent, readRecent } = await fresh()
    touchRecent(entry('folder:a'))
    touchRecent(entry('demo:b'))
    updateRecent('folder:a', { kept: 3, mode: 'write' })
    expect(readRecent().map((r) => r.id)).toEqual(['demo:b', 'folder:a'])
    expect(readRecent()[1]).toMatchObject({ kept: 3, mode: 'write' })
    expect(JSON.parse(localStorage.getItem('jazyk.recent')!)[1]).toMatchObject({ kept: 3 })
  })
})

describe('load', () => {
  it('reads an empty list from a corrupt value', async () => {
    localStorage.setItem('jazyk.recent', '{not json')
    expect((await fresh()).readRecent()).toEqual([])
    localStorage.setItem('jazyk.recent', '{"id":"demo:x"}')
    expect((await fresh()).readRecent()).toEqual([])
  })

  it('reads a stored list back', async () => {
    localStorage.setItem('jazyk.recent', JSON.stringify([{ ...entry('demo:x'), openedAt: 1 }]))
    expect((await fresh()).readRecent().map((r) => r.id)).toEqual(['demo:x'])
  })
})

describe('rememberFolder', () => {
  it('answers the same id for the same folder and a new one for another', async () => {
    const { rememberFolder } = await fresh()
    const a = handle()
    const b = handle()
    const idA = await rememberFolder(a)
    expect(idA).toMatch(/^folder:/)
    expect(await rememberFolder(a)).toBe(idA)
    const idB = await rememberFolder(b)
    expect(idB).not.toBe(idA)
    expect(await rememberFolder(b)).toBe(idB)
  })

  it('hands the handle back by id', async () => {
    const { rememberFolder, folderHandle } = await fresh()
    const a = handle()
    const id = await rememberFolder(a)
    expect(await folderHandle(id)).toBe(a)
    expect(await folderHandle('folder:nope')).toBeUndefined()
  })
})

describe('readOnlyPref', () => {
  it('is writing until changed', async () => {
    const { readOnlyPref, setReadOnlyPref } = await fresh()
    expect(readOnlyPref()).toBe(false)
    setReadOnlyPref(true)
    expect(readOnlyPref()).toBe(true)
    setReadOnlyPref(false)
    expect(readOnlyPref()).toBe(false)
  })
})
