import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { Change } from './browser-store'
import { FolderMirror, describeFolderError, folderIsEmpty, readFolder, type DirHandle } from './folder'

const bytes = (s: string) => new TextEncoder().encode(s)
const str = (b: Uint8Array) => new TextDecoder().decode(b)

// A folder in memory with the part of the File System Access API the mirror uses.
// `failOn` makes a write or remove of a path throw; `hold` makes a write wait.
class FakeDir {
  readonly kind = 'directory' as const
  dirs = new Map<string, FakeDir>()
  files = new Map<string, Uint8Array>()
  protected readonly root: FakeRoot
  constructor(
    readonly name: string,
    root: FakeRoot | null,
    private readonly path: string,
  ) {
    this.root = root ?? (this as unknown as FakeRoot)
  }

  async *entries(): AsyncIterableIterator<[string, FileSystemHandle]> {
    for (const [n, d] of this.dirs) yield [n, d as unknown as FileSystemHandle]
    for (const [n, b] of this.files) {
      yield [n, { kind: 'file', name: n, getFile: async () => ({ arrayBuffer: async () => b.slice().buffer }) } as unknown as FileSystemHandle]
    }
  }

  async getDirectoryHandle(n: string, opts?: { create?: boolean }) {
    let d = this.dirs.get(n)
    if (!d) {
      if (!opts?.create) throw new DOMException(n, 'NotFoundError')
      d = new FakeDir(n, this.root, this.at(n))
      this.dirs.set(n, d)
    }
    return d
  }

  async getFileHandle(n: string, opts?: { create?: boolean }) {
    const path = this.at(n)
    if (!this.files.has(n) && !opts?.create) throw new DOMException(n, 'NotFoundError')
    return {
      kind: 'file' as const,
      createWritable: async () => {
        let buf = new Uint8Array()
        return {
          write: async (b: ArrayBuffer) => {
            buf = new Uint8Array(b)
          },
          close: async () => {
            await this.root.hold?.(path)
            this.root.check(path)
            this.files.set(n, buf)
            this.root.log.push(`write ${path} ${str(buf)}`)
          },
        }
      },
    }
  }

  async removeEntry(n: string, opts?: { recursive?: boolean }) {
    const path = this.at(n)
    this.root.check(path)
    if (this.files.delete(n) || (opts?.recursive && this.dirs.delete(n))) {
      this.root.log.push(`remove ${path}`)
      return
    }
    throw new DOMException(n, 'NotFoundError')
  }

  private at(n: string) {
    return this.path ? `${this.path}/${n}` : n
  }
}

class FakeRoot extends FakeDir {
  log: string[] = []
  failOn = new Set<string>()
  hold?: (path: string) => Promise<void>
  constructor() {
    super('', null, '')
  }
  check(path: string) {
    if (this.failOn.has(path)) throw new DOMException(path, 'NotAllowedError')
  }
  // Every file, as { path: text }.
  tree(): Record<string, string> {
    const out: Record<string, string> = {}
    const walk = (d: FakeDir, prefix: string) => {
      for (const [n, b] of d.files) out[prefix + n] = str(b)
      for (const [n, sub] of d.dirs) walk(sub, `${prefix}${n}/`)
    }
    walk(this, '')
    return out
  }
}

const handle = (r: FakeRoot) => r as unknown as DirHandle

// A promise resolved from outside.
function deferred() {
  let resolve!: () => void
  const promise = new Promise<void>((r) => (resolve = r))
  return { promise, resolve }
}

let root: FakeRoot
beforeEach(() => {
  root = new FakeRoot()
  vi.spyOn(console, 'error').mockImplementation(() => {})
})
afterEach(() => {
  vi.restoreAllMocks()
})

describe('FolderMirror', () => {
  it('lands every mutation in order when writing', async () => {
    const m = new FolderMirror(handle(root), '/project', 'write')
    m.write('/project/a.md', bytes('a'))
    m.write('/project/d/b.md', bytes('b'))
    m.remove('/project/a.md', false)
    m.remove('/project/d', true)
    m.write('/project/d/c.md', bytes('c'))
    m.write('/elsewhere/x.md', bytes('x'))
    await m.flush()
    expect(root.log).toEqual(['write a.md a', 'write d/b.md b', 'remove a.md', 'remove d', 'write d/c.md c'])
    expect(root.tree()).toEqual({ 'd/c.md': 'c' })
    expect(m.keptCount).toBe(0)
    expect(m.error).toBeNull()
  })

  it('a remove of something already gone has landed', async () => {
    const m = new FolderMirror(handle(root), '/project', 'write')
    m.remove('/project/nope.md', false)
    m.remove('/project/nodir/x.md', false)
    await m.flush()
    expect(m.keptCount).toBe(0)
    expect(m.error).toBeNull()
  })

  it('keeps every change in read mode and touches nothing', async () => {
    const changes: string[] = []
    const m = new FolderMirror(handle(root), '/project', 'read')
    m.onChange = () => changes.push(`${m.keptCount}`)
    m.write('/project/a.md', bytes('a'))
    m.write('/project/b.md', bytes('b'))
    m.remove('/project/a.md', false)
    await m.flush()
    expect(root.log).toEqual([])
    expect(m.keptCount).toBe(2)
    expect(m.currentMode).toBe('read')
    expect(changes).toEqual(['1', '2', '2'])
  })

  it('keeps a failed write and every later change, in order', async () => {
    const m = new FolderMirror(handle(root), '/project', 'write')
    root.failOn.add('b.md')
    m.write('/project/a.md', bytes('a'))
    m.write('/project/b.md', bytes('b'))
    m.write('/project/c.md', bytes('c'))
    m.remove('/project/a.md', false)
    await m.flush()
    expect(root.log).toEqual(['write a.md a'])
    expect(m.keptCount).toBe(3)
    expect(m.error).toBe('the browser no longer grants write access')
    // Then, with the folder writable again, they land in the order they were kept.
    root.failOn.clear()
    expect(await m.writeKept()).toBe(true)
    expect(root.log).toEqual(['write a.md a', 'write b.md b', 'write c.md c', 'remove a.md'])
    expect(m.keptCount).toBe(0)
    expect(m.error).toBeNull()
  })

  it('kept changes are the latest per path in the order the paths were last touched', async () => {
    const m = new FolderMirror(handle(root), '/project', 'read')
    m.write('/project/a.md', bytes('a1'))
    m.write('/project/b.md', bytes('b'))
    m.write('/project/a.md', bytes('a2'))
    expect(m.keptCount).toBe(2)
    expect(await m.startWriting()).toBe(true)
    expect(root.log).toEqual(['write b.md b', 'write a.md a2'])
  })

  it('landKept stops at the first change that fails again', async () => {
    const kept: [string, Change][] = [
      ['a.md', { op: 'write', bytes: bytes('a') }],
      ['b.md', { op: 'write', bytes: bytes('b') }],
      ['c.md', { op: 'write', bytes: bytes('c') }],
    ]
    const m = new FolderMirror(handle(root), '/project', 'write', null, { changes: kept, seq: 3 })
    expect(m.keptCount).toBe(3)
    root.failOn.add('b.md')
    expect(await m.writeKept()).toBe(false)
    expect(root.log).toEqual(['write a.md a'])
    expect(m.keptCount).toBe(2)
    expect(m.error).toBe('the browser no longer grants write access')
    expect(m.busy).toBe(false)
    root.failOn.clear()
    expect(await m.writeKept()).toBe(true)
    expect(root.tree()).toEqual({ 'a.md': 'a', 'b.md': 'b', 'c.md': 'c' })
  })

  it('one writeKept runs at a time', async () => {
    const m = new FolderMirror(handle(root), '/project', 'write', null, {
      changes: [['a.md', { op: 'write', bytes: bytes('a') }]],
      seq: 1,
    })
    const gate = deferred()
    root.hold = () => gate.promise
    const first = m.writeKept()
    expect(m.busy).toBe(true)
    expect(m.writeKept()).toBe(first)
    gate.resolve()
    await first
    expect(m.busy).toBe(false)
  })

  it('a write to a path made while its kept change lands stays kept', async () => {
    const m = new FolderMirror(handle(root), '/project', 'write', null, {
      changes: [['a.md', { op: 'write', bytes: bytes('old') }]],
      seq: 1,
    })
    const gate = deferred()
    let landing = false
    root.hold = async () => {
      landing = true
      await gate.promise
    }
    const done = m.writeKept()
    await vi.waitFor(() => expect(landing).toBe(true))
    // The old bytes are on their way; a newer write arrives and is kept.
    m.write('/project/a.md', bytes('new'))
    expect(m.keptCount).toBe(1)
    root.hold = undefined
    gate.resolve()
    expect(await done).toBe(true)
    expect(root.log).toEqual(['write a.md old', 'write a.md new'])
    expect(root.tree()).toEqual({ 'a.md': 'new' })
    expect(m.keptCount).toBe(0)
  })

  it('discard forgets the kept changes', () => {
    const m = new FolderMirror(handle(root), '/project', 'read')
    m.write('/project/a.md', bytes('a'))
    m.discard()
    expect(m.keptCount).toBe(0)
    expect(m.error).toBeNull()
  })
})

describe('describeFolderError', () => {
  it.each([
    ['NotAllowedError', 'the browser no longer grants write access'],
    ['SecurityError', 'the browser no longer grants write access'],
    ['NotFoundError', 'the folder is no longer where it was'],
    ['NoModificationAllowedError', 'the file is locked or was changed underneath'],
    ['InvalidStateError', 'the file is locked or was changed underneath'],
  ])('%s', (name, words) => {
    expect(describeFolderError(new DOMException('detail', name))).toBe(words)
  })

  it('falls back to the message, the name, or the value', () => {
    expect(describeFolderError(new Error('disk gone'))).toBe('disk gone')
    expect(describeFolderError(new DOMException('', 'AbortError'))).toBe('AbortError')
    expect(describeFolderError('nope')).toBe('nope')
  })
})

describe('readFolder', () => {
  it('reads every file by slash path and skips what no project needs', async () => {
    const d = await root.getDirectoryHandle('docs', { create: true })
    d.files.set('a.md', bytes('a'))
    root.files.set('README.md', bytes('r'))
    root.files.set('.DS_Store', bytes('x'))
    const git = await root.getDirectoryHandle('.git', { create: true })
    git.files.set('HEAD', bytes('ref'))
    const out = await readFolder(handle(root))
    expect(Object.fromEntries(Object.entries(out).map(([k, v]) => [k, str(v)]))).toEqual({ 'README.md': 'r', 'docs/a.md': 'a' })
  })

  it('an empty folder is one with nothing but skipped names', async () => {
    expect(await folderIsEmpty(handle(root))).toBe(true)
    root.files.set('.DS_Store', bytes('x'))
    expect(await folderIsEmpty(handle(root))).toBe(true)
    root.files.set('a.md', bytes('a'))
    expect(await folderIsEmpty(handle(root))).toBe(false)
  })
})
