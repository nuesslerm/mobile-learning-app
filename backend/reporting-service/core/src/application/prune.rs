//! The size-driven cutting of the stored history: measure, and only over the high mark delete the oldest rows, nothing by age.
use crate::application::ports::{MetricsStore, PortError};
use crate::domain::prune::StoragePolicy;
use crate::domain::sample::PruneResult;

/// Rounds of delete-and-measure one pass may take before it leaves the rest to the next pass.
const MAX_ROUNDS: usize = 6;

/// While use is over the high mark, deletes a share of the oldest rows computed from the excess, until use is under the low mark.
pub async fn run(store: &dyn MetricsStore, policy: &StoragePolicy) -> Result<PruneResult, PortError> {
    let used_before = store.used_bytes().await?;
    let rows = store.row_count().await?;
    let mut result = PruneResult { deleted_rows: 0, used_before, used_after: used_before };
    tracing::info!(used_bytes = used_before, budget_bytes = policy.budget_bytes, rows, "prune.measured");
    for _ in 0..MAX_ROUNDS {
        let to_delete = policy.rows_to_delete(result.used_after, rows.saturating_sub(result.deleted_rows));
        if to_delete == 0 {
            break;
        }
        let before = result.used_after;
        let deleted = store.delete_oldest(to_delete).await?;
        result.deleted_rows += deleted;
        result.used_after = store.used_bytes().await?;
        tracing::warn!(deleted, used_bytes = result.used_after, high_bytes = policy.high_bytes(), "prune.deleted_oldest");
        // Space held by something other than the metrics is not theirs to free: stop rather than empty them for nothing.
        if deleted == 0 || result.used_after >= before {
            break;
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::sample::{Sample, SeriesPoint, SeriesQuery};
    use async_trait::async_trait;
    use std::sync::Mutex;

    /// `rows` samples of `bytes_per_row` each on top of `base` bytes held by other tables; deleting the oldest `n` removes exactly `n`.
    struct Store {
        rows: Mutex<u64>,
        bytes_per_row: u64,
        base: u64,
        deleted_calls: Mutex<Vec<u64>>,
    }

    impl Store {
        fn new(rows: u64, bytes_per_row: u64, base: u64) -> Self {
            Self { rows: Mutex::new(rows), bytes_per_row, base, deleted_calls: Mutex::new(Vec::new()) }
        }
    }

    #[async_trait]
    impl MetricsStore for Store {
        async fn insert_samples(&self, _: Vec<Sample>) -> Result<(), PortError> {
            Ok(())
        }
        async fn series_for(&self, _: i64, _: i64, _: &SeriesQuery) -> Result<Vec<SeriesPoint>, PortError> {
            Ok(Vec::new())
        }
        async fn latest(&self) -> Result<Vec<Sample>, PortError> {
            Ok(Vec::new())
        }
        async fn row_count(&self) -> Result<u64, PortError> {
            Ok(*self.rows.lock().unwrap())
        }
        async fn used_bytes(&self) -> Result<u64, PortError> {
            Ok(self.base + *self.rows.lock().unwrap() * self.bytes_per_row)
        }
        async fn delete_oldest(&self, rows: u64) -> Result<u64, PortError> {
            let mut held = self.rows.lock().unwrap();
            let gone = rows.min(*held);
            *held -= gone;
            self.deleted_calls.lock().unwrap().push(gone);
            Ok(gone)
        }
    }

    const POLICY: StoragePolicy = StoragePolicy { budget_bytes: 1000, high_pct: 90, low_pct: 85 };

    #[tokio::test]
    async fn given_use_under_the_high_mark_when_the_pass_runs_then_nothing_is_deleted() {
        let store = Store::new(8, 100, 0);
        let result = run(&store, &POLICY).await.unwrap();
        assert_eq!((result.deleted_rows, result.used_before, result.used_after), (0, 800, 800));
        assert!(store.deleted_calls.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn given_use_over_the_high_mark_when_the_pass_runs_then_the_oldest_rows_go_until_it_is_under_the_low_mark() {
        let store = Store::new(10, 100, 0);
        let result = run(&store, &POLICY).await.unwrap();
        assert_eq!((result.deleted_rows, result.used_after), (2, 800), "1000 bytes over ten rows: two rows bring it to 800, under the 850 mark");
    }

    #[tokio::test]
    async fn given_space_held_by_other_tables_when_the_pass_runs_then_it_stops_after_a_round_that_freed_nothing() {
        let store = Store::new(10, 0, 950);
        let result = run(&store, &POLICY).await.unwrap();
        assert_eq!((result.deleted_rows, result.used_after), (2, 950), "one round, then it leaves the other tables alone");
        assert_eq!(*store.deleted_calls.lock().unwrap(), [2]);
    }
}
