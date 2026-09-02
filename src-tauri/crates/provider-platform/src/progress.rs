use crate::{
    normalize_provider_error,
    text_policy::{safe_contract_identifier, safe_stable_basename},
    KnownProvider, ProviderErrorInput, ProviderFailure, ProviderFailureKind, ProviderResult,
};
use deltamod_product_contracts::{
    LifecycleOperationKind, OperationPhase, OperationProgress, OperationProgressPayload,
    OperationState,
};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

#[derive(Clone, Debug, Default)]
pub struct ProviderCancellationToken(Arc<AtomicBool>);

impl ProviderCancellationToken {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }

    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }

    pub fn checkpoint(
        &self,
        provider: KnownProvider,
        operation_id: Option<&str>,
    ) -> ProviderResult<()> {
        if self.is_cancelled() {
            let mut input = ProviderErrorInput::new(provider, ProviderFailureKind::Cancelled);
            input.operation_id = operation_id;
            input.phase = Some(OperationPhase::Downloading);
            Err(normalize_provider_error(input))
        } else {
            Ok(())
        }
    }
}

/// Monotonic download progress mapped directly to the frozen operation-progress contract.
#[derive(Clone, Debug)]
pub struct DownloadProgressTracker {
    provider: KnownProvider,
    operation_id: String,
    installation_id: String,
    kind: LifecycleOperationKind,
    state: OperationState,
    phase: OperationPhase,
    completed: u64,
    total: Option<u64>,
    cancellable: bool,
    current_item: Option<String>,
    updated_at_ms: u64,
    cancellation: ProviderCancellationToken,
}

impl DownloadProgressTracker {
    pub fn new(
        provider: KnownProvider,
        operation_id: impl AsRef<str>,
        installation_id: impl AsRef<str>,
        kind: LifecycleOperationKind,
        total: Option<u64>,
        updated_at_ms: u64,
        cancellation: ProviderCancellationToken,
    ) -> ProviderResult<Self> {
        let operation_id = operation_id.as_ref();
        let installation_id = installation_id.as_ref();
        if !safe_contract_identifier(operation_id, 128)
            || !safe_contract_identifier(installation_id, 256)
        {
            return Err(ProviderFailure::invalid_request(provider));
        }
        let tracker = Self {
            provider,
            operation_id: operation_id.to_owned(),
            installation_id: installation_id.to_owned(),
            kind,
            state: OperationState::Running,
            phase: OperationPhase::Downloading,
            completed: 0,
            total,
            cancellable: true,
            current_item: None,
            updated_at_ms,
            cancellation,
        };
        tracker.snapshot()?;
        Ok(tracker)
    }

    #[must_use]
    pub fn cancellation_token(&self) -> ProviderCancellationToken {
        self.cancellation.clone()
    }

    pub fn checkpoint(&self) -> ProviderResult<()> {
        self.cancellation
            .checkpoint(self.provider, Some(&self.operation_id))
    }

    pub fn advance(
        &mut self,
        completed: u64,
        total: Option<u64>,
        current_item: Option<&str>,
        updated_at_ms: u64,
    ) -> ProviderResult<OperationProgress> {
        self.checkpoint()?;
        if self.state != OperationState::Running
            || completed < self.completed
            || updated_at_ms < self.updated_at_ms
            || self
                .total
                .zip(total)
                .is_some_and(|(previous, next)| previous != next)
        {
            return Err(self.invalid_transition());
        }
        let effective_total = total.or(self.total);
        if effective_total.is_some_and(|value| completed > value) {
            return Err(self.invalid_transition());
        }
        self.completed = completed;
        self.total = effective_total;
        self.current_item = sanitize_current_item(current_item);
        self.updated_at_ms = updated_at_ms;
        self.snapshot()
    }

    pub fn request_cancel(&mut self, updated_at_ms: u64) -> ProviderResult<OperationProgress> {
        if self.state != OperationState::Running || updated_at_ms < self.updated_at_ms {
            return Err(self.invalid_transition());
        }
        self.cancellation.cancel();
        self.state = OperationState::Cancelling;
        self.cancellable = false;
        self.updated_at_ms = updated_at_ms;
        self.snapshot()
    }

    pub fn finish_cancelled(&mut self, updated_at_ms: u64) -> ProviderResult<OperationProgress> {
        if !matches!(
            self.state,
            OperationState::Running | OperationState::Cancelling
        ) || updated_at_ms < self.updated_at_ms
        {
            return Err(self.invalid_transition());
        }
        self.cancellation.cancel();
        self.state = OperationState::Cancelled;
        self.phase = OperationPhase::Complete;
        self.cancellable = false;
        self.current_item = None;
        self.updated_at_ms = updated_at_ms;
        self.snapshot()
    }

    pub fn finish_success(&mut self, updated_at_ms: u64) -> ProviderResult<OperationProgress> {
        self.checkpoint()?;
        if self.state != OperationState::Running || updated_at_ms < self.updated_at_ms {
            return Err(self.invalid_transition());
        }
        if let Some(total) = self.total {
            self.completed = total;
        }
        self.state = OperationState::Succeeded;
        self.phase = OperationPhase::Complete;
        self.cancellable = false;
        self.current_item = None;
        self.updated_at_ms = updated_at_ms;
        self.snapshot()
    }

    pub fn finish_failed(&mut self, updated_at_ms: u64) -> ProviderResult<OperationProgress> {
        if self.state != OperationState::Running || updated_at_ms < self.updated_at_ms {
            return Err(self.invalid_transition());
        }
        self.state = OperationState::Failed;
        self.phase = OperationPhase::Complete;
        self.cancellable = false;
        self.current_item = None;
        self.updated_at_ms = updated_at_ms;
        self.snapshot()
    }

    pub fn snapshot(&self) -> ProviderResult<OperationProgress> {
        OperationProgress::new(OperationProgressPayload {
            operation_id: self.operation_id.clone(),
            installation_id: self.installation_id.clone(),
            kind: self.kind,
            state: self.state,
            phase: self.phase,
            completed: self.completed,
            total: self.total,
            cancellable: self.cancellable,
            current_item: self.current_item.clone(),
            updated_at_ms: self.updated_at_ms,
        })
        .map_err(|_| self.invalid_transition())
    }

    fn invalid_transition(&self) -> ProviderFailure {
        let mut input = ProviderErrorInput::new(self.provider, ProviderFailureKind::InvalidRequest);
        input.operation_id = Some(&self.operation_id);
        input.phase = Some(self.phase);
        normalize_provider_error(input)
    }
}

fn sanitize_current_item(value: Option<&str>) -> Option<String> {
    safe_stable_basename(value?, 255)
}
