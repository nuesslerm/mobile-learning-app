use serde::Deserialize;

/// A cookie as the browser daemon reports it; only the two fields the rule reads.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct Cookie {
    pub name: String,
    pub value: String,
}

/// The `document.cookie`-format string the backend's session route expects, built from `LEETCODE_SESSION` and `csrftoken`;
/// `None` when either is missing, so an incomplete session is never pushed.
pub fn session_cookie_header(cookies: &[Cookie]) -> Option<String> {
    let session = cookies.iter().find(|c| c.name == "LEETCODE_SESSION")?;
    let csrf = cookies.iter().find(|c| c.name == "csrftoken")?;
    Some(format!("LEETCODE_SESSION={}; csrftoken={}", session.value, csrf.value))
}
