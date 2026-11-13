use thiserror::Error;

/// Why a call to the router failed.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum PaymentsError {
    /// The client could not be built (TLS backend, bad configuration).
    #[error("payments client misconfigured: {0}")]
    Config(String),
    /// No answer: timeout or connection error. Retry later.
    #[error("payments router unreachable: {0}")]
    Unreachable(String),
    /// The router answered an error status. `error_code` and `details` come
    /// from its JSON error body when there is one (`VALIDATION_ERROR`, …).
    #[error("payments router answered {status}: {error_code:?} {details:?}")]
    Rejected {
        status: u16,
        error_code: Option<String>,
        details: Option<String>,
    },
    /// A `2xx` whose body is not the expected shape.
    #[error("payments router answer not understood: {0}")]
    InvalidResponse(String),
}

impl PaymentsError {
    /// Whether retrying the same call later can succeed (no answer, `408`,
    /// `429` or `5xx`).
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::Unreachable(_) => true,
            Self::Rejected { status, .. } => *status == 408 || *status == 429 || *status >= 500,
            Self::Config(_) | Self::InvalidResponse(_) => false,
        }
    }
}
