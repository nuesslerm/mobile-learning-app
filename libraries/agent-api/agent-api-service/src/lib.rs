//! The `service` group of the agent API: `service.status`, which every mapper asks first. It names the protocol, the build and the groups
//! the service serves, so a mapper refuses at startup a service it cannot work with instead of failing on its first request.
use agent_api_proto::{GroupName, Method, Request};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub mod activity;
#[cfg(feature = "testing")]
pub mod testing;

pub const GROUP: GroupName = GroupName::new("service");

pub const STATUS: Method = Method::new(GROUP, "service.status");

pub use activity::CLIENT_ACTIVITY;

/// Every method of the crate: `service.status` of the `service` group and `service.clientActivity` of the `service.activity` sub-group.
pub const METHODS: &[Method] = &[STATUS, CLIENT_ACTIVITY];

/// The result of `service.status`: a group is listed only when every method of it is served.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StatusResult {
    pub protocol: u32,
    pub version: String,
    pub commit: String,
    pub groups: Vec<String>,
}

impl StatusResult {
    pub fn to_value(&self) -> Value {
        json!({ "protocol": self.protocol, "version": self.version, "commit": self.commit, "groups": self.groups })
    }
}

pub fn status_request(id: impl Into<Value>) -> Request {
    Request::new(id, STATUS, json!({}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_a_status_when_encoded_and_read_back_then_it_is_the_same_and_a_result_without_a_build_is_refused() {
        // Given a status of a service that serves two groups
        let status = StatusResult { protocol: 1, version: "1.2.3".into(), commit: "abc".into(), groups: vec!["service".into(), "state".into()] };
        // When it is encoded and read back, Then it is unchanged, with the fields in the order of the contract
        assert_eq!(serde_json::from_value::<StatusResult>(status.to_value()).unwrap(), status);
        assert_eq!(status.to_value().to_string(), r#"{"protocol":1,"version":"1.2.3","commit":"abc","groups":["service","state"]}"#);
        // And a result with no version or commit, or with no groups, is no status
        assert!(serde_json::from_value::<StatusResult>(json!({"protocol": 1, "groups": []})).is_err());
        assert!(serde_json::from_value::<StatusResult>(json!({"protocol": 1, "version": "v", "commit": "c"})).is_err());
    }

    #[test]
    fn given_the_status_request_when_encoded_then_it_names_the_method_of_the_group() {
        // Given the request a mapper sends first
        let request = status_request(1);
        // Then it asks for service.status and the method belongs to the service group
        assert_eq!((request.method.as_str(), STATUS.group()), (STATUS.name(), GROUP));
        assert_eq!(METHODS, [STATUS, CLIENT_ACTIVITY]);
        assert_eq!(CLIENT_ACTIVITY.group(), activity::GROUP);
    }
}
