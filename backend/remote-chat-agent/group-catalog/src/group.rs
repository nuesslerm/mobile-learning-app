use crate::ports::Refusal;
use crate::service::Catalog;
use agent_api_catalog::{self as api, CommandsParams, DetectParams, ModelsParams, SkillsParams};
use agent_api_proto::{code, GroupName, Response};
use async_trait::async_trait;
use rca_agent_core::{Call, Group, Served};
use serde::Serialize;
use serde_json::Value;
use std::sync::Arc;

pub const SERVED: [Served; 4] = [Served::service(api::MODELS_LIST, &[]), Served::service(api::AGENTS_DETECT, &[exec_proc::CLI_DETECT]), Served::service(api::SKILLS_LIST, &[exec_fs::SKILL_SCAN]), Served::service(api::COMMANDS_LIST, &[exec_fs::SKILL_SCAN])];

pub struct CatalogGroup {
    catalog: Arc<Catalog>,
}

impl CatalogGroup {
    pub fn new(catalog: Arc<Catalog>) -> CatalogGroup {
        CatalogGroup { catalog }
    }
}

fn invalid(e: agent_api_proto::ParamError) -> Refusal {
    Refusal::new(code::INVALID_PARAMS, e.0)
}

fn value<T: Serialize>(result: Result<T, Refusal>) -> Result<Value, Refusal> {
    result.map(|r| serde_json::to_value(r).expect("a result serializes"))
}

#[async_trait]
impl Group for CatalogGroup {
    fn name(&self) -> GroupName {
        api::GROUP
    }

    fn methods(&self) -> &[Served] {
        &SERVED
    }

    async fn handle(&self, call: Call<'_>) -> Response {
        let params = call.params;
        let answer = match call.method.name() {
            "models.list" => match ModelsParams::parse(params) {
                Ok(p) => value(self.catalog.models(p)),
                Err(e) => Err(invalid(e)),
            },
            "agents.detect" => match DetectParams::parse(params) {
                Ok(p) => value(self.catalog.detect(p).await),
                Err(e) => Err(invalid(e)),
            },
            "skills.list" => match SkillsParams::parse(params) {
                Ok(p) => value(self.catalog.skills(p).await),
                Err(e) => Err(invalid(e)),
            },
            "commands.list" => match CommandsParams::parse(params) {
                Ok(p) => value(self.catalog.commands(p).await),
                Err(e) => Err(invalid(e)),
            },
            other => unreachable!("{other} is no method of catalog (SERVED)"),
        };
        match answer {
            Ok(result) => Response::result(call.id, result),
            Err(Refusal { code, message }) => Response::error(call.id, &code, message),
        }
    }
}
