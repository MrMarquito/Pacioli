use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{Direction, PostingDraft};

/// Computes the cryptographic hash linking the predecessor hash and entry contents.
/// Used by both the journal writer and the audit verifier so they cannot drift apart.
pub fn compute_entry_hash(
    previous_hash: Option<&[u8]>,
    entry_id: Uuid,
    currency: &str,
    postings: &[PostingDraft],
) -> Vec<u8> {
    let mut hasher = Sha256::new();
    match previous_hash {
        Some(prev) => hasher.update(prev),
        None => hasher.update(b"GENESIS_SEED_INVARIANT"),
    }

    hasher.update(entry_id.as_bytes());
    hasher.update(currency.as_bytes());

    for p in postings {
        hasher.update(p.account_id.0.as_bytes());
        // normalize() strips trailing zeros: 100.00 and 100.0000 both become "100"
        hasher.update(p.amount.normalize().to_string().as_bytes());
        match p.direction {
            Direction::Debit => hasher.update(b"D"),
            Direction::Credit => hasher.update(b"C"),
        }
    }

    hasher.finalize().to_vec()
}
