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

/// The ports Chromium refuses to load a page from (`net::ERR_UNSAFE_PORT`), as inclusive ranges.
const UNSAFE_PORTS: &[(u16, u16)] = &[
    (1, 1), (7, 7), (9, 9), (11, 11), (13, 13), (15, 15), (17, 17), (19, 23), (25, 25), (37, 37), (42, 43), (53, 53), (69, 69), (77, 77), (79, 79), (87, 87), (95, 95),
    (101, 104), (109, 111), (113, 113), (115, 115), (117, 117), (119, 119), (123, 123), (135, 135), (137, 137), (139, 139), (143, 143), (161, 161), (179, 179),
    (389, 389), (427, 427), (465, 465), (512, 515), (526, 526), (530, 532), (540, 540), (548, 548), (554, 554), (556, 556), (563, 563), (587, 587), (601, 601),
    (636, 636), (989, 990), (993, 993), (995, 995), (1719, 1720), (1723, 1723), (2049, 2049), (3659, 3659), (4045, 4045), (4190, 4190), (5060, 5061),
    (6000, 6000), (6566, 6566), (6665, 6669), (6679, 6679), (6697, 6697), (10080, 10080),
];

/// Whether a headless Chromium loads a page served on `port`.
pub fn browser_safe(port: u16) -> bool {
    !UNSAFE_PORTS.iter().any(|&(low, high)| (low..=high).contains(&port))
}

/// The port the `step`th request of process `pid` tries first, none when Chromium would refuse it.
pub fn draw(pid: u32, step: u32) -> Option<u16> {
    let port = 3000 + (pid.wrapping_mul(37).wrapping_add(step.wrapping_mul(101)) % 7000) as u16;
    browser_safe(port).then_some(port)
}

/// A port nothing listens on, in `exec_preview`'s range and one Chromium loads pages from, never handed out twice at once: each call takes a lock on a file named for the port that it keeps until its process ends, so test processes that run side by side (several crates, two stages on one runner) do not collide.
pub fn free_port() -> u16 {
    static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    static HELD: std::sync::Mutex<Vec<std::fs::File>> = std::sync::Mutex::new(Vec::new());
    let dir = std::env::temp_dir().join("exec-testkit-ports");
    std::fs::create_dir_all(&dir).expect("the port lock directory is made");
    for _ in 0..6000 {
        let step = NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let Some(port) = draw(std::process::id(), step) else { continue };
        let Ok(lock) = std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(dir.join(port.to_string())) else { continue };
        if lock.try_lock().is_err() {
            continue;
        }
        if std::net::TcpListener::bind(("127.0.0.1", port)).is_ok() {
            HELD.lock().unwrap().push(lock);
            return port;
        }
    }
    panic!("no free port between 3000 and 9999 that a browser loads")
}
