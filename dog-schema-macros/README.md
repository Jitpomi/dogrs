# dog-schema-macros

Procedural implementation of the `#[schema]` module attribute, re-exported by
`dog-schema`. See `dog-schema` for the supported rules, generated functions,
registration examples, and migration notes.

This crate provides an attribute macro, not a `Schema` derive or a JSON Schema
generator. Invalid declarations produce compiler errors rather than silently
omitting validation. The documented source API is 0.2.0.
