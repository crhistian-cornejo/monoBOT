//! The daily sweep that deletes messages older than `RETENTION_DAYS`.

use crate::config::{retention_days, Vars};
use crate::store::{Bodies, Index, StorageError};

const BATCH: i64 = 50;
const MAX_BATCHES_PER_RUN: usize = 40;
const DAY_MS: i64 = 24 * 60 * 60 * 1000;

/// Deletes messages older than the retention window, oldest first, and
/// returns how many were removed.
///
/// R2 bodies go before their D1 rows: if the row delete fails the row is
/// picked up again on the next run (R2 deletes are idempotent), whereas the
/// opposite order would orphan bodies permanently.
pub async fn run(
    vars: &Vars,
    index: &impl Index,
    bodies: &impl Bodies,
    now: i64,
) -> Result<usize, StorageError> {
    let cutoff = now - retention_days(vars.retention_days.as_deref()) * DAY_MS;
    let mut deleted = 0;
    for _ in 0..MAX_BATCHES_PER_RUN {
        let rows = index.expired(cutoff, BATCH).await?;
        if rows.is_empty() {
            break;
        }
        let keys: Vec<String> = rows.iter().map(|(_, key)| key.clone()).collect();
        let seqs: Vec<i64> = rows.iter().map(|(seq, _)| *seq).collect();
        bodies.delete(&keys).await?;
        index.delete(&seqs).await?;
        deleted += rows.len();
        if (rows.len() as i64) < BATCH {
            break;
        }
    }
    Ok(deleted)
}
