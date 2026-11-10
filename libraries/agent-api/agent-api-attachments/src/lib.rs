//! The `attachments` group of the agent API (`docs/plans/rca-parity/plan.md`, section 2): a file a person sends with a message. The service keeps
//! the bytes on its own volume under an id the caller names or the service mints (`upload.put`), reads them back in chunks (`attachment.get`),
//! drops them (`attachment.delete`), and places them into a workspace through the executor that owns it (`attachment.stage`, whose executor
//! action is `put_blob`). `turn.start` names attachments by these ids. Neutral names: snake_case fields, times in milliseconds since the epoch.
use agent_api_proto::{GroupName, Method, ParamError};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{json, Value};

#[cfg(any(test, feature = "testing"))]
pub mod testing;

pub const GROUP: GroupName = GroupName::new("attachments");

pub const UPLOAD_PUT: Method = Method::new(GROUP, "upload.put");
pub const GET: Method = Method::new(GROUP, "attachment.get");
pub const DELETE: Method = Method::new(GROUP, "attachment.delete");
pub const STAGE: Method = Method::new(GROUP, "attachment.stage");

/// Every method of the group.
pub const METHODS: &[Method] = &[UPLOAD_PUT, GET, DELETE, STAGE];

/// The most bytes one attachment holds; the executor's `put_blob` takes the same. Its base64 in one request must fit the 16 MiB frame the
/// agent socket and the executor channel accept, which a test holds.
pub const MAX_BYTES: u64 = 10 << 20;
/// The most bytes one `attachment.get` returns; a longer `length` is cut to it.
pub const CHUNK_MAX: u64 = 1 << 20;
/// The most ids one `attachment.stage` names.
pub const MAX_STAGE: usize = 100;
const MAX_ID: usize = 128;
const MAX_NAME: usize = 255;
const MAX_MIME: usize = 100;

/// Where `attachment.stage` places a file in the workspace: `.rca/attachments/<id>/<name>`.
pub const STAGE_DIR: &str = ".rca/attachments";
/// The first stage in a workspace also writes this file, which ignores everything below `.rca`: a staged file never shows in `git status`
/// and is never committed by `git add -A`.
pub const STAGE_IGNORE_PATH: &str = ".rca/.gitignore";
pub const STAGE_IGNORE_CONTENT: &str = "*\n";

pub mod code {
    pub const ATTACHMENT_NOT_FOUND: &str = "ATTACHMENT_NOT_FOUND";
    /// `upload.put` under an id that holds other bytes: an attachment never changes.
    pub const ATTACHMENT_EXISTS: &str = "ATTACHMENT_EXISTS";
    pub const ATTACHMENT_TOO_LARGE: &str = "ATTACHMENT_TOO_LARGE";
    /// The service could not read or write its attachments; nothing changed.
    pub const ATTACHMENTS_UNAVAILABLE: &str = "ATTACHMENTS_UNAVAILABLE";

    pub const ALL: [&str; 4] = [ATTACHMENT_NOT_FOUND, ATTACHMENT_EXISTS, ATTACHMENT_TOO_LARGE, ATTACHMENTS_UNAVAILABLE];
}

pub mod message {
    pub fn not_found(id: &str) -> String {
        format!("no attachment has the id {id}")
    }

    pub fn exists(id: &str) -> String {
        format!("attachment {id} already holds other bytes")
    }

    pub fn too_large(size: u64, max: u64) -> String {
        format!("an attachment is at most {max} bytes, this one is {size}")
    }
}

fn read<T: DeserializeOwned>(params: &Value) -> Result<T, ParamError> {
    let params = if params.is_null() { json!({}) } else { params.clone() };
    serde_json::from_value(params).map_err(|e| ParamError(e.to_string()))
}

/// What an id may be: it becomes a directory name in a workspace, so no separator, no dot at the start, nothing outside this set.
pub fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= MAX_ID && !id.starts_with('.') && id.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

/// The shape of standard padded base64: its alphabet, a length that is a multiple of four, padding only at the end. The service decodes it.
pub fn looks_base64(text: &str) -> bool {
    let body = text.trim_end_matches('=');
    text.len().is_multiple_of(4) && text.len() - body.len() <= 2 && body.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'+' || b == b'/')
}

fn check_id(name: &str, id: &str) -> Result<(), ParamError> {
    if valid_id(id) {
        return Ok(());
    }
    Err(ParamError(format!("{name} is 1 to {MAX_ID} letters, digits, dots, dashes or underscores, and does not start with a dot")))
}

/// The file name a staged attachment gets: the last path segment of `name`, control characters and separators replaced, no leading dots, cut to
/// 128 bytes at a character boundary; `attachment` when nothing is left.
pub fn safe_file_name(name: &str) -> String {
    let last = name.rsplit(['/', '\\']).next().unwrap_or("");
    let mut clean: String = last.chars().map(|c| if c.is_control() || matches!(c, '/' | '\\' | ':') { '_' } else { c }).collect();
    clean = clean.trim_start_matches('.').trim().to_string();
    while clean.len() > 128 {
        clean.pop();
    }
    if clean.is_empty() {
        "attachment".into()
    } else {
        clean
    }
}

/// The workspace-relative path `attachment.stage` writes an attachment to.
pub fn staged_path(id: &str, name: &str) -> String {
    format!("{STAGE_DIR}/{id}/{}", safe_file_name(name))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Image,
    File,
}

/// `upload.put`: keep `data` (base64, at most `MAX_BYTES` once decoded) as an attachment. `id` is the caller's (a mapper that handed one out
/// before the bytes came), or the service's when absent. The same id with the same bytes again is the same attachment; with other bytes it is refused.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PutParams {
    #[serde(default)]
    pub id: Option<String>,
    pub name: String,
    pub mime_type: String,
    pub kind: Kind,
    pub data: String,
}

impl PutParams {
    pub fn parse(params: &Value) -> Result<PutParams, ParamError> {
        let p: PutParams = read(params)?;
        if let Some(id) = &p.id {
            check_id("id", id)?;
        }
        if p.name.trim().is_empty() || p.name.len() > MAX_NAME {
            return Err(ParamError(format!("name is 1 to {MAX_NAME} bytes")));
        }
        if p.mime_type.trim().is_empty() || p.mime_type.len() > MAX_MIME {
            return Err(ParamError(format!("mime_type is 1 to {MAX_MIME} bytes")));
        }
        if p.data.is_empty() {
            return Err(ParamError("data is required".into()));
        }
        if p.data.len() as u64 > MAX_BYTES.div_ceil(3) * 4 {
            return Err(ParamError(format!("data is longer than {MAX_BYTES} bytes")));
        }
        if !looks_base64(&p.data) {
            return Err(ParamError("data is not base64".into()));
        }
        Ok(p)
    }
}

/// An attachment as kept.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AttachmentView {
    pub id: String,
    pub name: String,
    pub mime_type: String,
    pub kind: Kind,
    pub size_bytes: u64,
    /// SHA-256 of the bytes, lowercase hex: it changes only when the bytes do.
    pub sha256: String,
    pub created_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PutResult {
    pub attachment: AttachmentView,
}

/// `attachment.get`: `length` bytes of the attachment from `offset`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GetParams {
    pub id: String,
    #[serde(default)]
    pub offset: Option<u64>,
    #[serde(default)]
    pub length: Option<u64>,
}

impl GetParams {
    pub fn parse(params: &Value) -> Result<GetParams, ParamError> {
        let p: GetParams = read(params)?;
        check_id("id", &p.id)?;
        Ok(p)
    }

    pub fn offset(&self) -> u64 {
        self.offset.unwrap_or(0)
    }

    pub fn length(&self) -> u64 {
        self.length.unwrap_or(CHUNK_MAX).min(CHUNK_MAX)
    }
}

/// `data` is the bytes of the range, base64 (standard alphabet, padded); `eof` is true when the range reaches the end.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GetResult {
    pub attachment: AttachmentView,
    pub data: String,
    pub offset: u64,
    pub bytes: u64,
    pub eof: bool,
}

/// `attachment.delete` and the single-id params shape.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IdParams {
    pub id: String,
}

impl IdParams {
    pub fn parse(params: &Value) -> Result<IdParams, ParamError> {
        let p: IdParams = read(params)?;
        check_id("id", &p.id)?;
        Ok(p)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeleteResult {
    pub id: String,
    /// False when there was no such attachment: a delete is safe to repeat.
    pub deleted: bool,
}

/// `attachment.stage`: write each attachment into the workspace's executor, at `staged_path`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StageParams {
    pub workspace: String,
    pub ids: Vec<String>,
}

impl StageParams {
    pub fn parse(params: &Value) -> Result<StageParams, ParamError> {
        let p: StageParams = read(params)?;
        if p.workspace.trim().is_empty() {
            return Err(ParamError("workspace is required".into()));
        }
        if p.ids.is_empty() || p.ids.len() > MAX_STAGE {
            return Err(ParamError(format!("ids is 1 to {MAX_STAGE} attachment ids")));
        }
        p.ids.iter().try_for_each(|id| check_id("ids", id))?;
        Ok(p)
    }
}

/// An attachment placed in a workspace: `path` is relative to the workspace root.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StagedFile {
    pub id: String,
    pub name: String,
    pub path: String,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StageResult {
    pub files: Vec<StagedFile>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::fixtures;
    use agent_api_proto::Request;

    fn parse_params(method: &str, params: &Value) -> Result<(), ParamError> {
        match method {
            "upload.put" => PutParams::parse(params).map(drop),
            "attachment.get" => GetParams::parse(params).map(drop),
            "attachment.delete" => IdParams::parse(params).map(drop),
            "attachment.stage" => StageParams::parse(params).map(drop),
            other => panic!("{other} is no method of the group"),
        }
    }

    fn parse_result(method: &str, result: &Value) -> Result<(), String> {
        let read = |r: Result<(), serde_json::Error>| r.map_err(|e| e.to_string());
        match method {
            "upload.put" => read(serde_json::from_value::<PutResult>(result.clone()).map(drop)),
            "attachment.get" => read(serde_json::from_value::<GetResult>(result.clone()).map(drop)),
            "attachment.delete" => read(serde_json::from_value::<DeleteResult>(result.clone()).map(drop)),
            "attachment.stage" => read(serde_json::from_value::<StageResult>(result.clone()).map(drop)),
            other => panic!("{other} is no method of the group"),
        }
    }

    #[test]
    fn given_the_recorded_exchanges_when_checked_against_the_contract_then_every_method_has_a_success_and_a_refusal_the_params_and_results_read() {
        // Given the recorded exchanges of the group
        let recorded = fixtures();
        // Then there is one file per method, and no more
        let mut named: Vec<&str> = recorded.iter().map(|f| f.method.as_str()).collect();
        named.sort_unstable();
        let mut methods: Vec<&str> = METHODS.iter().map(|m| m.name()).collect();
        methods.sort_unstable();
        assert_eq!(named, methods);
        for fixture in &recorded {
            let method = fixture.method.as_str();
            // And each has an exchange that succeeds and one that is refused
            assert!(fixture.exchanges.iter().any(|e| e.succeeds()) && fixture.exchanges.iter().any(|e| !e.succeeds()), "{method}");
            for exchange in &fixture.exchanges {
                let what = format!("{method}: {}", exchange.name);
                for request in exchange.before.iter().chain([&exchange.request]) {
                    let parsed = Request::from_value(request).unwrap_or_else(|_| panic!("{what}: not a request"));
                    assert!(METHODS.iter().any(|m| m.name() == parsed.method), "{what}: {}", parsed.method);
                }
                assert_eq!(exchange.request["method"].as_str(), Some(method), "{what}");
                if exchange.succeeds() {
                    assert_eq!(parse_params(method, &exchange.request["params"]), Ok(()), "{what}");
                    // A volatile time is read as a number: its recorded stand-in is text
                    let result = exchange.filled(|pointer| (pointer == "/result/attachment/created_at_ms").then(|| json!(0)))["result"].clone();
                    assert_eq!(parse_result(method, &result), Ok(()), "{what}");
                    // And only `attachment.stage` asks an executor, for `put_blob` once per file
                    let wants_actions = method == "attachment.stage";
                    assert_eq!(exchange.actions.iter().all(|a| a == "put_blob") && !exchange.actions.is_empty(), wants_actions, "{what}: {:?}", exchange.actions);
                } else {
                    let code = exchange.response["error"]["code"].as_str().unwrap();
                    assert!(code::ALL.contains(&code) || ["INVALID_PARAMS", "NO_EXECUTOR"].contains(&code), "{what}: {code}");
                    if code == "INVALID_PARAMS" {
                        assert_eq!(parse_params(method, &exchange.request["params"]).unwrap_err().0, exchange.response["error"]["message"].as_str().unwrap(), "{what}");
                    }
                }
            }
        }
    }

    #[test]
    fn given_an_attachment_of_the_most_when_sent_as_one_request_then_the_frame_fits_what_a_websocket_end_accepts_by_default() {
        // Given the longest request `upload.put` can be: the most bytes, base64, and every field at its longest
        let encoded = "A".repeat((MAX_BYTES.div_ceil(3) * 4) as usize);
        let frame = json!({"id": u64::MAX, "method": "upload.put", "params": {"id": "i".repeat(128), "name": "n".repeat(255), "mime_type": "m".repeat(100), "kind": "image", "data": encoded}}).to_string();
        // Then it is under the frame limit of tungstenite's defaults, which both ends of the agent socket and the executor channel keep
        assert!((frame.len() as u64) < 16 << 20, "{} bytes", frame.len());
        assert_eq!(PutParams::parse(&json!({"name": "n", "mime_type": "m", "kind": "image", "data": encoded})), Ok(PutParams { id: None, name: "n".into(), mime_type: "m".into(), kind: Kind::Image, data: encoded }));
    }

    #[test]
    fn given_params_that_are_not_an_attachment_when_read_then_each_is_refused_with_what_is_wrong() {
        let put = |patch: Value| {
            let mut base = json!({"name": "shot.png", "mime_type": "image/png", "kind": "image", "data": "aGk="});
            base.as_object_mut().unwrap().extend(patch.as_object().unwrap().clone());
            base
        };
        // Given ids that would climb out of a directory, an empty name, an empty body, a body past the cap and an unknown field
        let cases = [
            (put(json!({"id": "../x"})), "id is 1 to 128 letters, digits, dots, dashes or underscores, and does not start with a dot"),
            (put(json!({"id": ".hidden"})), "id is 1 to 128 letters, digits, dots, dashes or underscores, and does not start with a dot"),
            (put(json!({"id": "a/b"})), "id is 1 to 128 letters, digits, dots, dashes or underscores, and does not start with a dot"),
            (put(json!({"name": " "})), "name is 1 to 255 bytes"),
            (put(json!({"data": ""})), "data is required"),
            (put(json!({"data": "A".repeat((MAX_BYTES.div_ceil(3) * 4) as usize + 1)})), &format!("data is longer than {MAX_BYTES} bytes")),
        ];
        for (params, message) in cases {
            // Then each is refused naming the field
            assert_eq!(PutParams::parse(&params).unwrap_err().0, message, "{params}");
        }
        assert!(PutParams::parse(&put(json!({"path": "/etc"}))).unwrap_err().0.contains("unknown field `path`"));
        assert!(PutParams::parse(&put(json!({"kind": "video"}))).is_err());
        for bad in ["***", "abc", "a=bc", "aGk==="] {
            assert_eq!(PutParams::parse(&put(json!({"data": bad}))).unwrap_err().0, "data is not base64", "{bad}");
        }
        // And a stage names 1 to 100 valid ids
        assert!(StageParams::parse(&json!({"workspace": "w", "ids": []})).is_err());
        assert!(StageParams::parse(&json!({"workspace": "w", "ids": (0..=MAX_STAGE).map(|i| format!("a{i}")).collect::<Vec<_>>()})).is_err());
        assert!(StageParams::parse(&json!({"workspace": "w", "ids": ["a1", "b-2.png"]})).is_ok());
    }

    #[test]
    fn given_names_that_would_leave_the_attachment_directory_when_staged_then_the_path_stays_below_it() {
        // Given file names with separators, dots, control characters, nothing, and a very long one
        let long = "é".repeat(200);
        let names = ["../../etc/passwd", "..\\..\\win.ini", ".bashrc", "a\nb.txt", "", "///", "dir/", long.as_str()];
        for name in names {
            // When the path is made
            let path = staged_path("a1", name);
            // Then it is `.rca/attachments/a1/<one segment>` with no dot segment, and the segment is at most 128 bytes
            let rest = path.strip_prefix(".rca/attachments/a1/").unwrap_or_else(|| panic!("{name:?} -> {path}"));
            assert!(!rest.contains('/') && !rest.starts_with('.') && !rest.is_empty() && rest.len() <= 128 && !rest.chars().any(char::is_control), "{name:?} -> {path}");
        }
        assert_eq!(staged_path("a1", "shot.png"), ".rca/attachments/a1/shot.png");
        assert_eq!(staged_path("a1", "../../etc/passwd"), ".rca/attachments/a1/passwd");
    }

    #[test]
    fn given_the_methods_of_the_group_when_listed_then_they_are_of_this_group_with_the_owners_names() {
        let names: Vec<&str> = METHODS.iter().map(|m| m.name()).collect();
        assert_eq!(names, ["upload.put", "attachment.get", "attachment.delete", "attachment.stage"]);
        assert!(METHODS.iter().all(|m| m.group() == GROUP) && GROUP.as_str() == "attachments");
        let get = GetParams::parse(&json!({"id": "a", "length": CHUNK_MAX * 9})).unwrap();
        assert_eq!((get.offset(), get.length()), (0, CHUNK_MAX));
    }
}
