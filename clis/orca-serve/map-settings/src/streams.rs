//! The rows that end a stream this host never opens (accounts, notifications and terminal have no `subscribe` here): the
//! phone's stream is not one of this connection's, and ending it answers that it is ended, as Orca does for an id it holds nothing under.
use orca_core::wire::Failure;
use serde_json::{json, Value};

pub const ACCOUNTS_UNSUBSCRIBE: &str = "accounts.unsubscribe";
pub const NOTIFICATIONS_UNSUBSCRIBE: &str = "notifications.unsubscribe";
pub const TERMINAL_UNSUBSCRIBE: &str = "terminal.unsubscribe";

pub(crate) fn call(method: &str, params: &Value) -> Option<Result<Value, Failure>> {
    let ended = || Ok(json!({"unsubscribed": true}));
    let named = |ok: bool, message: &str| if ok { ended() } else { Err(Failure::invalid(message)) };
    let id = params.get("subscriptionId").and_then(Value::as_str).filter(|s| !s.is_empty());
    Some(match method {
        ACCOUNTS_UNSUBSCRIBE | NOTIFICATIONS_UNSUBSCRIBE => named(id.is_some(), "Missing subscriptionId"),
        TERMINAL_UNSUBSCRIBE => named(id.is_some(), "Missing subscription ID"),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn refused(method: &str, params: Value) -> String {
        call(method, &params).unwrap().unwrap_err().message
    }

    #[test]
    fn given_a_stream_this_host_never_opened_when_the_phone_ends_it_then_it_is_ended_and_a_bad_id_is_refused_in_the_words_of_the_method() {
        // Given the rows that end a stream, When each is given an id, Then it answers ended
        for method in [ACCOUNTS_UNSUBSCRIBE, NOTIFICATIONS_UNSUBSCRIBE, TERMINAL_UNSUBSCRIBE] {
            assert_eq!(call(method, &json!({"subscriptionId": "a"})).unwrap().unwrap(), json!({"unsubscribed": true}), "{method}");
        }
        // When the id is missing or not a string, Then each refuses as the host does: each by name
        assert_eq!(refused(ACCOUNTS_UNSUBSCRIBE, json!({"subscriptionId": 7})), "Missing subscriptionId");
        assert_eq!(refused(TERMINAL_UNSUBSCRIBE, Value::Null), "Missing subscription ID");
    }

    #[test]
    fn a_method_that_is_not_a_stream_ending_is_left_to_the_area() {
        assert!(call("accounts.list", &json!({})).is_none());
    }
}
