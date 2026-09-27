// Home's `Connect to jazyk on this computer` card (docs/frontends/gui.md#home): the
// way in for a person who arrived without the link. Install the CLI, run `jazyk gui`
// in the project's folder, and the page opens itself; or a port and Connect reach a
// server already running. The token is asked for when the ping answers but the API
// refuses (the token prompt, components/ConnectionGuard.tsx).
import { useState } from 'react'
import { DEFAULT_PORT, useConnection } from '../lib/connect'

const send = (name: string, detail?: unknown) => window.dispatchEvent(new CustomEvent(name, { detail }))

export default function ConnectCard() {
  const conn = useConnection()
  const [port, setPort] = useState(String(DEFAULT_PORT))
  const n = Number(port)
  const valid = Number.isInteger(n) && n > 0 && n < 65536
  const busy = conn.state === 'connecting'

  return (
    <div className="home-card home-connect">
      <h3>Connect to jazyk on this computer</h3>
      <p className="muted">
        Install the CLI and run <span className="mono">jazyk gui</span> in the project's folder: it opens this page connected. This is the way to
        every job kind, the language server, and an agent. A server already running is reached by its port.
      </p>
      <form
        className="home-new-row"
        onSubmit={(e) => {
          e.preventDefault()
          if (valid && !busy) send('jazyk:connect', { port: n })
        }}
      >
        <input
          type="number"
          inputMode="numeric"
          min={1}
          max={65535}
          value={port}
          onChange={(e) => setPort(e.target.value)}
          aria-label="Port"
          className="mono home-port"
        />
        <button type="submit" className="primary" disabled={!valid || busy}>
          {busy ? 'Connecting…' : 'Connect'}
        </button>
      </form>
      {conn.state === 'unreachable' && (
        <p className="v-stale" role="status">
          Nothing answered on port {conn.port}. Run <span className="mono">jazyk gui</span> in the project's folder; it prints the port it
          serves on.
        </p>
      )}
      <p className="muted home-hint">If the server answers but refuses the connection, paste the token from the address it printed when asked.</p>
    </div>
  )
}
