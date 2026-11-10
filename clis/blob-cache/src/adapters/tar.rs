//! `Packer` over the `tar` binary: a gzipped archive of a directory's contents.
use crate::ports::Packer;
use std::path::Path;
use std::process::{Command, Stdio};

pub struct TarGz;

fn run(mut command: Command, what: &str) -> Result<(), String> {
    let output = command.stdin(Stdio::null()).output().map_err(|error| format!("tar did not start: {error}"))?;
    if output.status.success() {
        return Ok(());
    }
    let said = String::from_utf8_lossy(&output.stderr);
    Err(format!("tar {what} failed ({}): {}", output.status, said.lines().last().unwrap_or("no message")))
}

impl Packer for TarGz {
    fn pack(&self, dir: &Path, archive: &Path) -> Result<(), String> {
        let mut command = Command::new("tar");
        command.arg("-czf").arg(archive).arg("-C").arg(dir).arg(".");
        run(command, "pack")
    }

    fn unpack(&self, archive: &Path, dir: &Path) -> Result<(), String> {
        let mut command = Command::new("tar");
        command.arg("-xzf").arg(archive).arg("-C").arg(dir).arg("--no-same-owner");
        run(command, "unpack")
    }
}
