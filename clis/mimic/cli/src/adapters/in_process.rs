use mimic_core::cassette::DriverInfo;
use mimic_core::ports::{Driver, DriverError, Hello, Observed, Request};

/// A Rust closure as the target, for ports of a reference that live in the same process (the orca-crypto and DSL ports).
pub struct InProcessDriver<F: FnMut(&Request) -> Result<Observed, DriverError>> {
    info: DriverInfo,
    target_version: Option<String>,
    run: F,
}

impl<F: FnMut(&Request) -> Result<Observed, DriverError>> InProcessDriver<F> {
    pub fn new(name: &str, target_version: Option<&str>, run: F) -> Self {
        let info = DriverInfo { name: name.into(), version: "1".into(), protocol: "mimic-driver/1".into(), toolchain_lock_hash: None };
        Self { info, target_version: target_version.map(String::from), run }
    }
}

impl<F: FnMut(&Request) -> Result<Observed, DriverError>> Driver for InProcessDriver<F> {
    fn hello(&mut self) -> Result<Hello, DriverError> {
        Ok(Hello { driver: self.info.clone(), target_version: self.target_version.clone(), coverage: false })
    }

    fn run(&mut self, request: &Request) -> Result<Observed, DriverError> {
        (self.run)(request)
    }
}
