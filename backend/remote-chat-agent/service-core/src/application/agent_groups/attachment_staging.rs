//! `turn.start`'s attachments, placed by the attachments group: the agent group's port over the group's `attachment.stage`.
use agent_api_attachments::StageParams;
use async_trait::async_trait;
use rca_group_agent::{refusal::Refusal, AttachmentStaging};
use rca_group_attachments::Attachments;
use std::sync::Arc;

pub struct StagesAttachments(Arc<Attachments>);

impl StagesAttachments {
    pub fn new(attachments: Arc<Attachments>) -> StagesAttachments {
        StagesAttachments(attachments)
    }
}

#[async_trait]
impl AttachmentStaging for StagesAttachments {
    async fn stage(&self, workspace: &str, ids: &[String]) -> Result<Vec<String>, Refusal> {
        let staged = self.0.stage(StageParams { workspace: workspace.to_string(), ids: ids.to_vec() }).await.map_err(|r| Refusal::new(r.code, r.message))?;
        Ok(staged.files.into_iter().map(|f| f.path).collect())
    }
}
