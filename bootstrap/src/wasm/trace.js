// The worker's trace channel. A build runs synchronously inside the worker, so the
// page cannot poll it: the worker is inside the call and answers nothing until it
// returns. Each event is posted the moment the compiler emits it instead, which is
// what makes a browser build watchable. See docs/frontends/wasm.md#the-trace.
export function emitTrace(json) {
  self.postMessage({ kind: 'trace', event: json })
}

// The worker's event channel: the chat pane's events, in the shape the native event
// stream carries (`chat.update`, `chat.sessions`, `store.generation`), posted the
// moment a turn emits them. See docs/frontends/wasm.md#chat.
export function emitEvent(json) {
  self.postMessage({ kind: 'event', event: json })
}
