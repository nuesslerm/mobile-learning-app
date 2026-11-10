use async_trait::async_trait;
use reqwest::header::CONTENT_TYPE;
use reqwest::redirect::Policy;
use reqwest::{Client, Method, Response};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::VecDeque;
use std::fmt::Write as _;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use vm_ops_api_core::application::ports::{Containers, ContainersError, EngineReply, ExecOptions, ExecResult, LogFollow};
use vm_ops_api_core::domain::apps;
use vm_ops_api_core::domain::container_stats::DockerStats;
use vm_ops_api_core::domain::containers::{self, DockerContainer};
use vm_ops_api_core::domain::docker_log::{self, Demuxer};

const LIST_BODY_UNITS: usize = 200;
const REFUSAL_BODY_UNITS: usize = 300;

/// The Docker Engine API over its Unix socket, on unversioned paths. No request has a time limit: an attached exec answers when
/// its command ends, and a follow when the container stops.
pub struct DockerEngine {
    client: Client,
    socket: PathBuf,
}

#[derive(Serialize)]
#[serde(rename_all = "PascalCase")]
struct ExecCreate<'a> {
    attach_stdout: bool,
    attach_stderr: bool,
    cmd: &'a [String],
    #[serde(skip_serializing_if = "Option::is_none")]
    env: Option<&'a [String]>,
}

#[derive(Serialize)]
#[serde(rename_all = "PascalCase")]
struct ExecStart {
    detach: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ExecCreated {
    id: Option<String>,
}

struct Reply {
    status: u16,
    body: Vec<u8>,
}

impl Reply {
    fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}

impl DockerEngine {
    pub fn new(socket: &Path) -> Result<Self, reqwest::Error> {
        let client = Client::builder().unix_socket(socket).redirect(Policy::none()).pool_max_idle_per_host(0).build()?;
        Ok(Self { client, socket: socket.to_path_buf() })
    }

    async fn send(&self, method: Method, path: &str, payload: Option<(&'static str, Vec<u8>)>) -> Result<Response, ContainersError> {
        let mut request = self.client.request(method, format!("http://localhost{path}"));
        if let Some((content_type, bytes)) = payload {
            request = request.header(CONTENT_TYPE, content_type).body(bytes);
        }
        request.send().await.map_err(|error| self.unreached(&error))
    }

    async fn buffered(&self, method: Method, path: &str, payload: Option<(&'static str, Vec<u8>)>) -> Result<Reply, ContainersError> {
        let response = self.send(method, path, payload).await?;
        let status = response.status().as_u16();
        let body = response.bytes().await.map_err(|_| aborted())?.to_vec();
        Ok(Reply { status, body })
    }

    async fn post_json(&self, path: &str, body: &impl Serialize) -> Result<Reply, ContainersError> {
        let bytes = serde_json::to_vec(body).map_err(|error| ContainersError::failed(&error.to_string()))?;
        self.buffered(Method::POST, path, Some(("application/json", bytes))).await
    }

    async fn list(&self, path: &str) -> Result<Vec<DockerContainer>, ContainersError> {
        let reply = self.buffered(Method::GET, path, None).await?;
        if reply.status != 200 {
            return Err(ContainersError::refused("docker API", reply.status, &reply.text(), LIST_BODY_UNITS));
        }
        parse(&reply.text())
    }

    /// Node's own words for a connection that never opened, and for one that closed before it answered.
    fn unreached(&self, error: &reqwest::Error) -> ContainersError {
        if error.is_builder() {
            return ContainersError::failed(&error.to_string());
        }
        if !error.is_connect() {
            return ContainersError::failed("socket hang up");
        }
        let code = match io_error(error).map(std::io::Error::kind) {
            Some(ErrorKind::NotFound) => "ENOENT",
            Some(ErrorKind::ConnectionRefused) => "ECONNREFUSED",
            Some(ErrorKind::PermissionDenied) => "EACCES",
            _ => return ContainersError::failed(&format!("connect failed {}", self.socket.display())),
        };
        ContainersError::failed(&format!("connect {code} {}", self.socket.display()))
    }
}

fn aborted() -> ContainersError {
    ContainersError::failed("aborted")
}

fn io_error<'a>(error: &'a (dyn std::error::Error + 'static)) -> Option<&'a std::io::Error> {
    let mut current = Some(error);
    while let Some(link) = current {
        if let Some(io) = link.downcast_ref::<std::io::Error>() {
            return Some(io);
        }
        current = link.source();
    }
    None
}

fn parse<T: DeserializeOwned>(text: &str) -> Result<T, ContainersError> {
    serde_json::from_str(text).map_err(|error| {
        let blank = text.trim_matches([' ', '\t', '\n', '\r']).is_empty();
        ContainersError::failed(&if blank { "Unexpected end of JSON input".to_string() } else { error.to_string() })
    })
}

fn exit_code(inspect: &Value) -> Option<i64> {
    match inspect.get("ExitCode")? {
        Value::Number(code) => code.as_i64().or_else(|| code.as_f64().filter(|whole| whole.fract() == 0.0 && whole.abs() < 9e15).map(|whole| whole as i64)),
        _ => None,
    }
}

/// `encodeURIComponent`, which also escapes `'`: the `url` crate under reqwest writes it as `%27` in any query.
fn encode_uri_component(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b"-_.!~*()".contains(&byte) {
            out.push(char::from(byte));
        } else {
            let _ = write!(out, "%{byte:02X}");
        }
    }
    out
}

#[async_trait]
impl Containers for DockerEngine {
    async fn list_by_app(&self, app: &str) -> Result<Vec<DockerContainer>, ContainersError> {
        let listed = self.list("/containers/json?all=true").await?;
        Ok(containers::of_app(listed, apps::container_names(app).unwrap_or(&[])))
    }

    async fn list_running(&self) -> Result<Vec<DockerContainer>, ContainersError> {
        self.list("/containers/json").await
    }

    async fn exec(&self, container_id: &str, cmd: &[String], options: &ExecOptions) -> Result<ExecResult, ContainersError> {
        let attach = !options.detach;
        let create = ExecCreate { attach_stdout: attach, attach_stderr: attach, cmd, env: (!options.env.is_empty()).then_some(&options.env[..]) };
        let created = self.post_json(&format!("/containers/{container_id}/exec"), &create).await?;
        if created.status != 201 {
            return Err(ContainersError::refused("docker exec create", created.status, &created.text(), REFUSAL_BODY_UNITS));
        }
        let Some(exec_id) = parse::<ExecCreated>(&created.text())?.id else {
            return Err(ContainersError::failed("docker exec create answered no Id"));
        };
        let started = self.post_json(&format!("/exec/{exec_id}/start"), &ExecStart { detach: options.detach }).await?;
        if options.detach {
            return Ok(ExecResult::detached(exec_id));
        }
        let output = docker_log::demux(&started.body);
        let inspected = self.buffered(Method::GET, &format!("/exec/{exec_id}/json"), None).await?;
        Ok(ExecResult::attached(exit_code(&parse(&inspected.text())?), output))
    }

    async fn put_file(&self, container_id: &str, target_dir: &str, tar: &[u8]) -> Result<EngineReply, ContainersError> {
        let path = format!("/containers/{container_id}/archive?path={}", encode_uri_component(target_dir));
        let reply = self.buffered(Method::PUT, &path, Some(("application/x-tar", tar.to_vec()))).await?;
        Ok(EngineReply { status: reply.status, body: reply.text() })
    }

    async fn logs(&self, container_id: &str, tail: u32) -> Result<String, ContainersError> {
        let reply = self.buffered(Method::GET, &format!("/containers/{container_id}/logs?stdout=true&stderr=true&tail={tail}"), None).await?;
        if reply.status != 200 {
            return Err(ContainersError::refused("docker API", reply.status, &reply.text(), REFUSAL_BODY_UNITS));
        }
        Ok(docker_log::demux(&reply.body))
    }

    async fn follow_logs(&self, container_id: &str, tail: u32) -> Result<Box<dyn LogFollow>, ContainersError> {
        let path = format!("/containers/{container_id}/logs?follow=true&stdout=true&stderr=true&tail={tail}");
        let response = self.send(Method::GET, &path, None).await?;
        let status = response.status().as_u16();
        if status != 200 {
            let body = response.bytes().await.map_err(|_| aborted())?;
            return Err(ContainersError::refused("docker API", status, &String::from_utf8_lossy(&body), REFUSAL_BODY_UNITS));
        }
        Ok(Box::new(Follow { response, demuxer: Demuxer::new(), ready: VecDeque::new(), finished: false }))
    }

    async fn stats(&self, container_id: &str) -> Result<DockerStats, ContainersError> {
        let reply = self.buffered(Method::GET, &format!("/containers/{container_id}/stats?stream=false"), None).await?;
        if reply.status != 200 {
            return Err(ContainersError::refused("docker API", reply.status, &reply.text(), REFUSAL_BODY_UNITS));
        }
        parse(&reply.text())
    }
}

struct Follow {
    response: Response,
    demuxer: Demuxer,
    ready: VecDeque<String>,
    finished: bool,
}

#[async_trait]
impl LogFollow for Follow {
    async fn next_text(&mut self) -> Result<Option<String>, ContainersError> {
        loop {
            if let Some(text) = self.ready.pop_front() {
                return Ok(Some(text));
            }
            if self.finished {
                return Ok(None);
            }
            match self.response.chunk().await {
                Ok(Some(chunk)) => self.ready.extend(self.demuxer.feed(&chunk)),
                Ok(None) => self.finished = true,
                Err(_) => {
                    self.finished = true;
                    return Err(aborted());
                }
            }
        }
    }
}
