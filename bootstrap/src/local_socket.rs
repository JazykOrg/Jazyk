//! Native local edit transport, shared by the ACP proxy and MCP serving.
#[cfg(unix)]
pub(crate) use std::os::unix::net::{UnixListener, UnixStream};
#[cfg(windows)]
pub(crate) use uds_windows::{UnixListener, UnixStream};

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};

    #[test]
    fn local_edit_transport_round_trips_json_lines() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("edit.sock");
        let listener = UnixListener::bind(&path).unwrap();
        let worker = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                .unwrap();
            let mut line = String::new();
            BufReader::new(&stream).read_line(&mut line).unwrap();
            assert_eq!(
                serde_json::from_str::<serde_json::Value>(&line).unwrap()["path"],
                "docs/main.md"
            );
            writeln!(stream, "{{\"ok\":true}}").unwrap();
        });
        let mut stream = UnixStream::connect(&path).unwrap();
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();
        writeln!(stream, "{{\"path\":\"docs/main.md\"}}").unwrap();
        let mut reply = String::new();
        BufReader::new(&stream).read_line(&mut reply).unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&reply).unwrap()["ok"],
            true
        );
        worker.join().unwrap();
        drop(stream);
        std::fs::remove_file(path).unwrap();
    }
}
