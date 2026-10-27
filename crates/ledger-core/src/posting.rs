use crate::error::CoreError;
use crate::types::{AccountId, Direction, PostingId};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PostingDraft {
    pub account_id: AccountId,
    pub direction: Direction,
    pub amount: Decimal,
}

impl PostingDraft {
    pub fn new(
        account_id: AccountId,
        direction: Direction,
        amount: Decimal,
    ) -> Result<Self, CoreError> {
        if amount <= Decimal::ZERO {
            return Err(CoreError::NonPositivePostingAmount(amount));
        }
        Ok(Self {
            account_id,
            direction,
            amount,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Posting {
    pub id: PostingId,
    pub account_id: AccountId,
    pub direction: Direction,
    pub amount: Decimal,
    pub sequence_num: u32,
}
