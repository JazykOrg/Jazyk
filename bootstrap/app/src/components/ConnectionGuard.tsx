// Connection health made visible: a banner while the backend is unreachable, and a
// blocking token prompt when a request answers 401 (a restarted server minted a new
// token). Recovery happens in place, no reload, so unsaved editor work survives.
import { useEffect, useRef, useState } from 'react'
import { get, setToken } from '../lib/api'
import { useApp } from '../lib/store'
// For the .palette overlay classes the prompt reuses.
import '../routes/routes.css'

// How long a disconnect must last before the banner shows. The stream takes a
// moment to open on page load; a flash on every load would cry wolf.
const BANNER_DELAY_MS = 2500

export default function ConnectionGuard() {
  const connected = useApp((a) => a.connected)
  const authRequired = useApp((a) => a.authRequired)
  const [bannered, setBannered] = useState(false)

  useEffect(() => {
    if (connected) {
      setBannered(false)
      return
    }
    const t = window.setTimeout(() => setBannered(true), BANNER_DELAY_MS)
    return () => window.clearTimeout(t)
  }, [connected])

  if (authRequired) return <TokenPrompt />
  if (bannered)
    return (
      <div className="conn-banner mono">
        connection to the backend lost, retrying. Changes stay in this tab until it heals.
      </div>
    )
  return null
}

function TokenPrompt() {
  const setAuthRequired = useApp((a) => a.setAuthRequired)
  const bumpTokenEpoch = useApp((a) => a.bumpTokenEpoch)
  const [value, setValue] = useState('')
  const [busy, setBusy] = useState(false)
  const [err, setErr] = useState<string | null>(null)
  const inputRef = useRef<HTMLInputElement>(null)

  useEffect(() => {
    inputRef.current?.focus()
  }, [])

  const submit = async () => {
    // Accept the whole opened URL or the bare token.
    const m = value.match(/token=([0-9a-f]+)/)
    const candidate = m ? m[1] : value.trim()
    if (!candidate) return
    setBusy(true)
    setErr(null)
    setToken(candidate)
    try {
      // Verify before closing the prompt: any guarded read will do.
      await get('/api/status')
      setAuthRequired(false)
      // Long-lived connections (event stream, editor WebSocket) re-dial on this.
      bumpTokenEpoch()
    } catch (e) {
      const msg = (e as Error).message
      setErr(msg === '401' ? 'token rejected' : `backend unreachable (${msg})`)
    } finally {
      setBusy(false)
    }
  }

  return (
    <>
      <div className="palette-backdrop" />
      <div className="palette token-prompt">
        <p>
          <b>Session token rejected.</b>
        </p>
        <p className="muted">
          The server was likely restarted and minted a new token. Paste the new URL it
          printed (or just the token) to resume this session. Nothing in this tab is
          lost.
        </p>
        <form
          onSubmit={(e) => {
            e.preventDefault()
            submit()
          }}
        >
          <input
            ref={inputRef}
            className="mono"
            placeholder="http://127.0.0.1:4680/#token=… or the token"
            value={value}
            onChange={(e) => setValue(e.target.value)}
            disabled={busy}
          />
          <button type="submit" disabled={busy || !value.trim()}>
            {busy ? 'verifying…' : 'resume ▸'}
          </button>
        </form>
        {err && <p className="v-stale">{err}</p>}
      </div>
    </>
  )
}
