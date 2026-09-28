# DogRS repository guidance

All DogRS applications, including examples and generated applications, must follow
the layout and responsibilities in [docs/application-structure.md](docs/application-structure.md).
Use the full common module structure for each application and service. Additional
domain modules may extend it. Framework library crates are not application crates.

When migrating existing applications, preserve behavior and documented launch
commands, and run checks appropriate to the affected application. Do not introduce
a web framework, database or storage dependency solely to satisfy the layout.
