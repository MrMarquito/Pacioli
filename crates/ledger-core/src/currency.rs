use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Currency {
    USD,
    EUR,
    GBP,
    JPY,
    CAD,
}

impl Currency {
    /// Returns the standard number of decimal places for the currency.
    pub const fn precision(&self) -> u32 {
        match self {
            Currency::USD | Currency::EUR | Currency::GBP | Currency::CAD => 2,
            Currency::JPY => 0,
        }
    }
}

impl fmt::Display for Currency {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}
