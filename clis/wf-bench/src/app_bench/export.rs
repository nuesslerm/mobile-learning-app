//! A web or native export of the app (`expo export`) as the benchmarks run it: cold or warm, timed up to the
//! `Exported:` line, the output measured on disk.
use super::domain::export_finished;
use flate2::{write::GzEncoder, Compression};
use serde_json::Value;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

// Expo keeps its process alive for minutes after printing `Exported:`, so the clock stops at that line and the
// process group is ended; the same limit kills an export that never prints it.
const EXPORT_TIMEOUT: Duration = Duration::from_secs(15 * 60);

pub struct ExportParams {
    pub cwd: String,
    pub platforms: Vec<String>,
    pub source_maps: bool,
    pub env: Vec<(String, String)>,
    pub device_lab_flag: String,
}

impl ExportParams {
    pub fn from(params: &Value) -> Result<ExportParams, String> {
        let env: Vec<(String, String)> = params["env"].as_object().map(|e| e.iter().filter_map(|(k, v)| v.as_str().map(|v| (k.clone(), v.to_string()))).collect()).unwrap_or_default();
        let platform = params["platform"].as_str().unwrap_or("web").to_string();
        let platforms = match params["platforms"].as_array() {
            Some(list) => list.iter().filter_map(|p| p.as_str().map(str::to_string)).collect(),
            None => vec![platform],
        };
        Ok(ExportParams {
            cwd: params["cwd"].as_str().ok_or("scenario params lack a cwd")?.to_string(),
            platforms,
            source_maps: params["sourceMaps"].as_bool() == Some(true),
            device_lab_flag: env.iter().find(|(k, _)| k == "EXPO_PUBLIC_DEVICE_LAB").map(|(_, v)| v.clone()).unwrap_or_default(),
            env,
        })
    }

    /// The same parameters with more environment variables (the candidate arm of an A/B run).
    pub fn with_env(&self, extra: &[(String, String)]) -> ExportParams {
        let mut env = self.env.clone();
        for (k, v) in extra {
            match env.iter_mut().find(|(ek, _)| ek == k) {
                Some(entry) => entry.1 = v.clone(),
                None => env.push((k.clone(), v.clone())),
            }
        }
        ExportParams { cwd: self.cwd.clone(), platforms: self.platforms.clone(), source_maps: self.source_maps, device_lab_flag: self.device_lab_flag.clone(), env }
    }
}

pub struct Exporter {
    pub params: ExportParams,
    app_dir: PathBuf,
    out_dir: PathBuf,
}

pub struct ExportRun {
    pub ms: u64,
    pub log: String,
}

impl Exporter {
    pub fn new(params: ExportParams, repo_root: &Path, out_dir: &Path) -> Exporter {
        Exporter { app_dir: repo_root.join(&params.cwd), params, out_dir: out_dir.to_path_buf() }
    }

    /// Web keeps the root directory (the memory and runtime suites serve it); other platforms sit beside it.
    pub fn dir_for(&self, platform: &str) -> PathBuf {
        if platform == "web" {
            self.out_dir.clone()
        } else {
            PathBuf::from(format!("{}-{platform}", self.out_dir.display()))
        }
    }

    pub fn clear_caches(&self) {
        let _ = fs::remove_dir_all(self.app_dir.join("node_modules/.cache"));
        let _ = fs::remove_dir_all("/tmp/metro-cache");
        if let Ok(entries) = fs::read_dir("/tmp") {
            for entry in entries.flatten() {
                if entry.file_name().to_string_lossy().starts_with("metro-file-map-") {
                    let _ = fs::remove_dir_all(entry.path());
                }
            }
        }
    }

    pub fn run_export(&self, platform: &str) -> Result<ExportRun, String> {
        let dir = self.dir_for(platform);
        let _ = fs::remove_dir_all(&dir);
        let mut args: Vec<String> = ["--config.verify-deps-before-run=false", "exec", "expo", "export", "--platform", platform, "--output-dir"].map(String::from).to_vec();
        args.push(dir.to_string_lossy().into_owned());
        if self.params.source_maps {
            args.push("--source-maps".into());
        }
        let started = Instant::now();
        let mut child = Command::new("pnpm")
            .args(&args)
            .current_dir(&self.app_dir)
            .envs(self.params.env.iter().map(|(k, v)| (k, v)))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0)
            .spawn()
            .map_err(|e| format!("cannot run pnpm: {e}"))?;
        let group = child.id() as i32;
        let (tx, rx) = mpsc::channel::<String>();
        for stream in [child.stdout.take().map(|s| Box::new(s) as Box<dyn std::io::Read + Send>), child.stderr.take().map(|s| Box::new(s) as Box<dyn std::io::Read + Send>)].into_iter().flatten() {
            let tx = tx.clone();
            std::thread::spawn(move || {
                let mut reader = BufReader::new(stream);
                let mut line = Vec::new();
                while reader.read_until(b'\n', &mut line).unwrap_or(0) > 0 {
                    if tx.send(String::from_utf8_lossy(&line).into_owned()).is_err() {
                        break;
                    }
                    line.clear();
                }
            });
        }
        drop(tx);
        let mut log = String::new();
        let kill_group = || {
            // The child leads its own process group; this signals only that group.
            let _ = nix::sys::signal::killpg(nix::unistd::Pid::from_raw(group), nix::sys::signal::Signal::SIGKILL);
        };
        loop {
            match rx.recv_timeout(EXPORT_TIMEOUT.saturating_sub(started.elapsed())) {
                Ok(chunk) => {
                    log.push_str(&chunk);
                    if export_finished(&log) {
                        let ms = started.elapsed().as_millis() as u64;
                        kill_group();
                        let _ = child.wait();
                        return Ok(ExportRun { ms, log });
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    kill_group();
                    let _ = child.wait();
                    return Err(format!("export timed out after {} minutes\n{}", EXPORT_TIMEOUT.as_secs() / 60, tail(&log, 2000)));
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
        let ms = started.elapsed().as_millis() as u64;
        let status = child.wait().map_err(|e| e.to_string())?;
        kill_group();
        if status.success() {
            Ok(ExportRun { ms, log })
        } else {
            Err(format!("export exited {}\n{}", status.code().map_or("null".into(), |c| c.to_string()), tail(&log, 2000)))
        }
    }

    /// Bytes of the JavaScript the export wrote (`.js` for web, Hermes `.hbc` otherwise), raw and gzipped.
    pub fn measure_output(&self, platform: &str) -> (u64, u64) {
        let web = platform == "web";
        let mut raw = 0u64;
        let mut gzip = 0u64;
        for file in output_files(&self.dir_for(platform).join("_expo"), if web { ".js" } else { ".hbc" }) {
            let Ok(bytes) = fs::read(&file) else { continue };
            raw += bytes.len() as u64;
            if web {
                let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
                if encoder.write_all(&bytes).is_ok() {
                    gzip += encoder.finish().map_or(0, |v| v.len() as u64);
                }
            }
        }
        (raw, gzip)
    }

    pub fn environment(&self) -> Vec<(String, String)> {
        let node = Command::new("node").arg("--version").output().ok().filter(|o| o.status.success()).map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_else(|| "none".into());
        let os = fs::read_to_string("/proc/sys/kernel/osrelease").or_else(|_| Command::new("uname").arg("-r").output().map(|o| String::from_utf8_lossy(&o.stdout).into_owned())).unwrap_or_default();
        let name = if std::env::consts::OS == "macos" { "darwin" } else { std::env::consts::OS };
        vec![
            ("node".into(), node),
            ("os".into(), format!("{name} {}", os.trim())),
            ("cpus".into(), std::thread::available_parallelism().map_or(1, |n| n.get()).to_string()),
            ("platforms".into(), self.params.platforms.join(",")),
            ("deviceLabFlag".into(), self.params.device_lab_flag.clone()),
        ]
    }
}

fn tail(text: &str, n: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    chars[chars.len().saturating_sub(n)..].iter().collect()
}

fn output_files(dir: &Path, extension: &str) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else { return Vec::new() };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            out.extend(output_files(&path, extension));
        } else if entry.file_name().to_string_lossy().ends_with(extension) {
            out.push(path);
        }
    }
    out
}
