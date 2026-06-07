# Changelog

All notable changes to the `dog-schema-macros` crate will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.8] — 2026-06-07 — syn 2 Migration

### Changed
- **`syn`** bumped from `1.x` → `2.0`
- **`proc-macro2`** bumped to `1.0.95`
- **`quote`** bumped to `1.0.40`

### Migration (internal — no user-facing API change)

The `#[schema]` macro API is unchanged. If you extend the macro internals, note:

| syn 1 | syn 2 |
|---|---|
| `parse_macro_input!(args as AttributeArgs)` | `parse_macro_input!(args with syn::meta::parser(\|meta\| {...}))` |
| `attr.parse_meta()` + `NestedMeta` iteration | `attr.parse_nested_meta(\|meta\| {...})` |
| `attr.path.is_ident("x")` | `attr.path().is_ident("x")` — `path()` is now a method |
| `NestedMeta::Lit(syn::Lit::Int(n))` | `let n: syn::LitInt = content.parse()?` inside parenthesised block |
| `NestedMeta::Meta(Meta::NameValue(nv)); nv.lit` | `meta.value()?.parse::<LitBool>()?` |

### Behaviour Changes
- Unknown `#[dog(...)]` attributes (e.g. `#[dog(relation = "users")]`) are now **silently skipped** with proper token consumption rather than silently ignored via `_ => {}`. This preserves forward-compatibility with attributes intended for other layers.
- Missing `service` argument on `#[schema]` now emits a **compile error** rather than silently defaulting to an empty string (which would have caused a runtime panic).
