use crate::ports::Refusal;
use crate::service::Attachments;
use agent_api_attachments::{self as api, GetParams, IdParams, PutParams, StageParams};
use agent_api_proto::{code, GroupName, Response};
use async_trait::async_trait;
use rca_agent_core::{Call, Group, Served};
use serde::Serialize;
use serde_json::Value;
use std::sync::Arc;

pub const SERVED: [Served; 4] = [Served::service(api::UPLOAD_PUT, &[]), Served::service(api::GET, &[]), Served::service(api::DELETE, &[]), Served::service(api::STAGE, &[exec_fs::PUT_BLOB])];

pub struct AttachmentsGroup {
    attachments: Arc<Attachments>,
}

impl AttachmentsGroup {
    pub fn new(attachments: Arc<Attachments>) -> AttachmentsGroup {
        AttachmentsGroup { attachments }
    }
}

fn invalid(e: agent_api_proto::ParamError) -> Refusal {
    Refusal::new(code::INVALID_PARAMS, e.0)
}

fn value<T: Serialize>(result: Result<T, Refusal>) -> Result<Value, Refusal> {
    result.map(|r| serde_json::to_value(r).expect("a result serializes"))
}

#[async_trait]
impl Group for AttachmentsGroup {
    fn name(&self) -> GroupName {
        api::GROUP
    }

    fn methods(&self) -> &[Served] {
        &SERVED
    }

    async fn handle(&self, call: Call<'_>) -> Response {
        let params = call.params;
        let answer = match call.method.name() {
            "upload.put" => match PutParams::parse(params) {
                Ok(p) => value(self.attachments.put(p)),
                Err(e) => Err(invalid(e)),
            },
            "attachment.get" => match GetParams::parse(params) {
                Ok(p) => value(self.attachments.get(p)),
                Err(e) => Err(invalid(e)),
            },
            "attachment.delete" => match IdParams::parse(params) {
                Ok(p) => value(self.attachments.delete(p)),
                Err(e) => Err(invalid(e)),
            },
            "attachment.stage" => match StageParams::parse(params) {
                Ok(p) => value(self.attachments.stage(p).await),
                Err(e) => Err(invalid(e)),
            },
            other => unreachable!("{other} is no method of attachments (SERVED)"),
        };
        match answer {
            Ok(result) => Response::result(call.id, result),
            Err(Refusal { code, message }) => Response::error(call.id, &code, message),
        }
    }
}
