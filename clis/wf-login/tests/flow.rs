//! The flow tests of `wf secret login`: they enter through the thin `wf-login` binary's argv (the listener through the same binary, or
//! `VNC_AUTH_LISTENER_BIN`, the image's own build of it), with the tools' own login commands, `tailscale`, `s6-setuidgid` and `bws` as
//! stubs and the listener, the backend, vm-ops-api and the delivery sink as mock HTTP services.
#[path = "flow/support/mod.rs"]
mod support;

#[path = "rig/mod.rs"]
mod rig;

#[path = "flow/auth.rs"]
mod auth;
