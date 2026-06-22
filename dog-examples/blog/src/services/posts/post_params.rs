use crate::services::BlogParams;

#[derive(Clone)]
pub struct PostParams {
    pub include_drafts: bool,
}

impl From<&BlogParams> for PostParams {
    fn from(params: &BlogParams) -> Self {
        let include_drafts = params
            .query
            .get("includeDrafts")
            .map(|v| v == "true" || v == "1")
            .unwrap_or(false);

        Self { include_drafts }
    }
}
