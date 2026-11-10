//! What an operator does with the bucket: read where it stands against the budget, and make room. The decisions are plain
//! functions of what `BlobAdmin` reports; the bucket's own lifecycle rules do the unattended expiry.
use crate::domain::Budget;
use crate::ports::{BlobAdmin, ObjectInfo, StoreError, Usage};

/// The generation pointer lives here: no age or size may delete it, or every reader would fall back to generation 0.
const PROTECTED: &str = "meta/";

#[derive(Debug, PartialEq, Eq)]
pub struct Standing {
    pub usage: Usage,
    /// The caps that are exceeded: `storedBytes`, `requestsPerMonth`.
    pub over: Vec<&'static str>,
}

pub fn standing(admin: &impl BlobAdmin, budget: &Budget) -> Result<Standing, StoreError> {
    let usage = admin.usage()?;
    let mut over = Vec::new();
    if usage.stored_bytes > budget.stored_bytes {
        over.push("storedBytes");
    }
    if usage.requests_this_month.is_some_and(|requests| requests > budget.requests_per_month) {
        over.push("requestsPerMonth");
    }
    Ok(Standing { usage, over })
}

pub enum Clear<'a> {
    /// The oldest objects by creation time, until the bucket holds at most this many bytes.
    EvictTo(u64),
    /// Every object whose name starts with this.
    Prefix(&'a str),
}

#[derive(Debug, PartialEq, Eq)]
pub struct Cleared {
    pub deleted: usize,
    pub freed_bytes: u64,
    /// What is left, as the listing counted it.
    pub stored_bytes: u64,
}

pub fn clear(admin: &impl BlobAdmin, how: Clear) -> Result<Cleared, StoreError> {
    let all = admin.list("")?;
    let total: u64 = all.iter().map(|object| object.bytes).sum();
    let doomed: Vec<&ObjectInfo> = match how {
        Clear::Prefix(prefix) => all.iter().filter(|object| object.name.starts_with(prefix) && (prefix.starts_with(PROTECTED) || !object.name.starts_with(PROTECTED))).collect(),
        Clear::EvictTo(limit) => {
            let mut oldest: Vec<&ObjectInfo> = all.iter().filter(|object| !object.name.starts_with(PROTECTED)).collect();
            oldest.sort_by(|a, b| a.created_ms.cmp(&b.created_ms).then_with(|| a.name.cmp(&b.name)));
            let mut left = total;
            oldest
                .into_iter()
                .take_while(|object| {
                    let over = left > limit;
                    left = left.saturating_sub(object.bytes);
                    over
                })
                .collect()
        }
    };
    let names: Vec<String> = doomed.iter().map(|object| object.name.clone()).collect();
    let freed_bytes: u64 = doomed.iter().map(|object| object.bytes).sum();
    let deleted = admin.delete(&names)?;
    Ok(Cleared { deleted, freed_bytes, stored_bytes: total - freed_bytes })
}
