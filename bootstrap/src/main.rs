// The native binary. Everything it does lives in the library, so the same code
// serves the wasm backend (docs/frontends/wasm.md).
fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("__task_remote_watch") {
        if let (Some(root), Some(id)) = (args.get(2), args.get(3)) {
            let _ = jazyk::task_runtime::remote_watch(std::path::Path::new(root), id);
        }
        return;
    }
    if args.get(1).map(String::as_str) == Some("__task_watchdog") {
        if let Some(path) = args.get(2) {
            let _ = jazyk::task_runtime::watchdog(std::path::Path::new(path));
        }
        return;
    }
    if args.get(1).map(String::as_str) == Some("__task_supervise") {
        if let Some(path) = args.get(2) {
            if let Err(error) = jazyk::task_runtime::supervise(std::path::Path::new(path)) {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return;
    }
    if matches!(
        args.get(1).map(String::as_str),
        Some("tasks" | "runs" | "run" | "run-status" | "run-log" | "run-cancel")
    ) {
        match jazyk::task_runtime::cli(&args[1..]) {
            Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return;
    }
    jazyk::launcher::run();
}
