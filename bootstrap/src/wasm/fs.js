// The worker half of the on-disk mode. The in-memory tree is the truth the compiler
// reads; when the page opened a real folder, every mutation is also posted to the
// page, which applies it to the folder through the File System Access API. Reads
// never leave the worker. See docs/frontends/wasm.md#a-folder-on-disk.

let enabled = false

export function enableFsSink() {
  enabled = true
}

export function fsEnabled() {
  return enabled
}

export function fsWrite(path, bytes) {
  if (!enabled) return
  // A copy: the wasm memory behind `bytes` is reused as soon as this returns.
  self.postMessage({ kind: 'fs', op: 'write', path, bytes: new Uint8Array(bytes) })
}

export function fsRemove(path, dir) {
  if (!enabled) return
  self.postMessage({ kind: 'fs', op: 'remove', path, dir })
}
