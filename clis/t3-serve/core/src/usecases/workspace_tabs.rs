use super::queries::QueryFailure;
use super::service::{Refusal, Service};
use crate::ports::{AgentError, NoteAction, Op};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

const SCOPE: &str = "mobile.checkout.tabs";
const CONTEXT: &str = "mobile.checkout.context";

#[derive(Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct Layout {
    tabs: Vec<Tab>,
    active: Option<String>,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct Tab {
    id: String,
    kind: String,
    title: String,
    resource: String,
}

#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "camelCase")]
enum Mutation {
    Create {
        id: String,
        kind: String,
        url: Option<String>,
    },
    Select {
        id: Option<String>,
    },
    Close {
        id: String,
    },
}

fn upstream(err: AgentError) -> Refusal {
    Refusal {
        status: 502,
        body: json!({"message":err.message()}),
    }
}

fn resource_error(err: QueryFailure) -> Refusal {
    match err {
        QueryFailure::Error(body) => Refusal { status: 409, body },
        QueryFailure::Defect(message) => Refusal {
            status: 422,
            body: json!({"message":message}),
        },
    }
}

fn context_id(checkout: &str) -> String {
    format!(
        "wc-{}",
        &format!("{:x}", Sha256::digest(checkout.as_bytes()))[..20]
    )
}

impl Service {
    pub(super) fn resource_root(&self, thread: &str) -> Result<String, QueryFailure> {
        if !(thread.len() == 23
            && thread.starts_with("wc-")
            && thread[3..].bytes().all(|b| b.is_ascii_hexdigit()))
        {
            return Ok(self.thread_dir(thread));
        }
        let entry = self
            .agent()
            .run(&Op::StateGet {
                scope: CONTEXT.into(),
                key: thread.into(),
            })
            .map_err(|e| QueryFailure::Error(json!({"message":e.message()})))?;
        entry["value"]
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| QueryFailure::Error(json!({"message":"Checkout unavailable"})))
    }

    pub fn workspace_tabs_http(
        &self,
        auth: Option<&str>,
        checkout: &str,
        body: Option<Value>,
    ) -> Result<Value, Refusal> {
        self.read_gate("mobile.workspaceTabs", auth)?;
        let mutation = body
            .map(serde_json::from_value::<Mutation>)
            .transpose()
            .map_err(|_| Refusal::bare(422))?;
        if mutation.is_some() {
            self.scope_gate("mobile.workspaceTabs", auth, "orchestration:operate")?;
        }
        let catalog = self.agent().run(&Op::MobileWorktrees).map_err(upstream)?;
        let row = catalog["checkouts"]
            .as_array()
            .ok_or_else(|| Refusal::bare(502))?
            .iter()
            .find(|r| r["id"] == checkout && r["missing"] != true)
            .ok_or_else(|| Refusal {
                status: 404,
                body: json!({"code":"CHECKOUT_NOT_FOUND"}),
            })?;
        let root = row["path"].as_str().ok_or_else(|| Refusal::bare(502))?;
        let thread = context_id(checkout);
        let _turn = self
            .held()
            .workspace_edits
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let stored = self
            .agent()
            .run(&Op::StateGet {
                scope: SCOPE.into(),
                key: thread.clone(),
            })
            .map_err(upstream)?;
        let mut layout: Layout = if stored["value"].is_null() {
            Layout::default()
        } else {
            serde_json::from_value(stored["value"].clone()).map_err(|_| Refusal::bare(502))?
        };
        if let Some(action) = mutation {
            match action {
                Mutation::Create { id, kind, url } => {
                    if id.is_empty()
                        || id.len() > 23
                        || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
                    {
                        return Err(Refusal::bare(422));
                    }
                    if let Some(existing) = layout.tabs.iter().find(|t| t.id == id) {
                        if existing.kind != kind {
                            return Err(Refusal::bare(409));
                        }
                    } else {
                        self.agent()
                            .run(&Op::StateSet {
                                scope: CONTEXT.into(),
                                key: thread.clone(),
                                value: json!(root),
                            })
                            .map_err(upstream)?;
                        let (title, resource) = match kind.as_str() {
                            "terminal" => {
                                self.scope_gate("mobile.workspaceTabs", auth, "terminal:operate")?;
                                self.terminal_open(&json!({"threadId":thread,"terminalId":id,"cwd":root,"worktreePath":root})).map_err(resource_error)?;
                                ("Terminal".into(), id.clone())
                            }
                            "browser" => {
                                self.scope_gate("mobile.workspaceTabs", auth, "preview:operate")?;
                                let opened = self
                                    .preview_open(
                                        &json!({"threadId":thread,"runtime":"server","url":url}),
                                    )
                                    .map_err(resource_error)?;
                                (
                                    "Browser".into(),
                                    opened["tabId"]
                                        .as_str()
                                        .ok_or_else(|| Refusal::bare(502))?
                                        .into(),
                                )
                            }
                            "markdown" => {
                                let note =
                                    self.checkout_note_http(auth, checkout, NoteAction::Create)?;
                                let path = note["path"]
                                    .as_str()
                                    .ok_or_else(|| Refusal::bare(502))?
                                    .to_owned();
                                (path.clone(), path)
                            }
                            _ => return Err(Refusal::bare(422)),
                        };
                        layout.tabs.push(Tab {
                            id: id.clone(),
                            kind,
                            title,
                            resource,
                        });
                    }
                    layout.active = Some(id);
                }
                Mutation::Select { id } => {
                    if id
                        .as_ref()
                        .is_some_and(|id| !layout.tabs.iter().any(|t| &t.id == id))
                    {
                        return Err(Refusal::bare(404));
                    }
                    layout.active = id;
                }
                Mutation::Close { id } => {
                    if let Some(i) = layout.tabs.iter().position(|t| t.id == id) {
                        let tab = &layout.tabs[i];
                        if tab.kind == "browser" {
                            self.scope_gate("mobile.workspaceTabs", auth, "preview:operate")?;
                            self.preview_close(&json!({"threadId":thread,"tabId":tab.resource}))
                                .map_err(resource_error)?;
                        } else if tab.kind == "terminal" {
                            self.scope_gate("mobile.workspaceTabs", auth, "terminal:operate")?;
                            self.terminal_close(
                                &json!({"threadId":thread,"terminalId":tab.resource}),
                            )
                            .map_err(resource_error)?;
                        }
                        layout.tabs.remove(i);
                        if layout.active.as_deref() == Some(&id) {
                            layout.active = layout.tabs.last().map(|t| t.id.clone());
                        }
                    }
                }
            }
            self.agent()
                .run(&Op::StateSet {
                    scope: SCOPE.into(),
                    key: thread.clone(),
                    value: json!(layout),
                })
                .map_err(upstream)?;
        }
        Ok(
            json!({"threadId":thread,"checkout":checkout,"directory":root,"tabs":layout.tabs,"active":layout.active}),
        )
    }
}
