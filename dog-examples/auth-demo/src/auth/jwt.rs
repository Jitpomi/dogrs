use std::sync::Arc;

use dog_auth::JwtStrategy;

pub fn register_jwt<P: Send + Clone + 'static>(
    auth: &mut dog_auth::core::AuthenticationBuilder<P>,
) {
    auth.register("jwt", Arc::new(JwtStrategy::new()));
}
