// The model source chooser for the browser editor (docs/frontends/wasm.md#model-sources):
// a downloaded model, the browser's built-in model, or your own endpoint. The choice is
// kept in this browser's local storage only.
import { useEffect, useState } from 'react'
import { useCapabilities } from '../lib/capabilities'
import { builtInAvailability, endpointReady, useModelSource, writeSource, type SourceKind } from '../lib/model-source'
import { useApp } from '../lib/store'
import { DEFAULT_MODEL, webgpuAvailable } from '../lib/webllm'

export default function ModelSourcePanel() {
  const src = useModelSource()
  const caps = useCapabilities()
  const load = useApp((a) => a.modelLoad)
  const [builtIn, setBuiltIn] = useState<string>('checking')
  const [baseUrl, setBaseUrl] = useState(src.baseUrl ?? '')
  const [model, setModel] = useState(src.model ?? '')
  const [apiKey, setApiKey] = useState(src.apiKey ?? '')
  useEffect(() => {
    void builtInAvailability().then(setBuiltIn)
  }, [])

  const ready = caps.data?.model === 'in-page'
  const loadable = caps.data?.model === 'loadable'
  const choose = (kind: SourceKind) => writeSource({ kind, baseUrl, model, apiKey })
  const saveEndpoint = () => writeSource({ kind: 'endpoint', baseUrl: baseUrl.trim(), model: model.trim(), apiKey: apiKey.trim() })
  const endpointDirty = baseUrl !== (src.baseUrl ?? '') || model !== (src.model ?? '') || apiKey !== (src.apiKey ?? '')

  const builtInNote =
    builtIn === 'absent'
      ? 'Not in this browser. It needs Chrome with the built-in AI (Prompt API).'
      : builtIn === 'unavailable'
        ? 'This browser has it, but not on this device.'
        : builtIn === 'downloadable' || builtIn === 'downloading'
          ? 'Chrome downloads it on first use. Its context is small, so a large sync may not fit.'
          : builtIn === 'available'
            ? 'Ready on this device. Its context is small, so a large sync may not fit.'
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
          ? `${DEFAULT_MODEL.split('-q')[0]} from Hugging Face, about 1.7 GB, run on this computer. Downloaded once; the browser keeps it.`
          : 'Not in this browser: it needs WebGPU (Chrome, Edge, or Safari 26).',
        !webgpuAvailable(),
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
          Ready: {src.kind === 'endpoint' ? `${src.model} at ${src.baseUrl}` : src.kind === 'chrome' ? "the browser's built-in model" : DEFAULT_MODEL}.
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
