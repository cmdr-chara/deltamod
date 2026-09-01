use deltamod_product_contracts::{
    MutationCheckpoint, MutationSideEffect, OperationPhase, OperationState,
};
use std::fmt;

/// A durable checkpoint whose before/after edges can be faulted independently.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum JournalCheckpointKind {
    JournalCreated,
    Phase(OperationPhase),
    Mutation {
        index: u32,
        checkpoint: MutationCheckpoint,
    },
    ManifestTemporary,
    ManifestPublished,
    ManifestOnlyCommit,
    Terminal(OperationState),
}

/// Deterministic crash windows exposed by the Release-A runtime.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FaultPoint {
    BeforeStagingEffect {
        index: u32,
    },
    BeforeBackupEffect {
        index: u32,
    },
    BeforeFilesystemEffect {
        index: u32,
        effect: MutationSideEffect,
    },
    BeforeJournalCas(JournalCheckpointKind),
    AfterJournalCas(JournalCheckpointKind),
    BeforeCleanup,
    AfterCleanup,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InjectedFault {
    pub point: FaultPoint,
}

impl fmt::Display for InjectedFault {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "injected lifecycle fault at {:?}", self.point)
    }
}

impl std::error::Error for InjectedFault {}

pub trait FaultInjector: Send {
    fn check(&mut self, point: &FaultPoint) -> Result<(), InjectedFault>;
}

#[derive(Default)]
pub struct NoFaults;

impl FaultInjector for NoFaults {
    fn check(&mut self, _point: &FaultPoint) -> Result<(), InjectedFault> {
        Ok(())
    }
}

/// Fails exactly once at the selected point. Useful both for deterministic
/// crash tests and for host-side fault-injection harnesses.
pub struct FailOnce {
    target: FaultPoint,
    fired: bool,
}

impl FailOnce {
    #[must_use]
    pub fn new(target: FaultPoint) -> Self {
        Self {
            target,
            fired: false,
        }
    }

    #[must_use]
    pub const fn fired(&self) -> bool {
        self.fired
    }
}

impl FaultInjector for FailOnce {
    fn check(&mut self, point: &FaultPoint) -> Result<(), InjectedFault> {
        if !self.fired && *point == self.target {
            self.fired = true;
            Err(InjectedFault {
                point: point.clone(),
            })
        } else {
            Ok(())
        }
    }
}
