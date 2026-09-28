use crate::app::*;
#[derive(Clone)]
pub(crate) struct BillingContext {
    pub(crate) db: Arc<Client>,
    pub(crate) tenant: String,
    pub(crate) crash_after_effect: bool,
}
#[derive(Serialize, Deserialize)]
pub(crate) struct RecordPayment {
    pub(crate) invoice: String,
    pub(crate) mode: String,
    #[serde(default)]
    padding: String,
}
