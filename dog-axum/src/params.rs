pub use dog_transport::http::RestParams;

pub trait FromRestParams: Sized {
    fn from_rest_params(params: RestParams) -> Self;
}

impl FromRestParams for RestParams {
    fn from_rest_params(params: RestParams) -> Self {
        params
    }
}

impl FromRestParams for () {
    fn from_rest_params(_params: RestParams) -> Self {}
}

#[cfg(feature = "auth")]
impl FromRestParams for dog_auth::hooks::authenticate::AuthParams<RestParams> {
    fn from_rest_params(params: RestParams) -> Self {
        dog_auth::hooks::authenticate::AuthParams {
            inner: params.clone(),
            provider: Some(params.provider.clone()),
            headers: params.headers.clone(),
            authentication: None,
            authenticated: false,
            auth_result: None,
            connection: None,
        }
    }
}
