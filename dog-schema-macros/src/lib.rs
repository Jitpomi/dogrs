use proc_macro::TokenStream;
use quote::quote;
use syn::{
    parse_macro_input, spanned::Spanned, Attribute, Expr, ExprLit, ItemMod, Lit, LitBool, LitStr,
    Meta,
};

// ---------------------------------------------------------------------------
// Top-level attribute args parser  (#[schema(service = "...", ...)])
// ---------------------------------------------------------------------------
struct SchemaArgs {
    service: Option<LitStr>,
    error_message: Option<LitStr>,
    backend: Option<LitStr>,
}

impl syn::parse::Parse for SchemaArgs {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let mut service = None;
        let mut error_message = None;
        let mut backend = Option::None;

        let metas =
            syn::punctuated::Punctuated::<Meta, syn::Token![,]>::parse_terminated(input)?;
        for meta in metas {
            if let Meta::NameValue(nv) = meta {
                let key = nv.path.get_ident().map(|i| i.to_string()).unwrap_or_default();
                if let Expr::Lit(ExprLit { lit: Lit::Str(s), .. }) = nv.value {
                    match key.as_str() {
                        "service"       => service       = Some(s),
                        "error_message" => error_message = Some(s),
                        "backend"       => backend       = Some(s),
                        _ => {}
                    }
                }
            }
        }
        Ok(SchemaArgs { service, error_message, backend })
    }
}

// ---------------------------------------------------------------------------
// #[schema] proc-macro entry point
// ---------------------------------------------------------------------------
#[proc_macro_attribute]
pub fn schema(args: TokenStream, item: TokenStream) -> TokenStream {
    let SchemaArgs { service, error_message, backend } = parse_macro_input!(args as SchemaArgs);

    let mut module = parse_macro_input!(item as ItemMod);

    let service = match service {
        Some(s) => s,
        None => {
            return syn::Error::new(
                proc_macro2::Span::call_site(),
                "#[schema] requires a `service` argument: #[schema(service = \"my_service\")]",
            )
            .to_compile_error()
            .into();
        }
    };
    let error_message = error_message
        .unwrap_or_else(|| LitStr::new("Schema validation failed", proc_macro2::Span::call_site()));
    let backend =
        backend.unwrap_or_else(|| LitStr::new("built_in", proc_macro2::Span::call_site()));

    let (_, items) = match &mut module.content {
        Some((brace, items)) => (brace, items),
        None => {
            return syn::Error::new(module.span(), "#[schema] requires an inline module")
                .to_compile_error()
                .into();
        }
    };

    let mut create_struct: Option<syn::ItemStruct> = None;
    let mut patch_struct: Option<syn::ItemStruct> = None;

    for it in items.iter() {
        if let syn::Item::Struct(s) = it {
            if has_marker_attr(&s.attrs, "create") {
                create_struct = Some(s.clone());
            }
            if has_marker_attr(&s.attrs, "patch") {
                patch_struct = Some(s.clone());
            }
        }
    }

    let Some(create_struct) = create_struct else {
        return syn::Error::new(
            module.span(),
            "#[schema] module must contain a #[create] struct",
        )
        .to_compile_error()
        .into();
    };

    let create_rules = collect_field_rules(&create_struct);
    let patch_rules = patch_struct.as_ref().map(collect_field_rules);

    // Remove internal marker attrs so they don't reach rustc.
    strip_internal_attrs(items);

    let create_ident = create_struct.ident.clone();
    let patch_ident = patch_struct.as_ref().map(|s| s.ident.clone());

    let resolve_create_fn = gen_resolve_create(&create_rules, &error_message);
    let validate_create_fn =
        gen_validate_create(&create_rules, &error_message, &backend, &create_ident);
    let validate_patch_fn = patch_rules
        .as_ref()
        .map(|rules| {
            let patch_ident = patch_ident
                .as_ref()
                .expect("patch rules implies patch struct");
            gen_validate_patch(rules, &error_message, &backend, patch_ident)
        })
        .unwrap_or_else(|| quote! {});

    let register_fn = gen_register_fn(&service, patch_rules.is_some());

    if let Ok(it) = syn::parse2::<syn::Item>(resolve_create_fn) {
        items.push(it);
    }
    if let Ok(it) = syn::parse2::<syn::Item>(validate_create_fn) {
        items.push(it);
    }
    if !validate_patch_fn.is_empty() {
        if let Ok(it) = syn::parse2::<syn::Item>(validate_patch_fn) {
            items.push(it);
        }
    }
    if let Ok(it) = syn::parse2::<syn::Item>(register_fn) {
        items.push(it);
    }

    TokenStream::from(quote!(#module))
}

// ---------------------------------------------------------------------------
// Attribute helpers — syn 2.x: path() is a METHOD, not a field
// ---------------------------------------------------------------------------
fn has_marker_attr(attrs: &[Attribute], name: &str) -> bool {
    attrs.iter().any(|a| a.path().is_ident(name))
}

fn strip_internal_attrs(items: &mut [syn::Item]) {
    for it in items.iter_mut() {
        if let syn::Item::Struct(s) = it {
            s.attrs.push(syn::parse_quote!(#[allow(dead_code)]));

            // strip #[create]/#[patch]
            s.attrs
                .retain(|a| !(a.path().is_ident("create") || a.path().is_ident("patch")));

            // strip #[dog(...)] on fields
            if let syn::Fields::Named(named) = &mut s.fields {
                for f in named.named.iter_mut() {
                    f.attrs.retain(|a| !a.path().is_ident("dog"));
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// FieldRule extraction
// ---------------------------------------------------------------------------
#[derive(Clone)]
enum FieldKind {
    String,
    Bool,
    Other,
}

#[derive(Clone)]
struct FieldRule {
    json_key: String,
    kind: FieldKind,
    trim: bool,
    min_len: Option<usize>,
    default_bool: Option<bool>,
    optional: bool,
}

fn collect_field_rules(st: &syn::ItemStruct) -> Vec<FieldRule> {
    let mut rules = Vec::new();

    let fields = match &st.fields {
        syn::Fields::Named(n) => &n.named,
        _ => return rules,
    };

    for f in fields {
        let Some(ident) = f.ident.clone() else {
            continue;
        };
        let json_key = ident.to_string();

        let mut rule = FieldRule {
            json_key,
            kind: field_kind(&f.ty),
            trim: false,
            min_len: None,
            default_bool: None,
            optional: is_option_type(&f.ty),
        };

        // Parse #[dog(trim, min_len(3), default = false)] on fields
        for attr in &f.attrs {
            if !attr.path().is_ident("dog") {
                continue;
            }
            // syn 2.x: attr.meta is a field; Meta::List carries tokens
            if let Meta::List(ref list) = attr.meta {
                // parse the comma-separated inner meta items from tokens
                let nested = list.parse_args_with(
                    syn::punctuated::Punctuated::<Meta, syn::Token![,]>::parse_terminated,
                );
                if let Ok(metas) = nested {
                    for meta in metas {
                        match meta {
                            Meta::Path(p) => {
                                if p.is_ident("trim") {
                                    rule.trim = true;
                                } else if p.is_ident("optional") {
                                    rule.optional = true;
                                }
                            }
                            Meta::List(ml) => {
                                // min_len(3)
                                if ml.path.is_ident("min_len") {
                                    if let Ok(n) = ml.parse_args::<syn::LitInt>() {
                                        if let Ok(v) = n.base10_parse::<usize>() {
                                            rule.min_len = Some(v);
                                        }
                                    }
                                }
                            }
                            // syn 2.x: MetaNameValue.value is Expr, not Lit
                            Meta::NameValue(nv) if nv.path.is_ident("default") => {
                                if let Expr::Lit(ExprLit {
                                    lit: Lit::Bool(LitBool { value, .. }),
                                    ..
                                }) = nv.value
                                {
                                    rule.default_bool = Some(value);
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
        }

        rules.push(rule);
    }

    rules
}

fn is_option_type(ty: &syn::Type) -> bool {
    match ty {
        syn::Type::Path(p) => p.path.segments.last().is_some_and(|s| s.ident == "Option"),
        _ => false,
    }
}

fn field_kind(ty: &syn::Type) -> FieldKind {
    let inner = match ty {
        syn::Type::Path(p) => {
            let last = p.path.segments.last();
            if let Some(seg) = last {
                if seg.ident == "Option" {
                    if let syn::PathArguments::AngleBracketed(ab) = &seg.arguments {
                        if let Some(syn::GenericArgument::Type(t)) = ab.args.first() {
                            return field_kind(t);
                        }
                    }
                }
            }
            ty
        }
        _ => ty,
    };

    match inner {
        syn::Type::Path(p) => {
            if let Some(seg) = p.path.segments.last() {
                if seg.ident == "String" {
                    return FieldKind::String;
                }
                if seg.ident == "bool" {
                    return FieldKind::Bool;
                }
            }
            FieldKind::Other
        }
        _ => FieldKind::Other,
    }
}

// ---------------------------------------------------------------------------
// Code generation — unchanged from original
// ---------------------------------------------------------------------------
fn gen_resolve_create(rules: &[FieldRule], _error_message: &LitStr) -> proc_macro2::TokenStream {
    let trim_stmts = rules
        .iter()
        .filter(|r| r.trim && matches!(r.kind, FieldKind::String))
        .map(|r| {
            let key = &r.json_key;
            quote! {
                if let Some(serde_json::Value::String(s)) = obj.get_mut(#key) {
                    *s = s.trim().to_string();
                }
            }
        });

    let default_stmts = rules
        .iter()
        .filter_map(|r| r.default_bool.map(|v| (r, v)))
        .map(|(r, v)| {
            let key = &r.json_key;
            quote! {
                if !obj.contains_key(#key) {
                    obj.insert(#key.to_string(), serde_json::Value::Bool(#v));
                }
            }
        });

    quote! {
        pub fn resolve_create<P>(data: &mut serde_json::Value, _meta: &dog_schema::HookMeta<serde_json::Value, P>) -> anyhow::Result<()>
        where
            P: Send + Clone + 'static,
        {
            let Some(obj) = data.as_object_mut() else {
                return Ok(());
            };

            #(#trim_stmts)*
            #(#default_stmts)*

            Ok(())
        }
    }
}

fn gen_validate_create(
    rules: &[FieldRule],
    error_message: &LitStr,
    backend: &LitStr,
    create_ident: &syn::Ident,
) -> proc_macro2::TokenStream {
    if backend.value() == "validator" {
        return quote! {
            pub fn validate_create<P>(
                data: &serde_json::Value,
                _meta: &dog_schema::HookMeta<serde_json::Value, P>,
            ) -> anyhow::Result<()>
            where
                P: Send + Clone + 'static,
            {
                let _parsed: #create_ident = dog_schema_validator::validate::<#create_ident>(data, #error_message)?;
                Ok(())
            }
        };
    }

    let checks = rules.iter().map(|r| {
        let key = &r.json_key;
        let min_len = r.min_len;

        match r.kind {
            FieldKind::String => {
                let min_len_check = if let Some(n) = min_len {
                    quote! {
                        if v.chars().count() < #n {
                            errs.push_field(#key, format!("must be at least {} chars", #n));
                        }
                    }
                } else {
                    quote! {}
                };

                if r.optional {
                    quote! {
                        if let Some(v) = obj.get(#key).and_then(|v| v.as_str()) {
                            if v.trim().is_empty() {
                                errs.push_field(#key, "must not be empty");
                            }
                            #min_len_check
                        }
                    }
                } else {
                    quote! {
                        match obj.get(#key) {
                            None => errs.push_schema(format!("missing field `{}`", #key)),
                            Some(val) => {
                                if let Some(v) = val.as_str() {
                                    if v.trim().is_empty() {
                                        errs.push_field(#key, "must not be empty");
                                    }
                                    #min_len_check
                                } else {
                                    errs.push_field(#key, "must be a string");
                                }
                            }
                        }
                    }
                }
            }
            FieldKind::Bool => {
                let allow_missing = r.default_bool.is_some() || r.optional;
                if allow_missing {
                    quote! {
                        if let Some(val) = obj.get(#key) {
                            if !val.is_boolean() {
                                errs.push_field(#key, "must be a boolean");
                            }
                        }
                    }
                } else {
                    quote! {
                        match obj.get(#key) {
                            None => errs.push_schema(format!("missing field `{}`", #key)),
                            Some(val) => {
                                if !val.is_boolean() {
                                    errs.push_field(#key, "must be a boolean");
                                }
                            }
                        }
                    }
                }
            }
            FieldKind::Other => {
                if r.optional {
                    quote! {}
                } else {
                    quote! {
                        if obj.get(#key).is_none() {
                            errs.push_schema(format!("missing field `{}`", #key));
                        }
                    }
                }
            }
        }
    });

    quote! {
        pub fn validate_create<P>(data: &serde_json::Value, _meta: &dog_schema::HookMeta<serde_json::Value, P>) -> anyhow::Result<()>
        where
            P: Send + Clone + 'static,
        {
            let Some(obj) = data.as_object() else {
                return Err(dog_schema::schema_error(#error_message, "expected JSON object"));
            };

            let mut errs = dog_schema::SchemaErrors::default();

            #(#checks)*

            if errs.is_empty() {
                Ok(())
            } else {
                Err(errs.into_unprocessable_anyhow(#error_message))
            }
        }
    }
}

fn gen_validate_patch(
    rules: &[FieldRule],
    error_message: &LitStr,
    backend: &LitStr,
    patch_ident: &syn::Ident,
) -> proc_macro2::TokenStream {
    if backend.value() == "validator" {
        return quote! {
            pub fn validate_patch<P>(
                data: &serde_json::Value,
                _meta: &dog_schema::HookMeta<serde_json::Value, P>,
            ) -> anyhow::Result<()>
            where
                P: Send + Clone + 'static,
            {
                let _parsed: #patch_ident = dog_schema_validator::validate::<#patch_ident>(data, #error_message)?;
                Ok(())
            }
        };
    }

    let checks = rules.iter().map(|r| {
        let key = &r.json_key;
        let min_len = r.min_len;

        match r.kind {
            FieldKind::String => {
                let min_len_check = if let Some(n) = min_len {
                    quote! {
                        if v.chars().count() < #n {
                            errs.push_field(#key, format!("must be at least {} chars", #n));
                        }
                    }
                } else {
                    quote! {}
                };

                quote! {
                    if let Some(val) = obj.get(#key) {
                        if val.is_null() {
                            // allow null (treat as not provided)
                        } else if let Some(v) = val.as_str() {
                            if v.trim().is_empty() {
                                errs.push_field(#key, "must not be empty");
                            }
                            #min_len_check
                        } else {
                            errs.push_field(#key, "must be a string");
                        }
                    }
                }
            }
            FieldKind::Bool => {
                quote! {
                    if let Some(val) = obj.get(#key) {
                        if val.is_null() {
                            // allow null
                        } else if !val.is_boolean() {
                            errs.push_field(#key, "must be a boolean");
                        }
                    }
                }
            }
            FieldKind::Other => {
                quote! {
                    if let Some(val) = obj.get(#key) {
                        if val.is_null() {
                            // allow null
                        }
                    }
                }
            }
        }
    });

    quote! {
        pub fn validate_patch<P>(data: &serde_json::Value, _meta: &dog_schema::HookMeta<serde_json::Value, P>) -> anyhow::Result<()>
        where
            P: Send + Clone + 'static,
        {
            let Some(obj) = data.as_object() else {
                return Err(dog_schema::schema_error(#error_message, "expected JSON object"));
            };

            let mut errs = dog_schema::SchemaErrors::default();

            #(#checks)*

            if errs.is_empty() {
                Ok(())
            } else {
                Err(errs.into_unprocessable_anyhow(#error_message))
            }
        }
    }
}

fn gen_register_fn(service: &LitStr, has_patch: bool) -> proc_macro2::TokenStream {
    let svc = service.value();
    let svc_lit = LitStr::new(&svc, service.span());

    let patch = if has_patch {
        quote! {
            s.on_patch().validate(validate_patch);
        }
    } else {
        quote! {}
    };

    quote! {
        pub fn register<P>(builder: &mut dog_core::DogAppBuilder<serde_json::Value, P>) -> anyhow::Result<()>
        where
            P: Send + Clone + 'static,
        {
            use dog_schema::SchemaHooksExt;

            builder.service_hooks(#svc_lit, |h| {
                h.schema(|s| {
                    s.on_create().resolve(resolve_create).validate(validate_create);
                    #patch
                    s.on_update().validate(validate_create);
                });
            });

            Ok(())
        }
    }
}
