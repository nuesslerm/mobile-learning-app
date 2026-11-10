//! Running a provider CLI for a turn, without the transport and without the file system: `domain` (pure: the command a driver runs, what its
//! output lines mean, which environment a provider may see), `application` (the turns in flight, which reach a process only through the
//! `Spawner` port). The service's adapter starts the processes; `ScriptedSpawner` (feature `testing`) plays them in tests.

pub mod application;
pub mod domain;

#[cfg(any(test, feature = "testing"))]
pub mod testing;
