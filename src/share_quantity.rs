use derive_more::*;
use serde::{Deserialize, Serialize};

#[derive(
    Copy, Clone, Serialize, Deserialize, Display, Add, Sub, Mul, Div, AddAssign, SubAssign,
)]
#[mul(forward)]
#[div(forward)]
#[display("{_0:.2}")]
pub struct ShareQuantity(pub f64);
