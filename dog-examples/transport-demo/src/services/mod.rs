pub mod echo;
pub mod types;
pub fn configure(builder: &mut dog_core::DogAppBuilder<serde_json::Value, types::Params>) {
    builder.register_service("echo", std::sync::Arc::new(echo::echo_service::Echo));
}
