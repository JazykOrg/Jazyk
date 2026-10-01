// Connecting the page to a `jazyk gui` server on this computer
// (docs/frontends/gui.md#connecting-to-a-local-jazyk): ping
// `http://127.0.0.1:<port>/api/ping`, store the token under the port, swap the
// backend for the server's, remember the server in Recent, and open the documents.
// A server that does not answer is said in words; a token the API refuses surfaces
// as the token prompt (lib/api.ts), never as a quiet failure.
import { useSyncExternalStore } from 'react'
import type { QueryClient } from '@tanstack/react-query'
import { backend, serverBackend, setBackend, type Backend } from './backend'
import { clearServer, currentToken, get, setServer } from './api'
import { startEventStream } from './events'
import { touchRecent } from './recent'
import { useApp } from './store'

export const DEFAULT_PORT = 4680

export type Connection =
  | { state: 'idle' }
  | { state: 'connecting'; port: number }
  | { state: 'connected'; port: number; name: string; root: string }
  // Nothing answered the ping: no server on that port, or not a jazyk one.
  | { state: 'unreachable'; port: number }

let connection: Connection = { state: 'idle' }
const listeners = new Set<() => void>()

function publish(c: Connection) {
  connection = c
  for (const l of listeners) l()
}

export function useConnection(): Connection {
  return useSyncExternalStore(
    (notify) => {
      listeners.add(notify)
      return () => {
        listeners.delete(notify)
      }
    },
    () => connection,
  )
}

export function currentConnection(): Connection {
  return connection
}

export const serverUrl = (port: number) => `http://127.0.0.1:${port}`

// The project's name from its root, as the top bar shows it.
export const projectName = (root: string) => root.replace(/\/+$/, '').split('/').pop() || root

// The tokenless ping: the served project's root, or null when nothing (or
// something other than jazyk) answers within a moment.
export async function ping(port: number): Promise<{ root: string; version: string } | null> {
  const ctrl = new AbortController()
  const timer = window.setTimeout(() => ctrl.abort(), 4000)
  try {
    const res = await fetch(`${serverUrl(port)}/api/ping`, { signal: ctrl.signal })
    if (!res.ok) return null
    const j = (await res.json()) as { app?: string; root?: string; version?: string }
    if (j?.app !== 'jazyk-gui') return null
    return { root: String(j.root ?? ''), version: String(j.version ?? '') }
  } catch {
    return null
  } finally {
    window.clearTimeout(timer)
  }
}

interface Deps {
  qc: QueryClient
}

let deps: Deps | null = null
let stopStream: (() => void) | null = null
// The backend in the tab, to come back to when the server is left.
let local: Backend | null = null

export function installConnect(d: Deps) {
  deps = d
}

// Connect to the server on `port`. A token given (the connect route's) replaces the
// stored one; none given resumes with the stored one, and a server that refuses it
// shows the token prompt over the documents. Resolves to whether a server answered.
export async function connect(port: number, token?: string): Promise<boolean> {
  if (!deps) throw new Error('connect: the page has not installed the connection')
  const { qc } = deps
  publish({ state: 'connecting', port })
  const answer = await ping(port)
  if (!answer) {
    publish({ state: 'unreachable', port })
    return false
  }
  const name = projectName(answer.root)
  if (backend().kind !== 'server') local = backend()
  stopStream?.()
  stopStream = null
  setServer(port, token)
  setBackend(serverBackend(serverUrl(port), currentToken))
  // Everything cached belonged to the project the tab had open before.
  qc.clear()
  useApp.setState({ chatSessions: {}, chatUpdates: {}, chatSelected: null, authRequired: false, folder: null, stored: null, docMode: null, caret: null, editorLine: null })
  touchRecent({ id: `server:${port}`, kind: 'server', name, port, root: answer.root })
  publish({ state: 'connected', port, name, root: answer.root })
  // The first guarded read: a token the server refuses raises the prompt here
  // (api.ts flips authRequired on a 401) rather than on the first panel.
  try {
    await get('/api/status')
  } catch {
    // A 401 is already on the screen as the prompt; anything else the panels say.
  }
  stopStream = startEventStream(qc)
  window.location.hash = '#/docs'
  return true
}

// The page leaves the server for a project in the tab: the stream stops, the
// backend in the tab is back, and the token stays stored for the next connect.
export function disconnectServer() {
  if (backend().kind !== 'server') return
  stopStream?.()
  stopStream = null
  clearServer()
  if (local) setBackend(local)
  useApp.setState({ authRequired: false, connected: true })
  publish({ state: 'idle' })
}
