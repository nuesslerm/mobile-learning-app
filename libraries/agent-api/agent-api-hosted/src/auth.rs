//! `hosted.auth`: `sourceControl.*`, what the executor's machine is signed in to and the CLI's own sign-in. The service never sees a token: a
//! status is the provider, host, account and scopes; a sign-in is the one-time code and the address the person opens, then a wait.
use super::{spec, Spec, GROUP_AUTH};
use agent_api_proto::Method;

pub const DISCOVER: Method = Method::new(GROUP_AUTH, "sourceControl.discover");
pub const APP_ACCESS: Method = Method::new(GROUP_AUTH, "sourceControl.appAccess");
pub const LOOKUP_REPOSITORY: Method = Method::new(GROUP_AUTH, "sourceControl.lookupRepository");
pub const PUBLISH_REPOSITORY: Method = Method::new(GROUP_AUTH, "sourceControl.publishRepository");
pub const SIGN_IN_START: Method = Method::new(GROUP_AUTH, "sourceControl.signInStart");
pub const SIGN_IN_WAIT: Method = Method::new(GROUP_AUTH, "sourceControl.signInWait");
pub const SIGN_IN_CANCEL: Method = Method::new(GROUP_AUTH, "sourceControl.signInCancel");

pub const METHODS: &[Method] = &[DISCOVER, APP_ACCESS, LOOKUP_REPOSITORY, PUBLISH_REPOSITORY, SIGN_IN_START, SIGN_IN_WAIT, SIGN_IN_CANCEL];

/// `discover` asks the status of each provider (a request names the one it wants with `provider`, else the service asks for both), and
/// `appAccess` the status and then the repository's permission for the signed-in account.
pub const SPECS: &[Spec] = &[
    spec(DISCOVER, &[exec_hosted::AUTH_STATUS], None),
    spec(APP_ACCESS, &[exec_hosted::AUTH_STATUS, exec_hosted::REPO_VIEW], None),
    spec(LOOKUP_REPOSITORY, &[exec_hosted::REPO_VIEW], None),
    spec(PUBLISH_REPOSITORY, &[exec_hosted::REPO_PUBLISH], None),
    spec(SIGN_IN_START, &[exec_hosted::AUTH_LOGIN_START], None),
    spec(SIGN_IN_WAIT, &[exec_hosted::AUTH_LOGIN_WAIT], None),
    spec(SIGN_IN_CANCEL, &[exec_hosted::AUTH_LOGIN_CANCEL], None),
];
