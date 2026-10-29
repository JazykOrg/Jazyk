// The worker half of the model bridge. The page owns the model; this blocks the
// worker on Atomics.wait until the page answers, so the synchronous reconciler
// above it never learns that the call went anywhere.
//
// Layout of the shared control buffer:
//   [0] Int32  state: 0 pending, 1 answered
//   [1] Int32  byte length of the reply
//   [2] Int32  cancel: 1 once the page cancels the running chat turn
// The reply itself travels in a second buffer, grown by the page when needed.

let control = null // Int32Array over a SharedArrayBuffer
let inputHeader = null
let inputBytes = null
let replyBytes = null // Uint8Array over a SharedArrayBuffer

// Called once from the worker entry when the page hands over the shared buffers.
export function installBridge(controlBuffer, replyBuffer, inputBuffer = null) {
  inputHeader = inputBuffer ? new Int32Array(inputBuffer, 0, 4) : null
  inputBytes = inputBuffer ? new Uint8Array(inputBuffer, 16) : null
  control = new Int32Array(controlBuffer)
  replyBytes = new Uint8Array(replyBuffer)
}

export function bridgeReady() {
  return control !== null
}

// The page sets the cancel flag when a person cancels a chat turn; the worker is
// inside that turn and could not hear a message. See docs/frontends/wasm.md#chat.
export function cancelRequested() {
  return control !== null && control.length > 2 && Atomics.load(control, 2) === 1
}

export function clearCancel() {
  if (control !== null && control.length > 2) Atomics.store(control, 2, 0)
}

export function chatSync(body) {
  if (!control) {
    throw new Error(
      'no model bridge: the page did not install one (cross-origin isolation is required for SharedArrayBuffer)',
    )
  }
  Atomics.store(control, 0, 0)
  Atomics.store(control, 1, 0)
  // The page is listening for this and will run the model.
  self.postMessage({ kind: 'llm', body })
  // Block this worker. The page is on its own thread and stays responsive.
  Atomics.wait(control, 0, 0)
  const len = Atomics.load(control, 1)
  const bytes = replyBytes.slice(0, len)
  const text = new TextDecoder().decode(bytes)
  const reply = JSON.parse(text)
  if (reply.error) throw new Error(reply.error)
  return reply.body
}

// Called only at safe loop boundaries on the worker thread. The page is the
// single producer; published bytes remain immutable until consumption.
export function pollInputs() {
  if (!inputHeader || Atomics.load(inputHeader, 0) !== 1) return '[]'
  const length = Atomics.load(inputHeader, 1)
  if (length < 0 || length > inputBytes.length) {
    Atomics.store(inputHeader, 0, 0)
    throw new Error('invalid input mailbox length')
  }
  const text = new TextDecoder().decode(inputBytes.slice(0, length))
  JSON.parse(text)
  Atomics.store(inputHeader, 0, 0)
  self.postMessage({ kind: 'chat-input-consumed' })
  return text
}

export function closeInputs() {
  if (!inputHeader) return true
  const state = Atomics.compareExchange(inputHeader, 0, 0, 2)
  return state === 0 || state === 2
}
