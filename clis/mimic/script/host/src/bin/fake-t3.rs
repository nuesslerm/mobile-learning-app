//! A stand-in for the pinned `t3` release in the flow tests: `serve --host H --port P --no-browser W` answers a few endpoints on loopback and
//! `auth pairing create --json` prints a credential. `/environment-of-server` shows the environment it was started in.
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};

fn answer(mut stream: TcpStream) {
    let mut reader = BufReader::new(stream.try_clone().expect("a clone"));
    let mut line = String::new();
    let _ = reader.read_line(&mut line);
    let path = line.split(' ').nth(1).unwrap_or("/").to_string();
    let mut length = 0usize;
    loop {
        let mut header = String::new();
        if reader.read_line(&mut header).unwrap_or(0) == 0 || header.trim().is_empty() {
            break;
        }
        if let Some(n) = header.to_lowercase().strip_prefix("content-length:") {
            length = n.trim().parse().unwrap_or(0);
        }
    }
    let mut body = vec![0; length];
    let _ = reader.read_exact(&mut body);
    let (status, text) = match path.as_str() {
        "/.well-known/t3/environment" => ("200 OK", r#"{"environmentId":"fake"}"#.to_string()),
        "/oauth/token" => ("200 OK", format!(r#"{{"access_token":"tok-for-{}"}}"#, String::from_utf8_lossy(&body).replace(['"', '\\'], ""))),
        "/environment-of-server" => ("200 OK", format!("HOME={} PATH={} TZ={} ARGS={}", std::env::var("HOME").unwrap_or_default(), std::env::var("PATH").unwrap_or_default(), std::env::var("TZ").unwrap_or_default(), std::env::args().skip(1).collect::<Vec<_>>().join(" "))),
        _ => ("404 Not Found", String::new()),
    };
    let _ = stream.write_all(format!("HTTP/1.1 {status}\r\nConnection: close\r\nContent-Type: text/plain\r\nContent-Length: {}\r\n\r\n{text}", text.len()).as_bytes());
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("serve") => {
            let port = args.iter().position(|a| a == "--port").and_then(|i| args.get(i + 1)).and_then(|p| p.parse::<u16>().ok()).expect("--port");
            let listener = TcpListener::bind(("127.0.0.1", port)).expect("bind");
            for stream in listener.incoming().flatten() {
                std::thread::spawn(move || answer(stream));
            }
        }
        Some("auth") => println!(r#"{{"credential":"cred-{}"}}"#, std::process::id()),
        _ => std::process::exit(2),
    }
}
