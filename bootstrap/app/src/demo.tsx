// The demo entry: the same app, backed by the wasm compiler in this tab instead of
// a server. Routes live in the fragment, so the whole demo is one static page and
// needs no server rewriting unknown paths to index.html.
//
// The app mounts at once against a backend with no project, so Home renders the
// picker from the capability descriptor; the bundled project or a folder on the
// computer opens on the events Home dispatches. The model loads behind all of it,
// because every read works without one and only a build needs it.
// See docs/frontends/wasm.md#the-demo.
import React from 'react'
import ReactDOM from 'react-dom/client'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { HashRouter } from 'react-router'
import App from './App'
import { setBackend } from './lib/backend'
import { startWasmBackend, crossOriginIsolated_, type ModelHost, type WasmProject } from './lib/wasm-client'
import { DEFAULT_MODEL, loadModel, unavailableModel, webgpuAvailable } from './lib/webllm'
import { builtInModel, builtInPresent, endpointModel, endpointReady, readSource, type ModelSource } from './lib/model-source'
import { applyTheme, useApp } from './lib/store'
import { pushTraceEvents, pushWireEvent, refreshAfterWrite } from './lib/events'
import { ensurePermission, FolderMirror, pickFolder, readFolder } from './lib/folder'
import { folderHandle, readRecent, rememberFolder, setRecentMode, touchRecent, type FolderMode } from './lib/recent'
import { DEMOS } from './shell/Home'
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
  name: () => (readSource().kind === 'webllm' ? DEFAULT_MODEL : host.name()),
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
    : (say(`Loading ${DEFAULT_MODEL}…`), loadModel(DEFAULT_MODEL, (p) => say(p.text || `${Math.round(p.progress * 100)}%`)))
  start
    .then((m) => {
      if (mine !== epoch) return
      host = m
      loading = false
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
async function fetchDemo(id: string): Promise<WasmProject> {
  const demo = DEMOS.find((d) => d.id === id)
  if (!demo) throw new Error(`no demo named ${id}`)
  const res = await fetch(`demos/${id}.json`)
  if (!res.ok) throw new Error(`${demo.name}: the fixture failed to load (${res.status})`)
  const files = (await res.json()) as Record<string, string>
  return { files, name: demo.name }
}

async function boot() {
  const backend = await startWasmBackend({
    model,
    onTrace: (events) => {
      for (const e of events) console.info('[jazyk trace]', JSON.stringify(e))
      pushTraceEvents(qc, events)
    },
    // A chat turn's updates, as the event stream would carry them.
    onEvent: (event) => pushWireEvent(qc, event),
    onWrite: () => refreshAfterWrite(qc),
  })
  setBackend(backend)
  // There is no stream to drop: the backend is in this tab.
  useApp.getState().setConnected(true)
  // The backend answers the same envelope over the console, which is how a person
  // (or a test) pokes at it without the app in the way.
  ;(window as unknown as { jazyk: unknown }).jazyk = backend

  const opened = async (p: WasmProject) => {
    await backend.open(p)
    qc.clear()
    // The conversations belonged to the project the tab had open before.
    useApp.setState({ chatSessions: {}, chatUpdates: {}, chatSelected: null })
    window.location.hash = '#/docs'
  }

  // The folder open in this tab, if any: its mirror and its recent-list id.
  let folder: { id: string; mirror: FolderMirror } | null = null
  const publish = () => {
    useApp.getState().setFolder(
      folder ? { name: folder.mirror.root.name, mode: folder.mirror.currentMode, held: folder.mirror.heldCount } : null,
    )
  }
  const fail = (what: string, err: unknown) => {
    console.warn(`[jazyk ${what}]`, err)
    window.alert(err instanceof Error ? err.message : String(err))
  }

  const openDemo = async (id: string) => {
    const p = await fetchDemo(id)
    folder = null
    publish()
    touchRecent({ id: `demo:${id}`, kind: 'demo', name: p.name })
    await opened(p)
  }

  const openFolder = async (id: string, handle: FileSystemDirectoryHandle, mode: FolderMode) => {
    const files = await readFolder(handle as Parameters<typeof readFolder>[0])
    if (!('jazyk.toml' in files)) {
      window.alert(`${handle.name} has no jazyk.toml at its top; pick a project directory.`)
      return
    }
    const mirror = new FolderMirror(handle as ConstructorParameters<typeof FolderMirror>[0], '/project', mode)
    mirror.onChange = publish
    folder = { id, mirror }
    publish()
    touchRecent({ id, kind: 'folder', name: handle.name, mode })
    await opened({ files, name: handle.name, mirror })
  }

  window.addEventListener('jazyk:open-demo', (e) => {
    openDemo(String((e as CustomEvent).detail ?? '')).catch((err) => fail('demo', err))
  })

  // `detail.mode` is the read-only choice beside the button.
  window.addEventListener('jazyk:pick-folder', async (e) => {
    const mode: FolderMode = (e as CustomEvent).detail?.mode === 'write' ? 'write' : 'read'
    let handle: FileSystemDirectoryHandle
    try {
      handle = await pickFolder(mode)
    } catch (err) {
      // The picker was dismissed, or refused: nothing to do.
      console.info('[jazyk folder]', err)
      return
    }
    try {
      await openFolder(await rememberFolder(handle), handle, mode)
    } catch (err) {
      fail('folder', err)
    }
  })

  // A row of the recent list. The click is the gesture the permission prompt needs.
  window.addEventListener('jazyk:open-recent', async (e) => {
    const id = String((e as CustomEvent).detail ?? '')
    const entry = readRecent().find((r) => r.id === id)
    if (!entry) return
    try {
      if (entry.kind === 'demo') return await openDemo(id.slice('demo:'.length))
      const handle = await folderHandle(id)
      if (!handle) {
        window.alert(`${entry.name} is no longer remembered by this browser; open it with Open folder.`)
        return
      }
      const mode = entry.mode ?? 'read'
      if (!(await ensurePermission(handle, mode))) return
      await openFolder(id, handle, mode)
    } catch (err) {
      fail('folder', err)
    }
  })

  // The top bar's switch and the editor's prompt. Writing asks for write access,
  // then lands everything held.
  window.addEventListener('jazyk:folder-mode', async (e) => {
    const mode: FolderMode = (e as CustomEvent).detail === 'write' ? 'write' : 'read'
    if (!folder) return
    const { id, mirror } = folder
    try {
      if (mode === 'write') {
        if (!(await ensurePermission(mirror.root, 'write'))) return
        await mirror.startWriting()
      } else {
        mirror.readOnly()
      }
      setRecentMode(id, mode)
      publish()
    } catch (err) {
      fail('folder', err)
    }
  })

  // Held changes live only in this tab: closing it asks first.
  window.addEventListener('beforeunload', (e) => {
    if (folder && folder.mirror.heldCount > 0) e.preventDefault()
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
}

boot().catch((e) => {
  root.render(<Splash lines={['the backend failed to start', String(e)]} />)
})
