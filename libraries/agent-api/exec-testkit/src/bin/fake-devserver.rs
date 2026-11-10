//! The program that stands in for a workspace's dev server (see `exec_testkit::devserver`): it listens on `$PORT` of 127.0.0.1 and answers every
//! request with the names of the environment variables it was started with, its directory and its pid, so a test reads what the executor gave it.
//! Flags script its life: `--exit-now CODE` prints to stderr and exits before it listens, `--no-listen` stays up without listening,
//! `--delay-listen-ms N` listens after N ms, `--exit-after-ms N` exits with code 3 after N ms of listening, `--child` also starts a `sleep`
//! in its process group and prints its pid.
use std::io::{Read, Write};
use std::net::TcpListener;
use std::time::Duration;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let flag = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).and_then(|v| v.parse::<u64>().ok());
    if let Some(code) = flag("--exit-now") {
        eprintln!("fake-devserver: refusing to start");
        std::process::exit(code as i32);
    }
    if args.iter().any(|a| a == "--child") {
        #[allow(clippy::zombie_processes)] // the child must outlive this process so the test can see it ended with the group
        let child = std::process::Command::new("sleep").arg("300").spawn().expect("sleep starts");
        println!("child {}", child.id());
    }
    if args.iter().any(|a| a == "--no-listen") {
        std::thread::sleep(Duration::from_secs(300));
        return;
    }
    if let Some(ms) = flag("--delay-listen-ms") {
        std::thread::sleep(Duration::from_millis(ms));
    }
    let port: u16 = std::env::var("PORT").ok().and_then(|p| p.parse().ok()).expect("PORT is set");
    let listener = TcpListener::bind(("127.0.0.1", port)).expect("the port is free");
    println!("listening {port}");
    if let Some(ms) = flag("--exit-after-ms") {
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(ms));
            std::process::exit(3);
        });
    }
    for stream in listener.incoming().flatten() {
        let mut stream = stream;
        let mut buf = [0u8; 1024];
        let _ = stream.read(&mut buf);
        let mut names: Vec<String> = std::env::vars().map(|(k, _)| k).collect();
        names.sort();
        let body = serde_json::json!({"env": names, "cwd": std::env::current_dir().map(|d| d.display().to_string()).unwrap_or_default(), "pid": std::process::id(), "port_env": std::env::var("PORT").unwrap_or_default()}).to_string();
        let _ = write!(stream, "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}", body.len());
    }
}
