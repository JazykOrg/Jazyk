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
import { pushTraceEvents } from './lib/events'
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

async function boot() {
  const lines: string[] = ['starting the compiler']
  root.render(<Splash lines={lines} />)

  const backend = await startWasmBackend({
    files: demoProject as Record<string, string>,
    model,
    onTrace: (events) => pushTraceEvents(qc, events),
  })
  setBackend(backend)
  // There is no stream to drop: the backend is in this tab. The connection banners
  // are about a server that went away, and there is no server.
  useApp.getState().setConnected(true)

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
