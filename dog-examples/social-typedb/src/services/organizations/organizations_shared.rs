use dog_core::{ServiceCapabilities, ServiceMethodKind};

pub fn capabilities() -> ServiceCapabilities {
    ServiceCapabilities::from_methods(vec![
        ServiceMethodKind::Custom("read"),
        ServiceMethodKind::Custom("write"),
    ])
}

pub fn register_hooks() {
    // TODO: Implement organization-specific hooks
}
