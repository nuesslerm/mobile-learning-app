//! `wf secret verify --scope`: the registry's `scope` of a key says which CI token may read it, so a token's vault project must hold
//! every key of its scope and no key of another one. The vault is asked through the backend port; names only, never a value.
use super::registry::{Entry, Scope};
use serde_json::{json, Value};

pub fn ci_scope(name: &str) -> Option<Scope> {
    Scope::parse(name).filter(|s| matches!(s, Scope::CiOpen | Scope::CiDeploy))
}

pub fn vault_name(entry: &Entry) -> &str {
    entry.bitwarden_key.as_deref().unwrap_or(&entry.env_var)
}

pub struct Report {
    pub scope: Scope,
    pub present: Vec<String>,
    pub missing: Vec<String>,
    /// Registry keys of another scope that the token's project lists, with that scope; `None` when the backend cannot list.
    pub exposed: Option<Vec<(String, Scope)>>,
}

impl Report {
    pub fn ok(&self) -> bool {
        self.missing.is_empty() && self.exposed.iter().flatten().next().is_none()
    }

    pub fn to_json(&self) -> Value {
        let mut keys: Vec<Value> = self.missing.iter().map(|k| json!({ "key": k, "status": "missing" })).collect();
        keys.extend(self.present.iter().map(|k| json!({ "key": k, "status": "present" })));
        let exposed: Vec<Value> = self.exposed.iter().flatten().map(|(k, s)| json!({ "key": k, "scope": s.name() })).collect();
        json!({
            "scope": self.scope.name(),
            "ok": self.ok(),
            "keys": keys,
            "missing": self.missing,
            "exposed": exposed,
            "exposureChecked": self.exposed.is_some(),
        })
    }
}

/// `present` answers whether the vault holds a key; `visible` is every name the token lists, when it can say.
pub fn report(registry: &[Entry], scope: Scope, visible: Option<&[String]>, present: &mut dyn FnMut(&Entry) -> Result<bool, String>) -> Result<Report, String> {
    let (mut found, mut missing) = (Vec::new(), Vec::new());
    for entry in registry.iter().filter(|e| e.scope == scope && !e.bootstrap) {
        if present(entry)? {
            found.push(entry.key.clone());
        } else {
            missing.push(entry.key.clone());
        }
    }
    let exposed = visible.map(|names| {
        let mut wrong: Vec<(String, Scope)> = registry.iter().filter(|e| e.scope != scope && !e.bootstrap && names.iter().any(|n| n == vault_name(e))).map(|e| (e.key.clone(), e.scope)).collect();
        wrong.sort_by(|a, b| a.0.cmp(&b.0));
        wrong
    });
    found.sort();
    missing.sort();
    Ok(Report { scope, present: found, missing, exposed })
}
