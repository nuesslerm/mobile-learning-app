use std::time::Duration;

/// Where the browser runs: on the host's own display (a person at the machine) or on the container's virtual display, which the
/// person reaches through noVNC.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Local,
    Remote,
}

#[derive(Clone, Debug)]
pub struct CaptureConfig {
    pub mode: Mode,
    pub novnc_port: u16,
    pub capture_timeout: Duration,
    pub poll_interval: Duration,
    pub login_url: String,
}
