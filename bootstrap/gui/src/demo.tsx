// The demo entry: the same app, backed by the wasm compiler in this tab instead of
// a server. Routes live in the fragment, so the whole demo is one static page and
// needs no server rewriting unknown paths to index.html.
//
// The backend comes up first and the app renders against the seeded project right
// away; the model loads behind it, because every read works without one and only a
// build needs it. See docs/frontends/wasm.md#the-demo.
import React from 'react'
import ReactDOM from 'react-dom/client'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { HashRouter } from 'react-router'
import App from './App'
import { setBackend } from './lib/backend'
import { startWasmBackend, crossOriginIsolated_ } from './lib/wasm-client'
import { DEFAULT_MODEL, loadModel, unavailableModel, webgpuAvailable } from './lib/webllm'
import type { ModelHost } from './lib/wasm-client'
import { applyTheme, useApp } from './lib/store'
import { pushTraceEvents, refreshAfterWrite } from './lib/events'
import { FolderMirror, folderPickerAvailable, pickFolder, readFolder } from './lib/folder'
import demoProject from './wasm-backend/demo-project.json'
import './tokens.css'
import './app.css'

applyTheme()

const qc = new QueryClient({
  defaultOptions: { queries: { retry: 1, refetchOnWindowFocus: false } },
})

const root = ReactDOM.createRoot(document.getElementById('root')!)

function Splash({ lines }: { lines: string[] }) {
  return (
    <div style={{ padding: '2rem', fontFamily: 'ui-monospace, monospace', lineHeight: 1.7 }}>
      <h1 style={{ fontSize: '1.1rem', margin: '0 0 1rem' }}>jazyk in the browser</h1>
      {lines.map((l, i) => (
        <div key={i} style={{ opacity: i === lines.length - 1 ? 1 : 0.6 }}>
          {l}
        </div>
      ))}
    </div>
  )
}

// The model, once. Until it resolves a build waits; if it fails, a build is refused
// with the reason rather than hanging.
let host: ModelHost = unavailableModel('the model is still loading')
let reason: string | null = null
if (!crossOriginIsolated_()) {
  reason =
    'this page is not cross-origin isolated, so the backend cannot call a model (COOP/COEP headers are required for SharedArrayBuffer)'
} else if (!webgpuAvailable()) {
  reason = 'this browser has no WebGPU, so there is no model to call'
}
if (reason) host = unavailableModel(reason)

// The bridge hands every call to whatever `host` is at the time of the call, so the
// backend can start before the model has finished loading.
const model: ModelHost = {
  name: () => DEFAULT_MODEL,
  chat: (body) => host.chat(body),
}

function loadModelInBackground(say: (l: string) => void) {
  if (reason) {
    say(reason)
    return
  }
  say(`loading ${DEFAULT_MODEL} (cached after the first visit)`)
  loadModel(DEFAULT_MODEL, (p) => say(p.text || `${Math.round(p.progress * 100)}%`))
    .then((m) => {
      host = m
      say('the model is ready')
    })
    .catch((e) => {
      const msg = e instanceof Error ? e.message : String(e)
      host = unavailableModel(`the model failed to load: ${msg}`)
      say(`the model failed to load: ${msg}`)
    })
}

// A render failure in a tab is a blank page unless something says otherwise. The
// demo has no server log to fall back on, so it shows the error itself.
class ErrorBoundary extends React.Component<
  { children: React.ReactNode },
  { error: Error | null }
> {
  state: { error: Error | null } = { error: null }
  static getDerivedStateFromError(error: Error) {
    return { error }
  }
  render() {
    if (!this.state.error) return this.props.children
    return (
      <Splash
        lines={[
          'the app failed to render',
          this.state.error.message,
          ...(this.state.error.stack ?? '').split('\n').slice(1, 8),
        ]}
      />
    )
  }
}

// The two ways in: the bundled project in memory, or a folder on this computer,
// which needs the File System Access API (Chromium browsers have it).
function Chooser({ onDemo, onFolder }: { onDemo: () => void; onFolder: () => void }) {
  const canPick = folderPickerAvailable()
  return (
    <div style={{ padding: '2rem', fontFamily: 'ui-monospace, monospace', lineHeight: 1.7, maxWidth: 640 }}>
      <h1 style={{ fontSize: '1.1rem', margin: '0 0 1rem' }}>jazyk in the browser</h1>
      <p>
        The compiler runs in this tab as WebAssembly, against a model that runs on your GPU.
        Nothing leaves the machine.
      </p>
      <p>
        <button onClick={onDemo}>open the demo project</button>{' '}
        <span style={{ opacity: 0.7 }}>a small shop, already compiled, held in memory</span>
      </p>
      <p>
        <button onClick={onFolder} disabled={!canPick}>open a folder on this computer</button>{' '}
        <span style={{ opacity: 0.7 }}>
          {canPick
            ? 'a directory with a jazyk.toml; edits and builds write back into it'
            : 'this browser has no File System Access API'}
        </span>
      </p>
    </div>
  )
}

type Source =
  | { kind: 'demo' }
  | { kind: 'folder'; files: Record<string, Uint8Array>; mirror: FolderMirror; name: string }

function chooseSource(): Promise<Source> {
  return new Promise((resolve) => {
    const onFolder = async () => {
      try {
        const handle = await pickFolder()
        const files = await readFolder(handle)
        if (!('jazyk.toml' in files)) {
          root.render(<Splash lines={[`${handle.name} has no jazyk.toml at its top; pick a project directory`]} />)
          window.setTimeout(() => root.render(<Chooser onDemo={() => resolve({ kind: 'demo' })} onFolder={onFolder} />), 2500)
          return
        }
        resolve({ kind: 'folder', files, mirror: new FolderMirror(handle, '/project'), name: handle.name })
      } catch (e) {
        // The picker was dismissed, or refused: back to the choice.
        console.info('[jazyk folder]', e)
      }
    }
    root.render(<Chooser onDemo={() => resolve({ kind: 'demo' })} onFolder={onFolder} />)
  })
}

async function boot() {
  const source = await chooseSource()
  const lines: string[] = [
    source.kind === 'demo' ? 'starting the compiler on the demo project' : `starting the compiler on ${source.name}`,
  ]
  root.render(<Splash lines={lines} />)

  const backend = await startWasmBackend({
    files: source.kind === 'demo' ? (demoProject as Record<string, string>) : source.files,
    mirror: source.kind === 'folder' ? source.mirror : undefined,
    model,
    onTrace: (events) => {
      // The trace is also on the console, so a build is followable without the
      // activity panel open.
      const buf = ((window as unknown as { jazykTrace?: unknown[] }).jazykTrace ??= [])
      for (const e of events) {
        buf.push(e)
        console.info('[jazyk trace]', JSON.stringify(e))
      }
      pushTraceEvents(qc, events)
    },
  })
  // A write (a save, a build) is the only thing that moves the store, and there is
  // no event stream to say so: refetch after every one.
  setBackend({
    ...backend,
    async request(method, path, body) {
      const reply = await backend.request(method, path, body)
      if (method !== 'GET') {
        refreshAfterWrite(qc)
      }
      return reply
    },
  })
  // There is no stream to drop: the backend is in this tab. The connection banners
  // are about a server that went away, and there is no server.
  useApp.getState().setConnected(true)
  // The backend answers the same envelope over the console, which is how a person
  // (or a test) pokes at it without the app in the way.
  ;(window as unknown as { jazyk: unknown }).jazyk = backend

  root.render(
    <React.StrictMode>
      <ErrorBoundary>
        <QueryClientProvider client={qc}>
          <HashRouter>
            <App />
          </HashRouter>
        </QueryClientProvider>
      </ErrorBoundary>
    </React.StrictMode>,
  )

  // Progress goes to the console once the app owns the screen; the app's own status
  // surfaces name the model.
  loadModelInBackground((l) => console.info('[jazyk model]', l))
}

boot().catch((e) => {
  root.render(<Splash lines={['the backend failed to start', String(e)]} />)
})
