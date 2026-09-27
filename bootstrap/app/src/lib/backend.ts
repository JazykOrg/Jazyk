// The transport seam: every read and write goes through one request(method, path,
// body) the backend implements, so the app never knows which backend it has beyond
// the capability descriptor (docs/frontends/gui.md#backends-and-capabilities).
// The server backend is fetch against a `jazyk gui` server on this computer, with
// the bearer token (docs/frontends/gui.md#connecting-to-a-local-jazyk); a demo or
// browser backend installs its own before the app mounts.

export interface BackendReply {
  status: number
  // The parsed JSON body when the reply is JSON, else the text.
  body: unknown
  text: string
}

export interface Backend {
  kind: 'server' | 'demo' | 'wasm'
  request(method: string, path: string, body?: unknown, headers?: Record<string, string>): Promise<BackendReply>
}

export interface ServerBackend extends Backend {
  kind: 'server'
  // `http://127.0.0.1:<port>`: what every absolute URL (the event stream, the
  // editor's WebSocket, an image) starts with.
  base: string
}

// A server backend over `base`, the token read per request so a replacement token
// entered after a restart reaches the next call.
export function serverBackend(base: string, token: () => string | null): ServerBackend {
  const request = async (method: string, path: string, body?: unknown, extra?: Record<string, string>): Promise<BackendReply> => {
    const headers: Record<string, string> = { ...(extra ?? {}) }
    const t = token()
    if (t) headers['Authorization'] = `Bearer ${t}`
    if (body !== undefined) headers['Content-Type'] = 'application/json'
    const res = await fetch(`${base}${path}`, {
      method,
      headers,
      body: body !== undefined ? JSON.stringify(body) : undefined,
    })
    const text = await res.text()
    let parsed: unknown = text
    const ctype = res.headers.get('content-type') ?? ''
    if (ctype.includes('application/json')) {
      try {
        parsed = JSON.parse(text)
      } catch {
        parsed = text
      }
    }
    return { status: res.status, body: parsed, text }
  }
  return { kind: 'server', base, request }
}

// Before any backend is installed nothing answers: the page has no server of its
// own (it is a static page), so a request is refused rather than sent to the page's
// origin.
const none: Backend = {
  kind: 'demo',
  request: async () => ({ status: 501, body: { error: 'no backend is open' }, text: 'no backend is open' }),
}

let current: Backend = none

export function backend(): Backend {
  return current
}

export function setBackend(b: Backend) {
  current = b
}

// The connected server's base URL, or null while the backend is in the tab.
export function serverBase(): string | null {
  return current.kind === 'server' ? (current as ServerBackend).base : null
}
