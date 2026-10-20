//! Fail-closed query contracts for shared-database applications.
//!
//! This is deliberately not a TypeQL parser, automatic row-level security, or
//! an authentication provider. Contracts are trusted application configuration:
//! they must call the application's TypeQL authorization functions for every
//! affected resource. Untrusted callers supply a complete query, never a
//! contract name, database, transaction mode, or proof of authentication.
use anyhow::{ensure, Result};
use dog_core::{tenant::TenantContext, DogError};
use regex::Regex;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

const STRING: &str = r#""(?:[^"\\\x00-\x1f]|\\(?:["\\/bfnrt]|u[0-9a-fA-F]{4}))*""#;
const DATETIME: &str = r"[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}(?:\.[0-9]{1,9})?";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueryMethod {
    Read,
    Write,
}

/// Literal syntax and input bounds, not domain permission decisions.
#[derive(Clone, Debug)]
pub enum QueryParameter {
    String {
        max_bytes: usize,
    },
    Text {
        max_bytes: usize,
        non_blank: bool,
    },
    Identifier,
    DateTime,
    /// A string claim supplied by trusted server authentication hooks.
    AuthenticatedClaim,
}

/// Application-owned frontend TypeQL plus its literal/response contract.
/// Identity and tenant placeholders are required and cannot be client parameters.
pub struct QueryContract {
    method: QueryMethod,
    source: String,
    parameters: Vec<(String, QueryParameter)>,
    one_answer: bool,
}
impl QueryContract {
    pub fn new(method: QueryMethod, source: impl Into<String>) -> Self {
        Self {
            method,
            source: source.into(),
            parameters: vec![],
            one_answer: false,
        }
    }
    pub fn parameter(mut self, name: impl Into<String>, kind: QueryParameter) -> Self {
        self.parameters.push((name.into(), kind));
        self
    }
    /// Empty results are unavailable/not-authorized; multiple results fail
    /// before commit, rather than silently committing an unexpected bulk write.
    pub fn one_answer(mut self) -> Self {
        self.one_answer = true;
        self
    }
}

#[derive(Clone)]
enum Slot {
    Tenant,
    Identity,
    Now,
    Revision,
    Parameter(QueryParameter),
}
struct CompiledContract {
    method: QueryMethod,
    regex: Regex,
    slots: Vec<(String, Slot)>,
    one_answer: bool,
}
struct PolicyInner {
    contracts: Vec<CompiledContract>,
}

/// Compile once during application startup. Clones share immutable contracts.
#[derive(Clone)]
pub struct TenantQueryPolicy {
    inner: Arc<PolicyInner>,
}

/// Request-local capability. Not deserializable and never read from JSON.
/// It is tied to the policy instance that issued it, preventing accidental
/// reuse across services with different reviewed query sets.
pub struct TenantQueryContext {
    policy: Arc<PolicyInner>,
    tenant: String,
    identity: String,
    claims: BTreeMap<String, String>,
}

impl std::fmt::Debug for TenantQueryContext {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Request params can implement Debug without logging principal/claims.
        formatter
            .debug_struct("TenantQueryContext")
            .finish_non_exhaustive()
    }
}

pub(crate) struct PreparedQuery {
    pub query: String,
    one_answer: bool,
}
impl PreparedQuery {
    pub fn check_result(&self, result: &mut Value) -> Result<()> {
        let answers = result
            .pointer("/ok/answers")
            .and_then(Value::as_array)
            .ok_or_else(|| anyhow::anyhow!("Invalid TypeDB response"))?;
        if self.one_answer {
            if answers.is_empty() {
                return Err(
                    DogError::not_found("Resource unavailable or operation not permitted")
                        .into_anyhow(),
                );
            }
            ensure!(
                answers.len() == 1,
                "Unexpected query cardinality; transaction not committed"
            );
        }
        // Retain the standard answer envelope, but not credential-bearing query
        // text. Error messages are also sanitized by the scoped adapter.
        if let Some(ok) = result.get_mut("ok").and_then(Value::as_object_mut) {
            ok.remove("query");
        }
        Ok(())
    }
}

fn forbidden() -> anyhow::Error {
    DogError::forbidden("Query scope or contract check failed").into_anyhow()
}
fn bad_request(message: &str) -> anyhow::Error {
    DogError::bad_request(message).into_anyhow()
}
fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"-_.:".contains(&c))
}

// Validate placeholder placement in trusted contract source, not arbitrary
// TypeQL authorization. A quoted/commented placeholder cannot bind a principal.
fn is_code_at(source: &str, offset: usize) -> bool {
    let mut quote = None;
    let mut escaped = false;
    let mut comment = false;
    for byte in source.bytes().take(offset) {
        if comment {
            if byte == b'\n' {
                comment = false;
            }
            continue;
        }
        if let Some(delimiter) = quote {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == delimiter {
                quote = None;
            }
        } else if byte == b'#' {
            comment = true;
        } else if byte == b'"' || byte == b'\'' {
            quote = Some(byte);
        }
    }
    quote.is_none() && !comment
}

impl TenantQueryPolicy {
    pub fn new(contracts: impl IntoIterator<Item = QueryContract>) -> Result<Self> {
        let placeholders = Regex::new(r"@@([a-z_]+)@@")?;
        let mut compiled = Vec::new();
        let mut patterns = BTreeSet::new();
        for contract in contracts {
            ensure!(compiled.len() < 128, "Too many tenant query contracts");
            ensure!(
                !contract.source.is_empty() && contract.source.len() <= 60_000,
                "Invalid tenant query contract size"
            );
            let mut parameters = BTreeMap::new();
            for (name, kind) in contract.parameters {
                ensure!(
                    !matches!(
                        name.as_str(),
                        "tenant_id" | "identity_id" | "now" | "revision"
                    ),
                    "Reserved context parameter"
                );
                ensure!(
                    parameters.insert(name, kind).is_none(),
                    "Duplicate query parameter"
                );
            }
            let source = contract.source.trim();
            let mut pattern = String::from(r"\A");
            let mut slots = Vec::new();
            let mut end = 0;
            let mut used = BTreeSet::new();
            for capture in placeholders.captures_iter(source) {
                let matched = capture.get(0).unwrap();
                ensure!(
                    is_code_at(source, matched.start()),
                    "Context/value placeholders must be unquoted code literals"
                );
                let name = capture[1].to_string();
                let static_text = &source[end..matched.start()];
                ensure!(
                    !static_text.contains("@@"),
                    "Malformed contract placeholder"
                );
                pattern.push_str(&regex::escape(static_text));
                let slot = match name.as_str() {
                    "tenant_id" => Slot::Tenant,
                    "identity_id" => Slot::Identity,
                    "now" => Slot::Now,
                    "revision" => Slot::Revision,
                    _ => Slot::Parameter(
                        parameters
                            .get(&name)
                            .ok_or_else(|| anyhow::anyhow!("Undeclared parameter: {name}"))?
                            .clone(),
                    ),
                };
                pattern.push('(');
                pattern.push_str(
                    if matches!(slot, Slot::Now | Slot::Parameter(QueryParameter::DateTime)) {
                        DATETIME
                    } else {
                        STRING
                    },
                );
                pattern.push(')');
                used.insert(name.clone());
                slots.push((name, slot));
                ensure!(slots.len() <= 128, "Too many contract placeholders");
                end = matched.end();
            }
            ensure!(
                used.contains("tenant_id") && used.contains("identity_id"),
                "Every tenant contract must bind tenant_id and identity_id"
            );
            ensure!(
                parameters.keys().all(|name| used.contains(name)),
                "Unused contract parameter"
            );
            ensure!(
                !source[end..].contains("@@"),
                "Malformed contract placeholder"
            );
            pattern.push_str(&regex::escape(&source[end..]));
            pattern.push_str(r"\z");
            ensure!(
                patterns.insert((contract.method == QueryMethod::Write, pattern.clone())),
                "Duplicate tenant query shape"
            );
            compiled.push(CompiledContract {
                method: contract.method,
                regex: Regex::new(&pattern)?,
                slots,
                one_answer: contract.one_answer,
            });
        }
        ensure!(!compiled.is_empty(), "Tenant query policy cannot be empty");
        Ok(Self {
            inner: Arc::new(PolicyInner {
                contracts: compiled,
            }),
        })
    }

    /// Call ONLY after authentication and current tenant membership checks.
    /// TenantContext/header values alone are not evidence of membership.
    /// The library does not authenticate the supplied principal or claims.
    pub fn authenticated_context(
        &self,
        tenant: &TenantContext,
        identity: &str,
        claims: impl IntoIterator<Item = (String, String)>,
    ) -> Result<TenantQueryContext> {
        ensure!(
            identifier(&tenant.tenant_id.0) && identifier(identity),
            "Invalid authenticated tenant or identity"
        );
        let mut verified = BTreeMap::new();
        for (name, value) in claims {
            ensure!(
                verified.len() < 32 && value.len() <= 4096,
                "Authenticated claim too large"
            );
            ensure!(
                !matches!(
                    name.as_str(),
                    "tenant_id" | "identity_id" | "now" | "revision"
                ),
                "Reserved authenticated claim"
            );
            ensure!(
                verified.insert(name, value).is_none(),
                "Duplicate authenticated claim"
            );
        }
        Ok(TenantQueryContext {
            policy: self.inner.clone(),
            tenant: tenant.tenant_id.0.clone(),
            identity: identity.to_owned(),
            claims: verified,
        })
    }

    pub(crate) fn prepare(
        &self,
        context: &TenantQueryContext,
        data: Value,
        method: QueryMethod,
    ) -> Result<PreparedQuery> {
        if !Arc::ptr_eq(&context.policy, &self.inner) {
            return Err(forbidden());
        }
        let object = data
            .as_object()
            .filter(|o| o.len() == 1)
            .ok_or_else(|| bad_request("Supply {query} only"))?;
        let query = object
            .get("query")
            .and_then(Value::as_str)
            .filter(|q| !q.trim().is_empty() && q.len() <= 60_000)
            .ok_or_else(|| bad_request("Supply a nonempty query of at most 60000 bytes"))?
            .trim();
        for contract in self.inner.contracts.iter().filter(|c| c.method == method) {
            let Some(captures) = contract.regex.captures(query) else {
                continue;
            };
            let mut values = BTreeMap::<&str, &str>::new();
            let mut replacements = Vec::new();
            let now = chrono::Utc::now()
                .naive_utc()
                .format("%Y-%m-%dT%H:%M:%S%.6f")
                .to_string();
            let revision = serde_json::to_string(&uuid::Uuid::new_v4().to_string())?;
            for (i, (name, slot)) in contract.slots.iter().enumerate() {
                let m = captures.get(i + 1).unwrap();
                if values
                    .insert(name, m.as_str())
                    .is_some_and(|old| old != m.as_str())
                {
                    return Err(forbidden());
                }
                if matches!(slot, Slot::Now) {
                    replacements.push((m.start(), m.end(), now.clone()));
                    continue;
                }
                if matches!(slot, Slot::Revision) {
                    replacements.push((m.start(), m.end(), revision.clone()));
                    continue;
                }
                if matches!(slot, Slot::Parameter(QueryParameter::DateTime)) {
                    chrono::NaiveDateTime::parse_from_str(m.as_str(), "%Y-%m-%dT%H:%M:%S%.f")
                        .map_err(|_| bad_request("Invalid UTC datetime literal"))?;
                    continue;
                }
                let value: String = serde_json::from_str(m.as_str())
                    .map_err(|_| bad_request("Invalid string literal"))?;
                let expected = match slot {
                    Slot::Tenant => Some(context.tenant.as_str()),
                    Slot::Identity => Some(context.identity.as_str()),
                    Slot::Parameter(QueryParameter::AuthenticatedClaim) => {
                        Some(context.claims.get(name).ok_or_else(forbidden)?.as_str())
                    }
                    _ => None,
                };
                if expected.is_some_and(|expected| value != expected) {
                    return Err(forbidden());
                }
                match slot {
                    Slot::Parameter(QueryParameter::Identifier) if !identifier(&value) => {
                        return Err(bad_request("Invalid identifier literal"))
                    }
                    Slot::Parameter(QueryParameter::String { max_bytes })
                        if value.len() > *max_bytes =>
                    {
                        return Err(bad_request("String literal too large"))
                    }
                    Slot::Parameter(QueryParameter::Text {
                        max_bytes,
                        non_blank,
                    }) if value.len() > *max_bytes
                        || (*non_blank && value.trim().is_empty())
                        || value.chars().any(char::is_control) =>
                    {
                        return Err(bad_request("Invalid text literal"))
                    }
                    _ => {}
                }
            }
            let mut rendered = query.to_owned();
            for (start, end, value) in replacements.into_iter().rev() {
                rendered.replace_range(start..end, &value);
            }
            return Ok(PreparedQuery {
                query: rendered,
                one_answer: contract.one_answer,
            });
        }
        Err(forbidden())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    const READ: &str = "match\nlet $r in visible(@@tenant_id@@,@@identity_id@@,@@now@@);\n$r has name @@name@@; fetch {\"name\":$r.name};";
    fn policy() -> TenantQueryPolicy {
        TenantQueryPolicy::new([QueryContract::new(QueryMethod::Read, READ)
            .parameter("name", QueryParameter::String { max_bytes: 1024 })])
        .unwrap()
    }
    fn context(policy: &TenantQueryPolicy, tenant: &str, identity: &str) -> TenantQueryContext {
        policy
            .authenticated_context(&TenantContext::new(tenant), identity, [])
            .unwrap()
    }
    fn query(tenant: &str, identity: &str, name: &str) -> String {
        READ.replace("@@tenant_id@@", &serde_json::to_string(tenant).unwrap())
            .replace("@@identity_id@@", &serde_json::to_string(identity).unwrap())
            .replace("@@now@@", "1900-01-01T00:00:00")
            .replace("@@name@@", &serde_json::to_string(name).unwrap())
    }
    #[test]
    fn contracts_fail_closed_on_scope_shape_method_and_extra_fields() {
        let policy = policy();
        let ctx = context(&policy, "a", "alice");
        let valid = query("a", "alice", "hello");
        for data in [
            json!({"query":query("b","alice","hello")}),
            json!({"query":query("a","bob","hello")}),
            json!({"query":valid.replace("visible(","unguarded(")}),
            json!({"query":format!("{valid}\nmatch $x isa secret;")}),
            json!({"query":"match $r isa resource;"}),
            json!({"query":valid,"tenant_id":"b"}),
            json!({"query":valid,"bindings":{}}),
            json!({"query":valid,"database":"b"}),
            json!({"query":valid,"authenticated":true}),
            json!({"query":valid,"transaction":"schema"}),
            json!({"query":null}),
            json!({"query":"x".repeat(60_001)}),
        ] {
            assert!(policy.prepare(&ctx, data, QueryMethod::Read).is_err());
        }
        assert!(policy
            .prepare(&ctx, json!({"query":valid}), QueryMethod::Write)
            .is_err());
        assert!(policy
            .prepare(&ctx, json!({"query":valid}), QueryMethod::Read)
            .is_ok());
    }
    #[test]
    fn user_literals_never_become_query_structure_or_placeholders() {
        let policy = policy();
        let ctx = context(&policy, "a", "alice");
        for name in [
            "\"; delete $r;",
            "\\\"; #",
            "@@identity_id@@",
            "\n# delete",
            "a\\u0022; insert $x isa secret;",
            "é😀",
            "has tenant_id \"b\"",
        ] {
            let prepared = policy
                .prepare(
                    &ctx,
                    json!({"query":query("a","alice",name)}),
                    QueryMethod::Read,
                )
                .unwrap();
            assert!(prepared
                .query
                .contains(&serde_json::to_string(name).unwrap()));
        }
    }
    #[test]
    fn contexts_are_policy_bound_and_cannot_override_reserved_claims() {
        let other = policy();
        let policy = policy();
        let ctx = context(&other, "a", "alice");
        assert!(policy
            .prepare(
                &ctx,
                json!({"query":query("a","alice","")}),
                QueryMethod::Read
            )
            .is_err());
        assert!(policy
            .authenticated_context(
                &TenantContext::new("a"),
                "alice",
                [("tenant_id".into(), "b".into())]
            )
            .is_err());
        assert!(policy
            .authenticated_context(&TenantContext::new(""), "alice", [])
            .is_err());
    }
    #[test]
    fn clock_and_revision_are_server_owned_and_repeated_literals_must_agree() {
        let source="match $t has tenant_id @@tenant_id@@; $i has id @@identity_id@@; let $now=@@now@@; update $t has revision @@revision@@; fetch {\"t\":$t.tenant_id};";
        let policy =
            TenantQueryPolicy::new([QueryContract::new(QueryMethod::Write, source)]).unwrap();
        let ctx = context(&policy, "a", "alice");
        let q = source
            .replace("@@tenant_id@@", "\"a\"")
            .replace("@@identity_id@@", "\"alice\"")
            .replace("@@now@@", "1900-01-01T00:00:00")
            .replace("@@revision@@", "\"client\"");
        let first = policy
            .prepare(&ctx, json!({"query":q}), QueryMethod::Write)
            .unwrap();
        let second = policy
            .prepare(&ctx, json!({"query":q}), QueryMethod::Write)
            .unwrap();
        assert!(!first.query.contains("1900-01-01") && !first.query.contains("\"client\""));
        assert_ne!(first.query, second.query);
        let repeated=TenantQueryPolicy::new([QueryContract::new(QueryMethod::Read,
            "match $t has id @@tenant_id@@; $i has id @@identity_id@@; $other has id @@tenant_id@@;")]).unwrap();
        assert!(repeated
            .prepare(
                &context(&repeated, "a", "alice"),
                json!({"query":
            "match $t has id \"a\"; $i has id \"alice\"; $other has id \"b\";"}),
                QueryMethod::Read
            )
            .is_err());
    }
    #[test]
    fn authenticated_claims_and_datetime_literals_are_checked() {
        let source="match $t has id @@tenant_id@@; $i has id @@identity_id@@, has version @@version@@; $g has expires @@expires@@;";
        let policy = TenantQueryPolicy::new([QueryContract::new(QueryMethod::Read, source)
            .parameter("version", QueryParameter::AuthenticatedClaim)
            .parameter("expires", QueryParameter::DateTime)])
        .unwrap();
        let ctx = policy
            .authenticated_context(
                &TenantContext::new("a"),
                "alice",
                [("version".into(), "current".into())],
            )
            .unwrap();
        let q = source
            .replace("@@tenant_id@@", "\"a\"")
            .replace("@@identity_id@@", "\"alice\"")
            .replace("@@version@@", "\"current\"")
            .replace("@@expires@@", "2027-01-01T00:00:00");
        assert!(policy
            .prepare(&ctx, json!({"query":q}), QueryMethod::Read)
            .is_ok());
        for q in [
            q.replace("\"current\"", "\"forged\""),
            q.replace("2027-01-01", "2027-99-99"),
        ] {
            assert!(policy
                .prepare(&ctx, json!({"query":q}), QueryMethod::Read)
                .is_err());
        }
    }
    #[test]
    fn literal_input_bounds_are_enforced_without_domain_role_rules() {
        let source =
            "match $t has id @@tenant_id@@; $i has id @@identity_id@@; $r has value @@value@@;";
        for (kind, valid, invalid) in [
            (QueryParameter::Identifier, "ok-id", "bad\"; delete"),
            (QueryParameter::String { max_bytes: 4 }, "four", "longer"),
            (
                QueryParameter::Text {
                    max_bytes: 8,
                    non_blank: true,
                },
                "hello",
                "  ",
            ),
            (
                QueryParameter::Text {
                    max_bytes: 8,
                    non_blank: true,
                },
                "hello",
                "\n",
            ),
        ] {
            let policy = TenantQueryPolicy::new([
                QueryContract::new(QueryMethod::Read, source).parameter("value", kind)
            ])
            .unwrap();
            let ctx = context(&policy, "a", "alice");
            for (value, accepted) in [(valid, true), (invalid, false)] {
                let q = source
                    .replace("@@tenant_id@@", "\"a\"")
                    .replace("@@identity_id@@", "\"alice\"")
                    .replace("@@value@@", &serde_json::to_string(value).unwrap());
                assert_eq!(
                    policy
                        .prepare(&ctx, json!({"query":q}), QueryMethod::Read)
                        .is_ok(),
                    accepted
                );
            }
        }
    }
    #[test]
    fn invalid_trusted_configuration_is_rejected_at_startup() {
        assert!(TenantQueryPolicy::new([]).is_err());
        for source in [
            "match $i has id @@identity_id@@;",
            "match $t has id @@tenant_id@@;",
            "match $t has id @@tenant_id@@; $i has id @@identity_id@@; let $x=@@undeclared@@;",
            "# @@tenant_id@@\nmatch $i has id @@identity_id@@;",
            "match $t has id \"@@tenant_id@@\"; $i has id @@identity_id@@;",
        ] {
            assert!(
                TenantQueryPolicy::new([QueryContract::new(QueryMethod::Read, source)]).is_err()
            );
        }
        assert!(
            TenantQueryPolicy::new([QueryContract::new(QueryMethod::Read, READ)
                .parameter("tenant_id", QueryParameter::Identifier)])
            .is_err()
        );
    }
    #[test]
    fn result_contract_hides_queries_and_rejects_bulk_and_empty_answers() {
        let prepared = PreparedQuery {
            query: "sensitive".into(),
            one_answer: true,
        };
        for answers in [json!([]), json!([{}, {}])] {
            assert!(prepared
                .check_result(&mut json!({"ok":{"answers":answers}}))
                .is_err());
        }
        let mut result = json!({"ok":{"answers":[{"data":{"id":"a"}}],"query":"sensitive"}});
        prepared.check_result(&mut result).unwrap();
        assert!(result.pointer("/ok/query").is_none());
        assert_eq!(result.pointer("/ok/answers/0/data/id"), Some(&json!("a")));
    }
}
