//! The stand-in for a workspace's dev server: the program `fake-devserver`, built once per process (or the one `EXEC_TESTKIT_DEVSERVER` names).
//! It listens on the `PORT` it is given and answers with the environment it saw (see the program's own docs for its flags).
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Names a built `fake-devserver` so a suite builds it once for everyone.
pub const ENV_DEVSERVER: &str = "EXEC_TESTKIT_DEVSERVER";

/// The program's path.
pub fn binary() -> &'static Path {
    static BINARY: OnceLock<PathBuf> = OnceLock::new();
    BINARY.get_or_init(|| match std::env::var_os(ENV_DEVSERVER) {
        Some(path) => {
            let path = PathBuf::from(path);
            assert!(path.is_file(), "{ENV_DEVSERVER}={} is no file", path.display());
            path
        }
        None => crate::cli::build("fake-devserver"),
    })
}

/// The shell command that starts the program with `flags`, as a request to `preview_start` gives it.
pub fn command(flags: &str) -> String {
    format!("exec '{}' {flags}", binary().display())
}

/// What the program answered on `port`: its environment variable names, its directory and its pid.
pub fn ask(port: u16) -> serde_json::Value {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap_or_else(|e| panic!("nothing listens on {port}: {e}"));
    stream.write_all(b"GET / HTTP/1.1\r\nhost: x\r\n\r\n").expect("the request is sent");
    let mut answer = String::new();
    stream.read_to_string(&mut answer).expect("the answer is read");
    serde_json::from_str(answer.split("\r\n\r\n").nth(1).unwrap_or("{}")).unwrap_or_else(|e| panic!("{answer}: {e}"))
}

/// A port nothing listens on, in `exec_preview`'s range, different at each call of a process (tests that start servers run side by side).
pub fn free_port() -> u16 {
    static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    for _ in 0..6000 {
        let step = NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let port = 3000 + ((std::process::id() * 37 + step * 101) % 7000) as u16;
        if std::net::TcpListener::bind(("127.0.0.1", port)).is_ok() {
            return port;
        }
    }
    panic!("no free port between 3000 and 9999")
}
