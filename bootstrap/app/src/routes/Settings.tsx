// Settings: jazyk.toml as a form. Null means the key is unset in the file and the
// effective value is the default. Saving rewrites the file canonically and applies
// live. Limits are a built-in registry, not settings; the executors table routes
// goal kinds and classes to agent profiles (docs/compiler/project-settings.md#executors).
import ModelSourcePanel from '../components/ModelSourcePanel'
import { useState } from 'react'
import { keepPreviousData, useQuery, useQueryClient } from '@tanstack/react-query'
import { get, put } from '../lib/api'
import { useProject } from '../lib/queries'
import { useCapabilities } from '../lib/capabilities'
import { useApp } from '../lib/store'
import { useWords } from '../lib/words'
import './routes.css'

interface SettingsPayload {
  exists: boolean
  hash: string
  redirect: string | null
  unknown: string[]
  settings: {
    docsGlob: string[] | null
    roots: string[] | null
    deliverable: string | null
    llm: {
      baseUrl: string | null
      model: string | null
      apiKeyEnv: string | null
      temperature: number | null
      apiKeySet: boolean
    }
    linting: { warnings: string[]; errors: string[] }
    executors: Record<string, string>
  }
  executorKeys: string[]
  unknownExecutors?: string[]
  defaults: {
    docsGlob: string[]
    deliverable: string
  }
}

// Everything held as strings while editing; parsed on save. '' = unset.
interface Form {
  docsGlob: string[]
  roots: string[]
  deliverable: string
  baseUrl: string
  model: string
  apiKeyEnv: string
  temperature: string
  warnings: string
  errors: string
  executors: { key: string; value: string }[]
}

function initForm(p: SettingsPayload): Form {
  return {
    docsGlob: p.settings.docsGlob ?? [],
    roots: p.settings.roots ?? [],
    deliverable: p.settings.deliverable ?? '',
    baseUrl: p.settings.llm.baseUrl ?? '',
    model: p.settings.llm.model ?? '',
    apiKeyEnv: p.settings.llm.apiKeyEnv ?? '',
    temperature: p.settings.llm.temperature === null ? '' : String(p.settings.llm.temperature),
    warnings: p.settings.linting.warnings.join('\n'),
    errors: p.settings.linting.errors.join('\n'),
    executors: Object.entries(p.settings.executors ?? {}).map(([key, value]) => ({ key, value })),
  }
}

const lines = (text: string) => text.split('\n').map((l) => l.trim()).filter((l) => l.length > 0)
const cleaned = (list: string[]) => list.map((s) => s.trim()).filter((s) => s.length > 0)

function ListEditor({
  items,
  onChange,
  fallback,
}: {
  items: string[]
  onChange: (v: string[]) => void
  fallback?: string[]
}) {
  return (
    <div>
      {items.map((v, i) => (
        <div key={i} className="listrow">
          <input
            type="text"
            value={v}
            onChange={(e) => onChange(items.map((x, j) => (j === i ? e.target.value : x)))}
          />
          <button onClick={() => onChange(items.filter((_, j) => j !== i))}>✕</button>
        </div>
      ))}
      {items.length === 0 && fallback && fallback.length > 0 && (
        <p className="muted mono" style={{ margin: '4px 0' }}>default: {fallback.join('  ')}</p>
      )}
      <button onClick={() => onChange([...items, ''])}>add</button>
    </div>
  )
}

// The backend kind and what it can do (docs/frontends/gui.md#settings).
function Connection() {
  const caps = useCapabilities()
  const c = caps.data
  if (!c) return <p className="muted">reading the backend's capabilities…</p>
  const yes = (v: boolean) => (v ? 'yes' : 'no')
  return (
    <div className="settings-grid">
      <p style={{ margin: '2px 0' }}>
        <b>backend</b> <span className="mono">{c.backend}</span>
      </p>
      <p style={{ margin: '2px 0' }}>
        <b>documents</b> <span className="mono">{c.docs}</span>
      </p>
      <p style={{ margin: '2px 0' }}>
        <b>live updates</b> <span className="mono">{c.events}</span>
      </p>
      <p style={{ margin: '2px 0' }}>
        <b>language server</b> <span className="mono">{yes(c.lsp)}</span>
      </p>
      <p style={{ margin: '2px 0' }}>
        <b>can run</b> <span className="mono">{c.jobs.join(', ') || 'nothing'}</span>
      </p>
      <p style={{ margin: '2px 0' }}>
        <b>agent</b> <span className="mono">{c.agent}</span>
      </p>
      <p style={{ margin: '2px 0' }}>
        <b>model</b> <span className="mono">{c.model}</span>
      </p>
      <p style={{ margin: '2px 0' }}>
        <b>renders diagrams</b> <span className="mono">{c.rendering}</span>
      </p>
      <p style={{ margin: '2px 0' }}>
        <b>persistence</b> <span className="mono">{c.persistence}</span>
      </p>
    </div>
  )
}

function ProjectSettings() {
  const qc = useQueryClient()
  const project = useProject()
  const q = useQuery({
    queryKey: ['settings'],
    queryFn: () => get<SettingsPayload>('/api/settings'),
    placeholderData: keepPreviousData,
    staleTime: 5_000,
  })

  // The form edits against the payload it was initialized from (baseHash included),
  // so a background refetch never clobbers in-progress edits.
  const [form, setForm] = useState<Form | null>(null)
  const [baseline, setBaseline] = useState<Form | null>(null)
  const [baseHash, setBaseHash] = useState('')
  const [saveError, setSaveError] = useState<string | null>(null)
  const [saving, setSaving] = useState(false)

  if (q.error)
    return (
      <p className="error-inline">
        {q.error.message}{' '}
        <a href="#retry" onClick={(e) => { e.preventDefault(); q.refetch() }}>retry</a>
      </p>
    )
  if (!q.data) return <p className="muted">loading…</p>
  const data = q.data

  const reset = (p: SettingsPayload) => {
    const f = initForm(p)
    setForm(f)
    setBaseline(f)
    setBaseHash(p.hash)
    setSaveError(null)
  }
  if (form === null || baseline === null) {
    reset(data)
    return <p className="muted">loading…</p>
  }
  const f = form
  const dirty = JSON.stringify(form) !== JSON.stringify(baseline)
  const blocked = data.unknown.length > 0
  const set = (patch: Partial<Form>) => setForm({ ...f, ...patch })

  const save = async () => {
    setSaving(true)
    setSaveError(null)
    const executors: Record<string, string> = {}
    for (const { key, value } of f.executors) {
      if (key.trim() && value.trim()) executors[key.trim()] = value.trim()
    }
    try {
      const fresh = await put<SettingsPayload>('/api/settings', {
        baseHash,
        settings: {
          docsGlob: cleaned(f.docsGlob),
          roots: cleaned(f.roots),
          deliverable: f.deliverable.trim() || null,
          llm: {
            baseUrl: f.baseUrl.trim() || null,
            model: f.model.trim() || null,
            apiKeyEnv: f.apiKeyEnv.trim() || null,
            temperature: f.temperature.trim() === '' ? null : Number(f.temperature),
          },
          linting: { warnings: lines(f.warnings), errors: lines(f.errors) },
          executors,
        },
      })
      qc.setQueryData(['settings'], fresh)
      reset(fresh)
    } catch (e) {
      setSaveError((e as Error).message)
    } finally {
      setSaving(false)
    }
  }

  const reload = (e: React.MouseEvent) => {
    e.preventDefault()
    q.refetch().then((r) => {
      if (r.data) reset(r.data)
    })
  }

  const executorKeys = data.executorKeys ?? []
  const setExec = (i: number, patch: Partial<{ key: string; value: string }>) =>
    set({ executors: f.executors.map((x, j) => (j === i ? { ...x, ...patch } : x)) })

  return (
    <div className="settings">
      <h1>Settings</h1>
      {project.data && (
        <p className="muted mono">{project.data.root}/jazyk.toml</p>
      )}
      {!data.exists && <p className="muted">jazyk.toml does not exist yet; saving creates it</p>}
      {data.redirect && (
        <p className="muted">
          this jazyk.toml redirects to <span className="mono">{data.redirect}</span>
        </p>
      )}
      {blocked && (
        <p className="error-inline">
          jazyk.toml holds keys this form does not know ({data.unknown.join(', ')}); edit the file
          directly
        </p>
      )}

      <h2>Documents</h2>
      <ListEditor
        items={f.docsGlob}
        onChange={(v) => set({ docsGlob: v })}
        fallback={data.defaults.docsGlob}
      />
      <p className="muted">later patterns override earlier ones; a leading ! excludes</p>

      <h2>Lint rules</h2>
      <label>
        warnings (one plain-English rule per line)
        <textarea rows={4} value={f.warnings} onChange={(e) => set({ warnings: e.target.value })} />
      </label>
      <label>
        errors
        <textarea rows={4} value={f.errors} onChange={(e) => set({ errors: e.target.value })} />
      </label>

      <h2>Roots</h2>
      <ListEditor items={f.roots} onChange={(v) => set({ roots: v })} />
      <p className="muted">files that seed the link-graph schedule</p>

      <h2>Product</h2>
      <label>
        deliverable directory
        <input
          type="text"
          value={f.deliverable}
          placeholder={data.defaults.deliverable}
          onChange={(e) => set({ deliverable: e.target.value })}
        />
      </label>

      <h2>Model and agent</h2>
      <h3>Executors</h3>
      <p className="muted">
        which agent profile runs each goal kind or class; unset kinds fall back to the
        class, then the [acp] agent
      </p>
      {(data.unknownExecutors ?? []).length > 0 && (
        <p className="error-inline">
          unknown executor keys in the file: {(data.unknownExecutors ?? []).join(', ')}
        </p>
      )}
      {f.executors.map((row, i) => (
        <div key={i} className="listrow">
          <select value={row.key} onChange={(e) => setExec(i, { key: e.target.value })}>
            <option value="">(pick a kind or class)</option>
            {executorKeys.map((k) => (
              <option key={k} value={k}>
                {k}
              </option>
            ))}
          </select>
          <input
            type="text"
            placeholder="agent profile"
            value={row.value}
            onChange={(e) => setExec(i, { value: e.target.value })}
          />
          <button onClick={() => set({ executors: f.executors.filter((_, j) => j !== i) })}>✕</button>
        </div>
      ))}
      <button onClick={() => set({ executors: [...f.executors, { key: '', value: '' }] })}>add</button>

      <h3>Model</h3>
      <div className="settings-grid">
        <label>
          base url
          <input type="text" value={f.baseUrl} onChange={(e) => set({ baseUrl: e.target.value })} />
        </label>
        <label>
          model
          <input type="text" value={f.model} onChange={(e) => set({ model: e.target.value })} />
        </label>
        <label>
          api key env var
          <input type="text" value={f.apiKeyEnv} onChange={(e) => set({ apiKeyEnv: e.target.value })} />
        </label>
        <label>
          temperature
          <input
            type="number"
            step={0.1}
            value={f.temperature}
            onChange={(e) => set({ temperature: e.target.value })}
          />
        </label>
      </div>
      <p className="muted">
        {data.settings.llm.apiKeySet
          ? 'api key: set in the file (carried over on save)'
          : 'api key: not in the file; prefer the environment or api_key_env'}
      </p>
      <p className="muted">
        session and build limits are a built-in registry, not settings; a node's own
        threshold is raised from its board card (dismiss)
      </p>

      <h2>Connection</h2>
      <Connection />

      <div className="actionrow">
        <button disabled={!dirty || blocked || saving} onClick={save}>
          save
        </button>
        {dirty && (
          <a href="#revert" onClick={(e) => { e.preventDefault(); setForm(baseline) }}>
            revert
          </a>
        )}
        {saveError && (
          <span className="error-inline" style={{ margin: 0 }}>
            {saveError}{' '}
            <a href="#reload" onClick={reload}>reload settings</a>
          </span>
        )}
      </div>
    </div>
  )
}

// The workflow modes (docs/frontends/gui.md#workflow-modes): control-plane state,
// shared with every agent, here as Build on save or on request and Generate after build
// or on request. A tab keeps them in its own control plane and generates nothing, so
// it shows the Build switch alone (docs/frontends/wasm.md#workflow-modes-in-a-tab).
function Workflow({ browser }: { browser: boolean }) {
  const w = useWords()
  const watchMode = useApp((a) => a.watchMode)
  const genMode = useApp((a) => a.genMode)
  const manual = watchMode !== 'watch'
  const [error, setError] = useState<string | null>(null)
  const set = (body: Record<string, string>) => {
    setError(null)
    put('/api/watch', body).catch((e) => setError(e instanceof Error ? e.message : String(e)))
  }
  return (
    <section className="settings-workflow">
      <h1>Workflow</h1>
      <div className="settings-grid">
        <label title={browser ? 'On save: a build starts a moment after a save or an answer (spends model time). On request: changes wait until you run a build.' : 'On save: every saved change builds (spends model budget). On request: changes wait in the Inbox until you run a build. Shared with attached agents.'}>
          Build
          <select aria-label="Build" value={manual ? 'queue' : 'watch'} onChange={(e) => set({ mode: e.target.value })}>
            <option value="queue">{w.buildOnRequest}</option>
            <option value="watch">{w.buildOnSave}</option>
          </select>
        </label>
        {!browser && (
          <label title="After build: a finished build with entities to generate queues a generate run. On request: generating is a click.">
            Generate
            <select value={genMode} onChange={(e) => set({ gen: e.target.value })}>
              <option value="manual">{w.generateOnRequest}</option>
              <option value="auto">{w.generateAfterBuild}</option>
            </select>
          </label>
        )}
      </div>
      {error && <p className="error-inline">{error}</p>}
      <p className="muted">
        {browser ? 'Building on save spends model time, so it is on request unless changed.' : 'Automatic modes spend model budget, so both are on request unless changed.'}
      </p>
    </section>
  )
}

// In the browser the model is this browser's choice, not the project's: it leads the
// page (docs/frontends/wasm.md#model-sources).
export default function Settings() {
  const caps = useCapabilities()
  const browser = !!caps.data && caps.data.backend !== 'server'
  return (
    <>
      {browser && (
        <section className="settings-model">
          <h1>Model</h1>
          <ModelSourcePanel />
        </section>
      )}
      <ProjectSettings />
      <Workflow browser={browser} />
    </>
  )
}
