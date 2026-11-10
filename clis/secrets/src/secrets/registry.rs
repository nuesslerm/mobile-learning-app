//! The secrets registry, embedded from the single source of truth in clis/secrets.
use serde_json::Value;

const RAW: &str = include_str!("../data/registry.json");

/// Which vault project may hold a key: `ci-open` and `ci-deploy` are the two CI tokens' projects, `vm` is for keys no CI job may
/// read, `public` is a repo variable and never a vault key (docs/plans/ci-scoped-vault-access/plan.md).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Scope {
    CiOpen,
    CiDeploy,
    Vm,
    Public,
}

impl Scope {
    pub fn parse(s: &str) -> Option<Scope> {
        match s {
            "ci-open" => Some(Scope::CiOpen),
            "ci-deploy" => Some(Scope::CiDeploy),
            "vm" => Some(Scope::Vm),
            "public" => Some(Scope::Public),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Scope::CiOpen => "ci-open",
            Scope::CiDeploy => "ci-deploy",
            Scope::Vm => "vm",
            Scope::Public => "public",
        }
    }
}

#[derive(Clone)]
pub struct Entry {
    pub key: String,
    pub service: String,
    pub env_var: String,
    pub bitwarden_key: Option<String>,
    pub vault: Option<String>,
    pub bootstrap: bool,
    pub scope: Scope,
    pub description: String,
    /// How `wf secret verify` judges a value is usable: a `shape` and/or a `live` read-only call (see `verify.rs`).
    pub check: Option<Value>,
}

fn non_empty(
    obj: &serde_json::Map<String, Value>,
    key: &str,
    entry: &str,
) -> Result<String, String> {
    match obj.get(key) {
        Some(Value::String(s)) if !s.is_empty() => Ok(s.clone()),
        _ => Err(format!(
            "registry entry {entry}: {key} must be a non-empty string"
        )),
    }
}

pub fn load() -> Result<Vec<Entry>, String> {
    parse(RAW)
}

fn parse(raw: &str) -> Result<Vec<Entry>, String> {
    let root: Value = serde_json::from_str(raw).map_err(|e| format!("registry.json: {e}"))?;
    let map = root
        .as_object()
        .ok_or("registry.json: expected an object")?;
    map.iter()
        .map(|(key, v)| {
            let o = v
                .as_object()
                .ok_or_else(|| format!("registry entry {key}: expected an object"))?;
            let bitwarden_key = match o.get("bitwardenKey") {
                None => None,
                Some(_) => Some(non_empty(o, "bitwardenKey", key)?),
            };
            let vault = match o.get("vault").and_then(Value::as_str) {
                None => None,
                Some(s @ ("deploy" | "gh-ci" | "admin")) => Some(s.to_string()),
                Some(other) => {
                    return Err(format!("registry entry {key}: invalid vault \"{other}\""))
                }
            };
            let scope = match o.get("scope").and_then(Value::as_str) {
                None => return Err(format!("registry entry {key}: scope is required (ci-open, ci-deploy, vm or public)")),
                Some(s) => Scope::parse(s).ok_or_else(|| format!("registry entry {key}: invalid scope \"{s}\" (ci-open, ci-deploy, vm or public)"))?,
            };
            let description = non_empty(o, "description", key)?;
            let check = o.get("check").cloned();
            if check.as_ref().is_some_and(|c| !c.is_object()) {
                return Err(format!("registry entry {key}: check must be an object"));
            }
            let uncheckable = match o.get("uncheckable") {
                None => None,
                Some(_) => Some(non_empty(o, "uncheckable", key)?),
            };
            if check.is_some() && uncheckable.is_some() {
                return Err(format!("registry entry {key}: has both a check and an uncheckable reason"));
            }
            Ok(Entry {
                key: key.clone(),
                service: non_empty(o, "service", key)?,
                env_var: non_empty(o, "envVar", key)?,
                bitwarden_key,
                vault,
                bootstrap: o.get("bootstrap").and_then(Value::as_bool).unwrap_or(false),
                scope,
                description,
                check,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(scope: Option<&str>) -> String {
        let scope = scope.map_or(String::new(), |s| format!(r#""scope": "{s}","#));
        format!(r#"{{ "SOME_KEY": {{ "service": "some-key", "envVar": "SOME_KEY", {scope} "description": "WHAT: a test key" }} }}"#)
    }

    #[test]
    fn the_embedded_registry_gives_every_key_a_scope() {
        // Given the registry compiled into wf
        // When it is loaded
        let entries = load().unwrap();

        // Then every key carries one of the four scopes
        assert!(!entries.is_empty());
        for e in &entries {
            assert!(Scope::parse(e.scope.name()) == Some(e.scope), "{}", e.key);
        }
    }

    #[test]
    fn a_key_without_a_scope_is_rejected_naming_the_key() {
        // Given a registry entry with no scope
        // When it is parsed
        let error = parse(&entry(None)).err().unwrap();

        // Then the error names the key and says a scope is required
        assert!(error.contains("SOME_KEY") && error.contains("scope is required"), "{error}");
    }

    #[test]
    fn a_key_with_an_unknown_scope_is_rejected_naming_the_scope() {
        // Given a registry entry whose scope is not one of the four
        // When it is parsed
        let error = parse(&entry(Some("ci-everything"))).err().unwrap();

        // Then the error names the key and the bad scope
        assert!(error.contains("SOME_KEY") && error.contains("invalid scope \"ci-everything\""), "{error}");
    }

    #[test]
    fn each_known_scope_is_accepted() {
        for scope in ["ci-open", "ci-deploy", "vm", "public"] {
            // Given an entry with a known scope, when parsed, then it loads with that scope
            assert_eq!(parse(&entry(Some(scope))).unwrap()[0].scope.name(), scope);
        }
    }
}
