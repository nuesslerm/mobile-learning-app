//! The WebSocket on a worker thread that owns it: commands in, events out, so the interpreter never blocks on a read. The worker polls the
//! socket in short slices, sends what it was told to and reports frames, the peer's close and the connection ending.
use mimic_script_core::Event;
use std::net::TcpStream;
use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};
use std::time::Duration;
use tungstenite::protocol::CloseFrame;
use tungstenite::{Error, Message, WebSocket};

const SLICE: Duration = Duration::from_millis(5);

enum Command {
    Send(String),
    Close(u16, String),
}

pub struct Socket {
    commands: Sender<Command>,
    events: Receiver<Event>,
}

fn would_block(e: &Error) -> bool {
    matches!(e, Error::Io(io) if matches!(io.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut))
}

fn run(mut ws: WebSocket<TcpStream>, commands: Receiver<Command>, events: Sender<Event>) {
    let _ = ws.get_ref().set_read_timeout(Some(SLICE));
    loop {
        loop {
            match commands.try_recv() {
                Ok(Command::Send(text)) => {
                    let _ = ws.send(Message::text(text));
                }
                Ok(Command::Close(code, reason)) => {
                    let _ = ws.close(Some(CloseFrame { code: code.into(), reason: reason.into() }));
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return,
            }
        }
        match ws.read() {
            Ok(Message::Text(t)) => drop(events.send(Event::Frame { opcode: 1, data: t.as_str().as_bytes().to_vec() })),
            Ok(Message::Binary(b)) => drop(events.send(Event::Frame { opcode: 2, data: b.to_vec() })),
            Ok(Message::Close(frame)) => {
                let (code, reason) = frame.map_or((None, String::new()), |f| (Some(u16::from(f.code)), f.reason.to_string()));
                let _ = events.send(Event::Close { code, reason });
            }
            Ok(_) => {}
            Err(Error::ConnectionClosed | Error::AlreadyClosed) => {
                let _ = events.send(Event::Dropped);
                return;
            }
            Err(e) if would_block(&e) => {}
            Err(_) => {
                let _ = events.send(Event::Dropped);
                return;
            }
        }
    }
}

impl Socket {
    pub fn spawn(ws: WebSocket<TcpStream>) -> Socket {
        let (commands, command_rx) = channel();
        let (event_tx, events) = channel();
        std::thread::spawn(move || run(ws, command_rx, event_tx));
        Socket { commands, events }
    }

    pub fn send(&self, text: &str) {
        let _ = self.commands.send(Command::Send(text.to_string()));
    }

    pub fn close(&self, code: u16, reason: &str) {
        let _ = self.commands.send(Command::Close(code, reason.to_string()));
    }

    pub fn drain(&self) -> Vec<Event> {
        self.events.try_iter().collect()
    }
}
