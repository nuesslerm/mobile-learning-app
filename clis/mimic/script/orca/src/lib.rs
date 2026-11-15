//! The Orca target: a started server is the codec sidecar (the phone's own pinned client behind JSON lines) with the host it pairs with.
//! The interpreter reaches it only through its `Phone`; there is no HTTP address. Repositories a case names are made under a scratch folder of
//! their own each, so a path an earlier case gave the host never comes back, and every one of them is masked as `<scratch>` in the steps.
use mimic_script_core::Phone;
use mimic_script_host::driver::{Launch, Server};
use mimic_script_host::git::make_repo;
use mimic_script_net::{Sidecar, SidecarPhone};
use std::cell::RefCell;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};

static SCRATCH: AtomicU64 = AtomicU64::new(0);

/// How to start the sidecar: its command and exactly the environment it gets (the host to pair with is named there, by variable, never in a case).
pub struct OrcaLaunch {
    pub sidecar: Vec<String>,
    pub env: Vec<(String, String)>,
    pub cwd: Option<PathBuf>,
}

struct OrcaServer {
    phone: SidecarPhone,
    roots: RefCell<Vec<PathBuf>>,
}

impl Server for OrcaServer {
    fn addr(&self) -> SocketAddr {
        SocketAddr::from(([127, 0, 0, 1], 0))
    }

    fn workspace(&self) -> String {
        String::new()
    }

    fn mint(&self) -> Result<String, String> {
        Err("an Orca host is paired through the environment, not minted".into())
    }

    fn stop(&self) {
        self.phone.0.borrow_mut().stop();
        for root in self.roots.borrow_mut().drain(..) {
            let _ = std::fs::remove_dir_all(root);
        }
    }

    fn phone(&self) -> Option<Box<dyn Phone>> {
        Some(Box::new(self.phone.clone()))
    }

    fn make_git(&self, name: &str) -> Result<String, String> {
        let root = std::env::temp_dir().join(format!("mimic-scratch-{}-{}", std::process::id(), SCRATCH.fetch_add(1, Ordering::Relaxed)));
        let repo = root.join(name);
        make_repo(&repo)?;
        self.roots.borrow_mut().push(root);
        Ok(repo.display().to_string())
    }

    fn masks(&self) -> Vec<(String, String)> {
        self.roots.borrow().iter().map(|r| (r.display().to_string(), "<scratch>".to_string())).collect()
    }
}

impl Drop for OrcaServer {
    fn drop(&mut self) {
        self.stop();
    }
}

impl Launch for OrcaLaunch {
    fn start(&mut self) -> Result<Rc<dyn Server>, String> {
        let (program, args) = self.sidecar.split_first().ok_or("the Orca target names no sidecar command")?;
        let sidecar = Sidecar::spawn(program, args, &self.env, self.cwd.as_deref())?;
        Ok(Rc::new(OrcaServer { phone: SidecarPhone::new(sidecar), roots: RefCell::default() }))
    }
}
