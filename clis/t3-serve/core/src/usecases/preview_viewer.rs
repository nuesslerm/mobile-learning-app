use super::service::{refuse, Refusal, Service};
use crate::ports::{AgentError, Op};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::{json, Value};

pub struct Viewer {
    service: Service,
    workspace: String,
    tab: String,
    thread: String,
    width: u32,
    height: u32,
    permitted: bool,
    controlling: bool,
    mobile: bool,
    navigation: Option<Value>,
    image_request: Option<String>,
    image_picker_armed: bool,
}

impl Service {
    pub fn preview_viewer(&self, query: &[(String, String)]) -> Result<Viewer, Refusal> {
        let param = |key: &str| {
            query
                .iter()
                .find(|(k, _)| k == key)
                .map(|(_, v)| v.as_str())
        };
        let principal = self.socket_ticket(param("wsTicket"), "media")?;
        if !principal.scopes.iter().any(|s| s == "orchestration:read") {
            return Err(self.insufficient_scope(&principal, "orchestration:read"));
        }
        let thread = param("threadId").unwrap_or_default();
        let tab = param("tabId").unwrap_or_default();
        let listed = self
            .agent()
            .run(&Op::Preview {
                method: "preview.list",
                params: json!({"thread":thread}),
            })
            .map_err(|e| refuse(503, json!({"message":e.message()})))?;
        let session = listed["sessions"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|row| row["tab"] == tab && row["runtime"] == "server")
            .ok_or_else(|| refuse(404, json!({"message":"Preview tab closed"})))?;
        let workspace = session["workspace"]
            .as_str()
            .ok_or_else(|| refuse(404, json!({"message":"Preview checkout unavailable"})))?;
        let side = |field: &str, limit: &str, default: u32| {
            session["viewport"][field]
                .as_u64()
                .and_then(|v| u32::try_from(v).ok())
                .or_else(|| param(limit)?.parse().ok())
                .unwrap_or(default)
        };
        let (width, height) = (
            side("width", "maxWidth", 1280),
            side("height", "maxHeight", 800),
        );
        if !valid_dimensions(width, height) {
            return Err(refuse(
                400,
                json!({"message":"Unsupported browser dimensions"}),
            ));
        }
        let permitted = principal.scopes.iter().any(|s| s == "preview:operate")
            && param("interactive") != Some("false");
        let mut viewer = Viewer {
            service: self.clone(),
            tab: tab.into(),
            thread: thread.into(),
            workspace: workspace.into(),
            width,
            height,
            permitted,
            controlling: false,
            image_request: None,
            image_picker_armed: false,
            mobile: false,
            navigation: (param("navigation") == Some("1")).then_some(Value::Null),
        };
        let unavailable = |e: AgentError| refuse(503, json!({"message":e.message()}));
        let pages = viewer.page("browser.pages", json!({})).map_err(unavailable)?;
        if !pages["pages"].as_array().is_some_and(|rows| rows.iter().any(|page| page["key"] == tab)) {
            let mut restore = json!({});
            if let Some(url) = session["nav"]["url"].as_str().filter(|u| *u != "about:blank") {
                restore["url"] = json!(url);
            }
            viewer.page("browser.open", restore).map_err(unavailable)?;
        }
        viewer.take_control();
        Ok(viewer)
    }
}

fn valid_dimensions(w: u32, h: u32) -> bool {
    (1..=4096).contains(&w) && (1..=4096).contains(&h) && w * h <= 5_184_000
}

impl Viewer {
    fn page(&self, method: &'static str, mut params: Value) -> Result<Value, AgentError> {
        params["workspace"] = json!(self.workspace);
        if method != "browser.pages" { params["key"] = json!(self.tab); }
        self.service
            .agent()
            .run(&Op::PreviewPage { method, params })
    }

    pub fn frame(&mut self) -> Result<(Vec<u8>, Option<Value>), AgentError> {
        let shot = self.page(
            "browser.frame",
            json!({"width":self.width,"height":self.height,"quality":72,"mobile":self.mobile}),
        )?;
        let jpeg = STANDARD
            .decode(shot["data"].as_str().unwrap_or_default())
            .ok()
            .filter(|b| b.starts_with(&[255, 216]))
            .ok_or_else(|| AgentError::Failed("Executor returned an invalid preview image".into()))?;
        let changed = self.navigation.as_mut().and_then(|last| {
            let nav = shot.get("navigation")?;
            if !nav.is_object() || *last == *nav { return None; }
            *last = nav.clone();
            Some(json!({"type":"navigation","url":nav["url"],"canGoBack":nav["can_go_back"],"canGoForward":nav["can_go_forward"]}))
        });
        Ok((jpeg, changed))
    }

    pub fn control(&self) -> Value {
        let claimed = self
            .service
            .2
            .preview_controllers
            .lock()
            .unwrap()
            .contains(&self.tab);
        json!({"type":"control","canOperate":self.permitted,"controller":if self.controlling {"you"}else if claimed {"another-viewer"}else{"unclaimed"},"generation":1,"dialog":null})
    }

    fn take_control(&mut self) {
        if self.permitted && !self.controlling {
            self.controlling = self
                .service
                .2
                .preview_controllers
                .lock()
                .unwrap()
                .insert(self.tab.clone());
        }
    }
    fn release(&mut self) {
        if self.controlling {
            self.service
                .2
                .preview_controllers
                .lock()
                .unwrap()
                .remove(&self.tab);
            self.controlling = false;
            self.image_request = None;
            self.image_picker_armed = false;
        }
    }
    pub fn viewport(&self) -> Value {
        json!({"type":"viewport","width":self.width,"height":self.height})
    }

    pub fn input(&mut self, msg: &Value) -> Result<Option<Value>, AgentError> {
        let kind = msg["type"].as_str().unwrap_or_default();
        if kind == "takeControl" {
            self.take_control();
            return Ok(Some(self.control()));
        }
        if kind == "releaseControl" {
            self.release();
            return Ok(Some(self.control()));
        }
        if !self.controlling {
            return Ok(None);
        }
        match kind {
            "resize" => {
                let (w, h) = (
                    msg["width"]
                        .as_u64()
                        .and_then(|v| u32::try_from(v).ok())
                        .unwrap_or(0),
                    msg["height"]
                        .as_u64()
                        .and_then(|v| u32::try_from(v).ok())
                        .unwrap_or(0),
                );
                if valid_dimensions(w, h) {
                    self.service.agent().run(&Op::Preview {method:"preview.resize",params:json!({"thread":self.thread,"tab":self.tab,"viewport":{"mode":"freeform","width":w,"height":h}})})?;
                    (self.width, self.height) = (w, h);
                    self.mobile = msg["mobile"].as_bool().unwrap_or(false);
                    return Ok(Some(self.viewport()));
                }
            }
            "mouse" if msg["action"] == "click" => {
                let button = if msg["button"] == "right" { "right" } else { "left" };
                self.page("browser.input", json!({"kind":"mouse_click","x":msg["x"],"y":msg["y"],"button":button,"modifiers":msg["modifiers"]}))?;
            }
            "mouse" => {
                self.page(
                    "browser.input",
                    json!({"kind":"mouse_move","x":msg["x"],"y":msg["y"]}),
                )?;
                let action = match msg["action"].as_str() {
                    Some("down") => "mouse_down",
                    Some("up") => "mouse_up",
                    _ => return Ok(None),
                };
                if action == "mouse_down" { self.image_picker_armed = false; }
                if action == "mouse_down" && msg["imagePicker"] == true {
                    if let (Some(x), Some(y)) = (msg["x"].as_f64(), msg["y"].as_f64()) {
                        let script = format!("Boolean(document.elementFromPoint({x},{y})?.closest('[aria-label=\"Attach image\"]') && (window.rcaImagePickerUntil=Date.now()+3000))");
                        self.image_picker_armed = self.page("browser.eval", json!({"expression":script}))?["value"] == true;
                    }
                }
                let report_focus = action == "mouse_up" && msg["reportFocus"] == true;
                let input = self.page(
                    "browser.input",
                    json!({"kind":action,"button":msg["button"],"report_focus":report_focus}),
                )?;
                if action == "mouse_up" && std::mem::take(&mut self.image_picker_armed) {
                    let request = self.page("browser.eval", json!({"expression":"window.rcaImageRequest?.id ?? null"}))?;
                    if let Some(id) = request["value"].as_str().filter(|id| id.len() == 36 && id.bytes().all(|c| c.is_ascii_hexdigit() || c == b'-')) {
                        if self.image_request.as_deref() != Some(id) {
                            self.image_request = Some(id.into());
                            return Ok(Some(json!({"type":"imagePicker","id":id})));
                        }
                    }
                }
                if report_focus {
                    if let Some(editable) = input["editable"].as_bool() {
                        return Ok(Some(json!({"type":"textFocus","editable":editable})));
                    }
                    // Older executors report only the pointer position.
                    let focus = self.page("browser.eval", json!({"expression":
                        "(() => { const e = document.activeElement; return !!e && (e.isContentEditable || (!e.disabled && !e.readOnly && (e.tagName === 'TEXTAREA' || (e.tagName === 'INPUT' && ['text','email','number','password','search','tel','url'].includes(e.type))))); })()"
                    }))?;
                    return Ok(Some(json!({"type":"textFocus","editable":focus["value"] == true})));
                }
            }
            "imagePickerResult" => {
                let Some(id) = msg["id"].as_str() else { return Ok(None) };
                if self.image_request.as_deref() != Some(id) { return Ok(None); }
                let part = &msg["part"];
                let done = msg["done"] == true;
                if !part.is_null() && !part.as_str().is_some_and(|s| s.len() <= 48 * 1024 && s.bytes().all(|c| c.is_ascii_alphanumeric() || b"+/=".contains(&c))) {
                    return Err(AgentError::Failed("Invalid image selection".into()));
                }
                if done || part.is_null() { self.image_request = None; }
                let script = format!("(() => {{const r=window.rcaImageRequest;if(r?.id==={})r.receive({part},{done});return true}})()", msg["id"]);
                self.page("browser.eval", json!({"expression":script}))?;
            }
            "wheel" => {
                self.page(
                    "browser.input",
                    json!({"kind":"mouse_wheel","dx":msg["deltaX"],"dy":msg["deltaY"]}),
                )?;
            }
            "text" => {
                self.page(
                    "browser.input",
                    json!({"kind":"insert_text","text":msg["text"]}),
                )?;
            }
            "key" if msg["action"] == "down" => {
                self.page(
                    "browser.input",
                    json!({"kind":"key_press","text":msg["key"]}),
                )?;
            }
            "navigate" | "history" | "reload" => {
                let page = match kind {
                    "navigate" => self.page("browser.goto", json!({"url":msg["url"]})),
                    "reload" => self.page("browser.history", json!({"direction":"reload"})),
                    _ => self.page(
                        "browser.history",
                        json!({"direction":if msg["delta"]==-1 {"back"}else{"forward"}}),
                    ),
                }?;
                if let Some(url) = page["url"]
                    .as_str()
                    .filter(|u| u.starts_with("http://") || u.starts_with("https://"))
                {
                    self.service.agent().run(&Op::Preview {method:"preview.report",params:json!({"thread":self.thread,"tab":self.tab,"nav":{"state":"success","url":url,"title":page["title"].as_str().unwrap_or("")},"can_go_back":page["canGoBack"].as_bool().unwrap_or(false),"can_go_forward":page["canGoForward"].as_bool().unwrap_or(false)})})?;
                }
            }
            _ => {}
        }
        Ok(None)
    }
}

impl Drop for Viewer {
    fn drop(&mut self) {
        self.release();
    }
}
