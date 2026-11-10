//! When the stored history may be cut back: only when it nears what the free tier gives, and then the oldest rows go first.

/// The storage the metrics may fill and the marks that start and stop the cutting, as shares of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StoragePolicy {
    pub budget_bytes: u64,
    /// Cutting starts when use passes this share (percent) of the budget.
    pub high_pct: u64,
    /// Cutting stops when use is back under this share (percent).
    pub low_pct: u64,
}

impl StoragePolicy {
    /// The Always Free Autonomous Database: 20 GB for everything in it.
    pub const FREE_TIER: Self = Self { budget_bytes: 20 * 1024 * 1024 * 1024, high_pct: 90, low_pct: 85 };

    pub fn high_bytes(&self) -> u64 {
        self.budget_bytes / 100 * self.high_pct
    }

    pub fn low_bytes(&self) -> u64 {
        self.budget_bytes / 100 * self.low_pct
    }

    /// How many of the oldest `rows` to delete so that `used` bytes, spread evenly over them, fall to the low mark; 0 while use is at or
    /// under the high mark.
    pub fn rows_to_delete(&self, used: u64, rows: u64) -> u64 {
        if used <= self.high_bytes() || rows == 0 {
            return 0;
        }
        let excess = used - self.low_bytes();
        let share = excess as f64 / used as f64;
        ((rows as f64 * share).ceil() as u64).clamp(1, rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GB: u64 = 1024 * 1024 * 1024;
    const POLICY: StoragePolicy = StoragePolicy { budget_bytes: 100 * GB, high_pct: 90, low_pct: 85 };

    #[test]
    fn given_use_at_or_under_the_high_mark_when_asked_then_nothing_is_deleted() {
        assert_eq!(POLICY.rows_to_delete(90 * GB, 1_000), 0);
        assert_eq!(POLICY.rows_to_delete(10 * GB, 1_000), 0);
    }

    #[test]
    fn given_use_over_the_high_mark_when_asked_then_enough_oldest_rows_go_to_reach_the_low_mark() {
        // 95 of 100 GB over 1000 rows: 10 GB have to go, a tenth of 95 GB's rows is 105.3 -> 106 rows.
        assert_eq!(POLICY.rows_to_delete(95 * GB, 1_000), 106);
    }

    #[test]
    fn given_a_full_budget_when_asked_then_the_share_is_capped_at_every_row_and_never_zero() {
        assert_eq!(POLICY.rows_to_delete(100 * GB, 3), 1);
        assert_eq!(POLICY.rows_to_delete(1000 * GB, 3), 3);
        assert_eq!(POLICY.rows_to_delete(95 * GB, 0), 0);
    }

    #[test]
    fn given_the_free_tier_when_the_marks_are_read_then_they_are_90_and_85_percent_of_20_gb() {
        assert_eq!(StoragePolicy::FREE_TIER.high_bytes(), 20 * GB / 100 * 90);
        assert_eq!(StoragePolicy::FREE_TIER.low_bytes(), 20 * GB / 100 * 85);
    }
}
