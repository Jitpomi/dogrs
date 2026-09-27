//! Implementation of `dog_schema::schema`. See dog-schema for the public API.
use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{ext::IdentExt, parse_macro_input, spanned::Spanned, ItemMod, LitStr, Meta};

struct SchemaArgs {
    service: LitStr,
    message: LitStr,
    backend: String,
}
impl syn::parse::Parse for SchemaArgs {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let mut service = None;
        let mut message = None;
        let mut backend = None;
        for meta in input.parse_terminated(Meta::parse, syn::Token![,])? {
            let Meta::NameValue(nv) = &meta else {
                return Err(syn::Error::new_spanned(meta, "expected key = string"));
            };
            let slot = if nv.path.is_ident("service") {
                &mut service
            } else if nv.path.is_ident("error_message") {
                &mut message
            } else if nv.path.is_ident("backend") {
                &mut backend
            } else {
                return Err(syn::Error::new_spanned(meta, "unknown schema argument"));
            };
            if slot.is_some() {
                return Err(syn::Error::new_spanned(meta, "duplicate schema argument"));
            }
            let value = &nv.value;
            *slot = Some(syn::parse2::<LitStr>(quote!(#value))?);
        }
        let service = service.ok_or_else(|| input.error("schema requires service = \"name\""))?;
        if service.value().trim().is_empty() {
            return Err(syn::Error::new_spanned(
                service,
                "service must not be empty",
            ));
        }
        let backend = backend.unwrap_or_else(|| LitStr::new("built_in", service.span()));
        if !matches!(backend.value().as_str(), "built_in" | "validator") {
            return Err(syn::Error::new_spanned(
                backend,
                "backend must be built_in or validator",
            ));
        }
        Ok(Self {
            message: message
                .unwrap_or_else(|| LitStr::new("Schema validation failed", service.span())),
            service,
            backend: backend.value(),
        })
    }
}

#[proc_macro_attribute]
pub fn schema(args: TokenStream, item: TokenStream) -> TokenStream {
    let args = parse_macro_input!(args as SchemaArgs);
    let module = parse_macro_input!(item as ItemMod);
    expand(args, module)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

struct Field {
    key: String,
    ty: syn::Type,
    string: bool,
    optional: bool,
    trim: bool,
    min: Option<usize>,
    max: Option<usize>,
    default: Option<bool>,
}
fn inner_type(ty: &syn::Type) -> &syn::Type {
    if let syn::Type::Path(p) = ty {
        if let Some(seg) = p.path.segments.last() {
            if seg.ident == "Option" {
                if let syn::PathArguments::AngleBracketed(args) = &seg.arguments {
                    if let Some(syn::GenericArgument::Type(inner)) = args.args.first() {
                        return inner;
                    }
                }
            }
        }
    }
    ty
}
fn named(ty: &syn::Type, name: &str) -> bool {
    matches!(ty, syn::Type::Path(p) if p.path.segments.last().is_some_and(|s| s.ident == name))
}
fn fields(st: &syn::ItemStruct, backend: &str) -> syn::Result<Vec<Field>> {
    if st
        .attrs
        .iter()
        .chain(st.fields.iter().flat_map(|f| f.attrs.iter()))
        .any(|a| a.path().is_ident("cfg") || a.path().is_ident("cfg_attr"))
    {
        return Err(syn::Error::new_spanned(st, "conditional schema structs or fields are unsupported; put cfg on the entire schema module"));
    }
    if !st.generics.params.is_empty() || st.generics.where_clause.is_some() {
        return Err(syn::Error::new_spanned(
            &st.generics,
            "schema structs must not be generic",
        ));
    }
    let syn::Fields::Named(fields) = &st.fields else {
        return Err(syn::Error::new_spanned(st, "schema requires named fields"));
    };
    if backend == "built_in" && st.attrs.iter().any(|a| a.path().is_ident("serde")) {
        return Err(syn::Error::new_spanned(
            st,
            "serde attributes on schema structs require backend = \"validator\"",
        ));
    }
    fields.named.iter().map(|f| {
        let ty = inner_type(&f.ty);
        let mut rule = Field {
            key: f.ident.as_ref().unwrap().unraw().to_string(), ty: f.ty.clone(),
            string: named(ty, "String"), optional: named(&f.ty, "Option"),
            trim: false, min: None, max: None, default: None,
        };
        let mut seen = std::collections::HashSet::new();
        for attr in &f.attrs {
            if backend == "built_in" && (attr.path().is_ident("serde") || attr.path().is_ident("validate")) {
                return Err(syn::Error::new_spanned(attr, "serde/validate attributes require backend = \"validator\""));
            }
            if !attr.path().is_ident("dog") { continue; }
            if backend == "validator" {
                return Err(syn::Error::new_spanned(attr, "use serde and validate attributes with the validator backend; dog rules are built_in only"));
            }
            attr.parse_nested_meta(|meta| {
                let Some(key) = meta.path.get_ident().map(ToString::to_string) else { return Err(meta.error("unknown dog rule")); };
                if !seen.insert(key.clone()) { return Err(meta.error("duplicate dog rule")); }
                match key.as_str() {
                    "trim" => rule.trim = true,
                    "optional" => rule.optional = true,
                    "min_len" | "max_len" => {
                        let content; syn::parenthesized!(content in meta.input);
                        let n = content.parse::<syn::LitInt>()?.base10_parse::<usize>()?;
                        if !content.is_empty() { return Err(content.error("expected one non-negative integer")); }
                        if key == "min_len" { rule.min = Some(n); } else { rule.max = Some(n); }
                    }
                    "default" => rule.default = Some(meta.value()?.parse::<syn::LitBool>()?.value),
                    _ => return Err(meta.error("unknown dog rule; supported: trim, optional, min_len, max_len, default")),
                }
                Ok(())
            })?;
        }
        if (rule.trim || rule.min.is_some() || rule.max.is_some()) && !rule.string {
            return Err(syn::Error::new_spanned(f, "trim and length rules require String or Option<String>"));
        }
        if rule.default.is_some() && !named(ty, "bool") {
            return Err(syn::Error::new_spanned(f, "boolean defaults require bool or Option<bool>"));
        }
        if matches!((rule.min, rule.max), (Some(min), Some(max)) if min > max) {
            return Err(syn::Error::new_spanned(f, "min_len must not exceed max_len"));
        }
        Ok(rule)
    }).collect()
}

fn expand(args: SchemaArgs, mut module: ItemMod) -> syn::Result<proc_macro2::TokenStream> {
    let span = module.span();
    let (_, items) = module
        .content
        .as_mut()
        .ok_or_else(|| syn::Error::new(span, "schema requires an inline module"))?;
    let mut create = None;
    let mut patch = None;
    for item in items.iter_mut() {
        if let syn::Item::Struct(st) = item {
            let markers: Vec<_> = st
                .attrs
                .iter()
                .filter(|a| a.path().is_ident("create") || a.path().is_ident("patch"))
                .collect();
            if markers.len() > 1 {
                return Err(syn::Error::new_spanned(st, "one schema marker per struct"));
            }
            let Some(marker) = markers.first() else {
                if st
                    .fields
                    .iter()
                    .any(|f| f.attrs.iter().any(|a| a.path().is_ident("dog")))
                {
                    return Err(syn::Error::new_spanned(
                        st,
                        "dog rules require a create or patch struct",
                    ));
                }
                continue;
            };
            if !matches!(marker.meta, Meta::Path(_)) {
                return Err(syn::Error::new_spanned(marker, "marker takes no arguments"));
            }
            let target = if marker.path().is_ident("create") {
                &mut create
            } else {
                &mut patch
            };
            if target.is_some() {
                return Err(syn::Error::new_spanned(st, "duplicate schema struct"));
            }
            *target = Some((st.ident.clone(), fields(st, &args.backend)?));
            st.attrs
                .retain(|a| !a.path().is_ident("create") && !a.path().is_ident("patch"));
            st.attrs.push(syn::parse_quote!(#[allow(dead_code)]));
            for f in &mut st.fields {
                f.attrs.retain(|a| !a.path().is_ident("dog"));
            }
        }
    }
    let (create_ident, create_fields) =
        create.ok_or_else(|| syn::Error::new(span, "schema requires a create struct"))?;
    let mut generated = vec![
        resolve("resolve_create", &create_fields, &args.message, true),
        validate(
            "validate_create",
            &create_fields,
            &args,
            &create_ident,
            false,
        ),
    ];
    let patch_hooks = if let Some((ident, rules)) = patch {
        generated.push(resolve("resolve_patch", &rules, &args.message, false));
        generated.push(validate("validate_patch", &rules, &args, &ident, true));
        quote! { s.on_patch().resolve(resolve_patch); s.on_patch().validate(validate_patch); }
    } else {
        // A schema that only describes complete writes cannot validate a patch.
        quote! { s.on_patch().validate(|_, _| Err(dog_schema::schema_error("Schema validation failed", "PATCH requires a patch schema"))); }
    };
    let service = args.service;
    generated.push(quote! {
        pub fn register<P>(builder: &mut dog_core::DogAppBuilder<serde_json::Value, P>) -> anyhow::Result<()>
        where P: Send + Clone + 'static {
            use dog_schema::SchemaHooksExt;
            builder.service_hooks(#service, |h| { h.schema(|s| {
                s.on_create().resolve(resolve_create);
                s.on_create().validate(validate_create);
                s.on_update().resolve(resolve_create);
                s.on_update().validate(validate_create);
                #patch_hooks
            }); });
            Ok(())
        }
    });
    for generated in generated {
        items.push(syn::parse2(generated)?);
    }
    Ok(quote!(#module))
}
fn resolve(
    name: &str,
    fields: &[Field],
    message: &LitStr,
    defaults: bool,
) -> proc_macro2::TokenStream {
    let name = format_ident!("{name}");
    let rules = fields.iter().map(|f| {
        let key = &f.key;
        let trim = f.trim.then(|| quote! { if let Some(serde_json::Value::String(s)) = obj.get_mut(#key) { *s = s.trim().to_owned(); } });
        let default = f.default.filter(|_| defaults).map(|value| quote! { obj.entry(#key).or_insert(serde_json::Value::Bool(#value)); });
        quote! { #trim #default }
    });
    quote! {
        pub fn #name<P>(data: &mut serde_json::Value, _meta: &dog_schema::HookMeta<serde_json::Value, P>) -> anyhow::Result<()>
        where P: Send + Clone + 'static {
            let obj = data.as_object_mut().ok_or_else(|| dog_schema::schema_error(#message, "expected JSON object"))?;
            #(#rules)*
            Ok(())
        }
    }
}
fn validate(
    name: &str,
    fields: &[Field],
    args: &SchemaArgs,
    ident: &syn::Ident,
    patch: bool,
) -> proc_macro2::TokenStream {
    let name = format_ident!("{name}");
    let message = &args.message;
    let body = if args.backend == "validator" {
        quote! { dog_schema_validator::validate::<#ident>(data, #message)?; }
    } else {
        let keys: Vec<_> = fields.iter().map(|f| &f.key).collect();
        let checks = fields.iter().map(|f| {
            let key = &f.key;
            let ty = &f.ty;
            let missing_allowed = patch || f.optional;
            let min = f.min.map(|n| quote! { if text.chars().count() < #n { errs.push_field(#key, format!("must be at least {} chars", #n)); } });
            let max = f.max.map(|n| quote! { if text.chars().count() > #n { errs.push_field(#key, format!("must be at most {} chars", #n)); } });
            let string_rules = f.string.then(|| quote! {
                if let Some(text) = value.as_str() {
                    if text.trim().is_empty() { errs.push_field(#key, "must not be empty"); }
                    #min #max
                }
            });
            quote! {
                match obj.get(#key) {
                    None if !#missing_allowed => errs.push_field(#key, "is required"),
                    Some(value) => {
                        if serde_json::from_value::<#ty>(value.clone()).is_err() {
                            // Do not echo user input or arbitrary custom deserializer errors.
                            errs.push_field(#key, "invalid type or value");
                        } else { #string_rules }
                    }
                    _ => {}
                }
            }
        });
        quote! {
            let obj = data.as_object().ok_or_else(|| dog_schema::schema_error(#message, "expected JSON object"))?;
            let mut errs = dog_schema::SchemaErrors::new();
            let allowed: &[&str] = &[#(#keys),*];
            for key in obj.keys() { if !allowed.contains(&key.as_str()) { errs.push_field(key, "unknown field"); } }
            #(#checks)*
            if !errs.is_empty() { return Err(errs.into_unprocessable_anyhow(#message)); }
        }
    };
    quote! {
        pub fn #name<P>(data: &serde_json::Value, _meta: &dog_schema::HookMeta<serde_json::Value, P>) -> anyhow::Result<()>
        where P: Send + Clone + 'static { #body Ok(()) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn args() -> SchemaArgs {
        syn::parse_str("service = \"test\"").unwrap()
    }
    #[test]
    fn rejects_invalid_arguments() {
        for input in [
            "service = \"x\", backend = \"typo\"",
            "service = \"x\", servcie = \"y\"",
            "service = false",
            "service = \"\"",
            "service = \"x\", service = \"y\"",
            "service(\"x\")",
            "",
        ] {
            assert!(syn::parse_str::<SchemaArgs>(input).is_err(), "{input}");
        }
    }
    #[test]
    fn rejects_invalid_or_ignored_field_rules() {
        for attr in [
            "min_lne(3)",
            "min_len(\"3\")",
            "min_len(-1)",
            "min_len(1,2)",
            "trim = true",
            "default = 1",
            "default = false",
            "relation = \"users\"",
            "trim, trim",
            "min_len(5), max_len(1)",
        ] {
            let module = syn::parse_str(&format!(
                "mod example {{ #[create] struct Create {{ #[dog({attr})] name: String }} }}"
            ))
            .unwrap();
            assert!(expand(args(), module).is_err(), "{attr}");
        }
        let module = syn::parse_quote!(
            mod example {
                #[create]
                struct Create {
                    #[dog(trim)]
                    count: u8,
                }
            }
        );
        assert!(expand(args(), module).is_err());
    }
    #[test]
    fn rejects_ambiguous_structures() {
        for input in [
            "mod x;",
            "mod x { #[create] struct X(String); }",
            "mod x { #[create] struct X<T> { value: T } }",
            "mod x { #[create] #[patch] struct X { name: String } }",
            "mod x { #[create] struct X {} #[create] struct Y {} }",
            "mod x { #[create(foo)] struct X {} }",
            "mod x { #[create] struct X { #[cfg(feature=\"x\")] value: String } }",
            "mod x { #[create] struct X {} struct Y { #[dog(trim)] name: String } }",
            "mod x { #[create] #[serde(rename_all=\"camelCase\")] struct X { first_name: String } }",
            "mod x { #[create] struct X { #[serde(rename=\"name\")] other: String } }",
        ] { assert!(expand(args(), syn::parse_str(input).unwrap()).is_err(), "{input}"); }
    }
    #[test]
    fn validator_backend_rejects_ignored_dog_rules() {
        let args = syn::parse_str("service = \"x\", backend = \"validator\"").unwrap();
        assert!(expand(
            args,
            syn::parse_quote!(
                mod x {
                    #[create]
                    struct X {
                        #[dog(trim)]
                        name: String,
                    }
                }
            )
        )
        .is_err());
    }
}
