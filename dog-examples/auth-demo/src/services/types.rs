use dog_axum::params::RestParams;

use dog_auth::hooks::authenticate::AuthParams;

// Type alias for authentication-enabled REST parameters
pub type AuthDemoParams = AuthParams<RestParams>;
