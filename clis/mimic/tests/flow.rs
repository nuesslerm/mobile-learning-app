//! The flow tests of `wf mimic`: they enter through the thin `wf-mimic` binary (or `WF_BIN`, the full wf through its router) and
//! record, replay, diff and verify the toy reference `mimic-toy`, a program that makes up an id, a timestamp and a token on every run.
#[path = "flow/support/mod.rs"]
mod support;

#[path = "flow/crawl.rs"]
mod crawl;
#[path = "flow/fixtures.rs"]
mod fixtures;
#[path = "flow/help.rs"]
mod help;
#[path = "flow/import.rs"]
mod import;
#[path = "flow/record.rs"]
mod record;
#[path = "flow/record_references.rs"]
mod record_references;
#[path = "flow/replay.rs"]
mod replay;
#[path = "flow/stabilize.rs"]
mod stabilize;
#[path = "flow/t3_import.rs"]
mod t3_import;
#[path = "flow/tap.rs"]
mod tap;
#[path = "flow/verify.rs"]
mod verify;
