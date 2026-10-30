// Model and input capabilities supplied by the page. The worker stays responsive
// while Rust awaits model promises. Project operations are serialized by the host.
let installed = false
let cancelled = false
let inputOpen = true
let inputs = []
let nextModelId = 0
const pending = new Map()

export function installBridge() {
  for (const waiter of pending.values()) waiter.reject(new Error('model bridge reset'))
  pending.clear()
  installed = true
  cancelled = false
  inputOpen = true
  inputs = []
}

export function bridgeReady() { return installed }
export function cancelRequested() { return cancelled }
export function clearCancel() { cancelled = false; inputOpen = true }

export function setCancelled(value) {
  cancelled = Boolean(value)
  if (cancelled) {
    for (const waiter of pending.values()) waiter.reject(new Error('execution cancelled'))
    pending.clear()
  }
}

export function chatAsync(body) {
  if (!installed) return Promise.reject(new Error('no model bridge: the page did not install one'))
  if (cancelled) return Promise.reject(new Error('execution cancelled'))
  const id = ++nextModelId
  return new Promise((resolve, reject) => {
    pending.set(id, { resolve, reject })
    self.postMessage({ kind: 'llm', id, body })
  })
}

export function resolveModelReply(id, reply) {
  const waiter = pending.get(id)
  if (!waiter) return
  pending.delete(id)
  if (reply.error) waiter.reject(new Error(reply.error))
  else if (typeof reply.body !== 'string') waiter.reject(new Error('invalid model response'))
  else waiter.resolve(reply.body)
}

export function enqueueInputs(items) {
  if (!inputOpen) return false
  if (!Array.isArray(items)) throw new Error('invalid input mailbox')
  inputs.push(...items)
  return true
}

export function clearInputs() { inputs = [] }

export function pollInputs() {
  const batch = inputs
  inputs = []
  if (batch.length) self.postMessage({ kind: 'chat-input-consumed' })
  return JSON.stringify(batch)
}

export function closeInputs() {
  if (inputs.length) return false
  inputOpen = false
  self.postMessage({ kind: 'chat-input-closed' })
  return true
}
