use dog_core::{
    DogApp, DogService, ServiceCaller, ServiceCapabilities, ServiceMethodKind, TenantContext,
};
use dog_schema::{schema, HookMeta, Rules};
use serde_json::{json, Value};
use std::sync::Arc;

#[schema(service = "audit")]
mod model {
    #[create]
    pub struct Create {
        #[dog(trim, min_len(3))]
        pub name: String,
        pub nickname: Option<String>,
        pub age: u8,
    }
    #[patch]
    pub struct Patch {
        #[dog(trim, min_len(3))]
        pub nickname: Option<String>,
    }
}
fn meta() -> HookMeta<Value, ()> {
    let app = DogApp::default();
    HookMeta {
        tenant: TenantContext::new("audit"),
        method: ServiceMethodKind::Create,
        params: (),
        config: app.config_snapshot(),
        services: ServiceCaller::new(app),
    }
}
struct Echo;
#[async_trait::async_trait]
impl DogService<Value, ()> for Echo {
    fn capabilities(&self) -> ServiceCapabilities {
        ServiceCapabilities::standard_crud()
    }
    async fn create(&self, _: &TenantContext, data: Value, _: ()) -> anyhow::Result<Value> {
        Ok(data)
    }
    async fn find(&self, _: &TenantContext, _: ()) -> anyhow::Result<Vec<Value>> {
        Ok(vec![])
    }
    async fn update(
        &self,
        _: &TenantContext,
        _: &str,
        data: Value,
        _: (),
    ) -> anyhow::Result<Value> {
        Ok(data)
    }
    async fn patch(
        &self,
        _: &TenantContext,
        _: Option<&str>,
        data: Value,
        _: (),
    ) -> anyhow::Result<Value> {
        Ok(data)
    }
}
fn app() -> DogApp<Value, ()> {
    let mut builder = DogApp::builder();
    builder.register_service("audit", Arc::new(Echo));
    model::register(&mut builder).unwrap();
    builder.build()
}
#[test]
fn optional_string_rejects_number() {
    assert!(
        model::validate_create(&json!({"name":"Alice","age":10,"nickname":42}), &meta()).is_err()
    );
}
#[test]
fn integer_rejects_string() {
    assert!(
        model::validate_create(&json!({"name":"Alice","age":"not an integer"}), &meta()).is_err()
    );
}
#[test]
fn rules_produce_structured_client_error() {
    let err = Rules::new().non_empty("name", "").check().unwrap_err();
    assert!(dog_core::errors::DogError::from_anyhow(&err).is_some());
}
#[tokio::test]
async fn valid_partial_patch_is_accepted() {
    assert!(app()
        .service("audit")
        .unwrap()
        .patch(
            TenantContext::new("audit"),
            Some("1"),
            json!({"nickname":"Bob"}),
            ()
        )
        .await
        .is_ok());
}
#[tokio::test]
async fn normalized_short_value_is_rejected() {
    assert!(app()
        .service("audit")
        .unwrap()
        .create(
            TenantContext::new("audit"),
            json!({"name":"  ab  ","age":10}),
            ()
        )
        .await
        .is_err());
}

#[schema(service = "types")]
mod types {
    #[create]
    pub struct Create {
        pub count: u8,
        pub tags: Vec<String>,
        pub enabled: Option<bool>,
        #[dog(default = false)]
        pub published: bool,
        pub r#type: String,
    }
    #[patch]
    pub struct Patch {
        pub count: u8,
        pub enabled: Option<bool>,
    }
}
#[test]
fn numeric_bounds_collections_nulls_and_unknown_fields() {
    let valid = json!({"count":255,"tags":["one"],"enabled":null,"published":false,"type":"item"});
    assert!(types::validate_create(&valid, &meta()).is_ok());
    for (key, value) in [
        ("count", json!(256)),
        ("count", json!(-1)),
        ("count", json!(1.5)),
        ("tags", json!([1])),
        ("enabled", json!(1)),
        ("published", json!(null)),
        ("extra", json!(true)),
    ] {
        let mut invalid = valid.clone();
        invalid[key] = value;
        let err = types::validate_create(&invalid, &meta()).unwrap_err();
        let dog = dog_core::errors::DogError::from_anyhow(&err).unwrap();
        assert_eq!(dog.code(), 422);
        assert!(dog.errors.as_ref().unwrap().get(key).is_some());
    }
    assert!(types::validate_patch(&json!({}), &meta()).is_ok());
    assert!(types::validate_patch(&json!({"count":null}), &meta()).is_err());
    assert!(types::validate_patch(&json!({"enabled":null}), &meta()).is_ok());
}
#[test]
fn defaults_only_fill_missing_create_fields() {
    let mut value = json!({"count":1,"tags":[],"type":"item"});
    types::resolve_create(&mut value, &meta()).unwrap();
    assert_eq!(value["published"], false);
    assert!(types::validate_create(&value, &meta()).is_ok());
    value["published"] = json!("false");
    types::resolve_create(&mut value, &meta()).unwrap();
    assert!(types::validate_create(&value, &meta()).is_err());
    assert!(types::resolve_create(&mut json!([]), &meta()).is_err());
}
#[tokio::test]
async fn patch_trims_before_checking_and_preserves_explicit_null() {
    let app = app();
    let service = app.service("audit").unwrap();
    let tenant = TenantContext::new("audit");
    assert!(service
        .patch(tenant.clone(), Some("1"), json!({"nickname":"  ab  "}), ())
        .await
        .is_err());
    assert_eq!(
        service
            .patch(tenant.clone(), Some("1"), json!({"nickname":" Bob "}), ())
            .await
            .unwrap(),
        json!({"nickname":"Bob"})
    );
    assert_eq!(
        service
            .patch(tenant, Some("1"), json!({"nickname":null}), ())
            .await
            .unwrap(),
        json!({"nickname":null})
    );
}
#[test]
fn structured_rules_accumulate_field_errors() {
    let err = Rules::new()
        .non_empty("name", "")
        .min_len("name", "", 3)
        .max_len("bio", "long", 2)
        .check()
        .unwrap_err();
    let dog = dog_core::errors::DogError::from_anyhow(&err).unwrap();
    assert_eq!(dog.code(), 422);
    assert_eq!(
        dog.errors.as_ref().unwrap()["name"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
}

#[schema(service = "create_only")]
mod create_only {
    #[create]
    pub struct Create {
        pub name: String,
    }
}
#[tokio::test]
async fn update_normalizes_and_read_methods_remain_unaffected() {
    let app = app();
    let service = app.service("audit").unwrap();
    let tenant = TenantContext::new("audit");
    assert!(service
        .update(tenant.clone(), "1", json!({"name":" ab ","age":10}), ())
        .await
        .is_err());
    let data = service
        .update(tenant.clone(), "1", json!({"name":" Alice ","age":10}), ())
        .await
        .unwrap();
    assert_eq!(data["name"], "Alice");
    assert!(service.find(tenant, ()).await.unwrap().is_empty());
}
#[tokio::test]
async fn missing_patch_schema_fails_closed() {
    let mut builder = DogApp::builder();
    builder.register_service("create_only", Arc::new(Echo));
    create_only::register(&mut builder).unwrap();
    let app = builder.build();
    let err = app
        .service("create_only")
        .unwrap()
        .patch(
            TenantContext::new("audit"),
            Some("1"),
            json!({"name":"Alice"}),
            (),
        )
        .await
        .unwrap_err();
    assert_eq!(
        dog_core::errors::DogError::from_anyhow(&err)
            .unwrap()
            .code(),
        422
    );
}
