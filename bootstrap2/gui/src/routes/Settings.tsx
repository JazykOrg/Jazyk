// Settings: jazyk.toml as a form. Null means the key is unset in the file and the
// effective value is the default. Saving rewrites the file canonically and applies live.
import { useState } from 'react'
import { keepPreviousData, useQuery, useQueryClient } from '@tanstack/react-query'
import { get, put } from '../lib/api'
import { useProject } from '../lib/queries'
import './routes.css'

const LIMIT_KEYS = [
  'turnRounds',
  'turnMutations',
  'contextBudget',
  'buildTurnFactor',
  'maxSectionChars',
  'maxDocSections',
  'maxEntityRequirements',
] as const
type LimitKey = (typeof LIMIT_KEYS)[number]

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
    limits: Record<LimitKey, number | null>
  }
  defaults: {
    docsGlob: string[]
    deliverable: string
    limits: Record<LimitKey, number>
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
  limits: Record<LimitKey, string>
}

function initForm(p: SettingsPayload): Form {
  const limits = {} as Record<LimitKey, string>
  for (const k of LIMIT_KEYS) limits[k] = p.settings.limits[k] === null ? '' : String(p.settings.limits[k])
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
    limits,
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

export default function Settings() {
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
    const limits: Record<string, number | null> = {}
    for (const k of LIMIT_KEYS) limits[k] = f.limits[k].trim() === '' ? null : Number(f.limits[k])
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
          limits,
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

      <h2>Generation</h2>
      <label>
        deliverable directory
        <input
          type="text"
          value={f.deliverable}
          placeholder={data.defaults.deliverable}
          onChange={(e) => set({ deliverable: e.target.value })}
        />
      </label>

      <h2>LLM</h2>
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

      <h2>Limits</h2>
      <div className="settings-grid">
        {LIMIT_KEYS.map((k) => (
          <label key={k}>
            {k}
            <input
              type="number"
              min={1}
              value={f.limits[k]}
              placeholder={String(data.defaults.limits[k])}
              onChange={(e) => set({ limits: { ...f.limits, [k]: e.target.value } })}
            />
          </label>
        ))}
      </div>
      <p className="muted">empty means unset; the placeholder is the effective default</p>

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
