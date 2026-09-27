// The model the page holds. WebLLM runs it on WebGPU on the main thread; the
// backend in the worker calls it through the shared-buffer bridge and blocks until
// this answers. The request and the reply are the OpenAI shapes `crate::llm`
// already speaks, so nothing in the compiler knows the model is local.
// See docs/frontends/wasm.md#the-model.

// The library is imported when the model loads, not with the page: it is most of
// the page's script, and a visit that only reads never needs it
// (docs/frontends/wasm.md#building).
import type * as webllm from '@mlc-ai/web-llm'
import type { ModelHost } from './wasm-client'

// A small instruct model with tool calling, so a first visit downloads ~2GB rather
// than ~5GB. The weights cache in the browser, so a second visit is instant.
export const DEFAULT_MODEL = 'Hermes-3-Llama-3.2-3B-q4f16_1-MLC'

// WebLLM's native tool calling is whitelisted to three 8B Hermes builds and refuses a
// custom system prompt, so the backend speaks the text codec, which renders the
// compile serving's tool schemas into the prompt: about 19k tokens before the model
// says a word. 32k holds that with room for the turns. For a 3B model the KV cache
// at 32k is about 3.5GB beside 2GB of weights, which a laptop with unified memory
// carries; see docs/frontends/wasm.md#the-model.
export const CONTEXT_WINDOW = 32768

export interface LoadProgress {
  text: string
  progress: number
}

export function webgpuAvailable(): boolean {
  return typeof navigator !== 'undefined' && 'gpu' in navigator
}

export async function loadModel(
  model: string,
  onProgress: (p: LoadProgress) => void,
): Promise<ModelHost> {
  // A session prompt carries the whole toolset in text (see CONTEXT_WINDOW), and the
  // engine's default window is 4k. The KV cache is what the window costs.
  const { CreateMLCEngine } = await import('@mlc-ai/web-llm')
  const engine = await CreateMLCEngine(
    model,
    { initProgressCallback: (r) => onProgress({ text: r.text, progress: r.progress }) },
    { context_window_size: CONTEXT_WINDOW },
  )

  return {
    name: () => model,
    available: () => true,
    async chat(body: string): Promise<string> {
      const req = JSON.parse(body) as Record<string, unknown>
      // The backend asks for whatever the native endpoint would get. Streaming has
      // no transport to stream over here, and the model id is the loaded one.
      const started = performance.now()
      const msgs = Array.isArray(req.messages) ? (req.messages as unknown[]).length : 0
      console.info(`[jazyk model] call: ${msgs} messages, ${body.length} chars, tools=${Array.isArray(req.tools) ? (req.tools as unknown[]).length : 0}`)
      // Streamed from the engine and assembled here into the one-shot shape the
      // backend expects, so progress is visible while a slow model works: the
      // backend is blocked on the answer and can say nothing itself.
      let reply: webllm.ChatCompletion
      try {
        const stream = await engine.chat.completions.create({
          ...req,
          model,
          stream: true,
          stream_options: { include_usage: true },
        } as webllm.ChatCompletionRequestStreaming)
        let content = ''
        let finish: string | null = null
        let usage: webllm.CompletionUsage | undefined
        let n = 0
        for await (const chunk of stream) {
          const delta = chunk.choices?.[0]?.delta?.content
          if (delta) {
            content += delta
            n += 1
            if (n % 100 === 0) console.info(`[jazyk model] … ${n} chunks, ${((performance.now() - started) / 1000).toFixed(0)}s`)
          }
          if (chunk.choices?.[0]?.finish_reason) finish = chunk.choices[0].finish_reason
          if (chunk.usage) usage = chunk.usage
        }
        reply = {
          id: 'in-page',
          object: 'chat.completion',
          created: Math.floor(Date.now() / 1000),
          model,
          choices: [{ index: 0, message: { role: 'assistant', content }, finish_reason: finish ?? 'stop', logprobs: null }],
          usage,
        } as webllm.ChatCompletion
      } catch (e) {
        console.error('[jazyk model] call failed:', e)
        throw e
      }
      // One line per reply, so a build is followable from the console: what it cost,
      // how it ended, and whether the model called a tool or wrote prose.
      const choice = reply.choices?.[0]
      const calls = choice?.message?.tool_calls?.map((c) => c.function?.name) ?? []
      console.info(
        `[jazyk model] reply in ${((performance.now() - started) / 1000).toFixed(1)}s: ` +
          `${reply.usage?.prompt_tokens ?? '?'} in, ${reply.usage?.completion_tokens ?? '?'} out, ` +
          `finish=${choice?.finish_reason}, tools=[${calls.join(', ')}], ` +
          `text=${JSON.stringify((choice?.message?.content ?? '').slice(0, 160))}`,
      )
      return JSON.stringify(reply)
    },
  }
}

// The host used when there is no WebGPU or no cross-origin isolation: every read
// still works, and a build says why it cannot run rather than hanging.
export function unavailableModel(reason: string): ModelHost {
  return {
    name: () => 'none',
    available: () => false,
    async chat(): Promise<string> {
      throw new Error(reason)
    },
  }
}

// The models the chooser offers (docs/frontends/wasm.md#model-sources): WebLLM's
// prebuilt instruct models in their 16-bit-activation builds, with the memory each
// needs as WebLLM states it. A static table rather than the library's own list, so
// the chooser renders without fetching the library; a unit test keeps it honest
// against the library's prebuilt config.
export interface ModelChoice {
  id: string
  // A short name for the chooser.
  name: string
  // Weights plus working memory, in gigabytes, as WebLLM's vram_required_MB says.
  gb: number
}

export const MODEL_CHOICES: ModelChoice[] = [
  { id: 'SmolLM2-1.7B-Instruct-q4f16_1-MLC', name: 'SmolLM2 1.7B', gb: 1.7 },
  { id: 'Qwen2.5-0.5B-Instruct-q4f16_1-MLC', name: 'Qwen 2.5 0.5B', gb: 0.9 },
  { id: 'Qwen2.5-1.5B-Instruct-q4f16_1-MLC', name: 'Qwen 2.5 1.5B', gb: 1.6 },
  { id: 'Llama-3.2-1B-Instruct-q4f16_1-MLC', name: 'Llama 3.2 1B', gb: 0.9 },
  { id: 'gemma-2-2b-it-q4f16_1-MLC', name: 'Gemma 2 2B', gb: 1.9 },
  { id: 'Hermes-3-Llama-3.2-3B-q4f16_1-MLC', name: 'Hermes 3 (Llama 3.2 3B)', gb: 2.2 },
  { id: 'Llama-3.2-3B-Instruct-q4f16_1-MLC', name: 'Llama 3.2 3B', gb: 2.2 },
  { id: 'Qwen2.5-3B-Instruct-q4f16_1-MLC', name: 'Qwen 2.5 3B', gb: 2.4 },
  { id: 'Phi-4-mini-instruct-q4f16_1-MLC', name: 'Phi 4 mini', gb: 3.4 },
  { id: 'Phi-3.5-mini-instruct-q4f16_1-MLC', name: 'Phi 3.5 mini', gb: 3.6 },
  { id: 'Hermes-2-Pro-Mistral-7B-q4f16_1-MLC', name: 'Hermes 2 Pro (Mistral 7B)', gb: 3.9 },
  { id: 'Mistral-7B-Instruct-v0.3-q4f16_1-MLC', name: 'Mistral 7B v0.3', gb: 4.5 },
  { id: 'Hermes-3-Llama-3.1-8B-q4f16_1-MLC', name: 'Hermes 3 (Llama 3.1 8B)', gb: 4.8 },
  { id: 'Llama-3.1-8B-Instruct-q4f16_1-MLC', name: 'Llama 3.1 8B', gb: 4.9 },
  { id: 'Qwen2.5-7B-Instruct-q4f16_1-MLC', name: 'Qwen 2.5 7B', gb: 5.0 },
  { id: 'gemma-2-9b-it-q4f16_1-MLC', name: 'Gemma 2 9B', gb: 6.3 },
]

// The choice's short name, for a model id the table knows; the id itself otherwise.
export function modelName(id: string): string {
  return MODEL_CHOICES.find((m) => m.id === id)?.name ?? id
}
