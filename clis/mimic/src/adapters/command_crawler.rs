//! The `Crawler` port over a child process: the command prints one `mimic-crawl-report/1` object on stdout (the driver's `crawl` verb,
//! which reads the pinned reference's source on Node). The child inherits the environment, so `MIMIC_T3_ROOT` and `MIMIC_ORCA_ROOT` reach it.
use super::StdioSpec;
use mimic_core::crawl::{CrawlError, Crawler, Report};
use std::process::{Command, Stdio};

pub struct CommandCrawler {
    spec: StdioSpec,
}

impl CommandCrawler {
    pub fn new(spec: StdioSpec) -> Self {
        Self { spec }
    }
}

impl Crawler for CommandCrawler {
    fn crawl(&mut self) -> Result<Report, CrawlError> {
        let (program, args) = self.spec.command.split_first().ok_or_else(|| CrawlError("the crawl command is empty".into()))?;
        let mut command = Command::new(program);
        command.args(args).envs(self.spec.env.iter().map(|(k, v)| (k, v))).stdin(Stdio::null());
        if let Some(cwd) = &self.spec.cwd {
            command.current_dir(cwd);
        }
        wf_kit::logfmt::mcp("info", "mimic", "crawl.spawn", &[("program", program.clone())]);
        let out = command.output().map_err(|e| CrawlError(format!("cannot start {program}: {e}")))?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            let tail: Vec<&str> = stderr.lines().rev().take(8).collect();
            return Err(CrawlError(format!("{program} exited {}: {}", out.status, tail.into_iter().rev().collect::<Vec<_>>().join(" | "))));
        }
        serde_json::from_slice(&out.stdout).map_err(|e| CrawlError(format!("the crawl command did not print a mimic-crawl-report/1 object: {e}")))
    }
}
