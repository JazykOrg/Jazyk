// The demo entry: the same app, backed by the wasm compiler in this tab instead of
// a server. Routes live in the fragment, so the whole demo is one static page and
// needs no server rewriting unknown paths to index.html.
//
// The app mounts at once against a backend with no project, so Home renders the
// picker from the capability descriptor; a bundled project, a project kept in the
// browser, or a folder on the computer opens on the events Home dispatches. The model loads behind all of it,
// because every read works without one and only a build needs it.
// See docs/frontends/wasm.md#the-demo.
import React from 'react'
import ReactDOM from 'react-dom/client'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { HashRouter } from 'react-router'
import App from './App'
import { setBackend } from './lib/backend'
import { startWasmBackend, crossOriginIsolated_, type ModelHost, type WasmProject } from './lib/wasm-client'
import { loadModel, unavailableModel, webgpuAvailable } from './lib/webllm'
import { builtInModel, builtInPresent, endpointModel, endpointReady, readSource, rememberLoaded, wasLoaded, webllmModelOf, type ModelSource } from './lib/model-source'
import { applyTheme, useApp } from './lib/store'
import { pushWireEvent, refreshAfterWrite } from './lib/events'
import { probeStorage } from './lib/browser-store'
import { installLocalProjects } from './lib/local-projects'
import { installConnect } from './lib/connect'
import { registerServiceWorker } from './lib/sw'
import { DEMOS } from './shell/Home'
import './tokens.css'
import './app.css'

applyTheme()
registerServiceWorker()

const qc = new QueryClient({
  // `networkMode: 'always'`: the backends are in this tab or on the loopback
  // address, so a query runs with the browser offline too (docs/frontends/wasm.md#offline).
  defaultOptions: { queries: { retry: 1, refetchOnWindowFocus: false, networkMode: 'always' }, mutations: { networkMode: 'always' } },
})
installConnect({ qc })

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

// A render failure in a tab is a blank page unless something says otherwise. The
// demo has no server log to fall back on, so it shows the error itself.
class ErrorBoundary extends React.Component<{ children: React.ReactNode }, { error: Error | null }> {
  state: { error: Error | null } = { error: null }
  static getDerivedStateFromError(error: Error) {
    return { error }
  }
  render() {
    if (!this.state.error) return this.props.children
    return <Splash lines={['the app failed to render', this.state.error.message, ...(this.state.error.stack ?? '').split('\n').slice(1, 8)]} />
  }
}

// The model, from the source this browser chose (docs/frontends/wasm.md#model-sources),
// loaded on request. Until it is ready a build is not offered; if it fails, a build is
// refused with the reason rather than hanging.
let host: ModelHost = unavailableModel('the model is not loaded')
let loading = false
// Bumped by every source change, so a load that finishes after a switch is dropped.
let epoch = 0

// Every source needs the shared buffers the worker blocks on; the downloaded model
// also needs WebGPU.
const isolation = crossOriginIsolated_()
  ? null
  : 'this page is not cross-origin isolated, so the backend cannot call a model (COOP/COEP headers are required for SharedArrayBuffer)'
const reasonFor = (s: ModelSource): string | null => {
  if (isolation) return isolation
  if (s.kind === 'webllm' && !webgpuAvailable()) return 'this browser has no WebGPU, so it cannot run the downloaded model'
  if (s.kind === 'chrome' && !builtInPresent()) return 'this browser has no built-in model'
  if (s.kind === 'endpoint' && !endpointReady(s)) return 'the endpoint needs a URL and a model name'
  return null
}

// The endpoint needs no loading: it is the model as soon as it is configured.
function applySource() {
  epoch += 1
  loading = false
  const s = readSource()
  host = s.kind === 'endpoint' && !reasonFor(s) ? endpointModel(s) : unavailableModel(reasonFor(s) ?? 'the model is not loaded')
  useApp.getState().setModelLoad(null)
  qc.invalidateQueries({ queryKey: ['capabilities'] })
}

// The bridge hands every call to whatever `host` is at the time of the call, so the
// backend can start before the model has finished loading.
const model: ModelHost = {
  name: () => (readSource().kind === 'webllm' ? webllmModelOf(readSource()) : host.name()),
  chat: (body) => host.chat(body),
  available: () => host.available(),
  // The source can run here, and nothing is loaded or loading yet.
  loadable: () => !reasonFor(readSource()) && !host.available(),
}

// Loads the chosen source, its progress on the console and in the Sync sheet.
function loadModelOnRequest() {
  const s = readSource()
  if (reasonFor(s) || loading || host.available()) return
  loading = true
  const mine = epoch
  const say = (text: string, failed = false) => {
    if (mine !== epoch) return
    console.info('[jazyk model]', text)
    useApp.getState().setModelLoad({ text, failed })
  }
  const start = s.kind === 'chrome'
    ? (say('Starting the built-in model…'), builtInModel((t) => say(t)))
    : (say(`Loading ${webllmModelOf(s)}…`), loadModel(webllmModelOf(s), (p) => say(p.text || `${Math.round(p.progress * 100)}%`)))
  start
    .then((m) => {
      if (mine !== epoch) return
      host = m
      loading = false
      rememberLoaded(s)
      useApp.getState().setModelLoad(null)
      // The descriptor changed: a build is on offer now.
      qc.invalidateQueries({ queryKey: ['capabilities'] })
    })
    .catch((e) => {
      if (mine !== epoch) return
      loading = false
      say(`The model failed to load: ${e instanceof Error ? e.message : String(e)}`, true)
    })
}

// The bundled projects (docs/frontends/wasm.md#the-demo): one fixture per example,
// packed by scripts/pack-demos.sh, fetched when picked rather than carried in the
// page.
async function fetchDemo(id: string): Promise<{ files: Record<string, string>; name: string }> {
  const demo = DEMOS.find((d) => d.id === id)
  if (!demo) throw new Error(`no demo named ${id}`)
  const res = await fetch(`demos/${id}.json`)
  if (!res.ok) throw new Error(`${demo.name}: the fixture failed to load (${res.status})`)
  const files = (await res.json()) as Record<string, string>
  return { files, name: demo.name }
}

async function boot() {
  // Whether this browser can keep projects: without storage a demo's edits stay in
  // the tab and no project can be started here.
  const stored = await probeStorage()
  const backend = await startWasmBackend({
    persistence: () => (stored ? 'browser' : 'memory'),
    model,
    // The trace reaches the app as job.trace through onEvent; the console gets it too.
    onTrace: (events, jobId) => {
      for (const e of events) console.info(`[jazyk trace] job ${jobId}`, JSON.stringify(e))
    },
    // A chat turn's updates and a job's lifecycle, as the event stream would carry them.
    onEvent: (event) => pushWireEvent(qc, event),
    onWrite: () => refreshAfterWrite(qc),
  })
  setBackend(backend)
  // There is no stream to drop: the backend is in this tab.
  useApp.getState().setConnected(true)
  // The backend answers the same envelope over the console, which is how a person
  // (or a test) pokes at it without the app in the way.
  ;(window as unknown as { jazyk: unknown }).jazyk = backend

  // `stay` keeps the route: a demo reset to its original reopens where it was.
  const opened = async (p: WasmProject, stay = false) => {
    await backend.open(p)
    // A page that stays mounted refetches what it shows; everything else is dropped.
    if (stay) void qc.resetQueries()
    else qc.clear()
    // The conversations and the jobs belonged to the project the tab had open before.
    useApp.setState({ chatSessions: {}, chatUpdates: {}, chatSelected: null, jobs: {}, trace: [], turns: {}, docModes: {}, editorLine: null })
    if (!stay) window.location.hash = '#/docs'
  }

  // Demos, projects started in the browser, and folders, with where their changes
  // go (docs/frontends/wasm.md#browser-storage).
  installLocalProjects({
    backend,
    opened,
    closed: () => {
      qc.clear()
      useApp.setState({ chatSessions: {}, chatUpdates: {}, chatSelected: null, jobs: {}, trace: [], turns: {}, docModes: {}, editorLine: null })
      window.location.hash = '#/'
    },
    refresh: () => qc.invalidateQueries({ queryKey: ['capabilities'] }),
    fetchDemo,
    demoName: (id) => DEMOS.find((d) => d.id === id)?.name ?? id,
  })

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

  window.addEventListener('jazyk:load-model', loadModelOnRequest)
  // The Sync sheet and Settings choose the source; the choice is already stored.
  window.addEventListener('jazyk:model-source-changed', applySource)
  applySource()
  // A source loaded here before loads again without asking; the weights are cached.
  if (wasLoaded(readSource())) loadModelOnRequest()
}

boot().catch((e) => {
  root.render(<Splash lines={['the backend failed to start', String(e)]} />)
})
