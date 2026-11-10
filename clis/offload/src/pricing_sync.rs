//! `wf offload ladder sync-pricing`: rewrites model-pricing.json from OpenRouter's public model list, per MTok.
use rca_core::stats::{js_json_pretty, to_fixed};
use rca_core::time::iso;
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};

const DEFAULT_URL: &str = "https://openrouter.ai/api/v1/models";
const LADDERS: &str = "libraries/rca-core/plugins/ladder/data/model-ladders.json";
const PRICING: &str = "libraries/rca-core/data/model-pricing.json";

const LADDER_TO_OPENROUTER: &[(&str, &str)] = &[
    ("claude-haiku-4-5", "anthropic/claude-haiku-4.5"),
    ("claude-sonnet-5", "anthropic/claude-sonnet-5"),
    ("claude-opus-5", "anthropic/claude-opus-5"),
    ("minimax/MiniMax-M3", "minimax/minimax-m3"),
    ("opencode-go/deepseek-v4.1-flash", "deepseek/deepseek-v4.1-flash"),
    ("kimi-code-plan-global/k3", "moonshotai/kimi-k3"),
    ("kimi-code-plan-global/kimi-for-coding", "moonshotai/kimi-k2.7-code"),
];

/// `Number(value)` then per MTok; the toFixed(9) strips the binary-float noise of the single multiplication.
fn parse_rate(value: Option<&Value>) -> Option<f64> {
    let text = value?.as_str()?;
    let t = text.trim();
    let n = if t.is_empty() { 0.0 } else { t.parse::<f64>().ok()? };
    n.is_finite().then(|| to_fixed(n * 1_000_000.0, 9).parse().unwrap_or(0.0))
}

fn num(v: Option<f64>) -> Value {
    v.map_or(Value::Null, |n| json!(n))
}

fn resolve(models: &[Value], ladder: &[String]) -> Map<String, Value> {
    let mut rows = Map::new();
    for id in ladder {
        let listed = LADDER_TO_OPENROUTER
            .iter()
            .find(|(l, _)| l == id)
            .and_then(|(_, or)| models.iter().find(|m| m["id"].as_str() == Some(or)));
        let row = match listed {
            None => json!({
                "inputPerMTok": 0, "outputPerMTok": 0, "cacheReadPerMTok": null, "cacheWritePerMTok": null, "unknown": true
            }),
            Some(m) => {
                let p = m.get("pricing");
                let get = |k: &str| p.and_then(|p| p.get(k));
                json!({
                    "inputPerMTok": parse_rate(get("prompt")).unwrap_or(0.0),
                    "outputPerMTok": parse_rate(get("completion")).unwrap_or(0.0),
                    "cacheReadPerMTok": num(parse_rate(get("input_cache_read"))),
                    "cacheWritePerMTok": num(parse_rate(get("input_cache_write"))),
                })
            }
        };
        rows.insert(id.clone(), row);
    }
    rows
}

fn repo_root() -> PathBuf {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    cwd.ancestors().take(20).find(|d| d.join(".git").exists()).map_or(cwd.clone(), Path::to_path_buf)
}

fn fetch(url: &str) -> Result<Value, String> {
    let response = cli_http::Request::get(url).follow_redirects().send().map_err(|e| format!("OpenRouter pricing sync: {e}"))?;
    if !response.ok() {
        return Err(format!("OpenRouter pricing sync: HTTP {} from {url}", response.status));
    }
    serde_json::from_slice(&response.body).map_err(|e| format!("OpenRouter pricing sync: unreadable response: {e}"))
}

fn sync() -> Result<(), String> {
    let url = std::env::var("WF_OPENROUTER_MODELS_URL").unwrap_or_else(|_| DEFAULT_URL.to_string());
    let payload = fetch(&url)?;
    let root = repo_root();
    let ladders_text = std::fs::read_to_string(root.join(LADDERS)).map_err(|e| format!("cannot read {LADDERS}: {e}"))?;
    let ladders: Value = serde_json::from_str(&ladders_text).map_err(|e| format!("{LADDERS}: {e}"))?;
    let ids: Vec<String> = ladders["models"].as_object().map(|m| m.keys().cloned().collect()).unwrap_or_default();
    let data = payload.get("data").and_then(Value::as_array).cloned().unwrap_or_default();
    let fetched_at = iso(wf_kit::time::now_ms());
    let rows = resolve(&data, &ids);
    let unknown = rows.values().filter(|v| v["unknown"] == json!(true)).count();
    let total = rows.len();
    let mut file = Map::new();
    file.insert("fetchedAt".into(), json!(fetched_at));
    file.extend(rows);
    let out = root.join(PRICING);
    let tmp = out.with_extension("json.tmp");
    std::fs::write(&tmp, format!("{}\n", js_json_pretty(&Value::Object(file)))).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &out).map_err(|e| e.to_string())?;
    println!("wrote model-pricing.json: {total} ladder model(s), {unknown} unresolved (unknown), fetchedAt {fetched_at}");
    Ok(())
}

pub fn run() -> Option<i32> {
    match sync() {
        Ok(()) => Some(0),
        Err(e) => {
            wf_kit::wf_err!("wf: {e}");
            Some(1)
        }
    }
}
