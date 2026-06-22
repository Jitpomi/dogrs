use std::sync::Arc;
use anyhow::Result;
use dog_core::DogAppBuilder;
use serde_json::Value;

pub mod types;
pub use types::DemoParams;

pub mod devices;
pub use devices::DevicesService;

pub fn configure(
    builder: &mut DogAppBuilder<Value, DemoParams>,
) -> Result<()> {
    let devices = Arc::new(DevicesService::new());
    builder.register_service("devices", Arc::clone(&devices) as Arc<dyn dog_core::DogService<Value, DemoParams>>);
    devices::devices_shared::register_hooks(builder)?;

    Ok(())
}
