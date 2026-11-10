use serde_json::Value;

/// Whether LeetCode's `globalData { userStatus { isSignedIn } }` reply says the person is signed in.
pub fn is_signed_in(reply: &Value) -> bool {
    reply.pointer("/data/userStatus/isSignedIn").and_then(Value::as_bool).unwrap_or(false)
}
