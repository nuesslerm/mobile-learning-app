//! `ScriptDriver`: a `mimic-driver/1` driver that plays cases with the step interpreter. It starts a server through a `Launch` when the first
//! case needs one, keeps it for the cases after, and starts a new one on `reset`; the interpreter (and the session its prelude built) lives
//! as long as the server does.
use mimic_core::ports::{Driver, DriverError, Hello, Observed, Request};
use mimic_script_core::case::Entry;
use mimic_script_core::{Interpreter, Phone, Policy, Target};
use mimic_script_net::{SystemClock, TcpTransport};
use std::net::SocketAddr;
use std::rc::Rc;

/// A started server as the driver sees it. Stopping is idempotent and also happens when the last handle is dropped.
pub trait Server {
    fn addr(&self) -> SocketAddr;
    fn workspace(&self) -> String;
    fn mint(&self) -> Result<String, String>;
    fn stop(&self);
    /// The Orca phone of a server that is reached through one instead of HTTP and WebSocket (`addr` is then never connected).
    fn phone(&self) -> Option<Box<dyn Phone>> {
        None
    }
    /// A deterministic one-commit repository `name` for a case's populated state (see `git::make_repo`).
    fn make_git(&self, _name: &str) -> Result<String, String> {
        Err("this server cannot make a repository".into())
    }
    /// Run-specific paths this server makes (its scratch folder) and the placeholder each becomes in every step.
    fn masks(&self) -> Vec<(String, String)> {
        Vec::new()
    }
}

pub trait Launch {
    fn start(&mut self) -> Result<Rc<dyn Server>, String>;
}

/// The interpreter's view of a started server.
struct Running {
    server: Rc<dyn Server>,
    generation: u64,
}

impl Target for Running {
    fn workspace(&self) -> String {
        self.server.workspace()
    }

    fn mint(&mut self) -> Result<String, String> {
        self.server.mint()
    }

    fn generation(&self) -> u64 {
        self.generation
    }

    fn make_git(&mut self, name: &str) -> Result<String, String> {
        self.server.make_git(name)
    }

    fn masks(&self) -> Vec<(String, String)> {
        self.server.masks()
    }
}

pub struct ScriptDriver {
    launch: Box<dyn Launch>,
    policy: Policy,
    prelude: Vec<Entry>,
    session: bool,
    target_version: Option<String>,
    generation: u64,
    current: Option<(Rc<dyn Server>, Interpreter)>,
}

impl ScriptDriver {
    pub fn new(launch: Box<dyn Launch>, policy: Policy, prelude: Vec<Entry>, session: bool, target_version: Option<String>) -> ScriptDriver {
        ScriptDriver { launch, policy, prelude, session, target_version, generation: 0, current: None }
    }

    fn interpreter(&mut self) -> Result<&mut Interpreter, DriverError> {
        if self.current.is_none() {
            let server = self.launch.start().map_err(DriverError)?;
            self.generation += 1;
            let target = Running { server: server.clone(), generation: self.generation };
            let mut interpreter = Interpreter::new(Box::new(TcpTransport::new(server.addr())), Box::new(target), Box::new(SystemClock::new()), self.policy.clone()).with_prelude(self.prelude.clone()).with_session(self.session);
            if let Some(phone) = server.phone() {
                interpreter = interpreter.with_phone(phone);
            }
            self.current = Some((server, interpreter));
        }
        Ok(&mut self.current.as_mut().expect("a server was started").1)
    }

    fn stop(&mut self) {
        if let Some((server, _)) = self.current.take() {
            server.stop();
        }
    }
}

impl Drop for ScriptDriver {
    fn drop(&mut self) {
        self.stop();
    }
}

impl Driver for ScriptDriver {
    fn hello(&mut self) -> Result<Hello, DriverError> {
        let mut hello = Interpreter::hello_info();
        hello.target_version = self.target_version.clone();
        Ok(hello)
    }

    fn run(&mut self, request: &Request) -> Result<Observed, DriverError> {
        self.interpreter()?.run(request)
    }

    fn reset(&mut self) -> Result<(), DriverError> {
        self.stop();
        Ok(())
    }
}
