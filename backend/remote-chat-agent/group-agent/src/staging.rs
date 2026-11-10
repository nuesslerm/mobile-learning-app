//! The port through which `turn.start` gets the files a person attached into the workspace its CLI works in. The composition root implements it
//! over the attachments group; this crate does not depend on that group.
use crate::refusal::Refusal;
use async_trait::async_trait;

#[async_trait]
pub trait AttachmentStaging: Send + Sync {
    /// Writes the attachments `ids` into `workspace` and returns where each now is, relative to the workspace root, in the order of `ids`.
    async fn stage(&self, workspace: &str, ids: &[String]) -> Result<Vec<String>, Refusal>;
}
