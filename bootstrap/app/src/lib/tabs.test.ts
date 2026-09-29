import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { PING_TIMEOUT, TabPresence, type TabChannel, type TabProject } from './tabs'

// A BroadcastChannel stand-in: a message reaches every other channel on the hub,
// never its sender, asynchronously as the browser delivers it.
class Hub {
  channels: FakeChannel[] = []
  open(): FakeChannel {
    const c = new FakeChannel(this)
    this.channels.push(c)
    return c
  }
}

class FakeChannel implements TabChannel {
  private fns: ((e: { data: unknown }) => void)[] = []
  closed = false
  constructor(private hub: Hub) {}
  postMessage(m: unknown) {
    if (this.closed) return
    const data = structuredClone(m)
    for (const c of this.hub.channels) if (c !== this && !c.closed) queueMicrotask(() => c.fns.forEach((f) => f({ data })))
  }
  addEventListener(_: 'message', fn: (e: { data: unknown }) => void) {
    this.fns.push(fn)
  }
}

const shop: TabProject = { entryId: 'demo:shop', name: 'The shop', kind: 'demo' }
const erp: TabProject = { entryId: 'demo:erp', name: 'The ERP', kind: 'demo' }
const flush = () => new Promise<void>((r) => queueMicrotask(r))
const names = (p: TabPresence) => p.list().map((t) => t.name)

describe('TabPresence', () => {
  beforeEach(() => {
    vi.useFakeTimers()
  })
  afterEach(() => {
    vi.useRealTimers()
  })

  it('learns the projects other tabs announce and answer a ping with', async () => {
    const hub = new Hub()
    const a = new TabPresence(hub.open(), 'a')
    const b = new TabPresence(hub.open(), 'b')
    b.set(erp)
    await flush()
    expect(a.list()).toEqual([{ tabId: 'b', ...erp }])
    expect(b.list()).toEqual([])

    // A tab opened later asks, and the tabs with a project answer.
    const c = new TabPresence(hub.open(), 'c')
    a.set(shop)
    await flush()
    c.ping()
    await flush()
    await flush()
    expect(names(c).sort()).toEqual(['The ERP', 'The shop'])
  })

  it('drops a tab that says bye or closes its project', async () => {
    const hub = new Hub()
    const a = new TabPresence(hub.open(), 'a')
    const b = new TabPresence(hub.open(), 'b')
    const c = new TabPresence(hub.open(), 'c')
    b.set(erp)
    c.set(shop)
    await flush()
    expect(names(a)).toEqual(['The ERP', 'The shop'])
    b.bye()
    await flush()
    expect(names(a)).toEqual(['The shop'])
    c.set(null)
    await flush()
    expect(a.list()).toEqual([])
  })

  it('drops a tab that does not answer a ping within the timeout', async () => {
    const hub = new Hub()
    const a = new TabPresence(hub.open(), 'a')
    const bChannel = hub.open()
    const b = new TabPresence(bChannel, 'b')
    const c = new TabPresence(hub.open(), 'c')
    b.set(erp)
    c.set(shop)
    await flush()
    expect(names(a)).toEqual(['The ERP', 'The shop'])

    // b's page went away without a word.
    bChannel.closed = true
    a.ping()
    await flush()
    await flush()
    expect(names(a)).toEqual(['The ERP', 'The shop'])
    vi.advanceTimersByTime(PING_TIMEOUT - 1)
    expect(names(a)).toEqual(['The ERP', 'The shop'])
    vi.advanceTimersByTime(1)
    expect(names(a)).toEqual(['The shop'])
  })

  it("lists one row per project and leaves out this tab's own", async () => {
    const hub = new Hub()
    const a = new TabPresence(hub.open(), 'a')
    const b = new TabPresence(hub.open(), 'b')
    const c = new TabPresence(hub.open(), 'c')
    const d = new TabPresence(hub.open(), 'd')
    b.set(erp)
    c.set(erp)
    d.set(shop)
    await flush()
    expect(names(a)).toEqual(['The ERP', 'The shop'])
    a.set(shop)
    expect(names(a)).toEqual(['The ERP'])
  })

  it('tells its listeners when the list changes, and only then', async () => {
    const hub = new Hub()
    const a = new TabPresence(hub.open(), 'a')
    const b = new TabPresence(hub.open(), 'b')
    const seen = vi.fn()
    a.subscribe(seen)
    b.set(erp)
    await flush()
    expect(seen).toHaveBeenCalledTimes(1)
    const before = a.list()
    // The same announcement again: the same list, the same object.
    b.announce()
    await flush()
    expect(seen).toHaveBeenCalledTimes(1)
    expect(a.list()).toBe(before)
  })
})
