//! `POST /services/<app>/redeploy` and `POST /services/<app>/retire`: thin handlers over `application::redeploy`, whose refusals each
//! have their own reply here. A sibling that ran is 200 when it exited 0 and 502 otherwise, with the same body.
use crate::api::reply::Reply;
use crate::api::routes::Call;
use crate::api::AppState;
use serde_json::json;
use vm_ops_api_core::application::redeploy::{AppDone, AppRequest, PairOutcome, Refused, RetireDone};
use vm_ops_api_core::domain::apps::KNOWN_APPS;
use vm_ops_api_core::domain::js::JsString;

const HINT: &str = "HOST_LABEL is unrecognized (this VM's .env should set it to main), or the app is not in main.tf's local.vm_apps. Add it to local.vm_apps in main.tf and to HOST_ALLOWED_APPS in src/domain/apps.ts together.";
const RECREATE: &str = "invalid recreate mode -- accepted value: 'all'";

fn refused(refused: Refused, app: &str) -> Reply {
    let error = |status: u16, message: &str| Reply::json(status, &json!({ "error": message }));
    match refused {
        Refused::UnknownApp => Reply::unknown_app(&KNOWN_APPS),
        Refused::NotOnThisHost { host, allowed } => {
            Reply::json(409, &json!({ "error": "app not declared for this host", "host": host, "app": app, "allowedApps": allowed, "hint": HINT }))
        }
        Refused::InvalidTag => error(400, "invalid tag format"),
        Refused::InvalidRef => error(400, "invalid ref format"),
        Refused::InvalidRecreate => error(400, RECREATE),
        Refused::NotARetiredApp => Reply::json(400, &json!({ "error": "not a retired app", "app": app })),
        Refused::NotAPairableApp => Reply::json(400, &json!({ "error": "not a pairable app", "app": app })),
        Refused::InvalidEnvBody(rule) => error(400, rule.rule()),
    }
}

/// `null` for the exit code of a sibling that a signal ended.
fn exit_code_json(code: Option<i32>) -> String {
    code.map_or_else(|| "null".to_string(), |code| code.to_string())
}

fn text(value: &str) -> String {
    JsString::from(value).to_json()
}

fn app_reply(done: &AppDone) -> Reply {
    let body = format!(
        r#"{{"app":{},"tag":{},"ref":{},"wipeState":{},"exitCode":{},"serverScriptSha":{},"stdout":{},"stderr":{}}}"#,
        text(&done.app),
        text(&done.tag),
        text(&done.reference),
        done.wipe_state,
        exit_code_json(done.exit_code),
        text(&done.server_script_sha),
        done.stdout.to_json(),
        done.stderr.to_json(),
    );
    Reply::json_text(if done.succeeded() { 200 } else { 502 }, body)
}

fn retire_reply(done: &RetireDone) -> Reply {
    let body = format!(r#"{{"app":{},"exitCode":{},"stdout":{},"stderr":{}}}"#, text(&done.app), exit_code_json(done.exit_code), done.stdout.to_json(), done.stderr.to_json());
    Reply::json_text(if done.succeeded() { 200 } else { 502 }, body)
}

pub async fn app(call: &Call<'_>, state: &AppState) -> Reply {
    let target = call.target;
    let app = target.parts[1].as_str();
    let [tag, reference, recreate, wipe_state, requested_by] = ["tag", "ref", "recreate", "wipeState", "requestedBy"].map(|name| target.query_param(name));
    let request = AppRequest { app, tag: tag.as_deref(), reference: reference.as_deref(), recreate: recreate.as_deref(), wipe_state: wipe_state.as_deref(), requested_by: requested_by.as_deref(), env_body: call.body };
    match state.redeploy.redeploy_app(&request).await {
        Ok(done) => app_reply(&done),
        Err(why) => refused(why, app),
    }
}

/// `POST /services/<app>/smoke`: the app's pulled image runs as a throwaway container on the spare port and is left running when it
/// passes. 200 with the redeploy's own body when it passed, 502 with the container's log when it did not.
pub async fn smoke(call: &Call<'_>, state: &AppState) -> Reply {
    let target = call.target;
    let app = target.parts[1].as_str();
    let [tag, reference] = ["tag", "ref"].map(|name| target.query_param(name));
    let request = AppRequest { app, tag: tag.as_deref(), reference: reference.as_deref(), ..AppRequest::default() };
    match state.redeploy.smoke_app(&request).await {
        Ok(done) => app_reply(&done),
        Err(why) => refused(why, app),
    }
}

/// `POST /services/<app>/smoke/stop`: removes the throwaway container.
pub async fn smoke_stop(call: &Call<'_>, state: &AppState) -> Reply {
    let app = call.target.parts[1].as_str();
    match state.redeploy.smoke_stop(app).await {
        Ok(done) => retire_reply(&done),
        Err(why) => refused(why, app),
    }
}

pub async fn retire(call: &Call<'_>, state: &AppState) -> Reply {
    let app = call.target.parts[1].as_str();
    match state.redeploy.retire(app).await {
        Ok(done) => retire_reply(&done),
        Err(why) => refused(why, app),
    }
}

/// `POST /services/orca-serve/pair`: 200 `{"ok":true,"url":<link>}`, the only place the link is written; 502 `{"ok":false,"error":...}` with
/// nothing of the sibling's output when it failed or printed no link.
pub async fn pair(call: &Call<'_>, state: &AppState) -> Reply {
    let app = call.target.parts[1].as_str();
    match state.redeploy.pair(app).await {
        Ok(outcome) => pair_reply(&outcome),
        Err(why) => refused(why, app),
    }
}

fn pair_reply(outcome: &PairOutcome) -> Reply {
    match outcome {
        PairOutcome::Minted(url) => Reply::json(200, &json!({ "ok": true, "url": url })),
        PairOutcome::Failed => Reply::json(502, &json!({ "ok": false, "error": "orca-serve pair failed or printed no pairing link" })),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn done(code: Option<i32>, stdout: JsString) -> AppDone {
        AppDone {
            app: "leetcode-backend".into(),
            tag: "latest".into(),
            reference: "dev".into(),
            wipe_state: false,
            exit_code: code,
            server_script_sha: "0123456789ab".into(),
            stdout,
            stderr: JsString::from(""),
        }
    }

    #[test]
    fn given_a_redeploy_that_ran_when_replied_then_the_keys_are_in_order_and_a_lone_surrogate_survives() {
        let cut = JsString::from("\u{1f600}").last(1);
        let plain = app_reply(&done(Some(0), JsString::from("ok")));
        let failed = app_reply(&done(None, cut));

        let body = |reply: &Reply| String::from_utf8_lossy(&reply.body).to_string();
        assert_eq!((plain.status.as_u16(), body(&plain)), (200, r#"{"app":"leetcode-backend","tag":"latest","ref":"dev","wipeState":false,"exitCode":0,"serverScriptSha":"0123456789ab","stdout":"ok","stderr":""}"#.to_string()));
        assert_eq!((failed.status.as_u16(), body(&failed)), (502, r#"{"app":"leetcode-backend","tag":"latest","ref":"dev","wipeState":false,"exitCode":null,"serverScriptSha":"0123456789ab","stdout":"\ude00","stderr":""}"#.to_string()));
    }

    #[test]
    fn given_the_refusals_when_mapped_then_each_has_nodes_status_and_body() {
        let host = Refused::NotOnThisHost { host: "other".into(), allowed: Vec::new() };
        let got: Vec<(u16, String)> = [Refused::InvalidTag, Refused::InvalidRef, Refused::InvalidRecreate, Refused::NotARetiredApp, host, Refused::NotAPairableApp]
            .into_iter()
            .map(|why| {
                let reply = refused(why, "leetcode-backend");
                (reply.status.as_u16(), String::from_utf8_lossy(&reply.body).to_string())
            })
            .collect();

        assert_eq!(got[0], (400, r#"{"error":"invalid tag format"}"#.to_string()));
        assert_eq!(got[2], (400, r#"{"error":"invalid recreate mode -- accepted value: 'all'"}"#.to_string()));
        assert_eq!(got[3], (400, r#"{"error":"not a retired app","app":"leetcode-backend"}"#.to_string()));
        assert!(got[4].1.starts_with(r#"{"error":"app not declared for this host","host":"other","app":"leetcode-backend","allowedApps":[],"hint":"HOST_LABEL is unrecognized"#), "{}", got[4].1);
        assert_eq!(got[4].0, 409);
        assert_eq!(got[5], (400, r#"{"error":"not a pairable app","app":"leetcode-backend"}"#.to_string()));
    }
}
