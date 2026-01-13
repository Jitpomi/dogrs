pub mod types;
pub use types::FleetParams;


use std::sync::Arc;

use dog_core::DogService;
use crate::typedb::TypeDBState;

pub mod vehicles;
pub mod deliveries;
pub mod operations;
pub mod employees;
pub mod tomtom;
pub mod jobs;
pub mod rules;

pub struct FleetServices {
    pub vehicles: Arc<dyn DogService<serde_json::Value, FleetParams>>,
    pub deliveries: Arc<dyn DogService<serde_json::Value, FleetParams>>,
    pub operations: Arc<dyn DogService<serde_json::Value, FleetParams>>,
    pub employees: Arc<dyn DogService<serde_json::Value, FleetParams>>,
    pub tomtom: Arc<dyn DogService<serde_json::Value, FleetParams>>,
    pub jobs: Arc<dyn DogService<serde_json::Value, FleetParams>>,
    pub rules: Arc<dyn DogService<serde_json::Value, FleetParams>>,
}

pub fn configure(
    app: &dog_core::DogApp<serde_json::Value, FleetParams>,
    state: Arc<TypeDBState>,
) -> anyhow::Result<FleetServices> {

    let vehicles: Arc<dyn DogService<serde_json::Value, FleetParams>> = Arc::new(vehicles::VehiclesService::new(Arc::clone(&state)));
    app.register_service("vehicles", Arc::clone(&vehicles));
    vehicles::vehicles_shared::register_hooks(app)?;

    let deliveries: Arc<dyn DogService<serde_json::Value, FleetParams>> = Arc::new(deliveries::DeliveriesService::new(Arc::clone(&state)));
    app.register_service("deliveries", Arc::clone(&deliveries));
    deliveries::deliveries_shared::register_hooks(app)?;

    let operations: Arc<dyn DogService<serde_json::Value, FleetParams>> = Arc::new(operations::OperationsService::new(Arc::clone(&state)));
    app.register_service("operations", Arc::clone(&operations));
    operations::operations_shared::register_hooks(app)?;

    let employees: Arc<dyn DogService<serde_json::Value, FleetParams>> = Arc::new(employees::EmployeesService::new(Arc::clone(&state)));
    app.register_service("employees", Arc::clone(&employees));
    employees::employees_shared::register_hooks(app)?;

    let tomtom: Arc<dyn DogService<serde_json::Value, FleetParams>> = Arc::new(tomtom::TomTomService::new(app)?);
    app.register_service("tomtom", Arc::clone(&tomtom));
    tomtom::tomtom_shared::register_hooks(app)?;

    let jobs: Arc<dyn DogService<serde_json::Value, FleetParams>> = Arc::new(jobs::JobsService::new(app)?);
    app.register_service("jobs", Arc::clone(&jobs));
    jobs::jobs_shared::register_hooks(app)?;
    
    let rules: Arc<dyn DogService<serde_json::Value, FleetParams>> = Arc::new(rules::RulesService::new(Arc::clone(&state)));
    app.register_service("rules", Arc::clone(&rules));
    rules::rules_shared::register_hooks(app)?;

    Ok(FleetServices {
        vehicles,
        deliveries,
        operations,
        employees,
        tomtom,
        jobs,
        rules,
    })
}
