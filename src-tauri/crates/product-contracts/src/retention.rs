use crate::{
    valid_id, ContractKind, ContractPayload, SchemaError, ValidatedContract, PRODUCT_SCHEMA_VERSION,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RetentionPolicy {
    pub cache_limit_bytes: u64,
    pub recovery_limit_bytes: u64,
    pub recovery_generations_per_installation: usize,
    pub minimum_free_space_reserve_bytes: u64,
    pub operation_history_items: usize,
    pub operation_history_age_days: u32,
}

impl Default for RetentionPolicy {
    fn default() -> Self {
        Self {
            cache_limit_bytes: crate::DEFAULT_CACHE_LIMIT_BYTES,
            recovery_limit_bytes: crate::DEFAULT_RECOVERY_LIMIT_BYTES,
            recovery_generations_per_installation:
                crate::DEFAULT_RECOVERY_GENERATIONS_PER_INSTALLATION,
            minimum_free_space_reserve_bytes: crate::MINIMUM_FREE_SPACE_RESERVE_BYTES,
            operation_history_items: crate::MAX_OPERATION_HISTORY_ITEMS,
            operation_history_age_days: crate::MAX_OPERATION_HISTORY_AGE_DAYS,
        }
    }
}

impl RetentionPolicy {
    #[must_use]
    pub fn required_free_space(&self, staging_bytes: u64, backup_bytes: u64) -> Option<u64> {
        staging_bytes
            .checked_add(backup_bytes)?
            .checked_add(self.minimum_free_space_reserve_bytes)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecoveryGeneration {
    pub generation_id: String,
    pub installation_id: String,
    pub size_bytes: u64,
    pub completed_at_ms: u64,
    pub last_accessed_at_ms: u64,
    pub active: bool,
    pub finished: bool,
    pub pinned: bool,
    pub journal_references: u32,
    pub viable: bool,
}

impl RecoveryGeneration {
    fn valid(&self) -> bool {
        valid_id(&self.generation_id, 128)
            && valid_id(&self.installation_id, 256)
            && self.last_accessed_at_ms >= self.completed_at_ms
            && (!self.active || !self.finished)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RetentionDecisionPayload {
    pub limit_bytes: u64,
    pub bytes_before: u64,
    pub bytes_after: u64,
    pub keep_generation_ids: Vec<String>,
    pub evict_generation_ids: Vec<String>,
    pub over_limit: bool,
}

pub type RetentionDecision = ValidatedContract<RetentionDecisionPayload>;

impl crate::schema::private::Sealed for RetentionDecisionPayload {}
impl ContractPayload for RetentionDecisionPayload {
    const KIND: ContractKind = ContractKind::RetentionDecision;
    const VERSION: u32 = PRODUCT_SCHEMA_VERSION;

    fn validate(&self) -> Result<(), SchemaError> {
        let keep: BTreeSet<_> = self.keep_generation_ids.iter().collect();
        let evict: BTreeSet<_> = self.evict_generation_ids.iter().collect();
        if self.bytes_after > self.bytes_before
            || self.over_limit != (self.bytes_after > self.limit_bytes)
            || keep.len() != self.keep_generation_ids.len()
            || evict.len() != self.evict_generation_ids.len()
            || !keep.is_disjoint(&evict)
            || keep.iter().chain(evict.iter()).any(|id| !valid_id(id, 128))
        {
            Err(SchemaError::InvalidDocument("retention decision"))
        } else {
            Ok(())
        }
    }
}

pub fn select_recovery_evictions(
    generations: &[RecoveryGeneration],
    policy: RetentionPolicy,
) -> Result<RetentionDecision, SchemaError> {
    if policy.recovery_generations_per_installation == 0
        || generations.iter().any(|generation| !generation.valid())
    {
        return Err(SchemaError::InvalidDocument("recovery generation"));
    }
    let mut ids = BTreeSet::new();
    if generations
        .iter()
        .any(|generation| !ids.insert(generation.generation_id.clone()))
    {
        return Err(SchemaError::InvalidDocument(
            "duplicate recovery generation",
        ));
    }

    let mut by_installation: BTreeMap<&str, Vec<&RecoveryGeneration>> = BTreeMap::new();
    for generation in generations {
        by_installation
            .entry(&generation.installation_id)
            .or_default()
            .push(generation);
    }

    let mut protected = BTreeSet::new();
    for group in by_installation.values_mut() {
        group.sort_by(|left, right| {
            right
                .completed_at_ms
                .cmp(&left.completed_at_ms)
                .then_with(|| right.generation_id.cmp(&left.generation_id))
        });
        for generation in group
            .iter()
            .filter(|generation| generation.finished)
            .take(policy.recovery_generations_per_installation)
        {
            protected.insert(generation.generation_id.clone());
        }
        let viable: Vec<_> = group
            .iter()
            .filter(|generation| generation.viable)
            .collect();
        if viable.len() == 1 {
            protected.insert(viable[0].generation_id.clone());
        }
        for generation in group.iter().filter(|generation| {
            generation.active
                || !generation.finished
                || generation.pinned
                || generation.journal_references > 0
        }) {
            protected.insert(generation.generation_id.clone());
        }
    }

    let bytes_before = generations.iter().try_fold(0_u64, |total, generation| {
        total.checked_add(generation.size_bytes)
    });
    let Some(bytes_before) = bytes_before else {
        return Err(SchemaError::InvalidDocument("recovery byte overflow"));
    };
    let mut bytes_after = bytes_before;
    let mut candidates: Vec<_> = generations
        .iter()
        .filter(|generation| !protected.contains(&generation.generation_id))
        .collect();
    candidates.sort_by(|left, right| {
        left.last_accessed_at_ms
            .cmp(&right.last_accessed_at_ms)
            .then_with(|| left.completed_at_ms.cmp(&right.completed_at_ms))
            .then_with(|| left.generation_id.cmp(&right.generation_id))
    });

    let mut evicted = BTreeSet::new();
    for generation in candidates {
        if bytes_after <= policy.recovery_limit_bytes {
            break;
        }
        bytes_after = bytes_after.saturating_sub(generation.size_bytes);
        evicted.insert(generation.generation_id.clone());
    }
    let mut keep_generation_ids: Vec<_> = generations
        .iter()
        .filter(|generation| !evicted.contains(&generation.generation_id))
        .map(|generation| generation.generation_id.clone())
        .collect();
    let mut evict_generation_ids: Vec<_> = evicted.into_iter().collect();
    keep_generation_ids.sort();
    evict_generation_ids.sort();
    RetentionDecision::new(RetentionDecisionPayload {
        limit_bytes: policy.recovery_limit_bytes,
        bytes_before,
        bytes_after,
        keep_generation_ids,
        evict_generation_ids,
        over_limit: bytes_after > policy.recovery_limit_bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn generation(id: &str, completed: u64) -> RecoveryGeneration {
        RecoveryGeneration {
            generation_id: id.into(),
            installation_id: "game".into(),
            size_bytes: 4,
            completed_at_ms: completed,
            last_accessed_at_ms: completed,
            active: false,
            finished: true,
            pinned: false,
            journal_references: 0,
            viable: true,
        }
    }

    #[test]
    fn latest_three_are_protected_and_oldest_fourth_is_lru_evicted() {
        let policy = RetentionPolicy {
            recovery_limit_bytes: 12,
            ..RetentionPolicy::default()
        };
        let decision = select_recovery_evictions(
            &[
                generation("g1", 1),
                generation("g2", 2),
                generation("g3", 3),
                generation("g4", 4),
            ],
            policy,
        )
        .unwrap();
        assert_eq!(decision.evict_generation_ids, ["g1"]);
        assert_eq!(decision.keep_generation_ids, ["g2", "g3", "g4"]);
        assert!(!decision.over_limit);
    }

    #[test]
    fn active_pinned_journal_and_sole_viable_generations_survive_pressure() {
        let policy = RetentionPolicy {
            recovery_generations_per_installation: 1,
            recovery_limit_bytes: 0,
            ..RetentionPolicy::default()
        };
        let mut active = generation("active", 1);
        active.active = true;
        active.finished = false;
        let mut pinned = generation("pinned", 2);
        pinned.pinned = true;
        let mut journal = generation("journal", 3);
        journal.journal_references = 1;
        let mut sole = generation("sole", 4);
        active.viable = false;
        pinned.viable = false;
        journal.viable = false;
        sole.viable = true;
        let decision = select_recovery_evictions(&[active, pinned, journal, sole], policy).unwrap();
        assert!(decision.evict_generation_ids.is_empty());
        assert!(decision.over_limit);
    }

    #[test]
    fn required_free_space_is_checked_for_overflow() {
        let policy = RetentionPolicy::default();
        assert!(policy.required_free_space(u64::MAX, 1).is_none());
    }
}
