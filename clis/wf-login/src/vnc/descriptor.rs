//! How one tool's interactive login is described, as data: the schema a built-in and a spec handed over the control
//! channel both satisfy (nothing about a built-in is privileged), and the capture / deliver / verify machinery that
//! drives a descriptor with no per-tool code.
use serde_json::{json, Map, Value};

pub const ENV_REFERENCE_OPEN: &str = "${";

#[derive(Clone, Debug)]
pub struct Capture {
    pub urls: Vec<String>,
    pub require: Vec<String>,
    pub json_format: bool,
}

#[derive(Clone, Debug)]
pub enum Deliver {
    Http { url: String, method: String, headers: Vec<(String, String)>, json_body: Vec<(String, String)> },
    Return,
}

#[derive(Clone, Debug)]
pub struct Verify {
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub true_field: String,
}

/// A vault secret that mirrors one of the tool's credential files (base64 of the file): set when the login finishes, cleared by
/// `--clear`, so a job that cannot reach the login container can restore the credential from the vault.
#[derive(Clone, Debug)]
pub struct VaultSecret {
    pub key: String,
    pub file: String,
}

#[derive(Clone, Debug)]
pub struct Descriptor {
    pub id: String,
    pub login_cmd: Option<String>,
    pub login_url: Option<String>,
    pub credential_paths: Vec<String>,
    /// The credential files a caller may fetch once the login finished (`wf secret login <tool> --copy`), `~/`-rooted.
    pub copy: Vec<String>,
    pub vault: Option<VaultSecret>,
    pub valid_cmd: Option<String>,
    pub valid_path: Option<String>,
    pub capture: Option<Capture>,
    pub deliver: Option<Deliver>,
    pub verify: Option<Verify>,
}

const GOOGLE_ERRORS: &str = r#"["401","403","invalid_grant","expired","not authenticated","unauthorized"]"#;

/// The tools this deployment ships, as the JSON a caller could equally have sent.
fn builtin_json(id: &str) -> Option<String> {
    let file = |login: &str, path: &str, errors: &str| {
        format!(r#"{{"id":"{id}","loginCmd":"{login}","credentialPaths":["{path}"],"copy":["{path}"],"validCheck":{{"path":"{path}"}},"authErrorPatterns":{errors}}}"#)
    };
    Some(match id {
        "claude" => r#"{"id":"claude","loginCmd":"claude setup-token","copy":["~/.claude/.credentials.json"],"validCheck":{"path":"~/.claude/.credentials.json"},"authErrorPatterns":["401","403","invalid_grant","expired","not authenticated","unauthorized","oauth token has expired"]}"#.to_string(),
        "codex" => file("codex login", "~/.codex/auth.json", GOOGLE_ERRORS),
        "opencode" => file("opencode auth login", "~/.opencode/data/auth.json", GOOGLE_ERRORS),
        "gemini" => file("gemini", "~/.gemini/oauth_creds.json", r#"["401","403","invalid_grant","expired","not authenticated","unauthorized","reauth"]"#),
        "gcloud" => r#"{"id":"gcloud","loginCmd":"gcloud auth login --enable-gdrive-access","credentialPaths":["~/.config/gcloud/credentials.db"],"copy":["~/.config/gcloud/credentials.db"],"vault":{"key":"GCLOUD_DRIVE_CREDENTIALS_DB_B64","file":"~/.config/gcloud/credentials.db"},"validCheck":{"cmd":"gcloud auth list --filter=status:ACTIVE --format=\"value(account)\" | grep ."},"authErrorPatterns":["401","403","invalid_grant","reauth","not authenticated","unauthorized","ERROR: (gcloud.auth)"]}"#.to_string(),
        "leetcode" => r#"{"id":"leetcode","loginUrl":"https://leetcode.com/accounts/login/","authErrorPatterns":["401","403","not authenticated","session expired","csrf","leetcode_session"],"capture":{"kind":"cookies","urls":["https://leetcode.com"],"require":["LEETCODE_SESSION","csrftoken"],"format":"cookie-header"},"deliver":{"kind":"http","url":"${LEETCODE_BACKEND_BASE_URL}/api/leetcode/session","method":"POST","headers":{"x-admin-secret":"${LEETCODE_BACKEND_ADMIN_SECRET}","authorization":"Bearer ${VM_GATEWAY_AUTH_SEED}"},"jsonBody":{"cookie":"${credential}"}},"verify":{"url":"${LEETCODE_BACKEND_BASE_URL}/api/leetcode/user-status","headers":{"authorization":"Bearer ${VM_GATEWAY_AUTH_SEED}"},"trueField":"isSignedIn"}}"#.to_string(),
        _ => return None,
    })
}

pub const KNOWN_TOOLS: [&str; 6] = ["claude", "codex", "opencode", "gemini", "gcloud", "leetcode"];

pub fn builtin(id: &str) -> Option<Descriptor> {
    let value: Value = serde_json::from_str(&builtin_json(id)?).ok()?;
    Descriptor::parse(&value).ok()
}

fn type_name(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

struct Issues(Vec<Value>);

impl Issues {
    fn add(&mut self, code: &str, path: &[Value], message: String) {
        self.0.push(json!({ "code": code, "path": path, "message": message }));
    }
    fn expected(&mut self, path: &[Value], expected: &str, got: &Value) {
        self.add("invalid_type", path, format!("Invalid input: expected {expected}, received {}", type_name(got)));
    }
}

fn at(path: &[Value], key: &str) -> Vec<Value> {
    let mut p = path.to_vec();
    p.push(json!(key));
    p
}

fn at_index(path: &[Value], index: usize) -> Vec<Value> {
    let mut p = path.to_vec();
    p.push(json!(index));
    p
}

fn missing(issues: &mut Issues, path: &[Value], expected: &str) {
    issues.add("invalid_type", path, format!("Invalid input: expected {expected}, received undefined"));
}

/// A string that must be present and non-empty.
fn req_str(issues: &mut Issues, obj: &Map<String, Value>, key: &str, path: &[Value]) -> Option<String> {
    let p = at(path, key);
    match obj.get(key) {
        None => {
            missing(issues, &p, "string");
            None
        }
        Some(v) => str_value(issues, v, &p),
    }
}

fn str_value(issues: &mut Issues, v: &Value, path: &[Value]) -> Option<String> {
    match v.as_str() {
        Some("") => {
            issues.add("too_small", path, "Too small: expected string to have >=1 characters".to_string());
            None
        }
        Some(s) => Some(s.to_string()),
        None => {
            issues.expected(path, "string", v);
            None
        }
    }
}

fn opt_str(issues: &mut Issues, obj: &Map<String, Value>, key: &str, path: &[Value]) -> Option<String> {
    obj.get(key).and_then(|v| str_value(issues, v, &at(path, key)))
}

fn str_array(issues: &mut Issues, v: &Value, path: &[Value], min_one: bool) -> Vec<String> {
    let Some(items) = v.as_array() else {
        issues.expected(path, "array", v);
        return Vec::new();
    };
    if min_one && items.is_empty() {
        issues.add("too_small", path, "Too small: expected array to have >=1 items".to_string());
    }
    items.iter().enumerate().filter_map(|(i, item)| str_value(issues, item, &at_index(path, i))).collect()
}

fn string_record(issues: &mut Issues, v: &Value, path: &[Value]) -> Vec<(String, String)> {
    let Some(map) = v.as_object() else {
        issues.expected(path, "record", v);
        return Vec::new();
    };
    map.iter()
        .filter_map(|(k, item)| match item.as_str() {
            Some(s) => Some((k.clone(), s.to_string())),
            None => {
                issues.expected(&at(path, k), "string", item);
                None
            }
        })
        .collect()
}

fn is_url(s: &str) -> bool {
    let Some((scheme, rest)) = s.split_once("://") else { return false };
    !scheme.is_empty() && scheme.chars().all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c)) && !rest.is_empty() && !s.contains(char::is_whitespace)
}

impl Descriptor {
    /// Validates like the schema the TypeScript listener used; `Err` carries zod-shaped issues (`code`, `path`, `message`).
    pub fn parse(spec: &Value) -> Result<Descriptor, Vec<Value>> {
        let mut issues = Issues(Vec::new());
        let Some(obj) = spec.as_object() else {
            issues.expected(&[], "object", spec);
            return Err(issues.0);
        };
        let root: Vec<Value> = Vec::new();
        let id = req_str(&mut issues, obj, "id", &root);
        let login_cmd = opt_str(&mut issues, obj, "loginCmd", &root);
        let login_url = opt_str(&mut issues, obj, "loginUrl", &root);
        let copy = obj.get("copy").map(|v| str_array(&mut issues, v, &at(&root, "copy"), false)).unwrap_or_default();
        for path in &copy {
            let rooted = path.strip_prefix("~/").is_some_and(|rest| !rest.is_empty() && !rest.split('/').any(|part| part == ".." || part.is_empty()));
            if !rooted {
                issues.add("custom", &at(&root, "copy"), format!("copy path `{path}` must be `~/` plus a path without `..`"));
            }
        }
        let vault = match obj.get("vault") {
            None => None,
            Some(v) => match v.as_object() {
                None => {
                    issues.expected(&at(&root, "vault"), "object", v);
                    None
                }
                Some(o) => {
                    let at_vault = at(&root, "vault");
                    let key = req_str(&mut issues, o, "key", &at_vault);
                    let file = req_str(&mut issues, o, "file", &at_vault);
                    match (key, file) {
                        (Some(key), Some(file)) if copy.contains(&file) => Some(VaultSecret { key, file }),
                        (Some(_), Some(file)) => {
                            issues.add("custom", &at_vault, format!("vault file `{file}` must be one of the `copy` paths"));
                            None
                        }
                        _ => None,
                    }
                }
            },
        };
        let credential_paths = obj.get("credentialPaths").map(|v| str_array(&mut issues, v, &at(&root, "credentialPaths"), false)).unwrap_or_default();

        let (mut valid_cmd, mut valid_path) = (None, None);
        if let Some(valid) = obj.get("validCheck") {
            let path = at(&root, "validCheck");
            match valid.as_object() {
                None => issues.expected(&path, "object", valid),
                Some(v) => {
                    valid_cmd = opt_str(&mut issues, v, "cmd", &path);
                    valid_path = opt_str(&mut issues, v, "path", &path);
                    if valid_cmd.is_none() && valid_path.is_none() && !issues.0.iter().any(|i| i["path"].as_array().is_some_and(|p| p.first() == Some(&json!("validCheck")))) {
                        issues.add("custom", &path, "validCheck requires at least one of `cmd` or `path`".to_string());
                    }
                }
            }
        }

        match obj.get("authErrorPatterns") {
            None => missing(&mut issues, &at(&root, "authErrorPatterns"), "array"),
            Some(v) => {
                str_array(&mut issues, v, &at(&root, "authErrorPatterns"), true);
            }
        }

        let capture = obj.get("capture").and_then(|v| parse_capture(&mut issues, v, &at(&root, "capture")));
        let deliver = obj.get("deliver").and_then(|v| parse_deliver(&mut issues, v, &at(&root, "deliver")));
        let verify = obj.get("verify").and_then(|v| parse_verify(&mut issues, v, &at(&root, "verify")));

        if !issues.0.is_empty() {
            return Err(issues.0);
        }
        if obj.get("capture").is_some_and(|v| !v.is_null()) != obj.get("deliver").is_some_and(|v| !v.is_null()) {
            issues.add("custom", &[], "capture and deliver go together: capturing with nowhere to put it, or delivering with nothing captured, is always a mistake".to_string());
        }
        if login_cmd.is_some() == login_url.is_some() {
            issues.add("custom", &[], "exactly one of `loginCmd` or `loginUrl` is required".to_string());
        }
        if !issues.0.is_empty() {
            return Err(issues.0);
        }
        Ok(Descriptor { id: id.unwrap_or_default(), login_cmd, login_url, credential_paths, copy, vault, valid_cmd, valid_path, capture, deliver, verify })
    }
}

fn parse_capture(issues: &mut Issues, v: &Value, path: &[Value]) -> Option<Capture> {
    let Some(obj) = v.as_object() else {
        issues.expected(path, "object", v);
        return None;
    };
    if obj.get("kind").and_then(Value::as_str) != Some("cookies") {
        issues.add("invalid_union", &at(path, "kind"), "Invalid input".to_string());
        return None;
    }
    let urls = match obj.get("urls") {
        None => {
            missing(issues, &at(path, "urls"), "array");
            Vec::new()
        }
        Some(u) => {
            let urls = str_array(issues, u, &at(path, "urls"), true);
            for (i, url) in urls.iter().enumerate() {
                if !is_url(url) {
                    issues.add("invalid_format", &at_index(&at(path, "urls"), i), "Invalid URL".to_string());
                }
            }
            urls
        }
    };
    let require = match obj.get("require") {
        None => {
            missing(issues, &at(path, "require"), "array");
            Vec::new()
        }
        Some(r) => str_array(issues, r, &at(path, "require"), true),
    };
    let json_format = match obj.get("format").and_then(Value::as_str) {
        None if obj.get("format").is_none() => false,
        Some("cookie-header") => false,
        Some("json") => true,
        _ => {
            issues.add("invalid_value", &at(path, "format"), "Invalid option: expected one of \"cookie-header\"|\"json\"".to_string());
            false
        }
    };
    Some(Capture { urls, require, json_format })
}

fn parse_deliver(issues: &mut Issues, v: &Value, path: &[Value]) -> Option<Deliver> {
    let Some(obj) = v.as_object() else {
        issues.expected(path, "object", v);
        return None;
    };
    match obj.get("kind").and_then(Value::as_str) {
        Some("return") => Some(Deliver::Return),
        Some("http") => {
            let url = req_str(issues, obj, "url", path);
            let method = match obj.get("method") {
                None => "POST".to_string(),
                Some(m) => str_value(issues, m, &at(path, "method")).unwrap_or_default(),
            };
            let headers = obj.get("headers").map(|h| string_record(issues, h, &at(path, "headers"))).unwrap_or_default();
            let json_body = match obj.get("jsonBody") {
                None => vec![("credential".to_string(), "${credential}".to_string())],
                Some(b) => string_record(issues, b, &at(path, "jsonBody")),
            };
            Some(Deliver::Http { url: url.unwrap_or_default(), method, headers, json_body })
        }
        _ => {
            issues.add("invalid_union", &at(path, "kind"), "Invalid input".to_string());
            None
        }
    }
}

fn parse_verify(issues: &mut Issues, v: &Value, path: &[Value]) -> Option<Verify> {
    let Some(obj) = v.as_object() else {
        issues.expected(path, "object", v);
        return None;
    };
    let url = req_str(issues, obj, "url", path);
    let headers = obj.get("headers").map(|h| string_record(issues, h, &at(path, "headers"))).unwrap_or_default();
    let true_field = req_str(issues, obj, "trueField", path);
    Some(Verify { url: url.unwrap_or_default(), headers, true_field: true_field.unwrap_or_default() })
}

/// `${NAME}` references resolved against `lookup`; an unset or empty one is an error, not an empty string (a bare
/// `Authorization: Bearer ` would read as a bad credential rather than a missing one).
pub fn interpolate_env(input: &str, lookup: &dyn Fn(&str) -> Option<String>) -> Result<String, String> {
    let mut out = String::new();
    let mut rest = input;
    while let Some(start) = rest.find(ENV_REFERENCE_OPEN) {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        match after.find('}') {
            Some(end) if !after[..end].is_empty() && after[..end].chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_') => {
                let name = &after[..end];
                match lookup(name).filter(|v| !v.is_empty()) {
                    Some(value) => out.push_str(&value),
                    None => return Err(format!("spec references ${{{name}}}, which is not set in this container's environment")),
                }
                rest = &after[end + 1..];
            }
            _ => {
                out.push_str("${");
                rest = after;
            }
        }
    }
    out.push_str(rest);
    Ok(out)
}

pub fn env_or_vault(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty()).or_else(|| secrets::load_values(&[name]).get(name).cloned().flatten())
}

/// One HTTP request. Returns the status and the body.
pub fn http_request(method: &str, url: &str, headers: &[(String, String)], body: Option<&str>, timeout_sec: u32) -> Result<(u16, String), String> {
    let mut request = cli_http::Request::new(method, url).timeout(std::time::Duration::from_secs(u64::from(timeout_sec)));
    for (k, v) in headers {
        request = request.header(k, v);
    }
    if let Some(b) = body {
        request = request.body(b.as_bytes().to_vec());
    }
    let response = request.send()?;
    Ok((response.status, response.text()))
}

/// `origin + pathname`, the only parts of a URL safe to log.
pub fn safe_url(url: &str) -> String {
    let Some((scheme, rest)) = url.split_once("://") else { return "<unparseable>".to_string() };
    let rest = rest.split(['?', '#']).next().unwrap_or_default();
    let (authority, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    format!("{}://{}{}", scheme.to_lowercase(), authority.rsplit('@').next().unwrap_or_default().to_lowercase(), path)
}

/// The credential a capture spec asks for, from the cookies the browser holds; every required name must be present.
pub fn assemble_credential(spec: &Capture, cookies: &[(String, String)]) -> Result<String, String> {
    let find = |name: &str| cookies.iter().find(|(n, _)| n == name).map(|(_, v)| v.clone());
    let missing: Vec<&String> = spec.require.iter().filter(|n| find(n).is_none()).collect();
    if !missing.is_empty() {
        return Err(format!("no session cookie found in the login browser: missing {}", missing.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")));
    }
    if spec.json_format {
        let mut map = Map::new();
        for name in &spec.require {
            map.insert(name.clone(), json!(find(name)));
        }
        return Ok(Value::Object(map).to_string());
    }
    Ok(spec.require.iter().map(|n| format!("{n}={}", find(n).unwrap_or_default())).collect::<Vec<_>>().join("; "))
}

/// Hands the credential to wherever the spec says it belongs.
pub fn deliver(spec: &Deliver, credential: &str, lookup: &dyn Fn(&str) -> Option<String>) -> Result<(), String> {
    let Deliver::Http { url, method, headers, json_body } = spec else { return Ok(()) };
    let url = interpolate_env(url, lookup)?;
    let mut body = Map::new();
    for (k, v) in json_body {
        body.insert(k.clone(), json!(interpolate_env(v, lookup)?.replace("${credential}", credential)));
    }
    let mut all = vec![("content-type".to_string(), "application/json".to_string())];
    for (k, v) in headers {
        all.push((k.clone(), interpolate_env(v, lookup)?));
    }
    let (status, _) = http_request(method, &url, &all, Some(&Value::Object(body).to_string()), 30).map_err(|e| format!("delivering the credential to {} failed: {e}", safe_url(&url)))?;
    if !(200..300).contains(&status) {
        return Err(format!("delivering the credential to {} failed: {status}", safe_url(&url)));
    }
    Ok(())
}

/// An HTTP probe answering "is this credential still good?": the response's top-level `trueField` must be `true`.
pub fn verify(spec: &Verify, lookup: &dyn Fn(&str) -> Option<String>) -> Result<bool, String> {
    let url = interpolate_env(&spec.url, lookup)?;
    let mut headers = Vec::new();
    for (k, v) in &spec.headers {
        headers.push((k.clone(), interpolate_env(v, lookup)?));
    }
    let (status, body) = http_request("GET", &url, &headers, None, 30)?;
    if !(200..300).contains(&status) {
        return Ok(false);
    }
    let payload: Value = serde_json::from_str(&body).map_err(|e| e.to_string())?;
    Ok(payload[&spec.true_field] == json!(true))
}
