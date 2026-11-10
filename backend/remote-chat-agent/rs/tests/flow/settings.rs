//! The variables the service reads and what a missing, empty or set value does to it. Each test reads through the flow's environment map as
//! `main` reads the process's and observes behavior (the address a request goes to, the directory the providers live in), so a change of the reader
//! keeps the same names and defaults.

use crate::harness::Env;
use rca_group_accounts::{Completion, ModelApi, ModelError};
use rca_service::infrastructure::model_api::HttpModelApi;
use serde_json::json;
use std::sync::Arc;

async fn asked_with(vars: &[(&str, &str)]) -> (Result<String, String>, Vec<(String, Option<serde_json::Value>, Vec<(String, String)>)>) {
    let env = Env::new(vars);
    let outbound = crate::doubles::FakeOutbound::new();
    outbound.answer(|_| Ok((200, json!({"choices": [{"message": {"content": "ok"}}]}))));
    let api = HttpModelApi::from_settings(&env.source().settings(), Arc::new(crate::doubles::OutboundPort(outbound.clone())));
    let done = api.complete(Completion { system: "s".into(), prompt: "p".into(), max_tokens: 5 }).await;
    let answer = match done {
        Ok(done) => Ok(done.model),
        Err(ModelError::Unavailable(message)) => Err(message),
        Err(other) => Err(format!("{other:?}")),
    };
    (answer, outbound.posts().into_iter().map(|p| (p.url, p.json, p.headers)).collect())
}

#[tokio::test]
async fn given_no_model_variables_when_a_completion_is_asked_then_it_is_refused_for_the_missing_key_and_nothing_is_posted() {
    // Given no model variable (an empty key counts as none) When a completion is asked
    for vars in [&[][..], &[("KIMI_API_KEY", "")][..]] {
        let (answer, posts) = asked_with(vars).await;
        // Then it is refused naming the key and no request was made
        assert!(answer.as_ref().is_err_and(|m| m.contains("KIMI_API_KEY")), "{answer:?}");
        assert!(posts.is_empty());
    }
}

#[tokio::test]
async fn given_only_a_model_key_when_a_completion_is_asked_then_the_default_address_and_model_are_used() {
    // Given only the key When a completion is asked
    let (answer, posts) = asked_with(&[("KIMI_API_KEY", "key-value-1"), ("RCA_MODEL_API_BASE_URL", ""), ("RCA_MODEL_API_MODEL", "")]).await;
    // Then the request goes to the default address as that model with the key as a bearer
    assert!(answer.is_ok(), "{answer:?}");
    assert_eq!(posts.len(), 1);
    assert_eq!(posts[0].0, "https://api.kimi.com/coding/v1/chat/completions");
    assert_eq!(posts[0].1.as_ref().unwrap()["model"], json!("kimi-for-coding"));
    assert_eq!(posts[0].2, [("Authorization".to_string(), "Bearer key-value-1".to_string())]);
}

#[tokio::test]
async fn given_an_address_and_a_model_when_a_completion_is_asked_then_they_are_used_with_a_trailing_slash_dropped() {
    // Given the three variables When a completion is asked
    let (_, posts) = asked_with(&[("KIMI_API_KEY", "k"), ("RCA_MODEL_API_BASE_URL", "https://models.example.test/v9/"), ("RCA_MODEL_API_MODEL", "other-model")]).await;
    // Then the request goes there as that model
    assert_eq!(posts[0].0, "https://models.example.test/v9/chat/completions");
    assert_eq!(posts[0].1.as_ref().unwrap()["model"], json!("other-model"));
}

#[test]
fn given_the_providers_root_variable_when_the_root_is_asked_then_a_set_one_wins_and_an_empty_or_missing_one_is_the_service_volume() {
    // Given each state of the variable When the root is asked Then the answer is the value, else the volume
    let env = Env::new(&[]);
    let root = || env.source().settings().providers_root;
    assert_eq!(root(), "/data/providers");
    env.set("RCA_PROVIDERS_ROOT", "");
    assert_eq!(root(), "/data/providers");
    env.set("RCA_PROVIDERS_ROOT", "/srv/logins");
    assert_eq!(root(), "/srv/logins");
}
