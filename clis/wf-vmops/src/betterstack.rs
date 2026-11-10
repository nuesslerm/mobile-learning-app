//! The BetterStack read both `wf vm logs` and the Android harness make: the ClickHouse HTTP interface (basic auth, SQL as the
//! raw body). BetterStack keeps only the newest rows in `remote(<source>_logs)` and moves older ones to
//! `s3Cluster(primary, <source>_s3)`, so every read is the union of both.
use regex::Regex;
use serde_json::Value;

pub struct Config {
    host: String,
    user: String,
    password: String,
    pub table: String,
}

fn is_placeholder(value: &str) -> bool {
    let v = value.trim();
    v == "PLACEHOLDER_FILL_ME"
        || v.starts_with("PLACEHOLDER_FILL_ME_")
        || Regex::new(r"^<[^<>]+>$").unwrap().is_match(v)
}

/// Reads a source's four settings (filled from the vault first) or fails with the fix.
pub fn load_config(source: &str) -> Result<Config, String> {
    let prefix = format!("CH_{}", source.to_uppercase());
    let names = [
        format!("{prefix}_HOST"),
        format!("{prefix}_USER"),
        format!("{prefix}_PASSWORD"),
        format!("{prefix}_TABLE"),
    ];
    let keys: Vec<&str> = names.iter().map(String::as_str).collect();
    let loaded = secrets::load_values(&keys);
    let (mut missing, mut unfilled) = (Vec::new(), Vec::new());
    let mut read = |name: &String| -> String {
        match loaded.get(name).cloned().flatten() {
            None => {
                missing.push(name.clone());
                String::new()
            }
            Some(v) if is_placeholder(&v) => {
                unfilled.push(name.clone());
                String::new()
            }
            Some(v) => v,
        }
    };
    let cfg = Config {
        host: read(&names[0]),
        user: read(&names[1]),
        password: read(&names[2]),
        table: read(&names[3]),
    };
    if !unfilled.is_empty() {
        return Err(format!(
            "BetterStack config for source \"{source}\" still holds PLACEHOLDER values: {}.\nThese are ClickHouse connection credentials. They are NOT on the source's page in the dashboard — connections are team-level and created one per data region, so if no connection exists for this source's region there is nothing to look up yet.\nList what exists:   GET  https://telemetry.betterstack.com/api/v1/connections\nCreate one:         POST https://telemetry.betterstack.com/api/v1/connections {{\"client_type\":\"clickhouse\",\"data_region\":\"<region>\",\"team_ids\":[<team>]}}\nThe source's region comes from GET /api/v1/sources (field `data_region`), and both calls authenticate with BETTERSTACK_API_TOKEN. Then: printf %s '<value>' | wf secret update <KEY> --value-stdin --apply",
            unfilled.join(", ")
        ));
    }
    if !missing.is_empty() {
        return Err(format!(
            "BetterStack config for source \"{source}\" missing: {}.\nThese are admin-scoped VAULT secrets and are resolved automatically — but only through the active backend, which defaults to the macOS keychain and is a no-op off darwin. If the values live in Bitwarden rather than your local keychain, run with SECRETS_BACKEND=bitwarden (needs BWS_DEPLOY_SECRETS_EDIT_ACCESS_TOKEN — they are admin-scoped, so the read-only deploy token cannot see them). See `wf secret list` for where each value comes from.",
            missing.join(", ")
        ));
    }
    Ok(cfg)
}

/// The archived half of a `<source>_logs` table: BetterStack moves rows out of it every so often, so the newest rows of a
/// quiet source (the mobile app) can be wholly archived.
fn archive_table(table: &str) -> Option<String> {
    table.strip_suffix("_logs").map(|base| format!("{base}_s3"))
}

/// A FROM clause over a log table's recent rows and, when the table name allows, its archived rows (`_row_type = 1` keeps the
/// log rows and skips the archive's metadata rows); `filter` goes into both so each half is pruned by `dt`.
pub fn rows_source(table: &str, columns: &str, filter: &str) -> String {
    match archive_table(table) {
        Some(cold) => format!(
            "(SELECT {columns} FROM remote({table}) WHERE {filter} UNION ALL SELECT {columns} FROM s3Cluster(primary, {cold}) WHERE _row_type = 1 AND {filter})"
        ),
        None => format!("(SELECT {columns} FROM remote({table}) WHERE {filter})"),
    }
}

/// A ClickHouse error in a body curl reported as success (JSON envelope or a bare `Code: N.` line).
fn detect_clickhouse_error(body: &str) -> Option<String> {
    let head: String = body.chars().take(4000).collect();
    let squash = |s: &str| {
        Regex::new(r"\s+")
            .unwrap()
            .replace_all(s, " ")
            .trim()
            .to_string()
    };
    if Regex::new(r#""exception"\s*:"#).unwrap().is_match(&head) {
        if let Ok(Value::Object(m)) = serde_json::from_str::<Value>(&head) {
            if let Some(Value::String(e)) = m
                .get("exception")
                .filter(|e| e.as_str().is_some_and(|s| !s.is_empty()))
            {
                return Some(squash(e));
            }
        }
    }
    Regex::new(r"(Code:\s*\d+\.\s*DB::Exception:[^\n]*)")
        .unwrap()
        .captures(&head)
        .map(|c| squash(&c[1]))
}

pub fn query(cfg: &Config, sql: &str) -> Result<Vec<Vec<String>>, String> {
    // The SQL is the raw body: ClickHouse reads `plain/text` verbatim, so urlencoding would break it.
    let response = cli_http::Request::post(&format!("https://{}/", cfg.host))
        .basic_auth(&cfg.user, &cfg.password)
        .header("Content-Type", "plain/text")
        .body(sql.as_bytes().to_vec())
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .map_err(|e| format!("BetterStack query failed (exit 1):\n{e}"))?;
    let stdout = response.text();
    if let Some(e) = detect_clickhouse_error(&stdout) {
        return Err(format!("BetterStack query failed: {e}"));
    }
    let lines: Vec<&str> = stdout.split('\n').filter(|l| !l.is_empty()).collect();
    Ok(lines
        .iter()
        .skip(1)
        .map(|l| l.split('\t').map(str::to_string).collect())
        .collect())
}

/// The mobile source's lines that carry one harness run id (the app adds it to every line it forwards), oldest first, as
/// `(timestamp, raw JSON)` rows, from the last `since_s` seconds.
pub fn run_lines(cfg: &Config, run_id: &str, since_s: u64) -> Result<Vec<Vec<String>>, String> {
    let id = run_id.replace('\\', "\\\\").replace('\'', "\\'");
    let filter = format!("dt > now() - INTERVAL {since_s} SECOND AND JSONExtractString(raw, 'runId') = '{id}'");
    let rows = rows_source(&cfg.table, "dt, raw", &filter);
    query(cfg, &format!("SELECT formatDateTime(dt, '%Y-%m-%dT%H:%i:%S.%fZ') AS ts, raw FROM {rows} ORDER BY dt ASC LIMIT 2000 FORMAT TabSeparatedWithNames"))
}
