//! The flow tests of `wf mimic` (the coverage tables and the pin tracking run on fixture trees whose references are real git checkouts, and on the repository's own tables): they enter through the thin `wf-mimic` binary (or `WF_BIN`, the full wf through its router) and
//! record, replay, diff and verify the toy reference `mimic-toy`, a program that makes up an id, a timestamp and a token on every run.
#[path = "flow/support/mod.rs"]
mod support;
#[path = "flow/coverage_support/mod.rs"]
mod coverage_support;
#[path = "flow/proxy_support/mod.rs"]
mod proxy_support;

#[path = "flow/child_env.rs"]
mod child_env;
#[path = "flow/coverage_orca.rs"]
mod coverage_orca;
#[path = "flow/coverage_oracle.rs"]
mod coverage_oracle;
#[path = "flow/coverage_recorded.rs"]
mod coverage_recorded;
#[path = "flow/coverage_repo.rs"]
mod coverage_repo;
#[path = "flow/coverage_t3.rs"]
mod coverage_t3;
#[path = "flow/coverage_track.rs"]
mod coverage_track;
#[path = "flow/crawl.rs"]
mod crawl;
#[path = "flow/fixtures.rs"]
mod fixtures;
#[path = "flow/help.rs"]
mod help;
#[path = "flow/import.rs"]
mod import;
#[path = "flow/real_clis.rs"]
mod real_clis;
#[path = "flow/proxy.rs"]
mod proxy;
#[path = "flow/proxy_canary.rs"]
mod proxy_canary;
#[path = "flow/proxy_record.rs"]
mod proxy_record;
#[path = "flow/proxy_shape.rs"]
mod proxy_shape;
#[path = "flow/proxy_vocabulary.rs"]
mod proxy_vocabulary;
#[path = "flow/record.rs"]
mod record;
#[path = "flow/record_projects.rs"]
mod record_projects;
#[path = "flow/record_references.rs"]
mod record_references;
#[path = "flow/replay.rs"]
mod replay;
#[path = "flow/script.rs"]
mod script;
#[path = "flow/stabilize.rs"]
mod stabilize;
#[path = "flow/t3_import.rs"]
mod t3_import;
#[path = "flow/tap.rs"]
mod tap;
#[path = "flow/verify.rs"]
mod verify;
