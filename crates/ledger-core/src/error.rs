use rust_decimal::Decimal;
use thiserror::Error;

#[derive(Error, Debug, PartialEq, Eq)]
pub enum CoreError {
    #[error("Zero-sum invariant broken: debits ({debits}) do not equal credits ({credits}). Net difference: {difference}")]
    UnbalancedTransaction {
        debits: Decimal,
        credits: Decimal,
        difference: Decimal,
    },

    #[error("A posting amount must be strictly positive. Found: {0}")]
    NonPositivePostingAmount(Decimal),

    #[error("A transaction draft must contain at least two postings. Found: {0}")]
    InsufficientPostings(usize),

    #[error("Currency mismatch: cannot mix currency {expected} with {found}")]
    CurrencyMismatch { expected: String, found: String },

    #[error("Account {0} is inactive and cannot accept postings")]
    AccountInactive(uuid::Uuid),
}
