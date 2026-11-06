//! Comparing two captures of the same kind.

use serde::Serialize;

use super::{Sounding, View};

#[derive(Clone, Debug, Serialize)]
pub struct Change {
    pub view: View,
    pub name: String,
    /// `None` when the pool only exists in the other run.
    pub before: Option<u64>,
    pub after: Option<u64>,
    pub delta: i64,
}

#[derive(Clone, Debug)]
pub struct Diff {
    pub before: Sounding,
    pub after: Sounding,
    /// Every pool that changed, biggest change first.
    pub changes: Vec<Change>,
}

impl Diff {
    pub fn draft_delta(&self) -> Option<i64> {
        Some(signed(self.after.draft()?) - signed(self.before.draft()?))
    }
}

/// Compares `before` with `after`. Both must be the same kind of capture.
pub fn diff(before: Sounding, after: Sounding) -> Result<Diff, String> {
    if before.kind != after.kind {
        return Err(format!(
            "can't compare a {} with a {}; capture both runs the same way",
            before.kind.describe(),
            after.kind.describe()
        ));
    }
    let mut changes: Vec<Change> = Vec::new();
    for pool in &before.pools {
        let after_bytes = after
            .pools
            .iter()
            .find(|p| p.view == pool.view && p.name == pool.name)
            .map(|p| p.bytes);
        changes.push(Change {
            view: pool.view,
            name: pool.name.clone(),
            before: Some(pool.bytes),
            after: after_bytes,
            delta: signed(after_bytes.unwrap_or(0)) - signed(pool.bytes),
        });
    }
    for pool in &after.pools {
        if !before
            .pools
            .iter()
            .any(|p| p.view == pool.view && p.name == pool.name)
        {
            changes.push(Change {
                view: pool.view,
                name: pool.name.clone(),
                before: None,
                after: Some(pool.bytes),
                delta: signed(pool.bytes),
            });
        }
    }
    changes.retain(|c| c.delta != 0);
    changes.sort_by(|a, b| {
        b.delta
            .unsigned_abs()
            .cmp(&a.delta.unsigned_abs())
            .then_with(|| a.name.cmp(&b.name))
    });
    Ok(Diff {
        before,
        after,
        changes,
    })
}

fn signed(bytes: u64) -> i64 {
    i64::try_from(bytes).unwrap_or(i64::MAX)
}
