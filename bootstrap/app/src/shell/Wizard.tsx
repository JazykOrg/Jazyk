// The new-project wizard (docs/frontends/gui.md#home): a served folder with no
// jazyk.toml yet. Pick the documents glob, optionally the product folder, the
// root documents, and the model; the wizard writes jazyk.toml through the settings
// endpoint, names the shape it detected, and runs the first build.
import { useState } from 'react'
import { useNavigate } from 'react-router'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { get, put } from '../lib/api'
import { useCapabilities } from '../lib/capabilities'

interface SettingsPayload {
  exists: boolean
  hash: string
  settings: {
    docsGlob: string[] | null
    roots: string[] | null
    deliverable: string | null
    llm: { baseUrl: string | null; model: string | null; apiKeyEnv: string | null; apiKeySet: boolean }
  }
  defaults: { docsGlob: string[]; deliverable: string }
}

export function useSettingsExists() {
  return useQuery({
    queryKey: ['settings'],
    queryFn: () => get<SettingsPayload>('/api/settings'),
    staleTime: 5_000,
    retry: 0,
  })
}

export default function Wizard({ payload, root }: { payload: SettingsPayload; root: string }) {
  const qc = useQueryClient()
  const navigate = useNavigate()
  const caps = useCapabilities()
  // In a tab the page's model source stands: no model fields, no release
  // (docs/frontends/gui.md#the-setup-wizard).
  const tab = caps.data !== undefined && caps.data.backend !== 'server'
  const [glob, setGlob] = useState((payload.defaults.docsGlob ?? ['**/*.md']).join('\n'))
  const [deliverable, setDeliverable] = useState('')
  const [roots, setRoots] = useState('')
  const [model, setModel] = useState(payload.settings.llm.model ?? '')
  const [baseUrl, setBaseUrl] = useState(payload.settings.llm.baseUrl ?? '')
  const [busy, setBusy] = useState<string | null>(null)
  const [err, setErr] = useState<string | null>(null)

  const lines = (t: string) => t.split('\n').map((l) => l.trim()).filter((l) => l.length > 0)
  const shape = deliverable.trim() ? (lines(glob).length > 0 ? 'documents and product' : 'product only: start by documenting it') : 'documents only'

  const save = async () => {
    setBusy('save')
    setErr(null)
    try {
      await put('/api/settings', {
        baseHash: payload.hash,
        settings: {
          docsGlob: lines(glob),
          roots: lines(roots),
          deliverable: deliverable.trim() || null,
          llm: { baseUrl: baseUrl.trim() || null, model: model.trim() || null, apiKeyEnv: null, temperature: null },
          linting: { warnings: [], errors: [] },
          executors: {},
        },
      })
      // The folder is a project now: the documents lens lists what the glob
      // matched, and the action button runs the first build
      // (docs/frontends/gui.md#the-setup-wizard).
      void qc.invalidateQueries()
      navigate('/docs')
    } catch (e) {
      setErr((e as Error).message)
    } finally {
      setBusy(null)
    }
  }

  return (
    <div className="wizard">
      <h2>Set up this project</h2>
      <p className="muted">
        <span className="mono">{root}</span> has no <span className="mono">jazyk.toml</span> yet. Say where the documents are, and where the product is if there is one.
      </p>
      <div className="settings-grid">
        <label>
          documents (one glob per line)
          <textarea rows={3} value={glob} onChange={(e) => setGlob(e.target.value)} />
        </label>
        <label>
          product folder (optional)
          <input type="text" placeholder="e.g. src, product, or . for the whole folder" value={deliverable} onChange={(e) => setDeliverable(e.target.value)} />
        </label>
        <label>
          root documents (optional, one per line)
          <textarea rows={2} placeholder="docs/main.md" value={roots} onChange={(e) => setRoots(e.target.value)} />
        </label>
        {!tab && (
          <>
            <label>
              model (optional; the global config's model otherwise)
              <input type="text" value={model} onChange={(e) => setModel(e.target.value)} />
            </label>
            <label>
              model base url (optional)
              <input type="text" value={baseUrl} onChange={(e) => setBaseUrl(e.target.value)} />
            </label>
          </>
        )}
      </div>
      <p>
        Shape: <b>{shape}</b>
      </p>
      {err && <p className="error-inline">{err}</p>}
      <div className="actions">
        <button className="primary" disabled={busy !== null} onClick={() => void save()}>
          {busy === 'save' ? 'Writing…' : 'Write jazyk.toml'}
        </button>
      </div>
    </div>
  )
}
