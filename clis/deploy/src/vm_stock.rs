//! The Hetzner stock verb: asks the Hetzner Cloud API which server types can be ordered where, and reports what changed since the last
//! result kept in a state file. The token is read-only and never printed, stored or put in an error.
use cli_http::Request;
use serde_json::{json, Value};
use std::time::Duration;
use wf_kit::args::parse;

const DEFAULT_API: &str = "https://api.hetzner.cloud/v1";
const TOKEN: &str = "HCLOUD_READ_TOKEN";
const MAX_PAGES: u64 = 20;

#[derive(Debug, PartialEq)]
pub struct Plan {
    pub name: String,
    pub arch: &'static str,
    pub category: String,
    pub vcpu: u64,
    pub ram_gb: f64,
    pub disk_gb: f64,
    pub price: Option<f64>,
    pub locations: Vec<(String, bool)>,
}

impl Plan {
    pub fn available(&self) -> bool {
        self.locations.iter().any(|(_, a)| *a)
    }

    fn available_in(&self) -> Vec<&str> {
        self.locations.iter().filter(|(_, a)| *a).map(|(n, _)| n.as_str()).collect()
    }
}

fn number(n: f64) -> Value {
    if n.fract() == 0.0 {
        json!(n as u64)
    } else {
        json!(n)
    }
}

fn price(item: &Value) -> Option<f64> {
    let prices = item["prices"].as_array()?;
    let chosen = prices.iter().find(|p| p["location"] == "hel1").or_else(|| prices.first())?;
    chosen["price_monthly"]["net"].as_str()?.parse().ok()
}

fn plan_of(item: &Value) -> Option<Plan> {
    let locations = item["locations"].as_array()?.iter().filter_map(|l| Some((l["name"].as_str()?.to_string(), l["available"].as_bool()?))).collect();
    Some(Plan {
        name: item["name"].as_str()?.to_string(),
        arch: if item["architecture"] == "arm" { "arm64" } else { "x86" },
        category: item["category"].as_str().unwrap_or_default().to_string(),
        vcpu: item["cores"].as_u64()?,
        ram_gb: item["memory"].as_f64()?,
        disk_gb: item["disk"].as_f64()?,
        price: price(item),
        locations,
    })
}

/// The family a name belongs to: its leading letters when digits follow (cx43 is cx, cax31 is cax).
fn family_of(name: &str) -> Option<String> {
    let letters: String = name.chars().take_while(|c| c.is_ascii_alphabetic()).collect();
    name[letters.len()..].starts_with(|c: char| c.is_ascii_digit()).then(|| letters.to_ascii_lowercase())
}

/// The plans of the families, at least `min_vcpu` and `min_ram` big, ordered by family as listed, then size.
pub fn select(types: &[Value], families: &[String], min_vcpu: u64, min_ram: f64) -> Vec<Plan> {
    let mut chosen: Vec<(usize, Plan)> = types
        .iter()
        .filter_map(plan_of)
        .filter_map(|p| {
            let at = families.iter().position(|f| family_of(&p.name).as_deref() == Some(f.as_str()))?;
            (p.vcpu >= min_vcpu && p.ram_gb >= min_ram).then_some((at, p))
        })
        .collect();
    chosen.sort_by(|(fa, a), (fb, b)| (fa, a.vcpu, &a.name).cmp(&(fb, b.vcpu, &b.name)));
    chosen.into_iter().map(|(_, p)| p).collect()
}

fn plan_json(p: &Plan) -> Value {
    json!({
        "name": p.name, "arch": p.arch, "category": p.category, "vcpu": p.vcpu, "ramGb": number(p.ram_gb), "diskGb": number(p.disk_gb),
        "priceMonthExVatEur": p.price, "locations": p.locations.iter().map(|(n, a)| (n.clone(), json!(a))).collect::<serde_json::Map<_, _>>(),
        "available": p.available(), "availableIn": p.available_in(),
    })
}

fn locations_of(plan: &Value) -> Vec<(String, bool)> {
    plan["locations"].as_object().into_iter().flatten().filter_map(|(n, a)| Some((n.clone(), a.as_bool()?))).collect()
}

fn flag(locations: &[(String, bool)], name: &str) -> bool {
    locations.iter().any(|(n, a)| n == name && *a)
}

fn overall(locations: &[(String, bool)]) -> bool {
    locations.iter().any(|(_, a)| *a)
}

/// Plans whose overall availability flipped, appeared (was false) or vanished (now false), with the locations that differ.
pub fn changes(before: &Value, plans: &[Plan]) -> Vec<Value> {
    let was: Vec<(String, Vec<(String, bool)>)> = before["plans"].as_array().into_iter().flatten().filter_map(|p| Some((p["name"].as_str()?.to_string(), locations_of(p)))).collect();
    let differing = |old: &[(String, bool)], new: &[(String, bool)]| -> Vec<String> {
        let mut names: Vec<&String> = Vec::new();
        for (n, _) in old.iter().chain(new) {
            if !names.contains(&n) {
                names.push(n);
            }
        }
        names.into_iter().filter(|n| flag(old, n) != flag(new, n)).cloned().collect()
    };
    let mut out: Vec<Value> = plans
        .iter()
        .filter_map(|p| {
            let old = was.iter().find(|(n, _)| *n == p.name).map(|(_, l)| l.as_slice()).unwrap_or(&[]);
            (overall(old) != p.available()).then(|| json!({ "name": p.name, "was": overall(old), "now": p.available(), "locations": differing(old, &p.locations) }))
        })
        .collect();
    for (name, old) in &was {
        if overall(old) && !plans.iter().any(|p| &p.name == name) {
            out.push(json!({ "name": name, "was": true, "now": false, "locations": differing(old, &[]) }));
        }
    }
    out
}

fn summary(plans: &[Plan]) -> String {
    if plans.is_empty() {
        return "no plans match".to_string();
    }
    let up: Vec<String> = plans.iter().filter(|p| p.available()).map(|p| format!("{} ({})", p.name, p.available_in().join(","))).collect();
    let down: Vec<&str> = plans.iter().filter(|p| !p.available()).map(|p| p.name.as_str()).collect();
    let mut line = format!("orderable now: {}", if up.is_empty() { "none".to_string() } else { up.join(", ") });
    if !down.is_empty() {
        line.push_str(&format!("; unavailable: {}", down.join(", ")));
    }
    line
}

fn fail(error: &str) -> Option<i32> {
    println!("{}", json!({ "ok": false, "error": error }));
    Some(1)
}

fn token() -> Option<String> {
    std::env::var(TOKEN).ok().filter(|t| !t.is_empty()).or_else(|| secrets::load_values(&[TOKEN]).get(TOKEN).cloned().flatten().filter(|t| !t.is_empty()))
}

/// Every server type of the account's catalogue, following `next_page`. Errors never carry the token.
fn fetch(api: &str, token: &str) -> Result<Vec<Value>, String> {
    let mut types = Vec::new();
    let mut page = 1;
    loop {
        let paged = if page == 1 { String::new() } else { format!("&page={page}") };
        let url = format!("{}/server_types?per_page=50{paged}", api.trim_end_matches('/'));
        let reply = Request::get(&url)
            .header("Authorization", &format!("Bearer {token}"))
            .header("User-Agent", "wf")
            .timeout(Duration::from_secs(30))
            .send()
            .map_err(|_| format!("the API at {api} did not answer"))?;
        match reply.status {
            401 | 403 => return Err("the token was refused".to_string()),
            s if !(200..300).contains(&s) => return Err(format!("API answered {s}")),
            _ => {}
        }
        let body: Value = serde_json::from_slice(&reply.body).unwrap_or(Value::Null);
        let Some(batch) = body["server_types"].as_array() else {
            return Err("no server types in the answer: the API changed".to_string());
        };
        types.extend(batch.iter().cloned());
        match body["meta"]["pagination"]["next_page"].as_u64() {
            Some(next) if next > page && next <= MAX_PAGES => page = next,
            _ => return Ok(types),
        }
    }
}

fn count(opts: &std::collections::HashMap<String, String>, key: &str) -> Result<f64, String> {
    opts.get(key).map_or(Ok(0.0), |v| v.parse::<f64>().ok().filter(|n| *n >= 0.0).ok_or(format!("--{key} needs a number, got '{v}'")))
}

pub fn run(args: &[String]) -> Option<i32> {
    let p = parse(args, &["provider", "family", "min-vcpu", "min-ram-gb", "api", "state"], &[]).filter(|p| p.positional.is_empty())?;
    let provider = p.opts.get("provider").map_or("hetzner", String::as_str);
    if provider != "hetzner" {
        return fail(&format!("unknown provider '{provider}': hetzner is the only one"));
    }
    let families: Vec<String> = p.opts.get("family").map_or("cx,cax", String::as_str).split(',').map(|f| f.trim().to_ascii_lowercase()).filter(|f| !f.is_empty()).collect();
    let (min_vcpu, min_ram) = match (count(&p.opts, "min-vcpu"), count(&p.opts, "min-ram-gb")) {
        (Ok(v), Ok(r)) if v.fract() == 0.0 => (v as u64, r),
        (Ok(v), Ok(_)) => return fail(&format!("--min-vcpu needs a whole number, got '{v}'")),
        (Err(e), _) | (_, Err(e)) => return fail(&e),
    };
    let Some(token) = token() else {
        return fail("HCLOUD_READ_TOKEN is not set");
    };
    let api = p.opts.get("api").map_or(DEFAULT_API, String::as_str);
    let types = match fetch(api, &token) {
        Ok(types) => types,
        Err(why) => return fail(&why),
    };
    let plans = select(&types, &families, min_vcpu, min_ram);
    let state = p.opts.get("state");
    let previous: Option<Value> = state.and_then(|f| std::fs::read_to_string(f).ok()).and_then(|t| serde_json::from_str(&t).ok());
    let mut result = json!({
        "ok": true,
        "provider": provider,
        "source": "api",
        "plans": plans.iter().map(plan_json).collect::<Vec<_>>(),
        "available": plans.iter().filter(|p| p.available()).map(|p| p.name.clone()).collect::<Vec<_>>(),
        "summary": summary(&plans),
    });
    if state.is_some() {
        result["changed"] = Value::Array(previous.as_ref().map(|before| changes(before, &plans)).unwrap_or_default());
        result["firstRun"] = json!(previous.is_none());
    }
    if let Some(file) = state {
        if let Err(why) = std::fs::write(file, result.to_string()) {
            return fail(&format!("cannot write the state file {file}: {why}"));
        }
    }
    println!("{result}");
    Some(0)
}
