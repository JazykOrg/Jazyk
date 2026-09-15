// The transport seam: every read and write goes through one request(method, path,
// body) the backend implements, so the app never knows which backend it has beyond
// the capability descriptor (docs/frontends/gui.md#backends-and-capabilities).
// The server backend is fetch with the bearer token; a demo or browser backend
// installs its own before the app mounts.

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

let token: string | null = null

export function currentToken(): string | null {
  return token
}

export function useToken(t: string | null) {
  token = t
}

async function serverRequest(
  method: string,
  path: string,
  body?: unknown,
  extra?: Record<string, string>,
): Promise<BackendReply> {
  const headers: Record<string, string> = { ...(extra ?? {}) }
  if (token) headers['Authorization'] = `Bearer ${token}`
  if (body !== undefined) headers['Content-Type'] = 'application/json'
  const res = await fetch(path, {
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

export const serverBackend: Backend = { kind: 'server', request: serverRequest }

let current: Backend = serverBackend

export function backend(): Backend {
  return current
}

export function setBackend(b: Backend) {
  current = b
}
