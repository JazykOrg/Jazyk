// The worker's trace channel. A build runs synchronously inside the worker, so the
// page cannot poll it: the worker is inside the call and answers nothing until it
// returns. Each event is posted the moment the compiler emits it instead, which is
// what makes a browser build watchable. See docs/frontends/wasm.md#the-trace.
export function emitTrace(json) {
  self.postMessage({ kind: 'trace', event: json })
}
