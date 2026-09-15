// The write-through to a folder on disk. The in-memory tree stays the truth the
// compiler reads; these calls tell the page what changed so it can mirror the
// change into the folder it opened. When no folder is open they are no-ops.
// See docs/frontends/wasm.md#a-folder-on-disk.
use std::path::Path;
use wasm_bindgen::prelude::*;

#[wasm_bindgen(module = "/src/wasm/fs.js")]
extern "C" {
    #[wasm_bindgen(js_name = fsEnabled)]
    fn fs_enabled() -> bool;
    #[wasm_bindgen(js_name = enableFsSink)]
    fn enable_fs_sink_js();
    #[wasm_bindgen(js_name = fsWrite)]
    fn fs_write(path: &str, bytes: &[u8]);
    #[wasm_bindgen(js_name = fsRemove)]
    fn fs_remove(path: &str, dir: bool);
}

pub fn enable() {
    enable_fs_sink_js();
}

pub fn wrote(path: &Path, bytes: &[u8]) {
    if fs_enabled() {
        fs_write(&path.to_string_lossy(), bytes);
    }
}

pub fn removed(path: &Path, dir: bool) {
    if fs_enabled() {
        fs_remove(&path.to_string_lossy(), dir);
    }
}
