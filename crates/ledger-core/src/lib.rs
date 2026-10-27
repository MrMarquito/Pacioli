pub mod currency;
pub mod entry;
pub mod error;
pub mod hashing;
pub mod posting;
pub mod types;

pub use currency::Currency;
pub use entry::JournalEntryDraft;
pub use error::CoreError;
pub use hashing::compute_entry_hash;
pub use posting::{Posting, PostingDraft};
pub use types::{AccountId, Direction, EntryId, PostingId};

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn test_valid_balanced_entry() {
        let acc1 = AccountId::new_v4();
        let acc2 = AccountId::new_v4();

        let postings = vec![
            PostingDraft::new(acc1, Direction::Debit, dec!(150.00)).unwrap(),
            PostingDraft::new(acc2, Direction::Credit, dec!(150.00)).unwrap(),
        ];

        let entry = JournalEntryDraft::new(
            "tx-12345".to_string(),
            "Customer funding transfer".to_string(),
            Currency::USD,
            postings,
        );

        assert!(entry.is_ok());
    }

    #[test]
    fn test_unbalanced_entry_fails() {
        let acc1 = AccountId::new_v4();
        let acc2 = AccountId::new_v4();

        let postings = vec![
            PostingDraft::new(acc1, Direction::Debit, dec!(150.00)).unwrap(),
            PostingDraft::new(acc2, Direction::Credit, dec!(149.99)).unwrap(),
        ];

        let result = JournalEntryDraft::new(
            "tx-12346".to_string(),
            "Fraudulent unbalanced transaction".to_string(),
            Currency::USD,
            postings,
        );

        match result {
            Err(CoreError::UnbalancedTransaction {
                debits,
                credits,
                difference,
            }) => {
                assert_eq!(debits, dec!(150.00));
                assert_eq!(credits, dec!(149.99));
                assert_eq!(difference, dec!(0.01));
            }
            _ => panic!("Expected UnbalancedTransaction error"),
        }
    }

    #[test]
    fn test_zero_or_negative_posting_rejected() {
        let acc = AccountId::new_v4();
        let err = PostingDraft::new(acc, Direction::Debit, dec!(0.00)).unwrap_err();
        assert_eq!(err, CoreError::NonPositivePostingAmount(dec!(0.00)));

        let err_negative = PostingDraft::new(acc, Direction::Debit, dec!(-5.00)).unwrap_err();
        assert_eq!(
            err_negative,
            CoreError::NonPositivePostingAmount(dec!(-5.00))
        );
    }
}
