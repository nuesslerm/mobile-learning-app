use crate::rca::Rca;
use axum::{
    extract::{
        ws::{Message, WebSocket},
        DefaultBodyLimit, Path, Query, Request, State, WebSocketUpgrade,
    },
    http::{HeaderMap, StatusCode},
    middleware::{from_fn_with_state, Next},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
    Json, Router,
};
use rca_mobile_core::{self as mobile, Agent, Devices, Failure, NewThread, ReadWrite};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{sync::Arc, time::Duration};
use tokio::sync::mpsc;

#[derive(Clone)]
struct App {
    devices: Arc<dyn Devices>,
    agent: Arc<Rca>,
}

pub fn router(devices: Arc<dyn Devices>, agent: Arc<Rca>) -> Router {
    let app = App { devices, agent };
    let protected = Router::new()
        .route("/v1/pair", delete(revoke))
        .route("/v1/repositories", get(repositories))
        .route("/v1/workspaces", get(workspaces).post(create_workspace))
        .route(
            "/v1/workspaces/{workspace}/threads",
            get(threads).post(create_thread),
        )
        .route("/v1/workspaces/{workspace}/tasks", get(tasks))
        .route("/v1/threads/{thread}", get(context))
        .route("/v1/threads/{thread}/history", get(history))
        .route("/v1/threads/{thread}/transcript", get(transcript))
        .route("/v1/threads/{thread}/turns/{id}", post(turn))
        .route("/v1/threads/{thread}/approvals/{id}", post(approve))
        .route("/v1/threads/{thread}/questions/{id}", post(answer))
        .route_layer(from_fn_with_state(app.clone(), authorize));
    protected
        .merge(
            Router::new()
                .route("/v1/pair", post(pair))
                .route("/v1/threads/{thread}/events", get(events)),
        )
        .layer(DefaultBodyLimit::max(256 * 1024))
        .with_state(app)
}

struct Error(Failure);
impl From<Failure> for Error {
    fn from(error: Failure) -> Self {
        Self(error)
    }
}
impl IntoResponse for Error {
    fn into_response(self) -> Response {
        let status = match self.0.code.as_str() {
            "UNAUTHORIZED" => StatusCode::UNAUTHORIZED,
            "THREAD_NOT_FOUND" | "NOT_FOUND" => StatusCode::NOT_FOUND,
            "INVALID_PARAMS" | "INVALID_ARGS" => StatusCode::BAD_REQUEST,
            "DISCONNECTED" | "TIMEOUT" => StatusCode::SERVICE_UNAVAILABLE,
            "STORE_ERROR" | "INTERNAL" => StatusCode::INTERNAL_SERVER_ERROR,
            _ => StatusCode::BAD_GATEWAY,
        };
        (
            status,
            Json(json!({"code":self.0.code,"error":self.0.message})),
        )
            .into_response()
    }
}
type Reply = Result<Json<Value>, Error>;
fn token(h: &HeaderMap) -> Option<&str> {
    h.get("authorization")?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
        .filter(|s| !s.is_empty())
}
async fn authorize(State(app): State<App>, req: Request, next: Next) -> Result<Response, Error> {
    if let Some(t) = token(req.headers()) {
        if app.devices.authenticated(t)? {
            return Ok(next.run(req).await);
        }
    }
    Err(Error(Failure::new("UNAUTHORIZED", "Pair this device")))
}
async fn blocking(f: impl FnOnce() -> mobile::Result<Value> + Send + 'static) -> Reply {
    let value = tokio::task::spawn_blocking(f)
        .await
        .map_err(|_| Error(Failure::new("INTERNAL", "Gateway worker stopped")))??;
    Ok(Json(value))
}

#[derive(Deserialize)]
struct Code {
    code: String,
}
async fn pair(State(app): State<App>, Json(body): Json<Code>) -> Reply {
    Ok(Json(json!({"token":app.devices.exchange(&body.code)?})))
}
async fn revoke(State(app): State<App>, headers: HeaderMap) -> Reply {
    app.devices.revoke(token(&headers).unwrap_or_default())?;
    Ok(Json(json!({"revoked":true})))
}
async fn repositories(State(app): State<App>) -> Reply {
    blocking(move || mobile::workspaces(&*app.agent, true)).await
}
async fn workspaces(State(app): State<App>) -> Reply {
    blocking(move || mobile::workspaces(&*app.agent, false)).await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Workspace {
    workspace: String,
    worktree: String,
    branch: String,
    base: String,
}
async fn create_workspace(State(app): State<App>, Json(new): Json<Workspace>) -> Reply {
    blocking(move || {
        app.agent.request(
            ReadWrite::CreateWorkspace,
            json!({"workspace":new.workspace,"worktree":new.worktree,
        "branch":new.branch,"base":new.base}),
        )
    })
    .await
}
async fn threads(State(app): State<App>, Path(id): Path<String>) -> Reply {
    blocking(move || mobile::threads(&*app.agent, &id)).await
}
async fn create_thread(
    State(app): State<App>,
    Path(id): Path<String>,
    Json(new): Json<NewThread>,
) -> Reply {
    blocking(move || mobile::create_thread(&*app.agent, &id, new)).await
}
async fn tasks(State(app): State<App>, Path(id): Path<String>) -> Reply {
    blocking(move || {
        app.agent.request(
            ReadWrite::Issues,
            json!({"workspace":id,"state":"open","limit":100}),
        )
    })
    .await
}
async fn context(State(app): State<App>, Path(id): Path<String>) -> Reply {
    blocking(move || mobile::context(&*app.agent, &id)).await
}
#[derive(Default, Deserialize)]
struct Cursor {
    #[serde(default)]
    after: u64,
}
async fn history(State(app): State<App>, Path(id): Path<String>, Query(c): Query<Cursor>) -> Reply {
    blocking(move || {
        app.agent.request(
            ReadWrite::History,
            json!({"thread":id,"after_seq":c.after,"limit":200}),
        )
    })
    .await
}
async fn transcript(State(app): State<App>, Path(id): Path<String>) -> Reply {
    blocking(move || {
        app.agent
            .request(ReadWrite::Transcript, json!({"thread":id}))
    })
    .await
}
#[derive(Deserialize)]
struct Prompt {
    prompt: String,
}
async fn turn(
    State(app): State<App>,
    Path((thread, id)): Path<(String, String)>,
    Json(p): Json<Prompt>,
) -> Reply {
    blocking(move || mobile::submit(&*app.agent, &thread, &id, &p.prompt)).await
}
#[derive(Deserialize)]
struct Decision {
    decision: String,
}
async fn approve(
    State(app): State<App>,
    Path((thread, id)): Path<(String, String)>,
    Json(p): Json<Decision>,
) -> Reply {
    blocking(move || {
        app.agent.request(
            ReadWrite::Approval,
            json!({"thread":thread,"approval":id,"decision":p.decision}),
        )
    })
    .await
}
#[derive(Deserialize)]
struct Answer {
    answer: String,
}
async fn answer(
    State(app): State<App>,
    Path((thread, id)): Path<(String, String)>,
    Json(p): Json<Answer>,
) -> Reply {
    blocking(move || {
        app.agent.request(
            ReadWrite::Input,
            json!({"thread":thread,"request":id,"answer":p.answer}),
        )
    })
    .await
}
async fn events(
    State(app): State<App>,
    Path(thread): Path<String>,
    Query(c): Query<Cursor>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    ws.max_message_size(8192)
        .on_upgrade(move |socket| stream(socket, app, thread, c.after))
}
#[derive(Deserialize)]
struct Identification {
    token: String,
}
async fn stream(mut socket: WebSocket, app: App, thread: String, after: u64) {
    let Ok(Some(Ok(Message::Text(frame)))) =
        tokio::time::timeout(Duration::from_secs(5), socket.recv()).await
    else {
        return;
    };
    let Ok(ident) = serde_json::from_str::<Identification>(&frame) else {
        return;
    };
    if !app.devices.authenticated(&ident.token).unwrap_or(false) {
        return;
    }
    let (tx, mut rx) = mpsc::channel(32);
    tokio::task::spawn_blocking(move || {
        if let Err(e) = app.agent.follow(&thread, after, tx.clone()) {
            let _ = tx.blocking_send(Err(e));
        }
    });
    let mut tick = tokio::time::interval(Duration::from_secs(2));
    loop {
        tokio::select! {
            _ = tick.tick() => if !app.devices.authenticated(&ident.token).unwrap_or(false) { break },
            inbound = socket.recv() => if !matches!(inbound,Some(Ok(Message::Pong(_) | Message::Ping(_)))) { break },
            next = rx.recv() => {
                let Some(Ok(event)) = next else { break };
                if !app.devices.authenticated(&ident.token).unwrap_or(false) { break; }
                if socket.send(Message::Text(event.to_string().into())).await.is_err() { break; }
            }
        }
    }
}
