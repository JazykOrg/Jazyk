// The native binary. Everything it does lives in the library, so the same code
// serves the wasm backend (docs/frontends/wasm.md).
fn main() {
    jazyk::launcher::run();
}
