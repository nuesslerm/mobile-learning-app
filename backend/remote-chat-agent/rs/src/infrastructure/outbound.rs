//! The HTTP client every outbound call of the service shares (the speech engines, the edit-stats sink, Telegram): redirects are
//! never followed (a credential must not be forwarded to another host), connecting gives up after 10 seconds and a whole exchange
//! after 60.

use remote_chat_agent_core::application::ports::{Answer, Outbound, Payload, Post, PostError};
use async_trait::async_trait;
use std::time::Duration;

pub struct Reqwest {
    client: reqwest::Client,
}

impl Default for Reqwest {
    fn default() -> Self {
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(60))
            .build()
            .expect("a client with a fixed configuration");
        Self { client }
    }
}

fn post_error(e: reqwest::Error) -> PostError {
    PostError { message: e.to_string(), without_url: e.without_url().to_string() }
}

#[async_trait]
impl Outbound for Reqwest {
    async fn post(&self, post: Post) -> Result<Answer, PostError> {
        let mut request = self.client.post(&post.url);
        for (name, value) in &post.headers {
            request = request.header(name, value);
        }
        if let Some((user, password)) = &post.basic_auth {
            request = request.basic_auth(user, Some(password));
        }
        request = match post.payload {
            Payload::Json(value) => request.json(&value),
            Payload::Form(parts) => {
                let mut form = reqwest::multipart::Form::new();
                for part in parts {
                    form = match part.file_name {
                        Some(file_name) => {
                            let mut file = reqwest::multipart::Part::bytes(part.bytes).file_name(file_name);
                            if let Some(mime) = &part.mime {
                                file = file.mime_str(mime).map_err(post_error)?;
                            }
                            form.part(part.name, file)
                        }
                        None => form.text(part.name, String::from_utf8_lossy(&part.bytes).into_owned()),
                    };
                }
                request.multipart(form)
            }
        };
        let response = request.send().await.map_err(post_error)?;
        let status = response.status().as_u16();
        let body = response.bytes().await.map_err(post_error)?.to_vec();
        Ok(Answer { status, body })
    }
}
