// The connect route, `#/connect?port=<port>&token=<token>`: the address `jazyk gui`
// opens (docs/frontends/gui.md#connecting-to-a-local-jazyk). It hands the port and
// the token to the connection, drops the fragment so a copied URL does not carry
// the secret, and says what happens: connecting, the documents (the connection
// moves there itself), or that nothing answered and how to start a server.
import { useEffect, useRef } from 'react'
import { Link, useLocation } from 'react-router'
import { DEFAULT_PORT, useConnection } from '../lib/connect'

const send = (name: string, detail?: unknown) => window.dispatchEvent(new CustomEvent(name, { detail }))

export function parseConnect(search: string): { port: number; token?: string } {
  const q = new URLSearchParams(search)
  const port = Number(q.get('port') ?? '') || DEFAULT_PORT
  const token = q.get('token')?.trim() || undefined
  return { port, token }
}

export default function ConnectRoute() {
  const loc = useLocation()
  const conn = useConnection()
  const started = useRef<string | null>(null)
  const { port, token } = parseConnect(loc.search)

  useEffect(() => {
    // Once per address: the strict-mode double mount must not connect twice.
    const key = `${port}:${token ?? ''}`
    if (started.current === key) return
    started.current = key
    // The fragment goes before anything else, so the secret is never copied on.
    if (token) history.replaceState(null, '', `${window.location.pathname}${window.location.search}#/connect?port=${port}`)
    send('jazyk:connect', { port, token })
  }, [port, token])

  return (
    <div className="ws-scroll">
      <div className="home connect-route">
        <h1>jazyk</h1>
        {conn.state === 'unreachable' && conn.port === port ? (
          <>
            <p>
              <b>Nothing answered on port {port}.</b>
            </p>
            <p className="muted">
              Run <span className="mono">jazyk gui</span> in the project's folder on this computer: it starts the server and opens this page
              itself. If it is running on another port, connect from <Link to="/">Home</Link>.
            </p>
            <div className="actions">
              <button className="primary" onClick={() => send('jazyk:connect', { port, token })}>
                Try again
              </button>
              <Link to="/" className="btn">
                Home
              </Link>
            </div>
          </>
        ) : (
          <p className="muted">Connecting to jazyk on this computer, port {port}…</p>
        )}
      </div>
    </div>
  )
}
