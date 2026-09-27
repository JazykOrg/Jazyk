import 'fake-indexeddb/auto'
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest'
import { createStore, entries, promisifyRequest } from 'idb-keyval'
import { BufferSink, StoreWriter, describeStoreError, overlay, probeStorage, relPath, type Change, type FsSink } from './browser-store'

const bytes = (s: string) => new TextEncoder().encode(s)
const str = (b: Uint8Array) => new TextDecoder().decode(b)

// The records under one prefix, as { path: text }.
async function stored(prefix: string): Promise<Record<string, string>> {
  const out: Record<string, string> = {}
  for (const [k, v] of await entries<string, Uint8Array>(createStore('jazyk-projects', 'files'))) {
    if (k.startsWith(prefix)) out[k.slice(prefix.length)] = str(v)
  }
  return out
}

async function seed(prefix: string, files: Record<string, string>) {
  await createStore('jazyk-projects', 'files')('readwrite', (s) => {
    for (const [p, c] of Object.entries(files)) s.put(bytes(c), prefix + p)
    return promisifyRequest(s.transaction)
  })
}

async function clearAll() {
  await createStore('jazyk-projects', 'files')('readwrite', (s) => {
    s.clear()
    return promisifyRequest(s.transaction)
  })
}

// The writer stores nothing until storage was probed.
beforeAll(async () => {
  expect(await probeStorage()).toBe(true)
})

beforeEach(clearAll)
afterEach(() => {
  vi.restoreAllMocks()
})

describe('StoreWriter', () => {
  const P = 'file:t:'

  it('stores puts and deletes under its prefix', async () => {
    const w = new StoreWriter(P)
    w.put('a.md', bytes('A'))
    w.put('d/b.md', bytes('B'))
    expect(w.pendingCount).toBe(2)
    await w.flush()
    expect(w.pendingCount).toBe(0)
    expect(await stored(P)).toEqual({ 'a.md': 'A', 'd/b.md': 'B' })
    w.del('a.md')
    await w.flush()
    expect(await stored(P)).toEqual({ 'd/b.md': 'B' })
  })

  it('coalesces to the latest operation per path', async () => {
    const w = new StoreWriter(P)
    w.put('a.md', bytes('one'))
    w.put('a.md', bytes('two'))
    expect(w.pendingCount).toBe(1)
    await w.flush()
    expect(await stored(P)).toEqual({ 'a.md': 'two' })
  })

  it('replays a put then a tree delete in that order', async () => {
    await seed(P, { 'd/old.md': 'old' })
    const w = new StoreWriter(P)
    w.put('d/new.md', bytes('new'))
    w.delTree('d')
    await w.flush()
    expect(await stored(P)).toEqual({})
  })

  it('replays a tree delete then a put in that order', async () => {
    await seed(P, { 'd/old.md': 'old', 'd': 'dir' })
    const w = new StoreWriter(P)
    w.delTree('d')
    w.put('d/new.md', bytes('new'))
    await w.flush()
    expect(await stored(P)).toEqual({ 'd/new.md': 'new' })
  })

  it('leaves a sibling whose name only starts with the deleted tree', async () => {
    await seed(P, { 'd/x.md': 'x', 'dd/y.md': 'y', 'd.md': 'z' })
    const w = new StoreWriter(P)
    w.delTree('d')
    await w.flush()
    expect(await stored(P)).toEqual({ 'dd/y.md': 'y', 'd.md': 'z' })
  })

  it('keeps a refused batch ahead of what arrived during the write, then lands it', async () => {
    const w = new StoreWriter(P)
    const states: (string | null)[] = []
    w.onState = () => states.push(w.error)
    w.put('a.md', bytes('a1'))
    w.put('b.md', bytes('b'))
    vi.spyOn(IDBObjectStore.prototype, 'put').mockImplementationOnce(() => {
      throw new DOMException('full', 'QuotaExceededError')
    })
    vi.spyOn(console, 'error').mockImplementation(() => {})
    const first = w.flush()
    // The batch was taken; these arrive while it is being written.
    await Promise.resolve()
    w.put('c.md', bytes('c'))
    w.put('a.md', bytes('a2'))
    await first
    expect(w.error).toBe("this browser's storage for the site is full")
    const pending = (w as unknown as { pending: Map<string, { put: Uint8Array }> }).pending
    // The refused batch first, minus the path a newer op replaced, then the newer ops.
    expect([...pending.keys()]).toEqual(['b.md', 'c.md', 'a.md'])
    expect(str(pending.get('a.md')!.put)).toBe('a2')
    expect(await stored(P)).toEqual({})
    await w.flush()
    expect(w.error).toBeNull()
    expect(w.pendingCount).toBe(0)
    expect(await stored(P)).toEqual({ 'a.md': 'a2', 'b.md': 'b', 'c.md': 'c' })
    expect(states).toEqual(["this browser's storage for the site is full", null])
  })

  it('stores nothing after close', async () => {
    const w = new StoreWriter(P)
    w.put('a.md', bytes('a'))
    w.close()
    expect(w.pendingCount).toBe(0)
    w.put('b.md', bytes('b'))
    await w.flush()
    expect(await stored(P)).toEqual({})
  })

  it('lands on its own a moment after the last mutation', async () => {
    vi.useFakeTimers()
    try {
      const w = new StoreWriter(P)
      w.put('a.md', bytes('a'))
      await vi.advanceTimersByTimeAsync(299)
      expect(w.pendingCount).toBe(1)
      await vi.advanceTimersByTimeAsync(1)
      expect(w.pendingCount).toBe(0)
    } finally {
      vi.useRealTimers()
    }
    expect(await stored(P)).toEqual({ 'a.md': 'a' })
  })
})

describe('relPath', () => {
  it('is null for the root itself and anything outside it', () => {
    expect(relPath('/project')).toBeNull()
    expect(relPath('/project/')).toBeNull()
    expect(relPath('/project//')).toBeNull()
    expect(relPath('/projects/a.md')).toBeNull()
    expect(relPath('/other/a.md')).toBeNull()
    expect(relPath('a.md')).toBeNull()
  })

  it('strips the root and collapses doubled slashes', () => {
    expect(relPath('/project/a.md')).toBe('a.md')
    expect(relPath('/project/d/a.md')).toBe('d/a.md')
    expect(relPath('/project//d//a.md')).toBe('d/a.md')
    expect(relPath('/project/d/a.md/')).toBe('d/a.md')
  })

  it('takes another root', () => {
    expect(relPath('/p/x/a.md', '/p/x')).toBe('a.md')
    expect(relPath('/project/a.md', '/p/x')).toBeNull()
  })
})

describe('overlay', () => {
  const base = { 'a.md': bytes('a'), 'd/b.md': bytes('b'), 'd/c.md': bytes('c'), 'dd/e.md': bytes('e') }
  const text = (r: Record<string, Uint8Array>) => Object.fromEntries(Object.entries(r).map(([k, v]) => [k, str(v)]))

  it('lays writes and removes over the base in order', () => {
    const changes: [string, Change][] = [
      ['a.md', { op: 'write', bytes: bytes('a2') }],
      ['n.md', { op: 'write', bytes: bytes('n') }],
      ['d/b.md', { op: 'remove', dir: false }],
    ]
    expect(text(overlay(base, changes))).toEqual({ 'a.md': 'a2', 'n.md': 'n', 'd/c.md': 'c', 'dd/e.md': 'e' })
    expect(text(base)).toEqual({ 'a.md': 'a', 'd/b.md': 'b', 'd/c.md': 'c', 'dd/e.md': 'e' })
  })

  it('removes a directory with everything under it and nothing beside it', () => {
    expect(text(overlay(base, [['d', { op: 'remove', dir: true }]]))).toEqual({ 'a.md': 'a', 'dd/e.md': 'e' })
  })

  it('a write after a directory remove comes back', () => {
    const changes: [string, Change][] = [
      ['d', { op: 'remove', dir: true }],
      ['d/b.md', { op: 'write', bytes: bytes('b2') }],
    ]
    expect(text(overlay(base, changes))).toEqual({ 'a.md': 'a', 'd/b.md': 'b2', 'dd/e.md': 'e' })
  })
})

describe('BufferSink', () => {
  it('replays what it held in order, once', () => {
    const b = new BufferSink()
    const log: string[] = []
    const into: FsSink = {
      write: (p, x) => log.push(`write ${p} ${str(x)}`),
      remove: (p, dir) => log.push(`remove ${p} ${dir}`),
    }
    b.write('/project/a.md', bytes('a'))
    b.remove('/project/d', true)
    b.write('/project/d/b.md', bytes('b'))
    b.replay(into)
    b.replay(into)
    expect(log).toEqual(['write /project/a.md a', 'remove /project/d true', 'write /project/d/b.md b'])
  })
})

describe('describeStoreError', () => {
  it('names a full quota, else the message', () => {
    expect(describeStoreError(new DOMException('x', 'QuotaExceededError'))).toBe("this browser's storage for the site is full")
    expect(describeStoreError(new Error('boom'))).toBe('boom')
    expect(describeStoreError(new DOMException('', 'AbortError'))).toBe('AbortError')
    expect(describeStoreError('refused')).toBe('refused')
  })
})
