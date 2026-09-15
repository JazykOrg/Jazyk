// The worker half of the model bridge (src/wasm/bridge.js, re-exported by the build
// script under a stable path).
export function installBridge(control: SharedArrayBuffer, reply: SharedArrayBuffer): void
export function bridgeReady(): boolean
