use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use dog_core::errors::DogError;

#[derive(Debug)]
pub struct DogAxumError(pub anyhow::Error);

impl From<anyhow::Error> for DogAxumError {
    fn from(e: anyhow::Error) -> Self {
        Self(e)
    }
}

impl IntoResponse for DogAxumError {
    fn into_response(self) -> Response {
        // If it’s a DogError (even if wrapped by anyhow contexts), preserve Feathers-ish fields
        if let Some(dog) = self.0.chain().find_map(|e| e.downcast_ref::<DogError>()) {
            let safe = dog.sanitize_for_client();
            let status = StatusCode::from_u16(safe.code())
                .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
            return (status, Json(safe.to_json())).into_response();
        }

        // Fallback: wrap any non-DogError as a DogError::GeneralError
        let dog = DogError::general_error(self.0.to_string());
        let safe = dog.sanitize_for_client();
        let status = StatusCode::from_u16(safe.code())
            .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        (status, Json(safe.to_json())).into_response()
    }
}

