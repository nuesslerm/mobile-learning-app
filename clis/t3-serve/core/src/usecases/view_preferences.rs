//! The phone's workspace view preferences: the state of the phone's Workspace-options UI, kept as device state in the agent API's `state`
//! group, so it outlives a restart of the server the way the server's own settings do. The pinned T3 API has no method for them, so
//! `mobile.viewPreferences.get` and `mobile.viewPreferences.set` are our extension.
use super::queries::QueryFailure;
use super::service::Service;
use crate::ports::Op;
use serde_json::{json, Value};
use t3_contracts::view_preferences as contract;
use t3_contracts::wire;

/// The scope the blob is kept under; the server's settings use the same one.
const SCOPE: &str = "t3";
/// The key the blob is kept under.
const KEY: &str = "mobile.viewPreferences";

/// A refusal as the wire carries it.
fn refusal(why: contract::Refusal) -> QueryFailure {
    QueryFailure::Error(wire::mobile_view_preferences_error(&why.message, &why.path))
}

impl Service {
    /// The stored blob, or none when the phone has set none.
    fn stored_view_preferences(&self) -> Result<Option<Value>, QueryFailure> {
        let read = self.agent().run(&Op::StateGet { scope: SCOPE.into(), key: KEY.into() }).map_err(|e| QueryFailure::Error(wire::host_error(&e.message())))?;
        Ok(read.get("value").filter(|value| !value.is_null()).cloned())
    }

    /// The preferences as they are stored, the server's defaults under them: a blob of another version is never read as this one.
    fn preferences_of(&self, stored: Option<&Value>) -> Result<Value, QueryFailure> {
        let Some(blob) = stored else { return Ok(contract::defaults()) };
        match blob.get("version").and_then(Value::as_f64) {
            Some(version) if version == contract::VERSION as f64 => Ok(contract::with_defaults(blob.get("preferences").unwrap_or(&Value::Null))),
            Some(version) => Err(refusal(contract::version_refusal(version))),
            None => Err(refusal(contract::Refusal { path: "version".into(), message: format!("The stored view preferences carry no version: this server reads version {}.", contract::VERSION) })),
        }
    }

    /// `mobile.viewPreferences.get`: the preferences as the phone last set them.
    pub(super) fn read_view_preferences(&self) -> Result<Value, QueryFailure> {
        let stored = self.stored_view_preferences()?;
        Ok(json!({"version": contract::VERSION, "preferences": self.preferences_of(stored.as_ref())?}))
    }

    /// `mobile.viewPreferences.set`: the patch is checked, merged over what is stored, written back and answered whole.
    pub(super) fn write_view_preferences(&self, payload: &Value) -> Result<Value, QueryFailure> {
        let Some(version) = payload.get("version").and_then(Value::as_f64) else {
            return Err(QueryFailure::Defect("Missing key\n  at [\"version\"]".into()));
        };
        if version != contract::VERSION as f64 {
            return Err(refusal(contract::version_refusal(version)));
        }
        let patch = payload.get("preferences").cloned().unwrap_or(Value::Null);
        contract::validate_patch(&patch).map_err(refusal)?;
        // The read and the write are one step, so two phones writing at once do not lose a change.
        let _one_at_a_time = self.held().view_preferences.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let stored = self.stored_view_preferences()?;
        let merged = contract::merge(&self.preferences_of(stored.as_ref())?, &patch);
        self.agent().run(&Op::StateSet { scope: SCOPE.into(), key: KEY.into(), value: contract::blob(&merged) }).map_err(|e| QueryFailure::Error(wire::host_error(&e.message())))?;
        self.log("mobile.viewPreferences.updated", &[]);
        Ok(json!({"version": contract::VERSION, "preferences": merged}))
    }
}
