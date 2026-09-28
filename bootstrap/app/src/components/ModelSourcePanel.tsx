// The model source chooser for the browser editor (docs/frontends/wasm.md#model-sources):
// a downloaded model, the browser's built-in model, or your own endpoint. The choice is
// kept in this browser's local storage only.
import { useEffect, useState } from 'react'
import { useCapabilities } from '../lib/capabilities'
import { builtInAvailability, endpointReady, sourceTier, useModelSource, webllmModelOf, writeSource, type SourceKind } from '../lib/model-source'
import { useApp } from '../lib/store'
import { MODEL_CHOICES, TIER_WORDS, downloadedModels, modelName, tier, webgpuAvailable } from '../lib/webllm'

export default function ModelSourcePanel() {
  const src = useModelSource()
  const caps = useCapabilities()
  const load = useApp((a) => a.modelLoad)
  const [builtIn, setBuiltIn] = useState<string>('checking')
  const [baseUrl, setBaseUrl] = useState(src.baseUrl ?? '')
  const [model, setModel] = useState(src.model ?? '')
  const [apiKey, setApiKey] = useState(src.apiKey ?? '')
  // The models already in this browser's cache, marked in the list
  // (docs/frontends/wasm.md#model-sources).
  const [downloaded, setDownloaded] = useState<Set<string>>(() => new Set())
  useEffect(() => {
    void builtInAvailability().then(setBuiltIn)
    if (webgpuAvailable()) void downloadedModels().then(setDownloaded)
  }, [])

  const ready = caps.data?.model === 'in-page'
  const loadable = caps.data?.model === 'loadable'
  const choose = (kind: SourceKind) => writeSource({ ...src, kind, baseUrl, model, apiKey })
  const chosen = MODEL_CHOICES.find((m) => m.id === webllmModelOf(src)) ?? MODEL_CHOICES[0]
  const saveEndpoint = () => writeSource({ ...src, kind: 'endpoint', baseUrl: baseUrl.trim(), model: model.trim(), apiKey: apiKey.trim() })
  const endpointDirty = baseUrl !== (src.baseUrl ?? '') || model !== (src.model ?? '') || apiKey !== (src.apiKey ?? '')

  const builtInNote =
    builtIn === 'absent'
      ? 'Not in this browser. It needs Chrome with the built-in AI (Prompt API).'
      : builtIn === 'unavailable'
        ? 'This browser has it, but not on this device.'
        : builtIn === 'downloadable' || builtIn === 'downloading'
          ? 'Chrome has it but not its weights yet: the first use downloads a few gigabytes once, then it loads at once. Its context is small: questions only, no sync.'
          : builtIn === 'available'
            ? 'Ready on this device. Its context is small: questions only, no sync.'
            : 'Checking…'

  const option = (kind: SourceKind, title: string, note: string, disabled: boolean) => (
    <label className={`msrc-option${src.kind === kind ? ' on' : ''}${disabled ? ' off' : ''}`}>
      <input type="radio" name="model-source" checked={src.kind === kind} disabled={disabled} onChange={() => choose(kind)} />
      <span>
        <b>{title}</b>
        <span className="msrc-note">{note}</span>
      </span>
    </label>
  )

  return (
    <div className="msrc">
      <p className="why">Syncs and chat run a model. Choose where it comes from.</p>
      {option(
        'webllm',
        'Download a model',
        webgpuAvailable()
          ? `${chosen.name} from Hugging Face, about ${chosen.gb} GB, run on this computer. Downloaded once; the browser keeps it.`
          : 'Not in this browser: it needs WebGPU (Chrome, Edge, or Safari 26).',
        !webgpuAvailable(),
      )}
      {src.kind === 'webllm' && webgpuAvailable() && (
        <div className="msrc-form">
          <label>
            Model
            <select value={chosen.id} onChange={(e) => writeSource({ ...src, webllmModel: e.target.value })} aria-label="Downloaded model">
              {MODEL_CHOICES.map((m) => (
                <option key={m.id} value={m.id}>
                  {m.name} · {m.gb} GB · {TIER_WORDS[tier(m.params)]}
                  {downloaded.has(m.id) ? ' · downloaded' : ''}
                </option>
              ))}
            </select>
          </label>
          <p className="muted">
            Strongest first. 7B and up syncs and chats; 3B chats and syncs rough; under 3B answers questions only. A bigger model downloads longer, and 7B and up needs several
            gigabytes free beside the weights.
          </p>
        </div>
      )}
      {option('chrome', "The browser's built-in model", builtInNote, builtIn === 'absent' || builtIn === 'unavailable')}
      {option('endpoint', 'Your endpoint', 'Any OpenAI-compatible API: a local server, LocalRouter, or a provider.', false)}

      {src.kind === 'endpoint' && (
        <div className="msrc-form">
          <label>
            Base URL
            <input value={baseUrl} onChange={(e) => setBaseUrl(e.target.value)} placeholder="https://api.openai.com/v1" spellCheck={false} />
          </label>
          <label>
            Model
            <input value={model} onChange={(e) => setModel(e.target.value)} placeholder="gpt-5.5" spellCheck={false} />
          </label>
          <label>
            API key <span className="muted">(optional)</span>
            <input type="password" value={apiKey} onChange={(e) => setApiKey(e.target.value)} autoComplete="off" />
          </label>
          <p className="row">
            <button onClick={saveEndpoint} disabled={!endpointDirty && endpointReady(src)}>
              Save
            </button>
            <span className="muted">The endpoint must allow calls from this page (CORS).</span>
          </p>
        </div>
      )}

      {ready && (
        <p className="msrc-status v-ok">
          Ready:{' '}
          {src.kind === 'endpoint'
            ? `${src.model} at ${src.baseUrl}`
            : `${src.kind === 'chrome' ? "the browser's built-in model" : modelName(webllmModelOf(src))}, ${sourceTier(src)} tier (${TIER_WORDS[sourceTier(src)]})`}
          .
        </p>
      )}
      {load && <p className={`msrc-status${load.failed ? ' v-bad' : ''}`}>{load.text}</p>}
      {loadable && src.kind !== 'endpoint' && (
        <p className="row">
          <button className="run" disabled={!!load && !load.failed} onClick={() => window.dispatchEvent(new CustomEvent('jazyk:load-model'))}>
            {load && !load.failed ? 'Loading…' : 'Load the model'}
          </button>
        </p>
      )}

      <p className="msrc-privacy muted">
        Your choice is kept in this browser's local storage. Nothing is saved on a jazyk server, and the key is sent only to your endpoint.
      </p>
    </div>
  )
}
