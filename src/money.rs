use derive_more::*;
use serde::{Deserialize, Serialize};

#[derive(
    Copy, Clone, PartialEq, PartialOrd, Serialize, Deserialize, Display, AddAssign, SubAssign,
)]
#[display(fmt = "${_0:.2}")]
pub struct Money(pub f64);
