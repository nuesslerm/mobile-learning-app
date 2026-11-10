//! One anonymised row per wf command in HEALTH_RPT.WF_USAGE (infra/oracle-cloud/sql/wf_usage.sql): the command path, exit
//! code, duration, the last error it printed, wf's version and the OS; never the host, the arguments or a key name. A detached
//! copy of wf posts it, so the command never waits; a post that failed is printed by the next command.
use serde_json::json;
use std::path::PathBuf;
use std::time::{Duration, Instant};

const URL_KEY: &str = "WF_USAGE_ORDS_BASE_URL";
const PASSWORD_KEY: &str = "HEALTH_REPORTS_ORDS_PASSWORD";
const ROW_VAR: &str = "WF_USAGE_ROW";
const ERROR_BYTES: usize = 1000;

fn get(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|v| !v.is_empty())
}

fn failure_file() -> Option<PathBuf> {
    get("HOME").map(|home| PathBuf::from(home).join(".cache/wf/usage.error"))
}

/// The error as stored: secrets and personal data scrubbed, the home directory as `~`, at most the column's 1000 bytes.
fn anonymised(error: &str, home: Option<&str>) -> String {
    let mut text = telemetry::redact::Redactor::from_env(Vec::new()).scrub(error).into_owned();
    if let Some(home) = home.filter(|h| h.len() > 1) {
        text = text.replace(home, "~");
    }
    let mut end = text.len().min(ERROR_BYTES);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.truncate(end);
    text
}

fn row(args: &[String], code: i32, elapsed: Duration, error: Option<&str>, home: Option<&str>, version: &str) -> serde_json::Value {
    let path = wf_kit::help::command_path(args).trim_start_matches("wf").trim().to_string();
    let command = if path.is_empty() { args.first().filter(|a| crate::routes::is_command(a)).cloned().unwrap_or_default() } else { path };
    // ORDS inserts NULL for an absent column, which would override TS's default.
    json!({
        "ts": wf_kit::time::iso(wf_kit::time::now_ms()),
        "command": if command.is_empty() { "(none)".to_string() } else { command },
        "exit_code": code,
        "duration_ms": u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX),
        "error": error.map(|e| anonymised(e, home)),
        "wf_version": version,
        "os": std::env::consts::OS,
    })
}

/// Called once when a command ends: hands its row to a detached `wf` and reports a row that could not be sent.
pub fn record(args: &[String], code: i32, started: Instant, version: &str) {
    if std::env::var("WF_USAGE").as_deref() == Ok("off") {
        return;
    }
    if let Some(file) = failure_file() {
        if let Ok(reason) = std::fs::read_to_string(&file) {
            let _ = std::fs::remove_file(&file);
            eprintln!("wf: the last usage row was not recorded: {}", reason.trim());
        }
    }
    if [URL_KEY, PASSWORD_KEY].iter().any(|k| get(k).is_none()) {
        // a command served by its help ends before the env files are read
        wf_kit::env_file::load();
    }
    if [URL_KEY, PASSWORD_KEY].iter().any(|k| get(k).is_none()) {
        // the CI runner holds only the vault read token, so its commands read the keys from the vault (its sealed copy)
        for (key, value) in secrets::load_values(&[URL_KEY, PASSWORD_KEY]) {
            if let (None, Some(value)) = (get(&key), value) {
                std::env::set_var(&key, value);
            }
        }
    }
    let missing: Vec<&str> = [URL_KEY, PASSWORD_KEY].into_iter().filter(|k| get(k).is_none()).collect();
    if !missing.is_empty() {
        eprintln!("wf: usage row not recorded: {} unset (put them in ~/.config/wf/.env or the vault, or set WF_USAGE=off)", missing.join(" and "));
        return;
    }
    let error = wf_kit::usage::last_error().filter(|_| code != 0);
    let row = row(args, code, started.elapsed(), error.as_deref(), get("HOME").as_deref(), version);
    let spawned = std::env::current_exe().and_then(|exe| {
        use std::os::unix::process::CommandExt;
        std::process::Command::new(exe)
            .env(ROW_VAR, row.to_string())
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .process_group(0)
            .spawn()
    });
    if let Err(e) = spawned {
        eprintln!("wf: usage row not recorded: {e}");
    }
}

/// In the detached copy (`WF_USAGE_ROW` set): posts the row and keeps a failure for the next command to print.
pub fn send_if_asked() -> Option<i32> {
    let row = get(ROW_VAR)?;
    let sent = (|| {
        let (url, password) = (get(URL_KEY).ok_or(format!("{URL_KEY} unset"))?, get(PASSWORD_KEY).ok_or(format!("{PASSWORD_KEY} unset"))?);
        let user = get("HEALTH_REPORTS_ORDS_USER").unwrap_or_else(|| "health_rpt".into());
        let response = crate::http::Request::post(&url)
            .basic_auth(&user, &password)
            .header("Content-Type", "application/json")
            .body(row.into_bytes())
            .timeout(Duration::from_secs(15))
            .send()?;
        if response.ok() {
            Ok(())
        } else {
            Err(format!("ORDS answered {}: {}", response.status, response.text().chars().take(300).collect::<String>()))
        }
    })();
    if let (Err(why), Some(file)) = (&sent, failure_file()) {
        if let Some(dir) = file.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(&file, why);
    }
    Some(if sent.is_ok() { 0 } else { 1 })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_a_command_line_when_its_row_is_built_then_it_names_the_command_only() {
        wf_kit::help::register(&[crate::help_table::HELP, secrets::HELP], wf_harness::device::cli::help_hooks());
        let args: Vec<String> = ["secret", "get", "GROQ_API_KEY", "--json"].map(String::from).to_vec();
        let r = row(&args, 0, Duration::from_millis(42), None, None, "0.0.0");

        assert_eq!((r["command"].as_str(), r["exit_code"].as_i64(), r["duration_ms"].as_u64()), (Some("secret get"), Some(0), Some(42)));
        assert!(!r.to_string().contains("GROQ") && r["error"].is_null(), "{r}");
    }

    #[test]
    fn given_an_error_with_personal_data_and_the_home_path_when_stored_then_both_are_gone_and_it_fits_the_column() {
        let text = anonymised("wf: cannot read /home/ana/.ssh/key for me@example.com", Some("/home/ana"));

        assert_eq!(text, "wf: cannot read ~/.ssh/key for [REDACTED]");
        assert!(anonymised(&"é".repeat(800), None).len() <= ERROR_BYTES);
    }
}
