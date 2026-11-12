//! `POST /clis/install?sha256=`: installs the `wf` build CI published, which a runner that cannot write to the VM's disk (a GitHub-hosted one)
//! could not do itself. The sibling fetches it from the fixed release and installs it only when it is the build the caller names.
use crate::application::ports::Siblings;
use crate::domain::clis_install::{sibling_args, valid_sha256};
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refused {
    /// No `sha256`, or not 64 lowercase hex digits: nothing was started.
    BadSha,
    /// The sibling ran and failed, or could not run; the message is its last stderr line.
    Failed(String),
}

pub struct ClisInstall {
    siblings: Arc<dyn Siblings>,
    image: String,
}

impl ClisInstall {
    /// `image` is the image this service runs (its siblings run it too).
    pub fn new(siblings: Arc<dyn Siblings>, image: String) -> Self {
        Self { siblings, image }
    }

    /// The tree hash of the installed build.
    pub async fn install(&self, sha256: Option<&str>) -> Result<String, Refused> {
        let sha256 = sha256.filter(|sha| valid_sha256(sha)).ok_or(Refused::BadSha)?;
        let result = self.siblings.run(&sibling_args(&self.image, sha256)).await;
        if result.code != Some(0) {
            let why = result.stderr.lines().rev().find(|line| !line.trim().is_empty()).unwrap_or("the install sibling failed");
            return Err(Refused::Failed(why.trim().to_string()));
        }
        let tree = result.stdout.lines().rev().find(|line| !line.trim().is_empty()).unwrap_or_default().trim().to_string();
        if tree.is_empty() {
            return Err(Refused::Failed("the install sibling printed no tree hash".into()));
        }
        Ok(tree)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::ports::SiblingResult;
    use crate::testing::{FakeSiblings, SiblingCall};

    const SHA: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    fn install(siblings: &Arc<FakeSiblings>) -> ClisInstall {
        ClisInstall::new(siblings.clone(), "ns/vm-ops-api-rs:sha-1".into())
    }

    #[tokio::test]
    async fn given_a_sha_when_the_sibling_exits_0_then_its_last_line_is_the_tree_hash() {
        let siblings = Arc::new(FakeSiblings::new().with_run("", vec![SiblingResult::exited(0, "noise\ntree1234\n", "")]));

        let tree = install(&siblings).install(Some(SHA)).await;

        assert_eq!(tree, Ok("tree1234".to_string()));
        assert_eq!(siblings.calls().len(), 1);
        assert!(matches!(&siblings.calls()[0], SiblingCall::Run(args) if args.iter().any(|a| a.contains(SHA))));
    }

    #[tokio::test]
    async fn given_no_sha_or_a_bad_one_when_installed_then_nothing_runs() {
        let siblings = Arc::new(FakeSiblings::new());
        for sha in [None, Some(""), Some("abc"), Some("0123456789ABCDEF0123456789abcdef0123456789abcdef0123456789abcdef")] {
            assert_eq!(install(&siblings).install(sha).await, Err(Refused::BadSha), "{sha:?}");
        }
        assert_eq!(siblings.calls(), []);
    }

    #[tokio::test]
    async fn given_a_sibling_that_fails_when_installed_then_the_refusal_is_its_last_stderr_line() {
        let siblings = Arc::new(FakeSiblings::new().with_run("", vec![SiblingResult::exited(1, "", "x\nthe release asset is not the build the caller named (sha256 differs)\n")]));

        let tree = install(&siblings).install(Some(SHA)).await;

        assert_eq!(tree, Err(Refused::Failed("the release asset is not the build the caller named (sha256 differs)".into())));
        let none = Arc::new(FakeSiblings::new());
        assert!(matches!(install(&none).install(Some(SHA)).await, Err(Refused::Failed(_))));
    }
}
