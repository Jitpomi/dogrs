//! Structural compiler. The function catalogue is read from the database schema;
//! applications do not register queries or duplicate function bodies in Rust.
use super::{label, literal, named};
use anyhow::{ensure, Result};
use futures::future::BoxFuture;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};
use typedb_driver::Transaction;
use typeql::{
    expression::{Expression, FunctionCall, FunctionName},
    pattern::Pattern,
    query::{
        schema::SchemaQuery,
        stage::{delete::DeletableKind, fetch::*, modifier::Operator, Stage},
        QueryStructure,
    },
    schema::definable::{
        function::{Output, ReturnReduction, ReturnStatement},
        type_::{CapabilityBase, Type as SchemaType},
        Definable, Function,
    },
    statement::{
        thing::{
            isa::{Isa, IsaInstanceConstraint, IsaKind},
            Constraint, HasValue, Head, Links, RolePlayer,
        },
        AssignmentPattern, Statement,
    },
    TypeRef, TypeRefAny, Variable,
};

const CTX: &str = "$dog_security_tenant, $dog_security_actor, $dog_security_now";
const CTX_ARGS: &str =
    "$dog_security_tenant: string, $dog_security_actor: string, $dog_security_now: datetime";

// Only compiler-generated SCHEMA introspection results belong here. Never cache
// caller queries, data, policy decisions, membership, clocks or authentication.
// One instance belongs to one transaction holding its schema lock; it must not
// be shared with a later request, tenant transaction or database.
#[derive(Clone, Default)]
pub(super) struct SchemaMetadata(Arc<Mutex<MetadataEntries>>);
#[derive(Default)]
struct MetadataEntries {
    rows: BTreeMap<String, Vec<serde_json::Value>>,
    bytes: usize,
    #[cfg(test)]
    fetches: usize,
}
impl SchemaMetadata {
    fn get(&self, query: &str) -> Result<Option<Vec<serde_json::Value>>> {
        Ok(self
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("Schema metadata lock poisoned"))?
            .rows
            .get(query)
            .cloned())
    }
    fn insert(&self, query: &str, rows: &[serde_json::Value], limit: usize) -> Result<()> {
        let mut entries = self
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("Schema metadata lock poisoned"))?;
        if let Some(previous) = entries.rows.get(query) {
            ensure!(
                previous == rows,
                "Schema metadata changed within transaction"
            );
            return Ok(());
        }
        let encoded_bytes = serde_json::to_vec(rows)?.len();
        let bytes = entries
            .bytes
            .checked_add(query.len())
            .and_then(|n| n.checked_add(encoded_bytes))
            .ok_or_else(|| anyhow::anyhow!("Schema metadata size overflow"))?;
        ensure!(bytes <= limit, "Schema metadata byte limit exceeded");
        entries.rows.insert(query.into(), rows.into());
        entries.bytes = bytes;
        Ok(())
    }
}

#[cfg(test)]
#[path = "../../tests/isolation/metadata_cache.rs"]
mod metadata_tests;
#[derive(Clone, Default)]
struct Env {
    objects: BTreeMap<String, String>,
    values: BTreeSet<String>,
}
impl Env {
    fn contains(&self, name: &str) -> bool {
        self.objects.contains_key(name) || self.values.contains(name)
    }
    fn require(&self, variable: &Variable) -> Result<String> {
        let n = named(variable)?;
        ensure!(self.contains(&n), "Unbound or unclassified variable");
        Ok(n)
    }
}
pub(super) struct Compiled {
    pub source: String,
    pub checks: BTreeSet<String>,
}
impl Compiled {
    pub fn check_result(&self, result: &mut serde_json::Value) -> Result<()> {
        let rows = result["ok"]["answers"]
            .as_array_mut()
            .ok_or_else(|| anyhow::anyhow!("Missing result stream"))?;
        for row in rows {
            let checks = row["data"]["checks"]
                .as_object()
                .ok_or_else(|| anyhow::anyhow!("Missing post-write checks"))?;
            ensure!(
                checks.len() == self.checks.len(),
                "Incomplete post-write checks"
            );
            for key in &self.checks {
                ensure!(
                    checks.get(key).and_then(serde_json::Value::as_bool) == Some(true),
                    "Post-write read policy denied"
                );
            }
            let data = row["data"]
                .get_mut("result")
                .ok_or_else(|| anyhow::anyhow!("Missing projected result"))?
                .take();
            row["data"] = data;
        }
        Ok(())
    }
}

pub(super) struct Compiler<'a> {
    tx: &'a Transaction,
    options: &'a crate::QueryOptions,
    functions: BTreeMap<String, Function>,
    types: BTreeMap<String, SchemaType>,
    emitted: BTreeMap<String, String>,
    definitions: Vec<String>,
    visiting: BTreeSet<String>,
    serial: usize,
    checks: BTreeSet<String>,
    checkpoint: bool,
    lineage: bool,
    created: Vec<(String, String)>,
    metadata: SchemaMetadata,
}
impl<'a> Compiler<'a> {
    pub fn new(
        tx: &'a Transaction,
        options: &'a crate::QueryOptions,
        schema: &str,
    ) -> Result<Self> {
        ensure!(
            schema.len() <= 2 * 1024 * 1024,
            "Schema compilation limit exceeded"
        );
        let mut functions = BTreeMap::new();
        let mut types = BTreeMap::new();
        for query in typeql::parse_queries(schema)? {
            let QueryStructure::Schema(SchemaQuery::Define(define)) = query.structure else {
                anyhow::bail!("Unexpected schema export")
            };
            for definition in define.definables {
                if let Definable::TypeDeclaration(t) = definition {
                    let entry = types
                        .entry(t.label.to_string())
                        .or_insert_with(|| t.clone());
                    ensure!(
                        entry.kind.is_none() || t.kind.is_none() || entry.kind == t.kind,
                        "Conflicting schema kinds"
                    );
                    if entry.kind.is_none() {
                        entry.kind = t.kind;
                    }
                    if entry != &t {
                        entry.capabilities.extend(t.capabilities);
                        entry.annotations.extend(t.annotations);
                    }
                } else if let Definable::Function(f) = definition {
                    ensure!(
                        !f.signature.ident.to_string().starts_with("dog_security_"),
                        "Schema function uses reserved compiler namespace"
                    );
                    functions.insert(f.signature.ident.to_string(), f);
                }
            }
        }
        Ok(Self {
            tx,
            options,
            functions,
            types,
            emitted: BTreeMap::new(),
            definitions: vec![],
            visiting: BTreeSet::new(),
            serial: 0,
            checks: BTreeSet::new(),
            checkpoint: false,
            lineage: false,
            created: Vec::new(),
            metadata: SchemaMetadata::default(),
        })
    }
    pub(super) fn with_metadata(mut self, metadata: SchemaMetadata) -> Self {
        self.metadata = metadata;
        self
    }
    fn fresh(&mut self) -> String {
        let n = self.serial;
        self.serial += 1;
        format!("dog_security_generated_{n}")
    }
    pub async fn compile(mut self, stages: &[Stage], write: bool) -> Result<Compiled> {
        self.compile_input(stages, write, &[], false).await
    }
    pub async fn compile_input(
        &mut self,
        stages: &[Stage],
        write: bool,
        inputs: &[(String, String, bool)],
        checkpoint: bool,
    ) -> Result<Compiled> {
        self.checkpoint = checkpoint;
        self.checks.insert("complete".into());
        let mut env = Env::default();
        let mut args = CTX_ARGS.to_string();
        let mut entry = String::new();
        for (name, ty, object) in inputs {
            if name == "dog_security_lineage" {
                ensure!(ty == "integer" && !object, "Invalid lineage input");
                self.lineage = true;
                args.push_str(", $dog_security_lineage: integer");
                continue;
            }
            ensure!(!name.starts_with("dog_security_"), "Reserved input");
            typeql::parse_label(ty)?;
            args.push_str(&format!(", ${name}: {ty}"));
            if *object {
                env.objects.insert(name.clone(), ty.clone());
                entry.push_str(&self.read_guard(name, ty).await?);
            } else {
                env.values.insert(name.clone());
            }
        }
        if !entry.is_empty() {
            entry.insert_str(0, "match ");
        }
        let (body, _) = self.pipeline(stages, env, write, true).await?;
        let mut source = self
            .definitions
            .iter()
            .map(|s| format!("with {s} "))
            .collect::<String>();
        source.push_str(&format!("given {args}; {entry} {body}"));
        typeql::parse_query(&source)?;
        self.options.validate_query(&source)?;
        Ok(Compiled {
            source,
            checks: self.checks.clone(),
        })
    }
    fn pipeline<'s>(
        &'s mut self,
        stages: &'s [Stage],
        mut env: Env,
        write: bool,
        root: bool,
    ) -> BoxFuture<'s, Result<(String, Env)>> {
        Box::pin(async move {
            ensure!(stages.len() <= 32, "Pipeline stage limit exceeded");
            let mut out = String::new();
            let mut mutated = false;
            let mut fetched = false;
            let mut mutation_marker = None;
            for (index, stage) in stages.iter().enumerate() {
                ensure!(!fetched, "Fetch must be terminal");
                if root
                    && write
                    && !mutated
                    && matches!(
                        stage,
                        Stage::Insert(_) | Stage::Put(_) | Stage::Update(_) | Stage::Delete(_)
                    )
                {
                    let marker = self.fresh();
                    out.push_str(&format!("match let ${marker} = true; "));
                    mutation_marker = Some(marker);
                }
                match stage {
                    Stage::Match(m) if !mutated => {
                        out.push_str("match ");
                        out.push_str(&self.block(&m.patterns, &mut env).await?);
                    }
                    Stage::Insert(insert) if write && !mutated && root => {
                        let (guard, insertion) = self.insert(&insert.patterns, &mut env).await?;
                        out.push_str(&format!("match {guard} insert {insertion}"));
                        mutated = true;
                    }
                    Stage::Put(put) if write && !mutated && root => {
                        // Require the complete insertion proposal even when put
                        // finds an existing object. Tenant ownership is injected
                        // into the match-or-insert pattern, never supplied by JSON.
                        let (guard, insertion) = self.insert(&put.patterns, &mut env).await?;
                        out.push_str(&format!("match {guard} put {insertion}"));
                        mutated = true;
                    }
                    Stage::Update(update) if write && !mutated && root => {
                        ensure!(
                            update.patterns.len() == 1,
                            "One ownership update per operation is supported"
                        );
                        let Pattern::Statement(Statement::Thing(thing)) = &update.patterns[0]
                        else {
                            anyhow::bail!("Unsupported update")
                        };
                        let Head::Variable(owner) = &thing.head else {
                            anyhow::bail!("Named update owner required")
                        };
                        let owner = env.require(owner)?;
                        let type_ = env
                            .objects
                            .get(&owner)
                            .ok_or_else(|| anyhow::anyhow!("Object update required"))?
                            .clone();
                        ensure!(
                            thing.constraints.len() == 1,
                            "Combined updates need a combined proposal policy"
                        );
                        let Constraint::Has(has) = &thing.constraints[0] else {
                            anyhow::bail!("Only ownership updates are supported")
                        };
                        let Some(TypeRefAny::Type(TypeRef::Label(attribute))) = &has.type_ else {
                            anyhow::bail!("Explicit attribute required")
                        };
                        let HasValue::Expression(value) = &has.value else {
                            anyhow::bail!("Literal update required")
                        };
                        let value = literal(value)?;
                        self.public_attribute(&attribute.to_string())?;
                        self.immutable_tenant(&type_, &attribute.to_string())
                            .await?;
                        let q = format!("match $t label {type_}; $a label {attribute}; let $f = get_owns_meta('dog.security.update', $t, $a); fetch {{ 'f': $f }};");
                        let f = self.metadata_function(&q, "f").await?;
                        let check = self.fresh();
                        out.push_str(&format!("match let ${check} = {f}({CTX}, ${owner}, {value}); ${check} == true; {stage} "));
                        mutated = true;
                    }
                    Stage::Delete(delete) if write && !mutated && root => {
                        ensure!(
                            !delete.deletables.is_empty() && delete.deletables.len() <= 64,
                            "Delete limit exceeded"
                        );
                        let mut deleted = BTreeSet::new();
                        let mut guard = String::new();
                        for d in &delete.deletables {
                            let DeletableKind::Concept { variable } = &d.kind else {
                                anyhow::bail!("Only whole-object deletes are supported; ownership/link deletion requires a distinct policy")
                            };
                            let owner = env.require(variable)?;
                            let type_ = env
                                .objects
                                .get(&owner)
                                .ok_or_else(|| {
                                    anyhow::anyhow!("Direct attribute deletion is forbidden")
                                })?
                                .clone();
                            let f = self.type_function(&type_, "delete").await?;
                            let check = self.fresh();
                            let incident = self.fresh();
                            // Never silently cascade into incident relations, including
                            // relations which themselves participate in other relations.
                            guard.push_str(&format!(
                                "let ${check} = {f}({CTX}, ${owner}); ${check} == true; "
                            ));
                            // TypeDB rejects an impossible links pattern even
                            // under not. A type with no plays capability cannot
                            // have incident relations, by the schema itself.
                            if !self.plays(&type_)?.is_empty() {
                                guard.push_str(&format!(
                                    "not {{ ${incident} links (${owner}); }}; "
                                ));
                            }
                            let kind = self
                                .ancestry(&type_)?
                                .iter()
                                .find_map(|t| t.kind.as_ref().map(ToString::to_string))
                                .ok_or_else(|| anyhow::anyhow!("Unknown object kind"))?;
                            ensure!(
                                kind == "entity" || kind == "relation",
                                "Direct attribute deletion is forbidden"
                            );
                            if kind == "relation" {
                                let player = self.fresh();
                                let relation_types = self
                                    .ancestry(&type_)?
                                    .iter()
                                    .map(|t| t.label.to_string())
                                    .collect::<BTreeSet<_>>();
                                let mut allowed = Vec::new();
                                for (n, ty) in &env.objects {
                                    if self.plays(ty)?.iter().any(|role| {
                                        role.split(':')
                                            .next()
                                            .is_some_and(|scope| relation_types.contains(scope))
                                    }) {
                                        allowed.push(format!("{{ ${player} is ${n}; }}"));
                                    }
                                }
                                if allowed.is_empty() {
                                    guard.push_str("$dog_security_actor == ''; ");
                                } else {
                                    guard.push_str(&format!(
                                        "not {{ ${owner} links (${player}); not {{ {}; }}; }}; ",
                                        allowed.join(" or ")
                                    ));
                                }
                            }
                            deleted.insert(owner);
                        }
                        out.push_str(&format!("match {guard} {stage} "));
                        for n in deleted {
                            env.objects.remove(&n);
                        }
                        // Values remain scoped immutable literals, never deleted concepts.
                        mutated = true;
                    }
                    Stage::Fetch(fetch) if index + 1 == stages.len() => {
                        let object = self.fetch(&fetch.object, &env).await?;
                        if root {
                            let mut checks = vec![format!(
                                "'complete': {}",
                                mutation_marker
                                    .as_ref()
                                    .map(|n| format!("${n}"))
                                    .unwrap_or_else(|| "true".into())
                            )];
                            if mutated {
                                for (n, type_) in &env.objects {
                                    let f = self.type_function(type_, "read").await?;
                                    let key = self.fresh();
                                    self.checks.insert(key.clone());
                                    checks.push(format!("'{key}': {f}({CTX}, ${n})"));
                                    if let Some(scope) =
                                        self.optional_type_function(type_, "scope").await?
                                    {
                                        if scope == f {
                                            continue;
                                        }
                                        let key = self.fresh();
                                        self.checks.insert(key.clone());
                                        checks.push(format!("'{key}': {scope}({CTX}, ${n})"));
                                    }
                                }
                                for (n, f) in self.created.clone() {
                                    let key = self.fresh();
                                    self.checks.insert(key.clone());
                                    checks.push(format!("'{key}': {f}({CTX}, ${n})"));
                                }
                            }
                            if self.checkpoint {
                                let mut selected = env
                                    .objects
                                    .keys()
                                    .chain(env.values.iter())
                                    .filter(|n| !n.starts_with("dog_security_"))
                                    .map(|n| format!("${n}"))
                                    .collect::<Vec<_>>();
                                selected.push(CTX.into());
                                if self.lineage {
                                    selected.push("$dog_security_lineage".into());
                                }
                                out.push_str("match ");
                                for item in &checks {
                                    let (key, value) = item.split_once(": ").unwrap();
                                    let key = key.trim_matches('\'');
                                    out.push_str(&format!(
                                        "let $dog_security_check_{key} = {value}; "
                                    ));
                                    selected.push(format!("$dog_security_check_{key}"));
                                }
                                out.push_str(&format!("select {};", selected.join(", ")));
                            } else {
                                let lineage = if self.lineage {
                                    "'lineage': $dog_security_lineage, "
                                } else {
                                    ""
                                };
                                out.push_str(&format!(
                                    "fetch {{ {lineage}'checks': {{ {} }}, 'result': {object} }};",
                                    checks.join(", ")
                                ));
                            }
                        } else {
                            out.push_str(&format!("fetch {object};"));
                        }
                        fetched = true;
                    }
                    Stage::Operator(operator) if !mutated => match operator {
                        Operator::Select(select) => {
                            let mut vars = Vec::new();
                            let mut names = BTreeSet::new();
                            for v in &select.variables {
                                names.insert(env.require(v)?);
                                vars.push(v.to_string());
                            }
                            env.objects.retain(|n, _| names.contains(n));
                            env.values.retain(|n| names.contains(n));
                            let lineage = if root && self.lineage {
                                ", $dog_security_lineage"
                            } else {
                                ""
                            };
                            out.push_str(&format!("select {}, {CTX}{lineage};", vars.join(", ")));
                        }
                        Operator::Sort(sort) => {
                            for v in &sort.ordered_variables {
                                env.require(&v.variable)?;
                            }
                            out.push_str(&stage.to_string());
                        }
                        Operator::Limit(_) | Operator::Offset(_) | Operator::Distinct(_) => {
                            out.push_str(&stage.to_string())
                        }
                        _ => anyhow::bail!("Unsupported stream modifier"),
                    },
                    _ => anyhow::bail!("Unsupported stage or write after write/filter after write"),
                }
                out.push(' ');
            }
            ensure!(!write || mutated, "Write method requires a mutation");
            if root && !fetched {
                ensure!(
                    mutated && matches!(stages.last(), Some(Stage::Delete(_))),
                    "Explicit fetch is required"
                );
                let marker =
                    mutation_marker.ok_or_else(|| anyhow::anyhow!("Missing mutation marker"))?;
                out.push_str(&format!("fetch {{ 'checks': {{ 'complete': ${marker} }}, 'result': {{ 'deleted': ${marker} }} }};"));
            }
            Ok((out, env))
        })
    }
    fn block<'s>(
        &'s mut self,
        patterns: &'s [Pattern],
        env: &'s mut Env,
    ) -> BoxFuture<'s, Result<String>> {
        Box::pin(async move {
            ensure!(patterns.len() <= 128, "Pattern limit exceeded");
            let inherited = env.objects.keys().cloned().collect::<BTreeSet<_>>();
            let mut normalized = patterns.to_vec();
            let mut aliases = BTreeMap::new();
            for (index, pattern) in normalized.iter_mut().enumerate() {
                if let Pattern::Statement(Statement::Thing(t)) = pattern {
                    let relation_head = if let Head::Relation(ty, relation) = &t.head {
                        Some((ty.clone(), relation.clone()))
                    } else {
                        None
                    };
                    let anonymous = matches!(
                        t.head,
                        Head::Variable(Variable::Anonymous { optional: None, .. })
                    );
                    if relation_head.is_some() || anonymous {
                        let alias = self.fresh();
                        t.head = Head::Variable(Variable::Named {
                            span: None,
                            ident: alias.clone().into(),
                            optional: None,
                        });
                        aliases.insert(index, alias);
                    }
                    if let Some((ty, relation)) = relation_head {
                        if let Some(ty) = ty {
                            ensure!(
                                !t.constraints
                                    .iter()
                                    .any(|c| matches!(c, Constraint::Isa(_))),
                                "Conflicting relation types"
                            );
                            t.constraints.push(Constraint::Isa(Isa::new(
                                None,
                                IsaKind::Subtype,
                                ty,
                                None,
                            )));
                        }
                        t.constraints
                            .push(Constraint::Links(Links::new(None, relation)));
                    }
                    let mut links = Vec::new();
                    for c in &mut t.constraints {
                        if let Constraint::Isa(isa) = c {
                            if let Some(IsaInstanceConstraint::Relation(relation)) = &isa.constraint
                            {
                                links.push(Constraint::Links(Links::new(None, relation.clone())));
                                isa.constraint = None;
                            }
                        }
                    }
                    t.constraints.extend(links);
                }
            }
            let patterns = &normalized;
            let mut guards = BTreeSet::new();
            // Resolve explicit object types before following role-player edges.
            for (index, pattern) in patterns.iter().enumerate() {
                if let Pattern::Statement(Statement::Thing(t)) = pattern {
                    if let Head::Variable(v) = &t.head {
                        let name = if let Some(alias) = aliases.get(&index) {
                            alias.clone()
                        } else {
                            named(v)?
                        };
                        for c in &t.constraints {
                            if let Constraint::Isa(isa) = c {
                                ensure!(
                                    isa.constraint.is_none(),
                                    "Inline isa values/relations are unsupported; use links"
                                );
                                let ty = label(&isa.type_)?;
                                if isa.kind != IsaKind::Exact {
                                    self.leaf(&ty).await?;
                                }
                                if let Some(old) = env.objects.get(&name) {
                                    if old != &ty {
                                        ensure!(
                                            self.optional_type_function(old, "scope")
                                                .await?
                                                .is_some()
                                                && (self
                                                    .ancestry(&ty)?
                                                    .iter()
                                                    .any(|t| t.label.to_string() == *old)
                                                    || self
                                                        .ancestry(old)?
                                                        .iter()
                                                        .any(|t| t.label.to_string() == ty)),
                                            "Conflicting variable types"
                                        );
                                    }
                                } else {
                                    env.objects.insert(name.clone(), ty.clone());
                                }
                                ensure!(
                                    !env.values.contains(&name),
                                    "Ambiguous value/object variable"
                                );
                                guards.insert(name.clone());
                            }
                        }
                    }
                }
            }
            let mut output = String::new();
            for (index, pattern) in patterns.iter().enumerate() {
                let text = match pattern {
                    Pattern::Statement(Statement::Thing(t)) => {
                        let Head::Variable(v) = &t.head else {
                            anyhow::bail!("Relations must have an explicit named variable and links constraint")
                        };
                        let owner = if let Some(alias) = aliases.get(&index) {
                            alias.clone()
                        } else {
                            env.require(v)?
                        };
                        ensure!(
                            env.objects.contains_key(&owner),
                            "Typed object owner required"
                        );
                        guards.insert(owner.clone());
                        for c in &t.constraints {
                            match c {
                                Constraint::Isa(_) | Constraint::Iid(_) => {}
                                Constraint::Links(links) => {
                                    for player in &links.relation.role_players {
                                        let v = match player {
                                            RolePlayer::Typed(_, v) | RolePlayer::Untyped(v) => v,
                                        };
                                        ensure!(
                                            env.objects.contains_key(&env.require(v)?),
                                            "Role players must be classified objects"
                                        );
                                        guards.insert(named(v)?);
                                    }
                                }
                                Constraint::Has(has) => {
                                    ensure!(
                                        matches!(
                                            has.type_,
                                            Some(TypeRefAny::Type(TypeRef::Label(_)))
                                        ),
                                        "Explicit attribute type required"
                                    );
                                    if let Some(TypeRefAny::Type(TypeRef::Label(a))) = &has.type_ {
                                        self.public_attribute(&a.to_string())?;
                                    }
                                    match &has.value {
                                        HasValue::Variable(v) => {
                                            let n = named(v)?;
                                            ensure!(
                                                !env.objects.contains_key(&n),
                                                "Attribute/object alias forbidden"
                                            );
                                            env.values.insert(n);
                                        }
                                        HasValue::Expression(e) => {
                                            literal(e)?;
                                        }
                                        HasValue::Comparison(c) => {
                                            // This Thing is rendered as AST below: do not
                                            // validate a rewritten call and emit its original.
                                            if let Expression::Variable(v) = &c.rhs {
                                                env.require(v)?;
                                            } else {
                                                literal(&c.rhs)?;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        // Has expressions are restricted to literals above, so rendering
                        // cannot bypass recursive function-call compilation.
                        t.to_string()
                    }
                    Pattern::Statement(Statement::Assignment(a)) => {
                        let AssignmentPattern::Variables(vars) = &a.lhs else {
                            anyhow::bail!("Struct destructuring is not supported")
                        };
                        let rhs = if let Expression::Function(call) = &a.rhs {
                            self.call_binding(call, env, true).await?
                        } else {
                            self.expression(&a.rhs, env).await?
                        };
                        let types = self.expression_types(&a.rhs)?;
                        ensure!(
                            vars.len() == types.len(),
                            "Function assignment arity mismatch"
                        );
                        for (v, ty) in vars.iter().zip(types) {
                            self.bind(env, v, &ty)?;
                        }
                        format!("let {} = {rhs}", a.lhs)
                    }
                    Pattern::Statement(Statement::InIterable(a)) => {
                        let rhs = if let Expression::Function(call) = &a.rhs {
                            self.call_binding(call, env, true).await?
                        } else {
                            self.expression(&a.rhs, env).await?
                        };
                        let types = self.expression_types(&a.rhs)?;
                        ensure!(
                            types.len() == a.lhs.len(),
                            "Stream assignment arity mismatch"
                        );
                        for (v, ty) in a.lhs.iter().zip(types) {
                            self.bind(env, v, &ty)?;
                        }
                        format!(
                            "let {} in {rhs}",
                            a.lhs
                                .iter()
                                .map(ToString::to_string)
                                .collect::<Vec<_>>()
                                .join(", ")
                        )
                    }
                    Pattern::Statement(Statement::Comparison(c)) => format!(
                        "{} {} {}",
                        self.expression(&c.lhs, env).await?,
                        c.comparison.comparator,
                        self.expression(&c.comparison.rhs, env).await?
                    ),
                    Pattern::Statement(Statement::Is(i)) => {
                        env.require(&i.lhs)?;
                        env.require(&i.rhs)?;
                        i.to_string()
                    }
                    Pattern::Conjunction(c) => {
                        format!("{{ {} }}", self.block(&c.patterns, env).await?)
                    }
                    Pattern::Negation(n) => {
                        let mut inner = env.clone();
                        format!("not {{ {} }}", self.block(&n.patterns, &mut inner).await?)
                    }
                    Pattern::Optional(_) => anyhow::bail!(
                        "Optional exports require optional value tracking and are not supported"
                    ),
                    Pattern::Disjunction(d) => {
                        ensure!(
                            !d.branches.is_empty() && d.branches.len() <= 32,
                            "Disjunction limit exceeded"
                        );
                        let mut branches = Vec::new();
                        let mut exports = Vec::new();
                        for b in &d.branches {
                            let mut e = env.clone();
                            branches.push(format!("{{ {} }}", self.block(b, &mut e).await?));
                            exports.push(e);
                        }
                        let mut common = exports.remove(0);
                        for e in exports {
                            common.objects.retain(|n, t| e.objects.get(n) == Some(t));
                            common.values.retain(|n| e.values.contains(n));
                        }
                        *env = common;
                        branches.join(" or ")
                    }
                    _ => anyhow::bail!("Unclassified schema/statement access"),
                };
                output.push_str(&text);
                output.push_str("; ");
            }
            // Function-returned objects need guarding even if no explicit isa
            // statement occurs after the call.
            guards.extend(env.objects.keys().cloned());
            // Parent blocks guard their own bindings; function arguments and
            // checkpoint inputs have entry guards. Repeating those policies in
            // every disjunction branch causes exponential native query plans.
            guards.retain(|name| !inherited.contains(name));
            for n in guards {
                let ty = env
                    .objects
                    .get(&n)
                    .ok_or_else(|| anyhow::anyhow!("Branch-dependent object escape"))?;
                output.push_str(&self.read_guard(&n, ty).await?);
            }
            Ok(output)
        })
    }
    fn bind(&self, env: &mut Env, variable: &Variable, ty: &str) -> Result<()> {
        let n = named(variable)?;
        ensure!(!env.contains(&n), "Rebinding/shadowing is not supported");
        if primitive(ty) {
            env.values.insert(n);
        } else {
            typeql::parse_label(ty)?;
            env.objects.insert(n, ty.into());
        }
        Ok(())
    }
    fn expression_types(&self, expr: &Expression) -> Result<Vec<String>> {
        if let Expression::Function(call) = expr {
            if let Some(f) = self.functions.get(&call.name.to_string()) {
                return Ok(outputs(f));
            }
        }
        Ok(vec!["string".into()]) // Value marker; TypeDB itself checks the actual value type.
    }
    fn expression<'s>(
        &'s mut self,
        expr: &'s Expression,
        env: &'s Env,
    ) -> BoxFuture<'s, Result<String>> {
        Box::pin(async move {
            Ok(match expr {
                Expression::Value(_) => expr.to_string(),
                Expression::Variable(v) => {
                    env.require(v)?;
                    v.to_string()
                }
                Expression::Paren(p) => format!("({})", self.expression(&p.inner, env).await?),
                Expression::Operation(o) => format!(
                    "({} {} {})",
                    self.expression(&o.left, env).await?,
                    o.op,
                    self.expression(&o.right, env).await?
                ),
                Expression::Function(call) => self.call(call, env).await?,
                _ => anyhow::bail!("Unsupported expression"),
            })
        })
    }
    fn call<'s>(
        &'s mut self,
        call: &'s FunctionCall,
        env: &'s Env,
    ) -> BoxFuture<'s, Result<String>> {
        self.call_binding(call, env, false)
    }
    fn call_binding<'s>(
        &'s mut self,
        call: &'s FunctionCall,
        env: &'s Env,
        guarded_binding: bool,
    ) -> BoxFuture<'s, Result<String>> {
        Box::pin(async move {
            let mut args = Vec::new();
            for e in &call.args {
                args.push(self.expression(e, env).await?);
            }
            let name = call.name.to_string();
            if matches!(call.name, FunctionName::Builtin(_)) {
                ensure!(
                    ["iid", "abs", "ceil", "floor", "round", "length"].contains(&name.as_str()),
                    "Non-data builtin is not public"
                );
                return Ok(format!("{name}({})", args.join(", ")));
            }
            let f = self
                .functions
                .get(&name)
                .ok_or_else(|| anyhow::anyhow!("Unknown schema function"))?
                .clone();
            if has_meta(&f.annotations, "dog.security.context", "true")? {
                ensure!(
                    f.signature.args.len() >= 3 && f.signature.args.len() == args.len() + 3,
                    "Context function arity mismatch"
                );
                ensure!(
                    f.signature
                        .args
                        .iter()
                        .take(3)
                        .map(|a| (a.var.name().unwrap_or(""), a.type_.to_string()))
                        .collect::<Vec<_>>()
                        == [
                            ("tenant", "string".into()),
                            ("actor", "string".into()),
                            ("now", "datetime".into())
                        ],
                    "Context function signature mismatch"
                );
                ensure!(
                    guarded_binding || outputs(&f).iter().all(|t| primitive(t)),
                    "Context functions must return values, not unguarded objects"
                );
                let separator = if args.is_empty() { "" } else { ", " };
                return Ok(format!("{name}({CTX}{separator}{})", args.join(", ")));
            }
            ensure!(
                f.signature.args.len() == args.len(),
                "Function arity mismatch"
            );
            let compiled = if let Some(n) = self.emitted.get(&name) {
                n.clone()
            } else {
                ensure!(
                    self.visiting.len() < 16 && self.definitions.len() < 64,
                    "Function compilation limit exceeded"
                );
                ensure!(
                    self.visiting.insert(name.clone()),
                    "Recursive function calls are not supported"
                );
                let generated = self.fresh();
                let mut function_env = Env::default();
                for arg in &f.signature.args {
                    self.bind(&mut function_env, &arg.var, &arg.type_.to_string())?;
                }
                let mut entry = String::from("match $dog_security_tenant != ''; $dog_security_actor != ''; $dog_security_now >= 0001-01-01T00:00:00; ");
                for (variable, ty) in &function_env.objects {
                    entry.push_str(&self.read_guard(variable, ty).await?);
                }
                let (body, final_env) = self
                    .pipeline(&f.block.stages, function_env, false, false)
                    .await?;
                self.validate_return(&f, &final_env)?;
                let arguments = f
                    .signature
                    .args
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ");
                let separator = if arguments.is_empty() { "" } else { ", " };
                let definition = format!(
                    "fun {generated}({CTX_ARGS}{separator}{arguments}) -> {}: {entry} {body} {}",
                    f.signature.output, f.block.return_stmt
                );
                typeql::parse_definition_function(&definition)?;
                self.definitions.push(definition);
                self.visiting.remove(&name);
                self.emitted.insert(name.clone(), generated.clone());
                generated
            };
            let separator = if args.is_empty() { "" } else { ", " };
            Ok(format!("{compiled}({CTX}{separator}{})", args.join(", ")))
        })
    }
    fn validate_return(&self, f: &Function, env: &Env) -> Result<()> {
        match &f.block.return_stmt {
            ReturnStatement::Stream(r) => {
                for v in &r.vars {
                    env.require(v)?;
                }
            }
            ReturnStatement::Single(r) => {
                for v in &r.vars {
                    env.require(v)?;
                }
            }
            ReturnStatement::Reduce(ReturnReduction::Check(_)) => {}
            ReturnStatement::Reduce(ReturnReduction::Value(reducers, _)) => {
                for reducer in reducers {
                    use typeql::query::stage::reduce::Reducer;
                    match reducer {
                        Reducer::Count(c) => {
                            if let Some(v) = &c.variable {
                                env.require(v)?;
                            }
                        }
                        Reducer::Stat(s) => {
                            env.require(&s.variable)?;
                        }
                        _ => anyhow::bail!("Collection return reducers are not yet supported"),
                    }
                }
            }
        }
        Ok(())
    }
    fn fetch<'s>(
        &'s mut self,
        object: &'s FetchObject,
        env: &'s Env,
    ) -> BoxFuture<'s, Result<String>> {
        Box::pin(async move {
            let FetchObjectBody::Entries(entries) = &object.body else {
                anyhow::bail!("Wildcard fetch is not supported")
            };
            let mut fields = Vec::new();
            for entry in entries {
                let value = match &entry.value {
                    FetchSome::Object(o) => self.fetch(o, env).await?,
                    FetchSome::Single(FetchSingle::Attribute(a)) => {
                        self.attribute(a, env)?;
                        a.to_string()
                    }
                    FetchSome::Single(FetchSingle::Expression(e)) => {
                        self.expression(e, env).await?
                    }
                    FetchSome::Single(FetchSingle::FunctionBlock(_)) => {
                        anyhow::bail!("Inline function blocks are not supported")
                    }
                    FetchSome::List(list) => {
                        let body = match &list.stream {
                            FetchStream::Attribute(a) => {
                                self.attribute(a, env)?;
                                a.to_string()
                            }
                            FetchStream::Function(f) => self.call(f, env).await?,
                            FetchStream::SubQueryFetch(stages) => {
                                self.pipeline(stages, env.clone(), false, false).await?.0
                            }
                            _ => anyhow::bail!("Nested function blocks are not supported"),
                        };
                        format!("[ {body} ]")
                    }
                };
                fields.push(format!("{}: {value}", entry.key));
            }
            Ok(format!("{{ {} }}", fields.join(", ")))
        })
    }
    fn attribute(&self, attribute: &FetchAttribute, env: &Env) -> Result<()> {
        ensure!(
            env.objects.contains_key(&env.require(&attribute.owner)?),
            "Fetch owner must be classified"
        );
        ensure!(
            matches!(attribute.attribute, TypeRefAny::Type(TypeRef::Label(_))),
            "Dynamic projection is forbidden"
        );
        if let TypeRefAny::Type(TypeRef::Label(a)) = &attribute.attribute {
            self.public_attribute(&a.to_string())?;
        }
        Ok(())
    }
    fn public_attribute(&self, attribute: &str) -> Result<()> {
        for ty in self.ancestry(attribute)? {
            ensure!(
                !has_meta(&ty.annotations, "dog.security.hidden", "true")?,
                "Private attribute is not public"
            );
        }
        Ok(())
    }
    async fn type_function(&self, type_: &str, operation: &str) -> Result<String> {
        // Absent permission is an intentional deny-by-default decision. Invalid
        // declared policies still fail as server errors; do not mask corruption.
        self.optional_type_function(type_, operation)
            .await?
            .ok_or_else(|| super::PolicyDenied.into())
    }
    async fn optional_type_function(&self, type_: &str, operation: &str) -> Result<Option<String>> {
        let key = format!("dog.security.{operation}");
        let mut declared = false;
        for ty in self.ancestry(type_)? {
            for a in &ty.annotations {
                if let typeql::Annotation::Meta(m) = a {
                    if m.key.unescape()? == key {
                        declared = true;
                    }
                }
            }
        }
        if !declared {
            return Ok(None);
        }
        let q = format!("match $t label {type_}; let $f = get_meta('dog.security.{operation}', $t); fetch {{ 'f': $f }};");
        let rows = self.rows(&q).await?;
        ensure!(
            !rows.is_empty(),
            "Declared schema policy metadata is missing"
        );
        Ok(Some(self.metadata_function(&q, "f").await?))
    }
    async fn metadata_function(&self, query: &str, key: &str) -> Result<String> {
        let rows = self.rows(query).await?;
        ensure!(rows.len() == 1, "Missing or ambiguous schema policy");
        let row = &rows[0]["data"];
        let s = row[key]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing schema policy"))?;
        typeql::parse_label(s)?;
        if key == "f" {
            let f = self
                .functions
                .get(s)
                .ok_or_else(|| anyhow::anyhow!("Missing policy function definition"))?;
            ensure!(outputs(f) == ["boolean"], "Policy must return one boolean");
            ensure!(matches!(f.block.return_stmt, ReturnStatement::Reduce(ReturnReduction::Check(_))), "Authorization policies must use return check so denial cannot erase a post-write result row");
            let signature = f
                .signature
                .args
                .iter()
                .take(3)
                .map(|a| (a.var.name().unwrap_or(""), a.type_.to_string()))
                .collect::<Vec<_>>();
            ensure!(
                signature
                    == [
                        ("tenant", "string".into()),
                        ("actor", "string".into()),
                        ("now", "datetime".into())
                    ],
                "Policy context signature mismatch"
            );
        }
        Ok(s.into())
    }
    async fn tenant_attribute(&self, type_: &str) -> Result<String> {
        let q = format!("match $t label {type_}; let $attribute = get_meta('dog.security.tenant', $t); fetch {{ 'attribute': $attribute }};");
        self.metadata_function(&q, "attribute").await
    }
    async fn read_guard(&mut self, variable: &str, type_: &str) -> Result<String> {
        let f = self.type_function(type_, "read").await?;
        if let Some(scope) = self.optional_type_function(type_, "scope").await? {
            let bound = self.fresh();
            if scope == f {
                return Ok(format!("${variable} isa {type_}; let ${bound} = {scope}({CTX}, ${variable}); ${bound} == true; "));
            }
            let allow = self.fresh();
            return Ok(format!("${variable} isa {type_}; let ${bound} = {scope}({CTX}, ${variable}); ${bound} == true; let ${allow} = {f}({CTX}, ${variable}); ${allow} == true; "));
        }
        let tenant = self.tenant_attribute(type_).await?;
        let other = self.fresh();
        let allow = self.fresh();
        Ok(format!("${variable} isa! {type_}; ${variable} has {tenant} == $dog_security_tenant; not {{ ${variable} has {tenant} ${other}; ${other} != $dog_security_tenant; }}; let ${allow} = {f}({CTX}, ${variable}); ${allow} == true; "))
    }
    async fn leaf(&self, ty: &str) -> Result<()> {
        // A schema-author-defined relation scope explicitly governs the whole
        // declared type, including its subtypes. Attribute-only policies retain
        // exact-leaf restrictions.
        if self.optional_type_function(ty, "scope").await?.is_some() {
            return Ok(());
        }
        let q = format!("match $t sub! {ty}; fetch {{ 'child': label($t) }};");
        ensure!(
            self.rows(&q).await?.is_empty(),
            "Subtype query requires explicit concrete isa! when subtypes exist"
        );
        Ok(())
    }
    fn ancestry(&self, ty: &str) -> Result<Vec<&SchemaType>> {
        let mut out = Vec::new();
        let mut next = ty.to_owned();
        let mut seen = BTreeSet::new();
        loop {
            ensure!(
                seen.insert(next.clone()) && seen.len() <= 128,
                "Invalid schema ancestry"
            );
            let t = self
                .types
                .get(&next)
                .ok_or_else(|| anyhow::anyhow!("Missing schema type"))?;
            out.push(t);
            if let Some(parent) = t.capabilities.iter().find_map(|c| match &c.base {
                CapabilityBase::Sub(s) => Some(s.supertype_label.to_string()),
                _ => None,
            }) {
                next = parent;
            } else {
                break;
            }
        }
        Ok(out)
    }
    fn plays(&self, ty: &str) -> Result<BTreeSet<String>> {
        Ok(self
            .ancestry(ty)?
            .iter()
            .flat_map(|t| &t.capabilities)
            .filter_map(|c| match &c.base {
                CapabilityBase::Plays(p) => Some(p.role.to_string()),
                _ => None,
            })
            .collect())
    }
    async fn rows(&self, q: &str) -> Result<Vec<serde_json::Value>> {
        if let Some(rows) = self.metadata.get(q)? {
            return Ok(rows);
        }
        #[cfg(test)]
        {
            self.metadata.0.lock().unwrap().fetches += 1;
        }
        let result = crate::transactions::typedb_answer_to_http_ok(
            self.tx.query(q).await?,
            "read",
            "",
            self.options,
        )
        .await?;
        let rows = result["ok"]["answers"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("Invalid metadata response"))?
            .clone();
        self.metadata
            .insert(q, &rows, self.options.max_response_bytes)?;
        Ok(rows)
    }
    async fn immutable_tenant(&self, ty: &str, attribute: &str) -> Result<()> {
        if self.optional_type_function(ty, "scope").await?.is_some() {
            // Relation ownership cannot be reassigned by an attribute update.
            // Every update still requires its own native proposal policy and
            // mandatory scope/read checks before commit.
            return Ok(());
        }
        let tenant = self.tenant_attribute(ty).await?;
        ensure!(tenant != attribute, "Tenant ownership is immutable");
        let q = format!("match $a label {attribute}; $t label {tenant}; {{ $a sub $t; }} or {{ $t sub $a; }}; fetch {{ 'overlap': label($a) }};");
        ensure!(
            self.rows(&q).await?.is_empty(),
            "Tenant ownership hierarchy is immutable"
        );
        Ok(())
    }
    async fn insert(&mut self, patterns: &[Pattern], env: &mut Env) -> Result<(String, String)> {
        ensure!(
            !patterns.is_empty() && patterns.len() <= 64,
            "Insert limit exceeded"
        );
        let existing = env.clone();
        if self.state_creation(patterns).await? {
            return self.insert_graph(patterns, env).await;
        }
        let mut guards = String::new();
        let mut writes = String::new();
        for pattern in patterns {
            let Pattern::Statement(Statement::Thing(t)) = pattern else {
                anyhow::bail!("Only typed object inserts are supported")
            };
            let Head::Variable(v) = &t.head else {
                anyhow::bail!("Inserted objects must be named")
            };
            let owner = named(v)?;
            ensure!(
                !env.contains(&owner),
                "Insert must create a new named object"
            );
            let mut ty = None;
            let mut proposals = BTreeMap::new();
            let mut attributes = BTreeSet::new();
            for c in &t.constraints {
                match c {
                    Constraint::Isa(isa) => {
                        ensure!(
                            ty.is_none() && isa.constraint.is_none(),
                            "Explicit insert type required"
                        );
                        ty = Some(label(&isa.type_)?);
                    }
                    Constraint::Has(has) => {
                        let Some(TypeRefAny::Type(TypeRef::Label(a))) = &has.type_ else {
                            anyhow::bail!("Explicit inserted attribute required")
                        };
                        self.public_attribute(&a.to_string())?;
                        let value = match &has.value {
                            HasValue::Expression(e) => literal(e)?,
                            HasValue::Variable(v) => {
                                let n = existing.require(v)?;
                                ensure!(
                                    existing.values.contains(&n),
                                    "Inserted attribute must be an owned value"
                                );
                                format!("${n}")
                            }
                            _ => anyhow::bail!("Unsupported insert value"),
                        };
                        ensure!(
                            proposals.insert(a.to_string(), value).is_none(),
                            "Repeated insert fields need a collection proposal policy"
                        );
                        attributes.insert(a.to_string());
                    }
                    Constraint::Links(links) => {
                        for player in &links.relation.role_players {
                            let RolePlayer::Typed(TypeRefAny::Type(TypeRef::Label(role)), v) =
                                player
                            else {
                                anyhow::bail!("Inserted roles must be explicit and unscoped")
                            };
                            let n = existing.require(v)?;
                            ensure!(
                                existing.objects.contains_key(&n),
                                "Inserted role players must already be guarded"
                            );
                            ensure!(
                                proposals
                                    .insert(role.to_string(), format!("${n}"))
                                    .is_none(),
                                "Repeated roles need a collection proposal policy"
                            );
                        }
                    }
                    _ => anyhow::bail!("Unsupported insert constraint"),
                }
            }
            let ty = ty.ok_or_else(|| anyhow::anyhow!("Missing insert type"))?;
            let tenant = self.tenant_attribute(&ty).await?;
            ensure!(
                !proposals.contains_key(&tenant),
                "The server assigns tenant ownership"
            );
            for key in &attributes {
                self.immutable_tenant(&ty, key).await?;
            }
            let f_name = self.type_function(&ty, "insert").await?;
            let f = self
                .functions
                .get(&f_name)
                .ok_or_else(|| anyhow::anyhow!("Missing insert policy definition"))?;
            ensure!(
                f.signature.args.len() == 3 + proposals.len(),
                "Insert proposal must cover every field and role"
            );
            let mut args = Vec::new();
            for arg in f.signature.args.iter().skip(3) {
                let name = named(&arg.var)?;
                args.push(
                    proposals
                        .remove(&name)
                        .ok_or_else(|| anyhow::anyhow!("Missing or unmatched proposal field"))?,
                );
            }
            ensure!(proposals.is_empty(), "Unvalidated insert proposal field");
            let check = self.fresh();
            let comma = if args.is_empty() { "" } else { ", " };
            guards.push_str(&format!(
                "let ${check} = {f_name}({CTX}{comma}{}); ${check} == true; ",
                args.join(", ")
            ));
            // Tenant attribute is an AST-generated constraint, not caller text.
            let mut inserted = t.clone();
            let q = typeql::parse_query(&format!(
                "match ${owner} has {tenant} == $dog_security_tenant;"
            ))?;
            let QueryStructure::Pipeline(p) = q.structure else {
                unreachable!()
            };
            let Stage::Match(m) = &p.stages[0] else {
                unreachable!()
            };
            let Pattern::Statement(Statement::Thing(extra)) = &m.patterns[0] else {
                unreachable!()
            };
            inserted.constraints.extend(extra.constraints.clone());
            writes.push_str(&format!("{inserted}; "));
            env.objects.insert(owner, ty);
        }
        Ok((guards, writes))
    }

    async fn state_creation(&self, patterns: &[Pattern]) -> Result<bool> {
        for pattern in patterns {
            if let Pattern::Statement(Statement::Thing(t)) = pattern {
                let ty = t
                    .constraints
                    .iter()
                    .find_map(|c| match c {
                        Constraint::Isa(isa) => Some(label(&isa.type_)),
                        _ => None,
                    })
                    .transpose()?;
                let inline = match &t.head {
                    Head::Relation(Some(ty), _) => Some(label(ty)?),
                    _ => None,
                };
                if let Some(ty) = ty.or(inline) {
                    if self.optional_type_function(&ty, "create").await?.is_some() {
                        return Ok(true);
                    }
                }
            }
        }
        Ok(false)
    }

    /// All new objects are classified first, then all roles are validated.
    /// State-based policies inspect the COMPLETE graph after the native insert/
    /// put, but before commit. No preflight query attempts to bind unborn objects.
    async fn insert_graph(
        &mut self,
        patterns: &[Pattern],
        env: &mut Env,
    ) -> Result<(String, String)> {
        let mut normalized = patterns.to_vec();
        let mut new = BTreeMap::new();
        for pattern in &mut normalized {
            let Pattern::Statement(Statement::Thing(t)) = pattern else {
                anyhow::bail!("Typed graph objects required")
            };
            if let Head::Variable(v) = &t.head {
                named(v)?;
            }
            if let Head::Relation(ty, relation) = t.head.clone() {
                let alias = self.fresh();
                t.head = Head::Variable(Variable::Named {
                    span: None,
                    ident: alias.into(),
                    optional: None,
                });
                t.constraints.push(Constraint::Links(Links {
                    span: None,
                    relation,
                }));
                if let Some(ty) = ty {
                    t.constraints
                        .push(Constraint::Isa(Isa::new(None, IsaKind::Subtype, ty, None)));
                }
            }
            let Head::Variable(v) = &t.head else {
                anyhow::bail!("Named graph object required")
            };
            let owner = v
                .name()
                .ok_or_else(|| anyhow::anyhow!("Named graph object required"))?
                .to_string();
            ensure!(
                !env.contains(&owner) && !new.contains_key(&owner),
                "Graph object already bound or repeated"
            );
            let types = t
                .constraints
                .iter()
                .filter_map(|c| match c {
                    Constraint::Isa(isa) => Some(isa),
                    _ => None,
                })
                .collect::<Vec<_>>();
            ensure!(
                types.len() == 1 && types[0].constraint.is_none(),
                "One explicit graph type required"
            );
            let ty = label(&types[0].type_)?;
            let create = self.type_function(&ty, "create").await?;
            ensure!(
                self.optional_type_function(&ty, "scope").await?.is_some(),
                "State creation requires explicit relation scope policy"
            );
            self.created.push((owner.clone(), create));
            new.insert(owner, ty);
        }
        for pattern in &normalized {
            let Pattern::Statement(Statement::Thing(t)) = pattern else {
                unreachable!()
            };
            for c in &t.constraints {
                match c {
                    Constraint::Isa(_) => {}
                    Constraint::Has(has) => {
                        ensure!(
                            matches!(has.type_, Some(TypeRefAny::Type(TypeRef::Label(_)))),
                            "Explicit graph attribute required"
                        );
                        if let Some(TypeRefAny::Type(TypeRef::Label(a))) = &has.type_ {
                            self.public_attribute(&a.to_string())?;
                        }
                        match &has.value {
                            HasValue::Expression(e) => {
                                literal(e)?;
                            }
                            HasValue::Variable(v) => {
                                let n = env.require(v)?;
                                ensure!(
                                    env.values.contains(&n),
                                    "Graph attribute requires bound value"
                                );
                            }
                            HasValue::Comparison(c) if c.comparator.to_string() == "==" => {
                                match &c.rhs {
                                    Expression::Variable(v) => {
                                        let n = env.require(v)?;
                                        ensure!(
                                            env.values.contains(&n),
                                            "Graph attribute requires bound value"
                                        );
                                    }
                                    value => {
                                        literal(value)?;
                                    }
                                }
                            }
                            _ => anyhow::bail!("Unsupported graph attribute"),
                        }
                    }
                    Constraint::Links(links) => {
                        for role in &links.relation.role_players {
                            let RolePlayer::Typed(TypeRefAny::Type(TypeRef::Label(_)), v) = role
                            else {
                                anyhow::bail!("Explicit graph roles required")
                            };
                            let n = named(v)?;
                            ensure!(
                                new.contains_key(&n) || env.objects.contains_key(&n),
                                "Unbound graph role player"
                            );
                        }
                    }
                    _ => anyhow::bail!("Unsupported graph constraint"),
                }
            }
        }
        env.objects.extend(new);
        Ok((
            "$dog_security_actor != ''; ".into(),
            normalized.iter().map(|p| format!("{p}; ")).collect(),
        ))
    }
}
fn has_meta(annotations: &[typeql::Annotation], key: &str, value: &str) -> Result<bool> {
    for annotation in annotations {
        if let typeql::Annotation::Meta(meta) = annotation {
            if meta.key.unescape()? == key && meta.value.unescape()? == value {
                return Ok(true);
            }
        }
    }
    Ok(false)
}
fn primitive(ty: &str) -> bool {
    [
        "string",
        "integer",
        "double",
        "boolean",
        "date",
        "datetime",
        "datetime-tz",
        "decimal",
        "duration",
    ]
    .contains(&ty)
}
fn outputs(f: &Function) -> Vec<String> {
    match &f.signature.output {
        Output::Single(s) => s.types.iter().map(ToString::to_string).collect(),
        Output::Stream(s) => s.types.iter().map(ToString::to_string).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn compiled() -> Compiled {
        Compiled {
            source: String::new(),
            checks: ["complete".into(), "read".into()].into_iter().collect(),
        }
    }
    #[test]
    fn mandatory_checks_cannot_be_spoofed_by_projected_data() {
        for checks in [
            json!({"complete":true}),
            json!({"complete":true,"read":false}),
            json!({"complete":true,"read":"true"}),
            json!({"complete":true,"read":true,"extra":true}),
        ] {
            let mut result = json!({"ok":{"answers":[{"data":{"checks":checks,"result":{"checks":{"read":true},"secret":"not-returned"}}}]}});
            assert!(compiled().check_result(&mut result).is_err());
        }
    }
    #[test]
    fn successful_checks_are_removed_without_changing_the_projection() {
        let mut result = json!({"ok":{"answers":[{"data":{"checks":{"complete":true,"read":true},"result":{"title":"allowed"}}}]}});
        compiled().check_result(&mut result).unwrap();
        assert_eq!(
            result["ok"]["answers"][0]["data"],
            json!({"title":"allowed"})
        );
    }
    #[test]
    fn every_row_is_checked_not_just_the_first() {
        let mut result = json!({"ok":{"answers":[
            {"data":{"checks":{"complete":true,"read":true},"result":{"id":1}}},
            {"data":{"checks":{"complete":true,"read":false},"result":{"id":2}}}
        ]}});
        assert!(compiled().check_result(&mut result).is_err());
    }
}
