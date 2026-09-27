// The session token per port (docs/frontends/gui.md#serving): stored under
// `jazyk.server.<port>`, resumed with no fragment, replaced by the prompt's paste,
// and carried by the absolute URLs of what cannot send a header.
import { beforeEach, describe, expect, it } from 'vitest'
import { apiUrl, clearServer, currentServer, currentToken, serverTokenKey, setServer, setToken, storeToken, storedToken, tokenParam } from './api'
import { serverBackend, serverBase, setBackend } from './backend'

beforeEach(() => {
  localStorage.clear()
  clearServer()
  setBackend({ kind: 'demo', request: async () => ({ status: 501, body: null, text: '' }) })
})

describe('the token per port', () => {
  it('is keyed by the port', () => {
    expect(serverTokenKey(4680)).toBe('jazyk.server.4680')
    expect(serverTokenKey(4681)).toBe('jazyk.server.4681')
  })

  it('stores the token the connect route brings and resumes with it', () => {
    expect(setServer(4680, 'abc')).toBe('abc')
    expect(localStorage.getItem('jazyk.server.4680')).toBe('abc')
    clearServer()
    expect(currentToken()).toBeNull()
    // No token given: the stored one is the connection's.
    expect(setServer(4680)).toBe('abc')
    expect(currentToken()).toBe('abc')
    expect(currentServer()).toEqual({ port: 4680, token: 'abc' })
  })

  it('keeps one token per port', () => {
    setServer(4680, 'one')
    setServer(4681, 'two')
    expect(storedToken(4680)).toBe('one')
    expect(storedToken(4681)).toBe('two')
    expect(setServer(4680)).toBe('one')
  })

  it('is replaced by the prompt for the connected port only', () => {
    setServer(4680, 'stale')
    setServer(4681, 'other')
    setServer(4680)
    setToken('fresh')
    expect(currentToken()).toBe('fresh')
    expect(storedToken(4680)).toBe('fresh')
    expect(storedToken(4681)).toBe('other')
  })

  it('is forgotten with the server', () => {
    storeToken(4680, 'abc')
    storeToken(4680, null)
    expect(storedToken(4680)).toBeNull()
    expect(setServer(4680)).toBeNull()
    expect(tokenParam()).toBe('')
  })
})

describe('apiUrl', () => {
  it('is relative with no server and absolute on the connected server, token in the query', () => {
    expect(apiUrl('/api/out/file?path=a.svg')).toBe('/api/out/file?path=a.svg')
    setServer(4690, 'tok')
    setBackend(serverBackend('http://127.0.0.1:4690', currentToken))
    expect(serverBase()).toBe('http://127.0.0.1:4690')
    expect(apiUrl('/api/out/file?path=a.svg')).toBe('http://127.0.0.1:4690/api/out/file?path=a.svg&token=tok')
    expect(apiUrl('/api/events')).toBe('http://127.0.0.1:4690/api/events?token=tok')
  })
})
