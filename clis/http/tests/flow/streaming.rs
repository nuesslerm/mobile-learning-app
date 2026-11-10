//! A file bigger than the read cap goes out as a stream. The file is sparse, so it costs no disk.
use super::fake_service::TempDir;
use cli_http::put_file;
use std::sync::mpsc;
use std::time::Duration;
use tiny_http::{Response, Server};

const SIZE: u64 = 600 * 1024 * 1024;

struct Cleanup(std::path::PathBuf);

impl Drop for Cleanup {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The highest resident memory this process has reached, in KiB.
fn peak_resident_kib() -> u64 {
    let status = std::fs::read_to_string("/proc/self/status").unwrap();
    let line = status.lines().find(|l| l.starts_with("VmHWM:")).unwrap();
    line.split_whitespace().nth(1).unwrap().parse().unwrap()
}

#[test]
fn given_a_file_above_the_read_cap_when_it_is_put_then_the_server_gets_every_byte_and_the_client_never_holds_them() {
    // Given a 600 MiB file, above the 512 MiB read cap, and a server that counts the body it is sent without keeping it
    let dir = TempDir::new("put-file");
    let _cleanup = Cleanup(dir.0.clone());
    let path = dir.0.join("big.bin");
    std::fs::File::create(&path).unwrap().set_len(SIZE).unwrap();
    let server = Server::http("127.0.0.1:0").unwrap();
    let url = format!("http://127.0.0.1:{}/o/big.bin", server.server_addr().to_ip().unwrap().port());
    let (told, counted) = mpsc::channel();
    std::thread::spawn(move || {
        let mut request = server.recv().unwrap();
        let announced = request.body_length();
        let mut received = 0u64;
        let mut chunk = vec![0u8; 64 * 1024];
        loop {
            match request.as_reader().read(&mut chunk).unwrap() {
                0 => break,
                n => received += n as u64,
            }
        }
        told.send((request.method().to_string(), announced, received)).unwrap();
        let _ = request.respond(Response::empty(200));
    });

    // When it is put, with the process's peak memory read around the call
    let before = peak_resident_kib();
    let response = put_file(&url, &path, Duration::from_secs(120)).unwrap();
    let grown = peak_resident_kib().saturating_sub(before);

    // Then the server was sent a PUT announcing the file's length and every byte arrived, and the peak grew by far less than the file
    let (method, announced, received) = counted.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(response.status, 200);
    assert_eq!((method.as_str(), announced, received), ("PUT", Some(SIZE as usize), SIZE));
    assert!(grown < 64 * 1024, "peak memory grew by {grown} KiB sending {} KiB", SIZE / 1024);
}
