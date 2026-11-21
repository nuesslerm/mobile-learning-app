//! `wf vm net device list|retire`, `auth-key-status` and the dry run of `auth-key rotate`: reads and device
//! deletes through the Tailscale admin API (OAuth client credentials). `up` joins through the local tailscaled and
//! `auth-key rotate --apply` mints a key and writes it to the vault.
use cli_http::form_encode;
use wf_kit::args::{parse_none, parse_spec, Positional, Spec};
use secrets::secret;
use wf_kit::time::now_ms;
use serde_json::Value;

const DEFAULT_API: &str = "https://api.tailscale.com/api/v2";
const AUTH_KEY_SECRET: &str = "TS_VM_AUTH_KEY";
const VM_TAG: &str = "tag:mobile-learning";
const KEY_DESCRIPTION: &str = "wf-rotate-TS-VM-AUTH-KEY";
const MAX_KEY_DAYS: f64 = 90.0;
const DEFAULT_STALE_MINUTES: f64 = 10.0;
const DEFAULT_WARN_DAYS: f64 = 14.0;
const MANAGED_PREFIXES: [&str; 2] = ["vm-ops-api-", "vm-gateway-"];

/// A `WfError`: printed as `wf: message`, exit 1.
struct Fail(String);

type Res<T> = Result<T, Fail>;

fn api() -> String {
    std::env::var("WF_TAILSCALE_API").unwrap_or_else(|_| DEFAULT_API.to_string())
}

fn require_secret(key: &str) -> Res<String> {
    match secret(key).map_err(Fail)? {
        Some(v) if !v.is_empty() && v != "PLACEHOLDER_FILL_ME" => Ok(v),
        _ => Err(Fail(format!(
            "{key} is not set (or is still a placeholder). It comes from the Tailscale OAuth client 'infra-agent-device-mgmt' (admin console -> Settings -> OAuth clients), scoped to devices:core + auth_keys with tag:mobile-learning. Store it with `wf secret set {key}`."
        ))),
    }
}

fn call_api(method: &str, url: &str, headers: &[String], body: Option<&str>) -> Res<(i64, String)> {
    let mut request = cli_http::Request::new(method, url).timeout(std::time::Duration::from_secs(30));
    for header in headers {
        let (name, value) = header.split_once(": ").unwrap_or((header, ""));
        request = request.header(name, value);
    }
    if let Some(body) = body {
        request = request.body(body.as_bytes().to_vec());
    }
    let response = request.send().map_err(Fail)?;
    Ok((i64::from(response.status), response.text()))
}

struct Client {
    tailnet: String,
    token: Option<String>,
}

impl Client {
    fn new() -> Res<Client> {
        Ok(Client { tailnet: require_secret("TS_TAILNET_ID")?, token: None })
    }

    /// Exchanges one client secret for an access token; a refused secret is `Ok(None)` so the next client can be tried.
    fn exchange(secret: &str) -> Res<Option<String>> {
        let (status, body) = call_api(
            "POST",
            &format!("{}/oauth/token", api()),
            &["Content-Type: application/x-www-form-urlencoded".into()],
            Some(&format!("client_secret={}", form_encode(secret))),
        )?;
        if status == 401 {
            return Ok(None);
        }
        if !(200..300).contains(&status) {
            return Err(Fail(format!("Tailscale OAuth exchange failed ({status}). Check TS_VM_OAUTH_CLIENT_SECRET is the current secret for the device-management client — secrets are shown once at generation, so a lost one means generating a new client and updating the vault.")));
        }
        Ok(serde_json::from_str::<Value>(&body).ok().and_then(|v| v["access_token"].as_str().filter(|t| !t.is_empty()).map(String::from)))
    }

    fn token(&mut self) -> Res<String> {
        if let Some(t) = &self.token {
            return Ok(t.clone());
        }
        // The device-management client may be revoked while the ACL-sync client still works; its secret is the fallback.
        let primary = require_secret("TS_VM_OAUTH_CLIENT_SECRET")?;
        let fallback = secret("TS_CI_OAUTH_CLIENT_SECRET").map_err(Fail)?.filter(|v| !v.is_empty() && v != "PLACEHOLDER_FILL_ME");
        let mut granted = Self::exchange(&primary)?;
        let tried_fallback = granted.is_none() && fallback.is_some();
        if let (None, Some(f)) = (&granted, &fallback) {
            granted = Self::exchange(f)?;
        }
        let token = granted.ok_or_else(|| {
            Fail(if tried_fallback {
                "Tailscale OAuth exchange failed (401). Both TS_VM_OAUTH_CLIENT_SECRET and TS_CI_OAUTH_CLIENT_SECRET were refused: secrets are shown once at generation, so a lost one means generating a new client and updating the vault.".into()
            } else {
                "Tailscale OAuth exchange failed (401). Check TS_VM_OAUTH_CLIENT_SECRET is the current secret for the device-management client — secrets are shown once at generation, so a lost one means generating a new client and updating the vault.".into()
            })
        })?;
        self.token = Some(token.clone());
        Ok(token)
    }

    fn call(&mut self, method: &str, path: &str) -> Res<Value> {
        self.call_with(method, path, None)
    }

    /// The status the API answers, whatever it is, for probes that expect a refusal.
    fn status(&mut self, method: &str, path: &str) -> Res<i64> {
        let token = self.token()?;
        let headers = [format!("Authorization: Bearer {token}")];
        call_api(method, &format!("{}{path}", api()), &headers, None).map(|(status, _)| status)
    }

    fn call_with(&mut self, method: &str, path: &str, json_body: Option<&str>) -> Res<Value> {
        let token = self.token()?;
        let mut headers = vec![format!("Authorization: Bearer {token}")];
        if json_body.is_some() {
            headers.push("Content-Type: application/json".into());
        }
        let (status, body) = call_api(method, &format!("{}{path}", api()), &headers, json_body)?;
        if !(200..300).contains(&status) {
            let hint = if status == 403 {
                " The client authenticated but was refused — regenerate it with WRITE scope on devices:core (a read-only client reports scope 'devices:core:read')."
            } else {
                ""
            };
            return Err(Fail(format!(
                "Tailscale {method} {path} failed ({status}). {}{hint}",
                body.chars().take(200).collect::<String>()
            )));
        }
        Ok(serde_json::from_str(&body).unwrap_or(Value::Null))
    }
}

struct Device {
    id: String,
    name: String,
    last_seen: String,
    tags: Vec<String>,
}

fn list_devices(c: &mut Client) -> Res<Vec<Device>> {
    let body = c.call("GET", &format!("/tailnet/{}/devices", c.tailnet))?;
    let strings = |v: &Value| -> Vec<String> {
        v.as_array()
            .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
            .unwrap_or_default()
    };
    Ok(body["devices"]
        .as_array()
        .map(|a| a.as_slice())
        .unwrap_or_default()
        .iter()
        .map(|d| Device {
            id: d["id"].as_str().unwrap_or_default().to_string(),
            name: d["name"].as_str().unwrap_or_default().split('.').next().unwrap_or_default().to_string(),
            last_seen: d["lastSeen"].as_str().unwrap_or_default().to_string(),
            tags: strings(&d["tags"]),
        })
        .collect())
}

/// Unix milliseconds of an RFC 3339 timestamp, or `None` when it does not parse.
fn parse_time(s: &str) -> Option<f64> {
    rca_core::time::parse(s).map(|ms| ms as f64)
}

fn age_minutes(last_seen: &str, now: f64) -> f64 {
    match parse_time(last_seen) {
        None => f64::INFINITY,
        Some(seen) => ((now - seen) / 60_000.0 + 0.5).floor().max(0.0),
    }
}

fn suffixed(name: &str) -> bool {
    match name.rsplit_once('-') {
        Some((_, tail)) => !tail.is_empty() && tail.bytes().all(|b| b.is_ascii_digit()),
        None => false,
    }
}

struct Assessment<'a> {
    device: &'a Device,
    verdict: &'static str,
    age: f64,
    suffixed: bool,
    selected: bool,
}

fn assess<'a>(devices: &'a [Device], stale: f64, force: bool, names: &[String]) -> Vec<Assessment<'a>> {
    let now = now_ms() as f64;
    devices
        .iter()
        .map(|d| {
            let age = age_minutes(&d.last_seen, now);
            let sfx = suffixed(&d.name);
            let verdict = if d.tags.is_empty() {
                "untagged"
            } else if age > stale {
                "stale"
            } else if sfx {
                "online-suffixed"
            } else {
                "healthy"
            };
            let selected = if !names.is_empty() {
                names.contains(&d.name) && (verdict == "stale" || force)
            } else {
                MANAGED_PREFIXES.iter().any(|p| d.name.starts_with(p))
                    && (verdict == "stale" || (verdict == "online-suffixed" && force))
            };
            Assessment { device: d, verdict, age, suffixed: sfx, selected }
        })
        .collect()
}

fn pad_end(s: &str, width: usize) -> String {
    format!("{s}{}", " ".repeat(width.saturating_sub(s.encode_utf16().count())))
}

fn format_assessment(a: &Assessment) -> String {
    let age = if a.age.is_infinite() { "?".to_string() } else { format!("{}", a.age as i64) };
    let tags = if a.device.tags.is_empty() { "(untagged)".to_string() } else { a.device.tags.join(",") };
    format!(
        "  {}age={age:>5}m  {}{tags}{}",
        pad_end(&a.device.name, 24),
        pad_end(a.verdict, 16),
        if a.selected { " <-- RETIRE" } else { "" }
    )
}

fn names_of(list: &[&Assessment]) -> String {
    list.iter().map(|a| a.device.name.as_str()).collect::<Vec<_>>().join(", ")
}

fn devices_list(stale: f64) -> Res<i32> {
    let mut c = Client::new()?;
    let devices = list_devices(&mut c)?;
    let assessments = assess(&devices, stale, false, &[]);
    println!("{} device(s) on the tailnet:", assessments.len());
    for a in &assessments {
        println!("{}", format_assessment(a));
    }
    let sfx: Vec<&Assessment> = assessments.iter().filter(|a| a.suffixed).collect();
    if !sfx.is_empty() {
        print!(
            "\nWARNING: {} device(s) carry a collision suffix: {}.\nvm-gateway forwards to the BARE hostname, so any service named this way is unreachable through it.\nRetire the stale record holding the bare name, then restart that host’s tailscale sidecar so it re-registers.\n",
            sfx.len(),
            names_of(&sfx)
        );
    }
    Ok(0)
}

fn devices_retire(stale: f64, apply: bool, force: bool, names: &[String], replaced_vm: bool) -> Res<i32> {
    let mut c = Client::new()?;
    let devices = list_devices(&mut c)?;
    if replaced_vm {
        let records = vm_records(&devices);
        if apply {
            for d in &records {
                c.call("DELETE", &format!("/device/{}", d.id))?;
            }
        }
        let list = records.iter().map(|d| d.name.as_str()).collect::<Vec<_>>().join(", ");
        match (records.is_empty(), apply) {
            (true, _) => println!("Nothing to retire: no tagged VM node record is left."),
            (false, true) => println!("Retired {} VM node record(s): {list}", records.len()),
            (false, false) => println!("Would retire {} VM node record(s): {list}\nDry run — re-run with --apply to delete.", records.len()),
        }
        return Ok(0);
    }
    let assessments = assess(&devices, stale, force, names);
    let selected: Vec<&Assessment> = assessments.iter().filter(|a| a.selected).collect();
    if !names.is_empty() {
        let unknown: Vec<&str> = names
            .iter()
            .filter(|n| !devices.iter().any(|d| &d.name == *n))
            .map(String::as_str)
            .collect();
        if !unknown.is_empty() {
            return Err(Fail(format!(
                "no such device on the tailnet: {}. Run `wf vm net device list` to see current names.",
                unknown.join(", ")
            )));
        }
    }
    if apply {
        for a in &selected {
            c.call("DELETE", &format!("/device/{}", a.device.id))?;
        }
    }
    for a in &assessments {
        println!("{}", format_assessment(a));
    }
    if selected.is_empty() {
        print!("\nNothing to retire.\n");
        let online: Vec<&Assessment> = assessments.iter().filter(|a| a.verdict == "online-suffixed").collect();
        if !online.is_empty() && !force {
            print!(
                "{} online device(s) hold a collision suffix but were spared — deleting a LIVE node\nneeds --force, since it drops that node off the tailnet until its sidecar re-registers.\n",
                online.len()
            );
        }
        return Ok(0);
    }
    println!(
        "\n{} {} device(s): {}",
        if apply { "Retired" } else { "Would retire" },
        selected.len(),
        names_of(&selected)
    );
    if apply {
        println!("Now restart the affected host’s tailscale sidecar so it re-registers and claims the freed name.");
    } else {
        println!("Dry run — re-run with --apply to delete.");
    }
    Ok(0)
}

/// Tagged nodes that live on a recreated VM, as opposed to the sandbox and other agent hosts.
const OFF_VM_PREFIXES: [&str; 1] = ["sandbox-agent"];

/// After the VM is destroyed and before its replacement boots, delete every record its nodes left behind, whatever
/// their age: a record still inside the staleness window would otherwise hold the bare hostname and push the new
/// node to a `-1` name that vm-gateway cannot reach.
pub fn retire_vm_records() -> Result<Vec<String>, String> {
    let run = || -> Res<Vec<String>> {
        let mut c = Client::new()?;
        let mut gone = Vec::new();
        for d in vm_records(&list_devices(&mut c)?) {
            c.call("DELETE", &format!("/device/{}", d.id))?;
            gone.push(d.name.clone());
        }
        Ok(gone)
    };
    run().map_err(|Fail(m)| m)
}

/// Every tagged node record that belongs to the VM, whatever its age: the selection of `retire_vm_records` and of `device retire --replaced-vm`.
fn vm_records(devices: &[Device]) -> Vec<&Device> {
    devices.iter().filter(|d| !d.tags.is_empty() && !OFF_VM_PREFIXES.iter().any(|p| d.name.starts_with(p))).collect()
}

const PROBE_DEVICE: &str = "wf-preflight-no-such-device";

/// Proof, without a side effect, that the credentials can list devices and delete them: a DELETE of an id that cannot exist must
/// answer not-found, while a read-only client is refused with 403 (a 404 only comes after the scope check).
pub fn check_api_access() -> Result<usize, String> {
    let run = || -> Res<usize> {
        let mut c = Client::new()?;
        let count = list_devices(&mut c)?.len();
        match c.status("DELETE", &format!("/device/{PROBE_DEVICE}"))? {
            404 => Ok(count),
            401 | 403 => Err(Fail("the OAuth client can list devices but may not delete them: it lacks the write scope `devices:core` (it has only `devices:core:read`), so the retire after the VM is destroyed would be refused. Regenerate the client with `devices:core` write.".into())),
            other => Err(Fail(format!("the delete probe of a device that cannot exist answered {other}, not 404, so the write scope is unproven."))),
        }
    };
    run().map_err(|Fail(m)| m)
}

fn auth_key_status(warn_days: f64) -> Res<i32> {
    let mut c = Client::new()?;
    let body = c.call("GET", &format!("/tailnet/{}/keys", c.tailnet))?;
    let keys = body["keys"].as_array().map(|a| a.as_slice()).unwrap_or_default();
    let mine: Vec<(&str, f64)> = keys
        .iter()
        .filter(|k| k["description"].as_str().unwrap_or_default() == KEY_DESCRIPTION)
        .map(|k| {
            let e = k["expires"].as_str().unwrap_or_default();
            (e, parse_time(e).unwrap_or(f64::NAN))
        })
        .collect();
    let (days, expires, needs, reason): (Option<f64>, Option<&str>, bool, String) = if mine.is_empty() {
        (None, None, true, format!("no auth key owned by this OAuth client matches '{KEY_DESCRIPTION}'. Either it was never rotated through `wf vm net auth-key rotate` (a console-made key is invisible to the API), or it has already expired and been removed. Rotate to take ownership."))
    } else {
        // Newest wins; a NaN comparison keeps the later entry, as the TypeScript reduce does.
        let newest = mine.iter().copied().reduce(|a, b| if a.1 > b.1 { a } else { b }).expect("non-empty");
        let days = ((newest.1 - now_ms() as f64) / 86_400_000.0).floor();
        if days.is_nan() {
            (None, Some(newest.0), true, format!("could not parse the key's expiry ('{}')", newest.0))
        } else if days <= warn_days {
            (Some(days), Some(newest.0), true, format!("expires in {} day(s), at or under the {}-day threshold. Expiry breaks the NEXT node join rather than running nodes, so this fails silently mid-rebuild if left.", n(days), n(warn_days)))
        } else {
            (Some(days), Some(newest.0), false, format!("expires in {} day(s), above the {}-day threshold", n(days), n(warn_days)))
        }
    };
    println!(
        "{AUTH_KEY_SECRET}: {}{}\n  {reason}",
        days.map_or("expiry UNKNOWN".to_string(), |d| format!("{} day(s) remaining", n(d))),
        expires.map_or(String::new(), |e| format!(" (expires {e})"))
    );
    if needs {
        println!("\nACTION NEEDED: run `wf vm net auth-key rotate --apply` (threshold {} days).", n(warn_days));
        return Err(Fail(format!("{AUTH_KEY_SECRET} needs rotation: {reason}")));
    }
    Ok(0)
}

fn rotate_dry(action: &str, expiry_days: f64) -> Res<i32> {
    Client::new()?;
    if action != "rotate" {
        return Err(Fail(format!("unknown auth-key action '{action}' — the only supported action is 'rotate'.")));
    }
    if !(1.0..=MAX_KEY_DAYS).contains(&expiry_days) {
        return Err(Fail(format!("expiryDays must be between 1 and {} — Tailscale rejects anything longer, which is why this rotation exists.", n(MAX_KEY_DAYS))));
    }
    print!(
        "Dry run — would mint a {}-day reusable tagged key and store it as {AUTH_KEY_SECRET}.\nNo key was minted: an unused key still counts against the tailnet’s key list and would need manual cleanup.\nRe-run with --apply to rotate.\n",
        n(expiry_days)
    );
    Ok(0)
}

fn rotate_apply(expiry_days: f64) -> Res<i32> {
    let mut c = Client::new()?;
    if !(1.0..=MAX_KEY_DAYS).contains(&expiry_days) {
        return Err(Fail(format!("expiryDays must be between 1 and {} — Tailscale rejects anything longer, which is why this rotation exists.", n(MAX_KEY_DAYS))));
    }
    let body = serde_json::json!({
        "capabilities": { "devices": { "create": { "reusable": true, "ephemeral": true, "preauthorized": true, "tags": [VM_TAG] } } },
        "expirySeconds": (expiry_days * 86_400.0) as u64,
        "description": KEY_DESCRIPTION,
    });
    let reply = c.call_with("POST", &format!("/tailnet/{}/keys", c.tailnet), Some(&body.to_string()))?;
    let key = reply["key"].as_str().filter(|k| !k.is_empty()).ok_or_else(|| Fail("Tailscale key creation returned no key.".into()))?;
    secrets::update_secret_value(AUTH_KEY_SECRET, key).map_err(Fail)?;
    print!(
        "Rotated {AUTH_KEY_SECRET}: minted a {}-day reusable tagged key ({} chars) and stored it.\nTailscale caps auth keys at {} days, and expiry breaks the NEXT join rather than running nodes — so rotate again before then.\n",
        n(expiry_days),
        key.encode_utf16().count(),
        n(MAX_KEY_DAYS)
    );
    Ok(0)
}

fn n(v: f64) -> String {
    if v.fract() == 0.0 && v.abs() < 1e15 {
        format!("{}", v as i64)
    } else {
        v.to_string()
    }
}

/// The numeric option as the TypeScript reads it (`value ? Number(value) : undefined`); `Err` for text that is
/// not a number, which is unsupported.
fn number(v: Option<&String>, default: f64) -> Result<f64, ()> {
    match v {
        None => Ok(default),
        Some(s) if s.is_empty() => Ok(default),
        Some(s) => s.trim().parse::<f64>().ok().filter(|x| x.is_finite()).ok_or(()),
    }
}

fn finish(r: Res<i32>) -> Option<i32> {
    Some(match r {
        Ok(code) => code,
        Err(Fail(m)) => {
            wf_kit::wf_err!("wf: {m}");
            1
        }
    })
}

pub fn run(args: &[String]) -> Option<i32> {
    let (sub, rest) = args.split_first()?;
    match sub.as_str() {
        "devices" => {
            let (action, rest) = rest.split_first()?;
            match action.as_str() {
                "list" => {
                    let p = parse_none(rest, &["stale-minutes"], &[])?;
                    let stale = number(p.opts.get("stale-minutes"), DEFAULT_STALE_MINUTES).ok()?;
                    finish(devices_list(stale))
                }
                "retire" => {
                    // `--name <name...>` takes every following word up to the next option.
                    let spec = Spec { values: &["stale-minutes"], variadic: &["name"], flags: &["apply", "force", "replaced-vm"], positional: Positional::None, ..Spec::NONE };
                    let p = parse_spec(rest, &spec)?;
                    let names: Vec<String> = p.values("name").into_iter().map(str::to_string).collect();
                    let stale = number(p.opts.get("stale-minutes"), DEFAULT_STALE_MINUTES).ok()?;
                    finish(devices_retire(
                        stale,
                        p.flags.iter().any(|f| f == "apply"),
                        p.flags.iter().any(|f| f == "force"),
                        &names,
                        p.flags.iter().any(|f| f == "replaced-vm"),
                    ))
                }
                _ => None,
            }
        }
        "up" => crate::tailscale_up::up(rest),
        "auth-key-status" => {
            let p = parse_none(rest, &["warn-days"], &[])?;
            let warn = number(p.opts.get("warn-days"), DEFAULT_WARN_DAYS).ok()?;
            finish(auth_key_status(warn))
        }
        "auth-key" => {
            let p = parse_spec(rest, &Spec { values: &["expiry-days"], flags: &["apply"], positional: Positional::Exactly(1), ..Spec::NONE })?;
            let days = number(p.opts.get("expiry-days"), MAX_KEY_DAYS).ok()?;
            if p.flags.iter().any(|f| f == "apply") {
                if p.positional[0] != "rotate" {
                    return finish(Err(Fail(format!("unknown auth-key action '{}' — the only supported action is 'rotate'.", p.positional[0]))));
                }
                return finish(rotate_apply(days));
            }
            finish(rotate_dry(&p.positional[0], days))
        }
        _ => None,
    }
}
