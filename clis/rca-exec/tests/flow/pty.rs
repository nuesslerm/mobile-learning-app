//! The pty family on real pseudo-terminals: the local adapter on a temp directory, and the channel client against an in-test rca that
//! reads the binary output frames. The shell is `/bin/sh`, so nothing depends on the machine's login shell.
use super::{accept, gone, wait_for, Seen};
use exec_pty::{PtyAttach, PtyAttached, PtyClose, PtyEvent, PtyOpen, PtyResize, PtyWrite};
use exec_proto::Code;
use rca_exec::adapters::exec_channel::ExecChannel;
use rca_exec::adapters::local_executor::LocalExecutor;
use rca_exec::domain::ToolExecutor;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::net::{TcpListener, TcpStream};
use std::os::unix::fs::symlink;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tungstenite::{Message, WebSocket};

fn executor(dir: &Path) -> LocalExecutor {
    LocalExecutor::new(dir).unwrap().with_pty_shell(Path::new("/bin/sh")).unwrap()
}

fn open(name: &str) -> PtyOpen {
    PtyOpen::parse(&json!({"pty": name})).unwrap()
}

fn write(ex: &LocalExecutor, name: &str, data: &str) {
    ex.pty_write(&PtyWrite { pty: name.into(), data: data.into() }).unwrap();
}

type Frames = Arc<Mutex<Vec<(u64, Vec<u8>)>>>;

/// An attach running on its own thread; `frames` is what it has sent so far.
struct Attached {
    frames: Frames,
    events: Arc<Mutex<Vec<PtyEvent>>>,
    done: std::thread::JoinHandle<PtyAttached>,
}

fn attach(ex: &Arc<LocalExecutor>, id: &str, name: &str, after_offset: Option<u64>) -> Attached {
    let (frames, events): (Frames, Arc<Mutex<Vec<PtyEvent>>>) = Default::default();
    let done = std::thread::spawn({
        let (ex, id, name, f, e) = (ex.clone(), id.to_string(), name.to_string(), frames.clone(), events.clone());
        move || ex.pty_attach(&id, &PtyAttach { pty: name, after_offset }, &mut |offset, bytes| f.lock().unwrap().push((offset, bytes.to_vec())), &mut |ev| e.lock().unwrap().push(ev)).unwrap()
    });
    Attached { frames, events, done }
}

fn bytes(frames: &Frames) -> Vec<u8> {
    frames.lock().unwrap().iter().flat_map(|(_, b)| b.clone()).collect()
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

fn wait_output(frames: &Frames, needle: &str) {
    wait_for(&format!("output {needle:?}"), || contains(&bytes(frames), needle.as_bytes()));
}

#[test]
fn a_command_typed_into_a_pty_comes_back_as_its_output_and_a_resize_reaches_the_shell() {
    // Given a pty in a temp directory with an attach reading it
    let dir = tempfile::tempdir().unwrap();
    let ex = Arc::new(executor(dir.path()));
    let opened = ex.pty_open(&PtyOpen::parse(&json!({"pty": "t1", "cols": 100, "rows": 30})).unwrap()).unwrap();
    let a = attach(&ex, "att", "t1", None);
    // When a command that prints is typed
    write(&ex, "t1", "echo round-trip-$((6*7))\n");
    // Then its computed output arrives (the typed text alone would read 6*7), at the directory the pty opened in
    wait_output(&a.frames, "round-trip-42");
    assert_eq!(opened.cwd, dir.path().canonicalize().unwrap().to_string_lossy());
    // And the shell sees the opened size, then the new one after a resize
    write(&ex, "t1", "stty size\n");
    wait_output(&a.frames, "30 100");
    let resized = ex.pty_resize(&PtyResize { pty: "t1".into(), cols: 132, rows: 50 });
    write(&ex, "t1", "stty size\n");
    wait_output(&a.frames, "50 132");
    assert_eq!(resized, Ok(()));
    let listed = ex.pty_list().unwrap();
    assert_eq!((listed[0].cols, listed[0].rows, listed[0].exited), (132, 50, false));
    ex.pty_close(&PtyClose { pty: "t1".into() }).unwrap();
    a.done.join().unwrap();
}

#[test]
fn output_frames_are_numbered_by_byte_offset_without_gaps_and_keep_bytes_that_are_not_utf8() {
    // Given a pty whose shell prints bytes that are no text
    let dir = tempfile::tempdir().unwrap();
    let ex = Arc::new(executor(dir.path()));
    ex.pty_open(&open("bin")).unwrap();
    let a = attach(&ex, "att", "bin", None);
    write(&ex, "bin", "printf '\\377\\376raw\\n'\n");
    // When the output has arrived
    wait_for("the raw bytes", || contains(&bytes(&a.frames), b"\xff\xferaw"));
    // Then every byte is kept as it was, and each frame starts where the one before ended
    let frames = a.frames.lock().unwrap().clone();
    let mut next = 0;
    for (offset, data) in &frames {
        assert_eq!(*offset, next);
        next += data.len() as u64;
    }
    assert_eq!(ex.pty_list().unwrap()[0].offset, next);
    assert!(a.events.lock().unwrap().iter().all(|e| matches!(e, PtyEvent::Attached { reset: false, start_offset: 0, .. })));
    ex.pty_close(&PtyClose { pty: "bin".into() }).unwrap();
    a.done.join().unwrap();
}

#[test]
fn an_attach_after_an_offset_gets_only_what_follows_and_one_past_the_kept_tail_is_told_to_reset() {
    // Given a pty that has printed a marker, and the offset the output had reached
    let dir = tempfile::tempdir().unwrap();
    let ex = Arc::new(executor(dir.path()));
    ex.pty_open(&open("r")).unwrap();
    let first = attach(&ex, "a1", "r", None);
    write(&ex, "r", "echo first-$((10+1))\n");
    wait_output(&first.frames, "first-11\r\n");
    let seen = bytes(&first.frames).len() as u64;
    // When a second attach starts after that offset and the shell prints another marker
    let second = attach(&ex, "a2", "r", Some(seen));
    write(&ex, "r", "echo second-$((20+2))\n");
    wait_output(&second.frames, "second-22\r\n");
    // Then it starts at that offset and never saw the first marker, while an attach from zero has both
    assert_eq!(second.frames.lock().unwrap()[0].0, seen);
    assert!(!contains(&bytes(&second.frames), b"first-11"));
    wait_output(&first.frames, "second-22");
    let again = attach(&ex, "a3", "r", Some(0));
    wait_output(&again.frames, "second-22");
    assert!(contains(&bytes(&again.frames), b"first-11"));
    // And once more output than is kept has passed, an attach from zero is told the stream starts later, and one past the end starts at the end
    write(&ex, "r", "yes x | head -c 1300000; echo tail-$((2+3))\n");
    wait_output(&first.frames, "tail-5\r\n");
    let info = ex.pty_list().unwrap().remove(0);
    assert_eq!(info.offset - info.base_offset, exec_pty::RETAIN as u64);
    let stale = attach(&ex, "a4", "r", Some(0));
    let future = attach(&ex, "a5", "r", Some(info.offset + 1000));
    wait_for("the reset events", || !stale.events.lock().unwrap().is_empty() && !future.events.lock().unwrap().is_empty());
    assert_eq!(stale.events.lock().unwrap()[0], PtyEvent::Attached { pty: "r".into(), requested_offset: 0, start_offset: info.base_offset, reset: true, offset: info.offset });
    assert!(matches!(future.events.lock().unwrap()[0], PtyEvent::Attached { reset: true, start_offset, .. } if start_offset >= info.offset));
    assert_eq!(stale.frames.lock().unwrap()[0].0, info.base_offset);
    ex.pty_close(&PtyClose { pty: "r".into() }).unwrap();
    for a in [first, second, again, stale, future] {
        a.done.join().unwrap();
    }
}

#[test]
fn a_cancel_ends_an_attach_but_not_the_pty_and_a_new_attach_continues_at_its_offset() {
    // Given an attach that has read a marker
    let dir = tempfile::tempdir().unwrap();
    let ex = Arc::new(executor(dir.path()));
    ex.pty_open(&open("c")).unwrap();
    let a = attach(&ex, "att", "c", None);
    write(&ex, "c", "echo before\n");
    wait_output(&a.frames, "before");
    // When it is cancelled by its id (twice: the second finds nothing)
    let (hit, miss) = (ex.cancel("att"), { std::thread::sleep(Duration::from_millis(200)); ex.cancel("att") });
    let result = a.done.join().unwrap();
    // Then the attach says it was detached with the pty still running, and the pty still takes input
    assert_eq!((hit, miss, result.detached, result.exited), (true, false, true, false));
    let b = attach(&ex, "att2", "c", Some(result.offset));
    write(&ex, "c", "echo after\n");
    wait_output(&b.frames, "after");
    assert!(!contains(&bytes(&b.frames), b"before"));
    ex.pty_close(&PtyClose { pty: "c".into() }).unwrap();
    assert!(b.done.join().unwrap().exited);
}

#[test]
fn closing_a_pty_kills_its_shell_and_the_background_processes_of_its_group() {
    // Given a pty whose shell started a background process
    let dir = tempfile::tempdir().unwrap();
    let ex = Arc::new(executor(dir.path()));
    let opened = ex.pty_open(&open("k")).unwrap();
    let a = attach(&ex, "att", "k", None);
    write(&ex, "k", "sleep 300 & echo child=$!=end\n");
    wait_for("the child pid", || String::from_utf8_lossy(&bytes(&a.frames)).contains("=end") && String::from_utf8_lossy(&bytes(&a.frames)).matches("child=").count() >= 2);
    let text = String::from_utf8_lossy(&bytes(&a.frames)).into_owned();
    let child: i32 = text.rsplit("child=").next().unwrap().split('=').next().unwrap().trim().parse().unwrap();
    assert!(!gone(child) && !gone(opened.pid as i32));
    // When the pty is closed
    let closed = ex.pty_close(&PtyClose { pty: "k".into() }).unwrap();
    // Then both are gone, the attach ended with the pty, and the name is free
    assert!(closed.closed && closed.exit_code.is_some(), "{closed:?}");
    wait_for("the child and the shell to die", || gone(child) && gone(opened.pid as i32));
    let ended = a.done.join().unwrap();
    assert!(ended.exited && !ended.detached);
    assert!(ex.pty_list().unwrap().is_empty());
    assert!(!ex.pty_close(&PtyClose { pty: "k".into() }).unwrap().closed);
    ex.pty_open(&open("k")).unwrap();
    ex.pty_close(&PtyClose { pty: "k".into() }).unwrap();
}

#[test]
fn a_shell_that_exits_ends_the_attach_with_its_code_and_refuses_further_input() {
    // Given a pty and an attach
    let dir = tempfile::tempdir().unwrap();
    let ex = Arc::new(executor(dir.path()));
    ex.pty_open(&open("x")).unwrap();
    let a = attach(&ex, "att", "x", None);
    // When the shell exits with a code
    write(&ex, "x", "exit 7\n");
    let ended = a.done.join().unwrap();
    // Then the attach reports it, the pty is listed as exited, and input is refused
    assert_eq!((ended.exited, ended.exit_code, ended.detached), (true, Some(7), false));
    let info = ex.pty_list().unwrap().remove(0);
    assert_eq!((info.exited, info.exit_code), (true, Some(7)));
    assert!(ex.pty_write(&PtyWrite { pty: "x".into(), data: "ls\n".into() }).is_err());
    ex.pty_close(&PtyClose { pty: "x".into() }).unwrap();
}

#[test]
fn a_pty_starts_only_inside_the_root_or_the_worktree_base_and_never_through_a_symlink_out() {
    // Given a root with a sibling directory outside it, a symlink to it, a subdirectory, and a worktree base
    let parent = tempfile::tempdir().unwrap();
    let (root, outside, base) = (parent.path().join("root"), parent.path().join("outside"), parent.path().join("base"));
    for d in [root.join("sub"), outside.clone(), base.join("wt")] {
        std::fs::create_dir_all(d).unwrap();
    }
    symlink(&outside, root.join("escape")).unwrap();
    std::fs::write(root.join("file"), "x").unwrap();
    let ex = LocalExecutor::new(&root).unwrap().with_worktree_base(&base).unwrap().with_pty_shell(Path::new("/bin/sh")).unwrap();
    let try_cwd = |name: &str, cwd: &str| ex.pty_open(&PtyOpen::parse(&json!({"pty": name, "cwd": cwd})).unwrap());
    // When a cwd climbs out, goes through the symlink, or names a path outside as an absolute one
    // Then each is PATH_OUTSIDE_ROOT and no pty exists
    for (i, cwd) in ["..", "../outside", "sub/../../outside", "escape", "escape/..", outside.to_str().unwrap(), "/", "/tmp"].iter().enumerate() {
        assert_eq!(try_cwd(&format!("n{i}"), cwd).unwrap_err().code, Code::PathOutsideRoot, "{cwd}");
    }
    assert!(ex.pty_list().unwrap().is_empty());
    // And a file or a missing directory is NOT_FOUND, with no pty either
    assert_eq!(try_cwd("f", "file").unwrap_err().code, Code::NotFound);
    assert_eq!(try_cwd("m", "--help").unwrap_err().code, Code::NotFound);
    // But the root, a subdirectory, and an absolute path under the base all start, and the name `-rf` is only a name
    let cwds: Vec<String> = [("a", "."), ("-rf", "sub"), ("c", base.join("wt").to_str().unwrap())].iter().map(|(n, c)| try_cwd(n, c).unwrap().cwd).collect();
    assert_eq!(cwds, [root.canonicalize().unwrap(), root.join("sub").canonicalize().unwrap(), base.join("wt").canonicalize().unwrap()].map(|p| p.to_string_lossy().into_owned()));
    for name in ["a", "-rf", "c"] {
        ex.pty_close(&PtyClose { pty: name.into() }).unwrap();
    }
}

#[test]
fn a_pty_shell_has_none_of_the_executors_secrets_and_only_the_allowed_variables_can_be_set() {
    // Given an executor process holding a secret in its environment
    std::env::set_var("RCA_PTY_TEST_SECRET", "hunter2-token");
    std::env::set_var("LC_ALL", "C");
    let dir = tempfile::tempdir().unwrap();
    let ex = Arc::new(executor(dir.path()));
    // When a pty opens with a TERM of its own and prints its environment
    ex.pty_open(&PtyOpen::parse(&json!({"pty": "e", "env": {"TERM": "dumb"}})).unwrap()).unwrap();
    let a = attach(&ex, "att", "e", None);
    write(&ex, "e", "env; echo env-done\n");
    wait_for("the environment listing", || String::from_utf8_lossy(&bytes(&a.frames)).contains("\nenv-done"));
    let text = String::from_utf8_lossy(&bytes(&a.frames)).into_owned();
    // Then the secret is absent, the requested TERM and the inherited PATH and LC_ALL are there
    assert!(!text.contains("RCA_PTY_TEST_SECRET") && !text.contains("hunter2-token"), "{text}");
    assert!(text.contains("TERM=dumb") && text.contains("PATH=") && text.contains("LC_ALL=C"), "{text}");
    ex.pty_close(&PtyClose { pty: "e".into() }).unwrap();
    // And a variable that could change what the shell runs is refused before any pty opens
    for bad in [json!({"PATH": "/tmp"}), json!({"LD_PRELOAD": "/tmp/x.so"}), json!({"ENV": "/tmp/rc"}), json!({"BASH_ENV": "/tmp/rc"})] {
        assert_eq!(PtyOpen::parse(&json!({"pty": "z", "env": bad})).unwrap_err().code, Code::InvalidArgs);
    }
    assert!(ex.pty_list().unwrap().is_empty());
}

#[test]
fn a_pty_that_does_not_exist_or_a_name_taken_is_refused_with_a_code() {
    // Given an executor with one pty
    let dir = tempfile::tempdir().unwrap();
    let ex = executor(dir.path());
    ex.pty_open(&open("one")).unwrap();
    // When a second open reuses the name, and write, resize and attach name a pty that is not there
    let again = ex.pty_open(&open("one")).unwrap_err();
    let write = ex.pty_write(&PtyWrite { pty: "ghost".into(), data: "x".into() }).unwrap_err();
    let resize = ex.pty_resize(&PtyResize { pty: "ghost".into(), cols: 10, rows: 10 }).unwrap_err();
    let attach = ex.pty_attach("a", &PtyAttach { pty: "ghost".into(), after_offset: None }, &mut |_, _| {}, &mut |_| {}).unwrap_err();
    // Then each is refused with its code, and the one pty is untouched
    assert_eq!((again.code, write.code, resize.code, attach.code), (Code::AlreadyExists, Code::NotFound, Code::NotFound, Code::NotFound));
    assert_eq!(ex.pty_list().unwrap().len(), 1);
    ex.pty_close(&PtyClose { pty: "one".into() }).unwrap();
}

/// An in-test rca: sends requests, collects the JSON frames by request id and the binary frames by attach id.
struct Wire {
    ws: WebSocket<TcpStream>,
    results: HashMap<String, Value>,
    events: HashMap<String, Vec<Value>>,
    binary: HashMap<String, Vec<(u64, Vec<u8>)>>,
}

impl Wire {
    fn new(ws: WebSocket<TcpStream>) -> Wire {
        Wire { ws, results: HashMap::new(), events: HashMap::new(), binary: HashMap::new() }
    }

    fn send(&mut self, id: &str, tool: &str, args: Value) {
        self.ws.send(Message::text(json!({"id": id, "tool": tool, "args": args}).to_string())).unwrap();
    }

    fn pump(&mut self) {
        match self.ws.read().unwrap() {
            Message::Text(t) => {
                let f: Value = serde_json::from_str(t.as_str()).unwrap();
                let id = f["id"].as_str().unwrap().to_string();
                if f.get("seq").is_some() {
                    self.events.entry(id).or_default().push(f["event"].clone());
                } else if f.get("ok").is_some() {
                    self.results.insert(id, f);
                }
            }
            Message::Binary(b) => {
                let (id, offset, data) = exec_pty::decode_frame(&b).expect("a frame of the contract");
                self.binary.entry(id).or_default().push((offset, data.to_vec()));
            }
            _ => {}
        }
    }

    fn request(&mut self, id: &str, tool: &str, args: Value) -> Value {
        self.send(id, tool, args);
        self.result(id)
    }

    fn result(&mut self, id: &str) -> Value {
        while !self.results.contains_key(id) {
            self.pump();
        }
        self.results[id].clone()
    }

    fn output(&self, id: &str) -> Vec<u8> {
        self.binary.get(id).map(|f| f.iter().flat_map(|(_, d)| d.clone()).collect()).unwrap_or_default()
    }

    fn until_output(&mut self, id: &str, needle: &[u8]) {
        while !contains(&self.output(id), needle) {
            self.pump();
        }
    }

    fn end(mut self) {
        self.ws.close(Some(tungstenite::protocol::CloseFrame { code: 1000.into(), reason: "".into() })).unwrap();
        while !matches!(self.ws.read(), Err(tungstenite::Error::ConnectionClosed)) {}
    }
}

fn rca(listener: TcpListener) -> Wire {
    let ws = accept(&listener, &Seen::default());
    ws.get_ref().set_read_timeout(Some(Duration::from_secs(20))).unwrap();
    Wire::new(ws)
}

/// What the in-test rca saw on one connection: the binary frames of the attach and the ordered events of its stream.
type Seen2 = (Vec<(u64, Vec<u8>)>, Vec<Value>);

#[test]
fn over_the_channel_a_pty_answers_with_binary_frames_and_a_reconnected_executor_resumes_by_offset() {
    // Given one executor that outlives its channels, and an in-test rca that serves two connections in turn
    let dir = tempfile::tempdir().unwrap();
    let ex: Arc<dyn ToolExecutor> = Arc::new(executor(dir.path()));
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let rca = std::thread::spawn(move || {
        // When rca opens a pty, attaches, and types a command whose output holds bytes that are not UTF-8
        let mut w = rca(listener.try_clone().unwrap());
        let opened = w.request("o", "pty_open", json!({"pty": "term", "cols": 90, "rows": 30}));
        w.send("att1", "pty_attach", json!({"pty": "term"}));
        let wrote = w.request("w1", "pty_write", json!({"pty": "term", "data": "printf '\\377\\376one\\n'\n"}));
        w.until_output("att1", b"\xff\xfeone\r\n");
        let first: Seen2 = (w.binary["att1"].clone(), w.events["att1"].clone());
        w.end();
        // And after the executor's channel is gone and a new one attaches, rca resumes from the offset it had reached
        let mut w = rca(listener);
        let resume_at = first.0.iter().map(|(_, d)| d.len() as u64).sum::<u64>();
        w.send("att2", "pty_attach", json!({"pty": "term", "after_offset": resume_at}));
        w.request("w2", "pty_write", json!({"pty": "term", "data": "echo two-$((1+1))\n"}));
        w.until_output("att2", b"two-2");
        w.send("att3", "pty_attach", json!({"pty": "term", "after_offset": 0}));
        w.until_output("att3", b"two-2");
        let listed = w.request("l", "pty_list", json!({}));
        let closed = w.request("c", "pty_close", json!({"pty": "term"}));
        let (a2, a3) = (w.result("att2"), w.result("att3"));
        let second = (w.binary["att2"].clone(), w.events["att2"].clone());
        let third = w.binary["att3"].clone();
        w.end();
        (opened, wrote, first, resume_at, second, third, listed, closed, a2, a3)
    });
    let first_channel = ExecChannel::open(&url, "pty", "tok", ex.clone()).unwrap();
    first_channel.wait().unwrap();
    let second_channel = ExecChannel::open(&url, "pty", "tok", ex).unwrap();
    let (opened, wrote, first, resume_at, second, third, listed, closed, a2, a3) = rca.join().unwrap();
    second_channel.wait().unwrap();
    // Then the pty opened at the asked size and took 23 bytes of input
    assert_eq!((opened["ok"].clone(), opened["result"]["cols"].clone(), opened["result"]["rows"].clone(), wrote["result"]["bytes"].clone()), (json!(true), json!(90), json!(30), json!(23)));
    // And the first attach's frames start at offset 0, follow each other without a gap, carry the raw bytes, and came after the attach event
    let mut next = 0;
    for (offset, data) in &first.0 {
        assert_eq!(*offset, next);
        next += data.len() as u64;
    }
    assert!(contains(&first.0.iter().flat_map(|(_, d)| d.clone()).collect::<Vec<_>>(), b"\xff\xfeone"));
    assert_eq!(first.1.len(), 1);
    assert_eq!((first.1[0]["kind"].clone(), first.1[0]["requested_offset"].clone(), first.1[0]["start_offset"].clone(), first.1[0]["reset"].clone()), (json!("pty.attached"), json!(0), json!(0), json!(false)));
    // And after the reconnect the second attach starts exactly at the offset reached, has the new output and none of the old
    assert_eq!(second.0[0].0, resume_at);
    assert_eq!(second.1[0]["reset"], json!(false));
    let again: Vec<u8> = second.0.iter().flat_map(|(_, d)| d.clone()).collect();
    assert!(contains(&again, b"two-2") && !contains(&again, b"\xff\xfeone"));
    // And an attach from zero replays the old output too, and the listing named the pty with the whole output's length
    assert!(contains(&third.iter().flat_map(|(_, d)| d.clone()).collect::<Vec<_>>(), b"\xff\xfeone"));
    assert_eq!(listed["result"]["ptys"][0]["pty"], json!("term"));
    // And closing the pty ended both attaches with the pty, and answered closed
    assert_eq!((closed["result"]["closed"].clone(), a2["result"]["exited"].clone(), a3["result"]["exited"].clone(), a2["result"]["detached"].clone()), (json!(true), json!(true), json!(true), json!(false)));
}

#[test]
fn closing_the_channel_while_attached_detaches_the_attach_and_leaves_the_pty_running() {
    // Given an attach open on a channel
    let dir = tempfile::tempdir().unwrap();
    let ex = Arc::new(executor(dir.path()));
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let rca = std::thread::spawn(move || {
        let mut w = rca(listener);
        w.request("o", "pty_open", json!({"pty": "keep"}));
        w.send("att", "pty_attach", json!({"pty": "keep"}));
        w.request("w", "pty_write", json!({"pty": "keep", "data": "echo up-$((1+1))\n"}));
        w.until_output("att", b"up-2\r\n");
        w
    });
    let channel = ExecChannel::open(&url, "pty", "tok", ex.clone() as Arc<dyn ToolExecutor>).unwrap();
    let w = rca.join().unwrap();
    // When the executor side closes the channel
    let closed = std::time::Instant::now();
    channel.close().ok();
    drop(w);
    // Then the close returns promptly (the attach did not hold it), and the pty is still there, running
    assert!(closed.elapsed() < Duration::from_secs(5));
    let info = ex.pty_list().unwrap().remove(0);
    assert_eq!((info.pty.as_str(), info.exited), ("keep", false));
    ex.pty_close(&PtyClose { pty: "keep".into() }).unwrap();
}
