// The projects open in this browser's other tabs (docs/frontends/gui.md#the-project-menu):
// each tab announces its project over a `BroadcastChannel` when it opens or closes
// one, and answers a `who` ping with it. A tab leaves the list when it says `bye`
// (on pagehide), announces no project, or does not answer a ping within two
// seconds.
import { useEffect, useSyncExternalStore } from 'react'
import type { RecentEntry } from './recent'

// A tab's project: the recent entry's id, so choosing it here opens it the way its
// Recent row does.
export interface TabProject {
  entryId: string
  name: string
  kind: RecentEntry['kind']
}

export interface OtherTab extends TabProject {
  tabId: string
}

type Message =
  | { type: 'here'; tabId: string; project: TabProject | null }
  | { type: 'who'; tabId: string }
  | { type: 'bye'; tabId: string }

// What the presence needs of a `BroadcastChannel`; the tests pass a fake.
export interface TabChannel {
  postMessage(m: unknown): void
  addEventListener(type: 'message', fn: (e: { data: unknown }) => void): void
}

export const PING_TIMEOUT = 2000

const same = (a: TabProject | null, b: TabProject | null) =>
  a === b || (!!a && !!b && a.entryId === b.entryId && a.name === b.name && a.kind === b.kind)

export class TabPresence {
  private mine: TabProject | null = null
  private others = new Map<string, TabProject>()
  // The tabs asked by the last ping that have not answered yet.
  private awaiting = new Set<string>()
  private timer: ReturnType<typeof setTimeout> | null = null
  private listeners = new Set<() => void>()
  private snapshot: OtherTab[] = []

  constructor(
    private channel: TabChannel,
    readonly tabId: string,
  ) {
    channel.addEventListener('message', (e) => this.receive(e.data))
  }

  // This tab's project, or null when it has none; announced when it changes.
  set(project: TabProject | null) {
    if (same(this.mine, project)) return
    this.mine = project ? { entryId: project.entryId, name: project.name, kind: project.kind } : null
    this.announce()
    this.changed()
  }

  announce() {
    this.post({ type: 'here', tabId: this.tabId, project: this.mine })
  }

  // Asks every tab for its project. A tab known before that does not answer within
  // PING_TIMEOUT is gone.
  ping() {
    for (const id of this.others.keys()) this.awaiting.add(id)
    this.post({ type: 'who', tabId: this.tabId })
    if (this.timer) clearTimeout(this.timer)
    this.timer = setTimeout(() => this.expire(), PING_TIMEOUT)
  }

  bye() {
    this.post({ type: 'bye', tabId: this.tabId })
  }

  // The other tabs' projects, one row per project, without this tab's own.
  list(): OtherTab[] {
    return this.snapshot
  }

  subscribe(fn: () => void): () => void {
    this.listeners.add(fn)
    return () => {
      this.listeners.delete(fn)
    }
  }

  private expire() {
    this.timer = null
    if (this.awaiting.size === 0) return
    for (const id of this.awaiting) this.others.delete(id)
    this.awaiting.clear()
    this.changed()
  }

  private receive(data: unknown) {
    const m = data as Message
    if (!m || typeof m !== 'object' || typeof m.tabId !== 'string' || m.tabId === this.tabId) return
    if (m.type === 'who') {
      if (this.mine) this.announce()
      return
    }
    this.awaiting.delete(m.tabId)
    if (m.type === 'here' && m.project) this.others.set(m.tabId, m.project)
    else if (m.type === 'here' || m.type === 'bye') this.others.delete(m.tabId)
    else return
    this.changed()
  }

  private post(m: Message) {
    try {
      this.channel.postMessage(m)
    } catch {
      // a closed channel: nobody to tell
    }
  }

  private changed() {
    const seen = new Set(this.mine ? [this.mine.entryId] : [])
    const next: OtherTab[] = []
    for (const [tabId, p] of this.others) {
      if (seen.has(p.entryId)) continue
      seen.add(p.entryId)
      next.push({ tabId, ...p })
    }
    const unchanged = next.length === this.snapshot.length && next.every((t, i) => t.tabId === this.snapshot[i].tabId && same(t, this.snapshot[i]))
    if (unchanged) return
    this.snapshot = next
    for (const l of this.listeners) l()
  }
}

let shared: TabPresence | null | undefined

// The page's presence, started on first use; null where the browser has no
// BroadcastChannel.
export function tabPresence(): TabPresence | null {
  if (shared !== undefined) return shared
  if (typeof BroadcastChannel === 'undefined') return (shared = null)
  const p = new TabPresence(new BroadcastChannel('jazyk-tabs'), crypto.randomUUID())
  window.addEventListener('pagehide', () => p.bye())
  // A page restored from the back-forward cache said bye when it was hidden.
  window.addEventListener('pageshow', (e) => {
    if (!e.persisted) return
    p.announce()
    p.ping()
  })
  p.ping()
  return (shared = p)
}

const NONE: OtherTab[] = []
const subscribe = (fn: () => void) => tabPresence()?.subscribe(fn) ?? (() => {})
const read = () => tabPresence()?.list() ?? NONE

// Announces this tab's project and returns the other tabs'.
export function useOtherTabs(mine: TabProject | null): OtherTab[] {
  const entryId = mine?.entryId
  const name = mine?.name
  const kind = mine?.kind
  useEffect(() => {
    tabPresence()?.set(entryId && name !== undefined && kind ? { entryId, name, kind } : null)
  }, [entryId, name, kind])
  return useSyncExternalStore(subscribe, read)
}
