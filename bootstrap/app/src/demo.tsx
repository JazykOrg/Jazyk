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
import { applyTheme, useApp } from './lib/store'
import { pushTraceEvents, refreshAfterWrite } from './lib/events'
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

// The model, once. Until it resolves a build is not offered; if it fails, a build
// is refused with the reason rather than hanging.
let host: ModelHost = unavailableModel('the model is still loading')
let reason: string | null = null
if (!crossOriginIsolated_()) {
  reason = 'this page is not cross-origin isolated, so the backend cannot call a model (COOP/COEP headers are required for SharedArrayBuffer)'
} else if (!webgpuAvailable()) {
  reason = 'this browser has no WebGPU, so there is no model to call'
}
if (reason) host = unavailableModel(reason)

// The bridge hands every call to whatever `host` is at the time of the call, so the
// backend can start before the model has finished loading.
const model: ModelHost = {
  name: () => DEFAULT_MODEL,
  chat: (body) => host.chat(body),
  available: () => host.available(),
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
      // The descriptor changed: a build is on offer now.
      qc.invalidateQueries({ queryKey: ['capabilities'] })
    })
    .catch((e) => {
      const msg = e instanceof Error ? e.message : String(e)
      host = unavailableModel(`the model failed to load: ${msg}`)
      say(`the model failed to load: ${msg}`)
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

  loadModelInBackground((l) => console.info('[jazyk model]', l))
}

boot().catch((e) => {
  root.render(<Splash lines={['the backend failed to start', String(e)]} />)
})
