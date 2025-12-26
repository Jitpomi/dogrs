use std::sync::Arc;

use dog_core::DogService;
use crate::typedb::TypeDBState;

pub mod types;
pub use types::SocialParams;


pub mod persons;

pub struct SocialServices {
  
    pub persons: Arc<dyn DogService<serde_json::Value, SocialParams>>,

}

pub fn configure(
    app: &dog_core::DogApp<serde_json::Value, SocialParams>,
    state: Arc<TypeDBState>,
) -> anyhow::Result<SocialServices> {
 
    let persons: Arc<dyn DogService<serde_json::Value, SocialParams>> = Arc::new(persons::PersonsService::new(Arc::clone(&state)));
    app.register_service("persons", Arc::clone(&persons));
    persons::persons_shared::register_hooks(app)?;
   

    Ok(SocialServices { persons })
}
