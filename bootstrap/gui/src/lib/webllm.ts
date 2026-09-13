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
    async chat(): Promise<string> {
      throw new Error(reason)
    },
  }
}
