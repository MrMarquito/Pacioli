use crate::currency::Currency;
use crate::error::CoreError;
use crate::posting::PostingDraft;
use crate::types::{Direction, EntryId};
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalEntryDraft {
    pub id: EntryId,
    pub idempotency_key: String,
    pub description: String,
    pub currency: Currency,
    pub postings: Vec<PostingDraft>,
    pub created_at: DateTime<Utc>,
}

impl JournalEntryDraft {
    pub fn new(
        idempotency_key: String,
        description: String,
        currency: Currency,
        postings: Vec<PostingDraft>,
    ) -> Result<Self, CoreError> {
        if postings.len() < 2 {
            return Err(CoreError::InsufficientPostings(postings.len()));
        }

        let mut total_debits = Decimal::ZERO;
        let mut total_credits = Decimal::ZERO;

        for posting in &postings {
            match posting.direction {
                Direction::Debit => total_debits += posting.amount,
                Direction::Credit => total_credits += posting.amount,
            }
        }

        if total_debits != total_credits {
            return Err(CoreError::UnbalancedTransaction {
                debits: total_debits,
                credits: total_credits,
                difference: total_debits - total_credits,
            });
        }

        Ok(Self {
            id: EntryId::new_v7(),
            idempotency_key,
            description,
            currency,
            postings,
            created_at: Utc::now(),
        })
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use crate::types::AccountId;
    use proptest::prelude::*;
    use rust_decimal::Decimal;

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(500))]

        #[test]
        fn test_property_zero_sum_invariant(
            // Generate between 1 and 10 random positive amounts up to $100,000 with 2 decimal places
            amounts in prop::collection::vec(1u32..10_000_000, 1..10)
        ) {
            let mut postings = Vec::new();
            let mut total_debit = Decimal::ZERO;

            for raw_amt in &amounts {
                let amt = Decimal::new(*raw_amt as i64, 2);
                total_debit += amt;
                postings.push(
                    PostingDraft::new(AccountId::new_v7(), Direction::Debit, amt).unwrap()
                );
            }

            // Exactly balance total debits with a single credit posting
            postings.push(
                PostingDraft::new(AccountId::new_v7(), Direction::Credit, total_debit).unwrap()
            );

            let draft = JournalEntryDraft::new(
                "prop-test-key".to_string(),
                "Property test entry".to_string(),
                Currency::USD,
                postings,
            );

            prop_assert!(draft.is_ok(), "Balanced multi-leg draft failed invariant validation");

            let entry = draft.unwrap();
            let debits: Decimal = entry.postings.iter()
                .filter(|p| p.direction == Direction::Debit)
                .map(|p| p.amount)
                .sum();
            let credits: Decimal = entry.postings.iter()
                .filter(|p| p.direction == Direction::Credit)
                .map(|p| p.amount)
                .sum();

            prop_assert_eq!(debits - credits, Decimal::ZERO, "Mathematical zero-sum broken");
        }
    }
}
