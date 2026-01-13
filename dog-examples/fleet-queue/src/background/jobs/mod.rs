pub mod gps_tracking;
pub mod employee_assignment;
pub mod route_rebalancing;
pub mod maintenance_scheduling;
pub mod sla_monitoring;
pub mod compliance_monitoring;

pub use gps_tracking::{GPSTrackingJob, FleetContext as GPSFleetContext};
pub use employee_assignment::{EmployeeAssignmentJob, FleetContext as EmployeeFleetContext};
pub use route_rebalancing::{RouteRebalancingJob, FleetContext as RouteFleetContext};
pub use maintenance_scheduling::{MaintenanceSchedulingJob, FleetContext as MaintenanceFleetContext};
pub use sla_monitoring::{SLAMonitoringJob, FleetContext as SLAFleetContext};
pub use compliance_monitoring::{ComplianceMonitoringJob, FleetContext as ComplianceFleetContext};

