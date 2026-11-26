use super::{ask, locate, plain, Page};
use agent_api_browser::{FRAME, PAGES};
use base64::{engine::general_purpose::STANDARD, Engine};
use orca_core::area::Out;
use orca_core::wire::Failure;
use orca_core::{AgentApi, Ctx, Subscription};
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub(super) fn open(ctx: &mut Ctx, params: &Value) -> Result<Box<dyn Subscription>, Failure> {
    let page = locate(
        ctx.agent,
        plain(params, "worktree")?,
        plain(params, "page")?,
    )?;
    let pages = ask(ctx.agent, PAGES, &page, json!({}))?;
    let tab = match pages["pages"].as_array().and_then(|rows| rows.iter().find(|row| row["key"] == page.key)) {
        Some(row) => row.clone(),
        None => {
            // Chromium frees idle pages. Only a browser tab still in this checkout's layout may reopen one.
            let _turn = orca_core::kept_state_turn();
            let tabs = orca_map_workspace::layout::kept(ctx.agent, &page.workspace)?;
            let saved = tabs.iter().find(|tab| tab["kind"] == "browser" && tab["id"] == page.key)
                .ok_or_else(|| Failure::new("browser_tab_not_found", "Browser page was not found"))?;
            let url = saved["url"].as_str().filter(|url| !url.is_empty() && *url != "about:blank");
            let args = url.map_or_else(|| json!({}), |url| json!({"url":url}));
            ask(ctx.agent, agent_api_browser::OPEN, &page, args)?
        }
    };
    let dimension = |viewport: &str, max: &str, default| {
        let value = params.get(viewport).or_else(|| params.get(max));
        match value {
            Some(v) => v
                .as_u64()
                .filter(|n| (1..=4096).contains(n))
                .ok_or_else(|| Failure::invalid(format!("Invalid {viewport}"))),
            None => Ok(default),
        }
    };
    let width = dimension("viewportWidth", "maxWidth", 1280)?;
    let height = dimension("viewportHeight", "maxHeight", 800)?;
    if width * height > 5_184_000 {
        return Err(Failure::invalid("Viewport is too large"));
    }
    if params.get("format").is_some_and(|v| v != "jpeg") {
        return Err(Failure::invalid("This stream requires jpeg"));
    }
    let id = ctx.subscription_id("browser-screencast:");
    let ack = params["frameAck"] == 1;
    let ready = json!({"type":"ready","subscriptionId":id,"browserPageId":page.key,"format":"jpeg","frameAck":ack,
        "tab":{"browserPageId":page.key,"title":tab["title"],"url":tab["url"],"active":true,"loaded":true,"worktreeId":params["worktree"].as_str().unwrap_or("").trim_start_matches("id:")}});
    let args = json!({"width":width,"height":height,"mobile":params["mobile"].as_bool().unwrap_or(false),"quality":params["quality"].as_u64().unwrap_or(72).clamp(1,100)});
    ctx.subscriptions.cancel_prefix("browser-screencast");
    Ok(Box::new(Stream {
        agent: ctx.agent_handle.clone(),
        page,
        id,
        ready: Some(ready),
        args,
        ack,
        pending: None,
        seq: 0,
        stopped: false,
        interval: Duration::from_millis(
            params["minFrameIntervalMs"]
                .as_u64()
                .unwrap_or(100)
                .clamp(100, 1000),
        ),
        next: Instant::now(),
    }))
}

struct Stream {
    agent: Arc<dyn AgentApi>,
    page: Page,
    id: String,
    ready: Option<Value>,
    args: Value,
    ack: bool,
    pending: Option<u32>,
    seq: u32,
    stopped: bool,
    interval: Duration,
    next: Instant,
}

impl Stream {
    fn capture(&self) -> Result<Vec<u8>, Failure> {
        let shot = ask(self.agent.as_ref(), FRAME, &self.page, self.args.clone())?;
        let jpeg = STANDARD
            .decode(shot["data"].as_str().unwrap_or_default())
            .map_err(|_| Failure::internal("Invalid browser image"))?;
        if !jpeg.starts_with(&[0xff, 0xd8]) {
            return Err(Failure::internal("Expected JPEG browser image"));
        }
        let metadata = json!({"deviceWidth":self.args["width"],"deviceHeight":self.args["height"],"imageWidth":self.args["width"],"imageHeight":self.args["height"]}).to_string();
        let mut bytes = Vec::with_capacity(16 + metadata.len() + jpeg.len());
        bytes.extend_from_slice(&[0x62, 1, 1, 1]);
        bytes.extend_from_slice(&self.seq.to_le_bytes());
        bytes.extend_from_slice(&(metadata.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&[0; 4]);
        bytes.extend_from_slice(metadata.as_bytes());
        bytes.extend_from_slice(&jpeg);
        Ok(bytes)
    }
}

impl Subscription for Stream {
    fn id(&self) -> &str {
        &self.id
    }
    fn poll(&mut self) -> Vec<Value> {
        self.ready.take().into_iter().collect()
    }
    fn end(&mut self) -> Vec<Value> {
        self.stopped = true;
        Vec::new()
    }
    fn acknowledge(&mut self, seq: u32) -> bool {
        if self.pending == Some(seq) {
            self.pending = None;
            true
        } else {
            false
        }
    }
    fn poll_out(&mut self) -> Vec<Out> {
        if let Some(ready) = self.ready.take() {
            return vec![Out::Text(ready.to_string())];
        }
        if self.stopped || self.pending.is_some() || Instant::now() < self.next {
            return Vec::new();
        }
        match self.capture() {
            Ok(bytes) => {
                self.next = Instant::now() + self.interval;
                self.pending = self.ack.then_some(self.seq);
                self.seq = self.seq.wrapping_add(1);
                vec![Out::Binary(bytes)]
            }
            Err(e) => {
                self.stopped = true;
                vec![Out::Text(json!({"type":"error","message":e.message,"error":{"code":e.code,"message":e.message}}).to_string())]
            }
        }
    }
}
