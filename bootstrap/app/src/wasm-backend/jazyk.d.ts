// The generated bindings, typed. wasm-bindgen emits the JS; this names what the
// worker calls (docs/frontends/wasm.md#the-worker).
export default function init(module?: unknown): Promise<unknown>
export function reset(): void
export function seed_file(path: string, bytes: Uint8Array): void
export function boot(root: string): string
export function set_model(name: string): void
export function enable_fs_sink(): void
export function list_files(): string
export function request(method: string, path: string, body: string): string
