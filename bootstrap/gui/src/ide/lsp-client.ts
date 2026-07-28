// Hand-rolled LSP client over one WebSocket: one bare JSON-RPC message per text
// frame, no Content-Length framing. Deliberately not monaco-languageclient; the
// server speaks a small known subset and the protocol fits in this file.

export interface LspPosition {
  line: number
  character: number
}

export interface LspRange {
  start: LspPosition
  end: LspPosition
}

export interface LspLocation {
  uri: string
  range: LspRange
}

export interface LspDiagnostic {
  range: LspRange
  severity?: number // 1 error, 2 warning, 3 info, 4 hint
  code?: string | number
  source?: string
  message: string
}

export interface LspHover {
  contents: string | { kind?: string; value: string }
  range?: LspRange
}

export interface LspCompletionItem {
  label: string
  kind?: number
  detail?: string
  documentation?: string | { value: string }
  insertText?: string
  filterText?: string
  textEdit?: { range: LspRange; newText: string }
}

export interface LspDocumentLink {
  range: LspRange
  target?: string
  tooltip?: string
}

export interface LspCodeLens {
  range: LspRange
  command?: { title: string; command: string; arguments?: unknown[] }
}

interface RpcMessage {
  jsonrpc?: string
  id?: number | string | null
  method?: string
  params?: unknown
  result?: unknown
  error?: { code: number; message: string }
}

type DiagListener = (diags: LspDiagnostic[]) => void

const CHANGE_DEBOUNCE_MS = 200
const BACKOFF_START_MS = 500
const BACKOFF_MAX_MS = 10_000

export class LspClient {
  // A factory, not a string: the session token can change mid-session (server
  // restart), and every re-dial must carry the current one.
  private url: () => string = () => ''
  private ws: WebSocket | null = null
  private disposed = false
  private backoff = BACKOFF_START_MS
  private nextId = 1
  private pending = new Map<number | string, { resolve: (v: unknown) => void; reject: (e: Error) => void }>()
  private diagListeners = new Map<string, Set<DiagListener>>()
  // Client-side mirror of the open-document overlay, replayed on reconnect.
  private open = new Map<string, { text: string; version: number }>()
  private changeTimers = new Map<string, number>()

  connect(url: () => string) {
    this.url = url
    this.disposed = false
    this.dial()
  }

  dispose() {
    this.disposed = true
    for (const t of this.changeTimers.values()) window.clearTimeout(t)
    this.changeTimers.clear()
    this.ws?.close()
    this.ws = null
  }

  onDiagnostics(uri: string, cb: DiagListener): () => void {
    let set = this.diagListeners.get(uri)
    if (!set) this.diagListeners.set(uri, (set = new Set()))
    set.add(cb)
    return () => {
      set.delete(cb)
    }
  }

  private dial() {
    let ws: WebSocket
    try {
      ws = new WebSocket(this.url())
    } catch {
      this.scheduleRedial()
      return
    }
    this.ws = ws
    ws.onopen = () => void this.handshake(ws)
    ws.onmessage = (ev) => {
      if (typeof ev.data === 'string') this.onMessage(ev.data)
    }
    ws.onclose = () => {
      if (this.ws === ws) this.onClosed()
    }
    ws.onerror = () => ws.close()
  }

  private async handshake(ws: WebSocket) {
    try {
      await this.request('initialize', {
        processId: null,
        rootUri: null,
        capabilities: {},
        clientInfo: { name: 'jazyk-gui' },
      })
      this.notify('initialized', {})
      this.backoff = BACKOFF_START_MS
      // Replay the overlay so the new server session matches the editor again.
      for (const [uri, doc] of this.open) {
        doc.version += 1
        this.notify('textDocument/didOpen', {
          textDocument: { uri, languageId: 'markdown', version: doc.version, text: doc.text },
        })
      }
    } catch {
      ws.close()
    }
  }

  private onClosed() {
    this.ws = null
    const err = new Error('lsp connection closed')
    for (const p of this.pending.values()) p.reject(err)
    this.pending.clear()
    this.scheduleRedial()
  }

  private scheduleRedial() {
    if (this.disposed) return
    const wait = this.backoff
    this.backoff = Math.min(this.backoff * 2, BACKOFF_MAX_MS)
    window.setTimeout(() => {
      if (!this.disposed && !this.ws) this.dial()
    }, wait)
  }

  private onMessage(raw: string) {
    let msg: RpcMessage
    try {
      msg = JSON.parse(raw) as RpcMessage
    } catch {
      return
    }
    if (msg.method !== undefined && msg.id !== undefined && msg.id !== null) {
      // Server-to-client request; nothing we support, answer with an empty result.
      this.send({ jsonrpc: '2.0', id: msg.id, result: null })
      return
    }
    if (msg.id !== undefined && msg.id !== null) {
      const p = this.pending.get(msg.id)
      if (!p) return
      this.pending.delete(msg.id)
      if (msg.error) p.reject(new Error(msg.error.message))
      else p.resolve(msg.result)
      return
    }
    if (msg.method === 'textDocument/publishDiagnostics') {
      const params = msg.params as { uri: string; diagnostics?: LspDiagnostic[] }
      const set = this.diagListeners.get(params.uri)
      if (set) for (const cb of set) cb(params.diagnostics ?? [])
    }
  }

  private send(msg: RpcMessage) {
    if (this.ws && this.ws.readyState === WebSocket.OPEN) this.ws.send(JSON.stringify(msg))
  }

  request<T>(method: string, params: unknown): Promise<T> {
    if (!this.ws || this.ws.readyState !== WebSocket.OPEN)
      return Promise.reject(new Error('lsp not connected'))
    const id = this.nextId++
    const p = new Promise<T>((resolve, reject) => {
      this.pending.set(id, { resolve: resolve as (v: unknown) => void, reject })
    })
    this.send({ jsonrpc: '2.0', id, method, params })
    return p
  }

  notify(method: string, params: unknown) {
    this.send({ jsonrpc: '2.0', method, params })
  }

  // ---- document sync (full text; didChange debounced per document) ----

  didOpen(uri: string, text: string) {
    this.open.set(uri, { text, version: 1 })
    this.notify('textDocument/didOpen', {
      textDocument: { uri, languageId: 'markdown', version: 1, text },
    })
  }

  didChange(uri: string, text: string) {
    const doc = this.open.get(uri)
    if (!doc) return
    doc.text = text
    const prev = this.changeTimers.get(uri)
    if (prev !== undefined) window.clearTimeout(prev)
    this.changeTimers.set(
      uri,
      window.setTimeout(() => this.flushChange(uri), CHANGE_DEBOUNCE_MS),
    )
  }

  private flushChange(uri: string) {
    const t = this.changeTimers.get(uri)
    if (t !== undefined) window.clearTimeout(t)
    this.changeTimers.delete(uri)
    const doc = this.open.get(uri)
    if (!doc) return
    doc.version += 1
    this.notify('textDocument/didChange', {
      textDocument: { uri, version: doc.version },
      contentChanges: [{ text: doc.text }],
    })
  }

  didSave(uri: string) {
    this.flushChange(uri)
    this.notify('textDocument/didSave', { textDocument: { uri } })
  }

  didClose(uri: string) {
    const t = this.changeTimers.get(uri)
    if (t !== undefined) window.clearTimeout(t)
    this.changeTimers.delete(uri)
    this.open.delete(uri)
    this.notify('textDocument/didClose', { textDocument: { uri } })
  }

  // ---- typed feature requests ----

  hover(uri: string, position: LspPosition): Promise<LspHover | null> {
    return this.request('textDocument/hover', { textDocument: { uri }, position })
  }

  async definition(uri: string, position: LspPosition): Promise<LspLocation[]> {
    const r = await this.request<LspLocation | LspLocation[] | null>('textDocument/definition', {
      textDocument: { uri },
      position,
    })
    return r == null ? [] : Array.isArray(r) ? r : [r]
  }

  async references(uri: string, position: LspPosition): Promise<LspLocation[]> {
    const r = await this.request<LspLocation[] | null>('textDocument/references', {
      textDocument: { uri },
      position,
      context: { includeDeclaration: true },
    })
    return r ?? []
  }

  async completion(uri: string, position: LspPosition): Promise<LspCompletionItem[]> {
    const r = await this.request<LspCompletionItem[] | { items: LspCompletionItem[] } | null>(
      'textDocument/completion',
      { textDocument: { uri }, position },
    )
    return r == null ? [] : Array.isArray(r) ? r : (r.items ?? [])
  }

  async documentLinks(uri: string): Promise<LspDocumentLink[]> {
    const r = await this.request<LspDocumentLink[] | null>('textDocument/documentLink', {
      textDocument: { uri },
    })
    return r ?? []
  }

  async codeLens(uri: string): Promise<LspCodeLens[]> {
    const r = await this.request<LspCodeLens[] | null>('textDocument/codeLens', {
      textDocument: { uri },
    })
    return r ?? []
  }
}
