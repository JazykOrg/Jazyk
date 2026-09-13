// The model the page holds. WebLLM runs it on WebGPU on the main thread; the
// backend in the worker calls it through the shared-buffer bridge and blocks until
// this answers. The request and the reply are the OpenAI shapes `crate::llm`
// already speaks, so nothing in the compiler knows the model is local.
// See docs/frontends/wasm.md#the-model.

import * as webllm from '@mlc-ai/web-llm'
import type { ModelHost } from './wasm-client'

// A small instruct model with tool calling, so a first visit downloads ~2GB rather
// than ~5GB. The weights cache in the browser, so a second visit is instant.
export const DEFAULT_MODEL = 'Hermes-3-Llama-3.2-3B-q4f16_1-MLC'
export const CONTEXT_WINDOW = 16384

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
  // A session prompt is several thousand tokens before the model says a word, and
  // the engine's default window is 4k. The model itself takes far more; the KV cache
  // is what the window costs, and 16k fits beside the weights on a laptop GPU.
  const engine = await webllm.CreateMLCEngine(
    model,
    { initProgressCallback: (r) => onProgress({ text: r.text, progress: r.progress }) },
    { context_window_size: CONTEXT_WINDOW },
  )

  return {
    name: () => model,
    async chat(body: string): Promise<string> {
      const req = JSON.parse(body) as Record<string, unknown>
      // The backend asks for whatever the native endpoint would get. Streaming has
      // no transport to stream over here, and the model id is the loaded one.
      const reply = await engine.chat.completions.create({
        ...req,
        model,
        stream: false,
      } as webllm.ChatCompletionRequestNonStreaming)
      return JSON.stringify(reply)
    },
  }
}

// The host used when there is no WebGPU or no cross-origin isolation: every read
// still works, and a build says why it cannot run rather than hanging.
export function unavailableModel(reason: string): ModelHost {
  return {
    name: () => 'none',
    async chat(): Promise<string> {
      throw new Error(reason)
    },
  }
}
