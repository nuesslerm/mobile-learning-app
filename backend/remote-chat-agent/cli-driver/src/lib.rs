//! Running a provider CLI for a turn, without the transport and without the file system: `domain` (pure: what a CLI's protocol lines mean, which environment a
//! provider may see), `application` (the live sessions of each CLI, which reach a process only through the `Linker` port). The service's adapter starts the
//! processes; `ScriptedLinker` (feature `testing`) plays them in tests.

pub mod application;
pub mod domain;

#[cfg(any(test, feature = "testing"))]
pub mod testing;
