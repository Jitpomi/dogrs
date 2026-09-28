# DogRS application structure

All DogRS applications follow this layout. This includes examples, generated
applications, HTTP servers, CLI applications and background workers. Framework
library crates retain their own internal layouts.

```text
src/
├── main.rs
├── lib.rs
├── app.rs
├── hooks.rs
├── channels.rs
└── services/
    ├── mod.rs
    ├── types.rs
    └── <service>/
        ├── mod.rs
        ├── <service>_service.rs
        ├── <service>_hooks.rs
        ├── <service>_shared.rs
        └── <service>_schema.rs
```

Replace `<service>` with the service's snake_case name, such as `posts`.

## Responsibilities

- `main.rs`: process startup, configuration loading, runtime setup and shutdown.
  Keep service logic out of the executable entry point.
- `lib.rs`: declare application modules and expose reusable construction APIs for
  tests and other entry points.
- `app.rs`: construct the application and compose services, global hooks, channels
  and selected transports.
- `hooks.rs`: application-wide hooks and their registration.
- `channels.rs`: application channel configuration and event routing.
- `services/mod.rs`: declare and register the application's services.
- `services/types.rs`: application service types shared across services, such as
  request parameters and shared state.
- `<service>/mod.rs`: declare service modules and expose their public API.
- `<service>_service.rs`: service implementation and business operations.
- `<service>_hooks.rs`: hooks specific to this service.
- `<service>_shared.rs`: service-local shared definitions and registration helpers.
- `<service>_schema.rs`: service input/output schemas and validation registration.

Keep these modules even when a concern is unused; document that explicitly in the
module instead of inventing hooks, channels or validation rules. Declare the modules
so the layout represents actual Rust module boundaries, not unused files.

Additional modules such as `config.rs`, `background/`, `metadata/` and
`services/adapters/`, plus `static/`, `tests/` and database schema files, can extend
this structure. They do not replace the common layout. Avoid placing an entire
application in `main.rs`.

The layout does not require a particular web server, transport, database or storage
provider. Keep those choices in application composition and adapters; service
business logic should not acquire a web framework dependency just to follow this
convention.

## Existing applications

This is the required convention for new applications and application refactors.
The workspace examples use this layout. When migrating other applications,
preserve behavior, public construction APIs and documented launch commands;
verify the affected application after moving code. A consistent layout alone does not certify an application for production.
