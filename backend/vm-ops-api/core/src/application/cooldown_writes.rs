//! The write of the hosted pool-cooldown ledger: `wf`'s mirror posts `{ledger, version}` and the store takes it only at its
//! current version, so two writers never overwrite each other unseen.
use crate::application::ports::{CooldownLedger, LedgerWrite};
use crate::domain::json_body::{as_integer, parse_record};
use serde_json::Value;

/// Why a post is refused before the store is asked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refused {
    NotJson,
    /// The ledger is no object, or the version no integer.
    BadShape,
}

/// The store's answer to a well-formed post: `ok` with the new version, or stale with the current one.
pub async fn write_cooldowns(ledger: &dyn CooldownLedger, body: &[u8]) -> Result<LedgerWrite, Refused> {
    let post = parse_record(body).ok_or(Refused::NotJson)?;
    let Some(Value::Object(next)) = post.get("ledger") else { return Err(Refused::BadShape) };
    let version = as_integer(post.get("version")).ok_or(Refused::BadShape)?;
    Ok(ledger.write(next.clone(), expected_version(version)).await)
}

/// A whole number the store can compare: one past the 64-bit range is never the current version, whichever side it is on.
fn expected_version(version: f64) -> i64 {
    if version >= i64::MAX as f64 {
        i64::MAX
    } else if version <= i64::MIN as f64 {
        i64::MIN
    } else {
        version as i64
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::FakeCooldownLedger;

    #[tokio::test]
    async fn given_a_post_at_the_current_version_when_written_then_it_is_stored_and_a_post_at_the_old_one_is_stale() {
        let store = FakeCooldownLedger::default();

        let first = write_cooldowns(&store, br#"{"ledger":{"a":{"until":1}},"version":0}"#).await;
        let again = write_cooldowns(&store, br#"{"ledger":{},"version":0}"#).await;

        assert_eq!(first, Ok(LedgerWrite { ok: true, version: 1 }));
        assert_eq!(again, Ok(LedgerWrite { ok: false, version: 1 }));
        assert_eq!(store.read().await.ledger.len(), 1);
    }

    #[tokio::test]
    async fn given_a_version_written_as_a_float_or_with_an_exponent_when_written_then_it_counts_as_the_integer_it_is() {
        let store = FakeCooldownLedger::default();

        let zero = write_cooldowns(&store, br#"{"ledger":{},"version":0.0}"#).await;
        let one = write_cooldowns(&store, br#"{"ledger":{},"version":1e0}"#).await;
        let negative = write_cooldowns(&store, br#"{"ledger":{},"version":-1}"#).await;
        let huge = write_cooldowns(&store, br#"{"ledger":{},"version":1e300}"#).await;

        assert_eq!((zero, one), (Ok(LedgerWrite { ok: true, version: 1 }), Ok(LedgerWrite { ok: true, version: 2 })));
        assert_eq!((negative, huge), (Ok(LedgerWrite { ok: false, version: 2 }), Ok(LedgerWrite { ok: false, version: 2 })));
    }

    #[tokio::test]
    async fn given_a_malformed_post_when_written_then_it_is_refused_and_the_store_is_never_asked() {
        let store = FakeCooldownLedger::default();

        assert_eq!(write_cooldowns(&store, b"{oops").await, Err(Refused::NotJson));
        let bad = ["", "null", "[]", r#"{"version":0}"#, r#"{"ledger":[],"version":0}"#, r#"{"ledger":null,"version":0}"#, r#"{"ledger":"x","version":0}"#, r#"{"ledger":{}}"#, r#"{"ledger":{},"version":"0"}"#, r#"{"ledger":{},"version":0.5}"#];
        for body in bad {
            assert_eq!(write_cooldowns(&store, body.as_bytes()).await, Err(Refused::BadShape), "{body:?}");
        }
        assert_eq!(store.read().await.version, 0);
    }
}
