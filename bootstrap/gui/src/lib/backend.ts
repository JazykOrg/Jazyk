// Which backend the app is talking to. Two answer the same API: the native server
// of `jazyk gui` over HTTP, and the wasm backend in a worker in this tab. The API
// client asks here instead of calling fetch, so no endpoint in api.ts knows the
// difference. See docs/frontends/wasm.md#one-api-two-transports.

export type BackendKind = 'server' | 'wasm'

export interface Reply {
  status: number
  body: unknown
}

export interface Backend {
  kind: BackendKind
  request(method: string, path: string, body?: unknown): Promise<Reply>
  // The trace of a running build. The server streams it over SSE; the wasm backend
  // buffers it and the page drains it between turns.
  drainTrace?(): Promise<unknown[]>
  ready?: Promise<void>
}

// The server backend: a plain fetch, with the session token on every request.
function serverBackend(token: () => string | null): Backend {
  return {
    kind: 'server',
    async request(method, path, body) {
      const headers: Record<string, string> = {}
      const t = token()
      if (t) headers['Authorization'] = `Bearer ${t}`
      if (body !== undefined) headers['Content-Type'] = 'application/json'
      const res = await fetch(path, {
        method,
        headers,
        body: body !== undefined ? JSON.stringify(body) : undefined,
      })
      // A non-JSON body (the out directory's files) still travels as a reply.
      const text = await res.text()
      let parsed: unknown = text
      try {
        parsed = text.length ? JSON.parse(text) : null
      } catch {
        // not JSON; the caller asked for text
      }
      return { status: res.status, body: parsed }
    },
  }
}

let current: Backend | null = null

export function setBackend(b: Backend) {
  current = b
}

export function backend(): Backend {
  if (!current) throw new Error('no backend installed')
  return current
}

export function installServerBackend(token: () => string | null) {
  if (!current) setBackend(serverBackend(token))
}

export function isWasm(): boolean {
  return current?.kind === 'wasm'
}
