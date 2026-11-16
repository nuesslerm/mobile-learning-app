//! The request payloads of the RPCs this server answers without a handler of its own, decoded the way the pinned server's Effect schema
//! refuses them: the first field in declaration order that is missing or of the wrong type is a defect, never a typed error.
use serde_json::Value;

/// Fields in the order the contract's struct declares them: `name:kind`, kind `s` string, `n` number, `b` boolean, a trailing `?` for an
/// optional key.
const SCHEMAS: &[(&str, &str)] = &[
    ("provider.install.start", "instanceId:s"),
    ("provider.install.cancel", "instanceId:s operationId:s"),
    ("provider.install.subscribe", "instanceId:s"),
    ("provider.install.remove", "instanceId:s"),
    ("server.searchAcpRegistry", "query:s limit:n?"),
    ("server.prepareAcpRegistryAgent", "agentId:s"),
    ("server.uninstallAcpRegistryManagedBinary", "agentId:s"),
    ("server.acceptAcpRegistryUrlAuth", "instanceId:s elicitationId:s"),
    ("server.listAcpRegistrySessions", "instanceId:s projectId:s cursor:s?"),
    ("server.importAcpRegistrySession", "instanceId:s projectId:s sessionId:s"),
    ("server.deleteAcpRegistrySession", "instanceId:s projectId:s sessionId:s"),
    ("server.listAcpRegistryProviders", "instanceId:s projectId:s"),
    ("server.setAcpRegistryProvider", "instanceId:s projectId:s providerId:s apiType:s baseUrl:s"),
    ("server.disableAcpRegistryProvider", "instanceId:s projectId:s providerId:s"),
    ("server.logoutAcpRegistry", "instanceId:s"),
];

fn expected(kind: &str) -> &'static str {
    match kind {
        "n" => "number",
        "b" => "boolean",
        _ => "string",
    }
}

fn is(kind: &str, value: &Value) -> bool {
    match kind {
        "n" => value.is_number(),
        "b" => value.is_boolean(),
        _ => value.is_string(),
    }
}

/// The defect text when `payload` does not decode as the schema of `tag`; `None` when it does or the tag has no schema here.
pub fn defect(tag: &str, payload: &Value) -> Option<String> {
    let (_, schema) = SCHEMAS.iter().find(|(t, _)| *t == tag)?;
    let body = payload.as_object()?;
    for field in schema.split(' ') {
        let (name, kind) = field.split_once(':')?;
        let (kind, optional) = kind.strip_suffix('?').map_or((kind, false), |k| (k, true));
        match body.get(name) {
            None if optional => {}
            None => return Some(format!("Missing key\n  at [\"{name}\"]")),
            Some(v) if !is(kind, v) => return Some(format!("Expected {}\n  at [\"{name}\"]", expected(kind))),
            Some(_) => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_first_field_that_does_not_decode_is_the_defect() {
        // Given a schema of two string fields
        let tag = "provider.install.cancel";
        // When the payload is empty, then the first key is missing; when it has the first and not the second, the second is
        assert_eq!(defect(tag, &json!({})).unwrap(), "Missing key\n  at [\"instanceId\"]");
        assert_eq!(defect(tag, &json!({"instanceId": "a"})).unwrap(), "Missing key\n  at [\"operationId\"]");
        // And a number where a string belongs names that field, and a whole payload decodes
        assert_eq!(defect(tag, &json!({"instanceId": 7, "operationId": "a"})).unwrap(), "Expected string\n  at [\"instanceId\"]");
        assert_eq!(defect(tag, &json!({"instanceId": "a", "operationId": "a"})), None);
    }

    #[test]
    fn a_tag_without_a_schema_here_is_left_to_its_handler() {
        assert_eq!(defect("server.probe", &json!({})), None);
    }
}
