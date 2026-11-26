//! What `wf mimic proxy` exports: the shape of what crossed the wire, never a value. A path segment, a query name, a header value, a
//! body value and a close reason are all replaced, so a credential, a ticket, an authorization value, a pairing code, a cookie and the
//! user's own text cannot reach the file. What stays is the protocol's own vocabulary: the HTTP verb, the route's words, the header
//! names, the JSON keys, the envelope tag and the RPC method name -- and only the ones the pinned T3 fixtures know. A name that is
//! not one of them is masked, so a name the traffic chooses cannot be a value that reaches the file.
use serde_json::{json, Map, Value};
use std::collections::HashSet;
use std::sync::OnceLock;

/// The words of the routes the observed server serves, from `clis/t3-serve/http/src/http.rs` (`route_name`) and the well-known paths
/// beside it. A path segment that is not one of them is a value the phone chose (a thread id, a ticket, a file name), so it is masked.
const ROUTE_WORDS: &[&str] = &[
    "acceleration",
    "api",
    "approval",
    "assets",
    "attach",
    "attachments",
    "auth",
    "authorize",
    "benchmarks",
    "bounded",
    "browser-session",
    "cancel",
    "clients",
    "cloud",
    "connect",
    "decision",
    "diff",
    "download",
    "environment",
    "guest",
    "health",
    "history",
    "hooks",
    "keys",
    "link",
    "link-proof",
    "link-state",
    "mcp",
    "messages",
    "mint-credential",
    "model-unload-timeout",
    "models",
    "mutate",
    "oauth",
    "oauth-authorization-server",
    "oauth-protected-resource",
    "openrouter",
    "orchestration",
    "pairing-links",
    "pairing-token",
    "post-process",
    "preferences",
    "prepare",
    "projects",
    "provider",
    "pull-requests",
    "reachability",
    "redeem",
    "register",
    "relay-config",
    "remove",
    "revoke",
    "revoke-others",
    "rpc",
    "select",
    "session",
    "share",
    "shell",
    "status",
    "t3",
    "t3-connect",
    "threads",
    "token",
    "transcribe",
    "transcript",
    "unlink",
    "upload",
    "use",
    "voice",
    "well-known",
    "websocket-ticket",
    "ws",
];

/// The names of the query parameters the observed server reads, plus the ones a client of its API names. An unknown name is masked,
/// because a name is free text on the wire.
const QUERY_WORDS: &[&str] = &[
    "after",
    "audience",
    "before",
    "benchmarks",
    "client_id",
    "code",
    "cursor",
    "format",
    "frameParts",
    "grant_type",
    "include",
    "kind",
    "limit",
    "model",
    "orchestrationProtocol",
    "page",
    "provider",
    "redirect_uri",
    "resource",
    "scope",
    "share",
    "since",
    "subject_token",
    "type",
    "until",
    "use",
    "wsTicket",
    "x-t3-orchestration-protocol",
];

/// The envelope tags of the orchestration socket, from `clis/t3-serve/contracts/src/rpc.rs`: what a client sends (Request, Ack,
/// Interrupt, Ping, Eof) and what the server sends (Chunk, Exit, Defect, ClientProtocolError, Pong).
const TAGS: &[&str] = &["Ack", "Chunk", "ClientProtocolError", "Defect", "Eof", "Exit", "Interrupt", "Ping", "Pong", "Request"];

/// The HTTP verbs the observed server answers, plus the two of the protocol that no route uses.
const VERBS: &[&str] = &["DELETE", "GET", "HEAD", "OPTIONS", "PATCH", "POST", "PUT"];

/// The keys of the OAuth forms the observed server reads: the token exchange of RFC 8693 (grant_type, subject_token,
/// subject_token_type, requested_token_type, audience, resource, scope, actor) and the authorization code (code, redirect_uri,
/// client_id, client_secret, code_verifier, code_challenge, refresh_token, state), and the three the pairing form carries.
const FORM_WORDS: &[&str] = &[
    "actor",
    "actor_token",
    "audience",
    "client_device_type",
    "client_id",
    "client_label",
    "client_os",
    "client_secret",
    "code",
    "code_challenge",
    "code_challenge_method",
    "code_verifier",
    "grant_type",
    "password",
    "redirect_uri",
    "refresh_token",
    "requested_token_type",
    "resource",
    "scope",
    "state",
    "subject_token",
    "subject_token_type",
    "username",
];

/// What a path segment that is not a route word becomes.
pub const SEGMENT: &str = "<seg>";
/// What a query parameter name that is not a known name becomes.
pub const PARAM: &str = "<param>";
/// What a header name that is not a known name becomes.
pub const HEADER: &str = "<header>";
/// What a key of a body that is not a known name becomes.
pub const KEY: &str = "<key>";
/// What an envelope tag that is not a known tag becomes.
pub const TAG: &str = "<tag>";
/// What an RPC method that is not a known method becomes.
pub const METHOD: &str = "<method>";
/// What an HTTP verb that is not a known verb becomes.
pub const VERB: &str = "<verb>";

/// A set of names, read from a committed file once. A line that starts with `#` is a note, not a name.
struct Names {
    text: &'static str,
    set: OnceLock<HashSet<&'static str>>,
}

impl Names {
    const fn new(text: &'static str) -> Names {
        Names { text, set: OnceLock::new() }
    }

    /// Whether the name is one of the vocabulary.
    fn has(&self, name: &str) -> bool {
        let text = self.text;
        self.set.get_or_init(|| text.lines().filter(|l| !l.is_empty() && !l.starts_with('#')).collect()).contains(name)
    }
}

/// The JSON keys the pinned T3 fixtures use: the field names of every body the app and the server exchange.
static KEYS: Names = Names::new(include_str!("proxy_keys.txt"));
/// The header names the pinned T3 fixtures use, and the standard ones of HTTP.
static HEADERS: Names = Names::new(include_str!("proxy_headers.txt"));
/// The RPC method names the pinned T3 fixtures use.
static METHODS: Names = Names::new(include_str!("proxy_methods.txt"));

/// Whether the key of a body is one the pinned T3 fixtures use.
pub fn known_key(name: &str) -> bool {
    KEYS.has(name)
}

/// Whether the header name is one the pinned T3 fixtures use, or a standard one of HTTP.
pub fn known_header(name: &str) -> bool {
    HEADERS.has(&name.to_lowercase())
}

/// Whether the RPC method is one the pinned T3 fixtures use.
pub fn known_method(name: &str) -> bool {
    METHODS.has(name)
}

/// Whether the envelope tag is one of the orchestration socket's own.
pub fn known_tag(name: &str) -> bool {
    TAGS.contains(&name)
}

/// Whether the verb is one the observed server answers.
pub fn known_verb(name: &str) -> bool {
    VERBS.contains(&name)
}

/// Whether the query name is one the observed server reads.
pub fn known_query(name: &str) -> bool {
    QUERY_WORDS.contains(&name)
}

/// Whether the form key is one of the OAuth forms the observed server reads.
pub fn known_form(name: &str) -> bool {
    FORM_WORDS.contains(&name)
}

/// The HTTP verb as it is written: the verb itself when the observed server answers it, a mask otherwise.
pub fn verb(name: &str) -> String {
    if known_verb(name) {
        name.to_string()
    } else {
        VERB.to_string()
    }
}

/// The names an object hides: every name the vocabulary does not know gets a mask of its own, so no entry of the object is merged
/// away and the shape of the body stays readable.
struct Masks {
    base: &'static str,
    seen: Vec<(String, String)>,
}

impl Masks {
    fn new(mask: &'static str) -> Masks {
        Masks { base: mask.strip_suffix('>').unwrap_or(mask), seen: Vec::new() }
    }

    /// The name as it is written: the name itself when the vocabulary knows it, a mask of its own otherwise.
    fn name(&mut self, name: &str, known: bool) -> String {
        if known {
            return name.to_string();
        }
        if let Some((_, masked)) = self.seen.iter().find(|(raw, _)| raw == name) {
            return masked.clone();
        }
        let masked = if self.seen.is_empty() { format!("{}>", self.base) } else { format!("{}{}>", self.base, self.seen.len() + 1) };
        self.seen.push((name.to_string(), masked.clone()));
        masked
    }
}

/// The path of a request target, with every segment that is not a route word masked.
pub fn route(target: &str) -> String {
    let path = target.split(['?', '#']).next().unwrap_or("");
    let mut out = String::new();
    for (i, segment) in path.split('/').enumerate() {
        if i > 0 {
            out.push('/');
        }
        if segment.is_empty() {
            continue;
        }
        out.push_str(&masked_word(segment, ROUTE_WORDS, SEGMENT));
    }
    out
}

/// The names of the query parameters of a request target; no value is ever read.
pub fn query_names(target: &str) -> Vec<String> {
    let Some((_, query)) = target.split_once('?') else { return Vec::new() };
    let mut masks = Masks::new(PARAM);
    query
        .split('&')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let name = p.split('=').next().unwrap_or("");
            masks.name(name, known_query(name))
        })
        .collect()
}

/// The names of the headers, lower-cased and sorted; no value is ever read.
pub fn header_names(headers: &[(String, String)]) -> Vec<String> {
    let mut originals: Vec<String> = headers.iter().map(|(k, _)| k.to_lowercase()).collect();
    originals.sort();
    originals.dedup();
    let mut masks = Masks::new(HEADER);
    originals
        .iter()
        .map(|name| {
            let known = known_header(name);
            masks.name(name, known)
        })
        .collect()
}

/// The shape of a body: what it is made of, never what it says. An empty body, a JSON body, a form body and anything else each have
/// their own shape.
pub fn body(bytes: &[u8], content_type: &str) -> Value {
    if bytes.is_empty() {
        return json!({"kind": "empty"});
    }
    let kind = content_type.to_lowercase();
    if kind.contains("json") {
        return match serde_json::from_slice::<Value>(bytes) {
            Ok(value) => shape(&value),
            Err(_) => json!({"kind": "text", "length": bytes.len()}),
        };
    }
    if kind.contains("x-www-form-urlencoded") {
        let mut masks = Masks::new(KEY);
        let mut fields = Map::new();
        for pair in String::from_utf8_lossy(bytes).split('&').filter(|p| !p.is_empty()) {
            let name = pair.split('=').next().unwrap_or("");
            let known = known_form(name);
            fields.insert(masks.name(name, known), json!({"type": "string"}));
        }
        return json!({"kind": "object", "fields": fields});
    }
    json!({"kind": "text", "length": bytes.len()})
}

/// The shape of a value: an object's fields by name, an array's length and the shape of its first item, a leaf's type. A field the
/// vocabulary does not know is masked, and every masked field keeps a place of its own.
pub fn shape(value: &Value) -> Value {
    match value {
        Value::Object(fields) => {
            let mut masks = Masks::new(KEY);
            let mut out = Map::new();
            for (name, item) in fields {
                let known = known_key(name);
                out.insert(masks.name(name, known), shape(item));
            }
            json!({"kind": "object", "fields": out})
        }
        Value::Array(items) => json!({"kind": "array", "length": items.len(), "items": items.first().map(shape).unwrap_or(Value::Null)}),
        Value::String(_) => json!({"type": "string"}),
        Value::Number(_) => json!({"type": "number"}),
        Value::Bool(_) => json!({"type": "boolean"}),
        Value::Null => json!({"type": "null"}),
    }
}

/// One frame of an orchestration stream: the envelope it is, the method it names, the request it belongs to and the shape of its
/// payload. The request id is the raw value; the recorder gives it a stable label.
#[derive(Clone, Debug, PartialEq)]
pub struct Envelope {
    pub name: String,
    pub method: Option<String>,
    pub request: Option<String>,
    pub payload: Value,
}

/// The envelopes of one WebSocket message: a message holds one value or an array of them.
pub fn envelopes(data: &[u8], binary: bool) -> Vec<Envelope> {
    if binary {
        return vec![Envelope { name: "binary".into(), method: None, request: None, payload: json!({"kind": "binary", "length": data.len()}) }];
    }
    let text = String::from_utf8_lossy(data);
    let Ok(value) = serde_json::from_str::<Value>(&text) else {
        return vec![Envelope { name: "text".into(), method: None, request: None, payload: json!({"kind": "text", "length": data.len()}) }];
    };
    let items = match value {
        Value::Array(items) => items,
        other => vec![other],
    };
    items.iter().map(envelope).collect()
}

/// One envelope of a message: its tag, the method a request names, the request it belongs to and the shape of what it carries. A tag
/// or a method the vocabulary does not know is masked.
fn envelope(value: &Value) -> Envelope {
    let Some(fields) = value.as_object() else {
        return Envelope { name: "text".into(), method: None, request: None, payload: shape(value) };
    };
    let name = match fields.get("_tag").and_then(Value::as_str) {
        Some(tag) if known_tag(tag) => tag.to_string(),
        Some(_) => TAG.to_string(),
        None => "text".to_string(),
    };
    let method = fields.get("tag").and_then(Value::as_str).map(|m| if known_method(m) { m.to_string() } else { METHOD.to_string() });
    let request = fields
        .get("id")
        .or_else(|| fields.get("requestId"))
        .and_then(|v| v.as_str().map(String::from).or_else(|| v.as_i64().map(|n| n.to_string())));
    let payload = if name == "Request" {
        shape(fields.get("payload").unwrap_or(&Value::Null))
    } else {
        let mut rest = Map::new();
        for (key, item) in fields {
            if !matches!(key.as_str(), "_tag" | "tag" | "id" | "requestId") {
                rest.insert(key.clone(), item.clone());
            }
        }
        shape(&Value::Object(rest))
    };
    Envelope { name, method, request, payload }
}

/// A word of the vocabulary, or the mask that stands for everything else.
fn masked_word(word: &str, vocabulary: &[&str], mask: &str) -> String {
    let (dot, rest) = word.strip_prefix('.').map_or(("", word), |rest| (".", rest));
    if vocabulary.contains(&rest) {
        format!("{dot}{rest}")
    } else {
        mask.to_string()
    }
}
