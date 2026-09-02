use crate::{text_policy::safe_contract_identifier, KnownProvider};
use deltamod_product_contracts::{
    OperationPhase, ProductError, ProductErrorCode, ProductErrorPayload, RecoveryAction,
};
use std::{collections::BTreeMap, fmt};

pub type ProviderResult<T> = Result<T, ProviderFailure>;

/// Runtime/provider failure categories accepted at the normalization boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProviderFailureKind {
    InvalidPayload,
    InvalidRequest,
    UnsupportedCapability,
    AuthenticationRequired,
    AuthenticationExpired,
    RateLimited,
    Offline,
    Http,
    Download,
    Cancelled,
    Internal,
}

/// Untrusted error input. `raw_message` exists only so callers can pass their complete runtime
/// observation to one boundary; it is deliberately ignored by the normalized output.
#[derive(Clone, Copy)]
pub struct ProviderErrorInput<'a> {
    pub provider: KnownProvider,
    pub kind: ProviderFailureKind,
    pub status: Option<u16>,
    pub retry_after_ms: Option<u64>,
    pub operation_id: Option<&'a str>,
    pub phase: Option<OperationPhase>,
    pub raw_message: Option<&'a str>,
}

impl fmt::Debug for ProviderErrorInput<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderErrorInput")
            .field("provider", &self.provider)
            .field("kind", &self.kind)
            .field("status", &self.status)
            .field("retry_after_ms", &self.retry_after_ms)
            .field("has_operation_id", &self.operation_id.is_some())
            .field("phase", &self.phase)
            .field("has_raw_message", &self.raw_message.is_some())
            .finish()
    }
}

impl<'a> ProviderErrorInput<'a> {
    #[must_use]
    pub const fn new(provider: KnownProvider, kind: ProviderFailureKind) -> Self {
        Self {
            provider,
            kind,
            status: None,
            retry_after_ms: None,
            operation_id: None,
            phase: None,
            raw_message: None,
        }
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct ProviderFailure {
    contract: ProductError,
}

impl ProviderFailure {
    #[must_use]
    pub fn contract(&self) -> &ProductError {
        &self.contract
    }

    #[must_use]
    pub fn into_contract(self) -> ProductError {
        self.contract
    }

    #[must_use]
    pub fn invalid_payload(provider: KnownProvider) -> Self {
        normalize_provider_error(ProviderErrorInput::new(
            provider,
            ProviderFailureKind::InvalidPayload,
        ))
    }

    #[must_use]
    pub fn invalid_request(provider: KnownProvider) -> Self {
        normalize_provider_error(ProviderErrorInput::new(
            provider,
            ProviderFailureKind::InvalidRequest,
        ))
    }

    #[must_use]
    pub fn unsupported(provider: KnownProvider) -> Self {
        normalize_provider_error(ProviderErrorInput::new(
            provider,
            ProviderFailureKind::UnsupportedCapability,
        ))
    }

    #[must_use]
    pub fn authentication_required(provider: KnownProvider) -> Self {
        normalize_provider_error(ProviderErrorInput::new(
            provider,
            ProviderFailureKind::AuthenticationRequired,
        ))
    }
}

impl fmt::Debug for ProviderFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderFailure")
            .field("code", &self.contract.code)
            .field("message_key", &self.contract.message_key)
            .field("retryable", &self.contract.retryable)
            .field("recovery_action", &self.contract.recovery_action)
            .field("safe_details", &self.contract.safe_details)
            .finish()
    }
}

impl fmt::Display for ProviderFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{}:{}",
            self.contract.code.as_str(),
            self.contract.message_key
        )
    }
}

impl std::error::Error for ProviderFailure {}

/// Converts untrusted provider/runtime failures into the frozen product error contract. Raw
/// messages, request URLs, response bodies, and credentials never cross this boundary.
#[must_use]
pub fn normalize_provider_error(input: ProviderErrorInput<'_>) -> ProviderFailure {
    let _discarded_untrusted_message = input.raw_message;
    let kind = if input.kind == ProviderFailureKind::Http {
        match input.status {
            Some(401 | 403) => ProviderFailureKind::AuthenticationRequired,
            Some(429) => ProviderFailureKind::RateLimited,
            _ => ProviderFailureKind::Http,
        }
    } else {
        input.kind
    };

    let (code, message_key, retryable, recovery_action) = match kind {
        ProviderFailureKind::InvalidPayload => (
            ProductErrorCode::InvalidRequest,
            "provider.invalid_payload",
            false,
            RecoveryAction::NoAction,
        ),
        ProviderFailureKind::InvalidRequest => (
            ProductErrorCode::InvalidRequest,
            "provider.invalid_request",
            false,
            RecoveryAction::NoAction,
        ),
        ProviderFailureKind::UnsupportedCapability => (
            ProductErrorCode::InvalidRequest,
            "provider.capability_unsupported",
            false,
            RecoveryAction::SelectExactSource,
        ),
        ProviderFailureKind::AuthenticationRequired => (
            ProductErrorCode::AuthenticationRequired,
            "provider.authentication_required",
            false,
            RecoveryAction::Reauthenticate,
        ),
        ProviderFailureKind::AuthenticationExpired => (
            ProductErrorCode::AuthenticationRequired,
            "provider.authentication_expired",
            false,
            RecoveryAction::Reauthenticate,
        ),
        ProviderFailureKind::RateLimited => (
            ProductErrorCode::RateLimited,
            "provider.rate_limited",
            true,
            RecoveryAction::Retry,
        ),
        ProviderFailureKind::Offline => (
            ProductErrorCode::ProviderUnavailable,
            "provider.offline",
            true,
            RecoveryAction::Retry,
        ),
        ProviderFailureKind::Http => {
            let retryable = input
                .status
                .is_some_and(|status| status == 408 || status >= 500);
            (
                if input.phase == Some(OperationPhase::Downloading) {
                    ProductErrorCode::DownloadFailed
                } else {
                    ProductErrorCode::ProviderUnavailable
                },
                "provider.http_failure",
                retryable,
                if retryable {
                    RecoveryAction::Retry
                } else {
                    RecoveryAction::NoAction
                },
            )
        }
        ProviderFailureKind::Download => (
            ProductErrorCode::DownloadFailed,
            "provider.download_failed",
            true,
            RecoveryAction::Retry,
        ),
        ProviderFailureKind::Cancelled => (
            ProductErrorCode::Cancelled,
            "provider.cancelled",
            false,
            RecoveryAction::NoAction,
        ),
        ProviderFailureKind::Internal => (
            ProductErrorCode::Internal,
            "provider.internal",
            false,
            RecoveryAction::NoAction,
        ),
    };

    let mut safe_details =
        BTreeMap::from([("provider".to_owned(), input.provider.as_str().into())]);
    if let Some(status) = input.status {
        safe_details.insert("http_status".into(), status.to_string());
    }
    if let Some(retry_after_ms) = input.retry_after_ms {
        safe_details.insert("retry_after_ms".into(), retry_after_ms.to_string());
    }
    let operation_id = input
        .operation_id
        .filter(|value| safe_contract_identifier(value, 128))
        .map(str::to_owned);

    let contract = ProductError::new(ProductErrorPayload {
        code,
        message_key: message_key.into(),
        operation_id,
        phase: input.phase,
        retryable,
        recovery_action,
        safe_details,
    })
    .expect("normalized provider errors are contract-valid");
    ProviderFailure { contract }
}
