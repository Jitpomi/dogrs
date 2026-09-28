//! Input/output schemas and validation for the payments service.

use anyhow::{ensure, Result};
use dog_schema::schema;
use serde::{Deserialize, Serialize};

#[schema(service = "payments", error_message = "Payments schema validation failed")]
pub mod def {
    #[create]
    pub struct PaymentPayload {
        #[dog(optional, trim, min_len(1), max_len(100))]
        pub invoice: Option<String>,

        #[dog(optional)]
        pub mode: Option<String>,

        #[dog(optional)]
        pub status_ids: Option<Vec<String>>,

        #[dog(optional)]
        pub padding: Option<String>,
    }
}

pub use def::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordPayment {
    pub invoice: String,
    pub mode: String,
    #[serde(default)]
    pub padding: String,
}

impl RecordPayment {
    pub fn new(invoice: impl Into<String>, mode: impl Into<String>) -> Self {
        Self {
            invoice: invoice.into(),
            mode: mode.into(),
            padding: String::new(),
        }
    }

    pub fn validate(&self) -> Result<()> {
        ensure!(
            !self.invoice.is_empty() && self.invoice.len() <= 100,
            "invalid synthetic invoice"
        );
        ensure!(
            ["normal", "retry", "long", "crash", "permanent"].contains(&self.mode.as_str()),
            "invalid mode"
        );
        Ok(())
    }
}

pub fn validate_status_batch(ids: &[String]) -> Result<()> {
    ensure!(
        ids.len() <= 1000 && ids.iter().all(|id| !id.is_empty()),
        "invalid status batch"
    );
    Ok(())
}
