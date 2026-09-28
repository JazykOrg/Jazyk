// The model source chooser for the browser editor (docs/frontends/wasm.md#model-sources):
// a downloaded model, the browser's built-in model, or your own endpoint, one line
// each with the explanations behind info icons. The choice is kept in this browser's
// local storage only. In the action button's popover choosing a model loads it
// (docs/frontends/gui.md#the-action-button).
import { useEffect, useId, useState } from 'react'
import { useCapabilities } from '../lib/capabilities'
import { builtInAvailability, endpointReady, sourceTier, useModelSource, webllmModelOf, writeSource, type ModelSource, type SourceKind } from '../lib/model-source'
import { useApp } from '../lib/store'
import { MODEL_CHOICES, TIER_WORDS, downloadedModels, modelName, tier, webgpuAvailable, type ModelTier } from '../lib/webllm'
import InfoTip from './InfoTip'

const loadModel = () => window.dispatchEvent(new CustomEvent('jazyk:load-model'))

// What a loaded model's tier means for a build, the tooltip beside its name.
const TIER_TIPS: Record<ModelTier, string | null> = {
  build: null,
  chat: 'A small model: builds run, and their results are rough. A 7B or larger model, or an endpoint, builds better.',
  reader: 'A build needs a model of 3B or more, or an endpoint. This one answers questions.',
}

export default function ModelSourcePanel({ loadOnChoose = false }: { loadOnChoose?: boolean }) {
  const src = useModelSource()
  const caps = useCapabilities()
  const load = useApp((a) => a.modelLoad)
  // One radio group per chooser: the popover, Settings, and the chat pane can show one each.
  const group = useId()
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
  // A source that loads (downloaded, built in) loads at once when chosen in the
  // popover; an endpoint is ready once saved.
  const pick = (next: ModelSource) => {
    writeSource(next)
    if (loadOnChoose && next.kind !== 'endpoint') loadModel()
  }
  const choose = (kind: SourceKind) => pick({ ...src, kind, baseUrl, model, apiKey })
  const chosen = MODEL_CHOICES.find((m) => m.id === webllmModelOf(src)) ?? MODEL_CHOICES[0]
  const saveEndpoint = () => writeSource({ ...src, kind: 'endpoint', baseUrl: baseUrl.trim(), model: model.trim(), apiKey: apiKey.trim() })
  const endpointDirty = baseUrl !== (src.baseUrl ?? '') || model !== (src.model ?? '') || apiKey !== (src.apiKey ?? '')

  const [builtInNote, builtInTip] =
    builtIn === 'absent'
      ? ['not in this browser', 'It needs Chrome with the built-in AI (Prompt API).']
      : builtIn === 'unavailable'
        ? ['not on this device', 'This browser has it, but not on this device.']
        : builtIn === 'downloadable' || builtIn === 'downloading'
          ? [
              `downloads on first use · ${TIER_WORDS.reader}`,
              'Chrome has it but not its weights yet: the first use downloads a few gigabytes once, then it loads at once. Its context is small: questions only, no build.',
            ]
          : builtIn === 'available'
            ? [`Ready on this device · ${TIER_WORDS.reader}`, 'Its context is small: questions only, no build.']
            : ['checking…', '']

  const option = (kind: SourceKind, title: string, note: string, tip: string, disabled: boolean) => (
    <label className={`msrc-option${src.kind === kind ? ' on' : ''}${disabled ? ' off' : ''}`}>
      <input type="radio" name={group} checked={src.kind === kind} disabled={disabled} onChange={() => choose(kind)} />
      <b>{title}</b>
      <span className="msrc-note">{note}</span>
      {tip && <InfoTip text={tip} />}
    </label>
  )

  const readyName =
    src.kind === 'endpoint' ? `${src.model} at ${src.baseUrl}` : src.kind === 'chrome' ? "the browser's built-in model" : modelName(webllmModelOf(src))
  const readyTier = sourceTier(src)
  const tierTip = src.kind === 'endpoint' ? null : TIER_TIPS[readyTier]

  return (
    <div className="msrc">
      {option(
        'webllm',
        'Download a model',
        // Chosen, the list below names the model; the line stays one line.
        !webgpuAvailable() ? 'needs WebGPU' : src.kind === 'webllm' ? '' : `${chosen.name} · ${chosen.gb} GB · ${TIER_WORDS[tier(chosen.params)]}`,
        webgpuAvailable()
          ? 'From Hugging Face, run on this computer, downloaded once and kept by the browser. Strongest first: 7B and up builds and chats; 3B chats and builds with rough results; under 3B answers questions only. A bigger model downloads longer, and 7B and up needs several gigabytes free beside the weights.'
          : 'It needs WebGPU (Chrome, Edge, or Safari 26).',
        !webgpuAvailable(),
      )}
      {src.kind === 'webllm' && webgpuAvailable() && (
        <div className="msrc-form">
          <label>
            Model
            <select value={chosen.id} onChange={(e) => pick({ ...src, webllmModel: e.target.value })} aria-label="Downloaded model">
              {MODEL_CHOICES.map((m) => (
                <option key={m.id} value={m.id}>
                  {m.name} · {m.gb} GB · {TIER_WORDS[tier(m.params)]}
                  {downloaded.has(m.id) ? ' · downloaded' : ''}
                </option>
              ))}
            </select>
          </label>
        </div>
      )}
      {option('chrome', "The browser's built-in model", builtInNote, builtInTip, builtIn === 'absent' || builtIn === 'unavailable')}
      {option(
        'endpoint',
        'Your endpoint',
        'OpenAI-compatible',
        'Any OpenAI-compatible API: a local server, LocalRouter, or a provider. The endpoint must allow calls from this page (CORS).',
        false,
      )}

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
            <InfoTip
              className="msrc-privacy"
              text="Kept in this browser's local storage. Nothing is saved on a jazyk server, and the key is sent only to your endpoint."
            />
          </p>
        </div>
      )}

      {ready && (
        <p className="msrc-status v-ok">
          Ready: {readyName}
          {src.kind !== 'endpoint' && ` · ${TIER_WORDS[readyTier]}`}
          {tierTip && (
            <>
              {' '}
              <InfoTip className="msrc-tier" text={tierTip} />
            </>
          )}
        </p>
      )}
      {load && <p className={`msrc-status${load.failed ? ' v-bad' : ''}`}>{load.text}</p>}
      {loadable && src.kind !== 'endpoint' && (
        <p className="row">
          <button className="run" disabled={!!load && !load.failed} onClick={loadModel}>
            {load && !load.failed ? 'Loading…' : 'Load the model'}
          </button>
        </p>
      )}
    </div>
  )
}
