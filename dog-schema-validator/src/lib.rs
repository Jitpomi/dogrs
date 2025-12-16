use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use validator::Validate;

use dog_schema::SchemaErrors;

fn friendly_message(code: &str) -> Option<&'static str> {
    match code {
        "required" => Some("is required"),
        "email" => Some("must be a valid email"),
        "length" => Some("has invalid length"),
        "range" => Some("is out of range"),
        "url" => Some("must be a valid URL"),
        _ => None,
    }
}

fn join_path(prefix: &str, field: &str) -> String {
    if prefix.is_empty() {
        field.to_string()
    } else {
        format!("{prefix}.{field}")
    }
}

fn join_index(prefix: &str, idx: usize) -> String {
    format!("{prefix}[{idx}]")
}

fn push_validation_errors(out: &mut SchemaErrors, prefix: &str, errs: &validator::ValidationErrors) {
    for (field, kind) in errs.errors() {
        match kind {
            validator::ValidationErrorsKind::Field(field_errors) => {
                let key = join_path(prefix, field);
                for e in field_errors {
                    let msg = e
                        .message
                        .as_ref()
                        .map(|m| m.to_string())
                        .or_else(|| friendly_message(&e.code).map(|m| m.to_string()))
                        .unwrap_or_else(|| e.code.to_string());
                    out.push_field(&key, msg);
                }
            }
            validator::ValidationErrorsKind::Struct(struct_errs) => {
                let next = join_path(prefix, field);
                push_validation_errors(out, &next, struct_errs.as_ref());
            }
            validator::ValidationErrorsKind::List(list_errs) => {
                let base = join_path(prefix, field);
                for (idx, nested) in list_errs {
                    let next = join_index(&base, *idx);
                    push_validation_errors(out, &next, nested.as_ref());
                }
            }
        }
    }
}

fn validator_errors_to_schema_errors(errs: &validator::ValidationErrors) -> SchemaErrors {
    let mut out = SchemaErrors::default();

    push_validation_errors(&mut out, "", errs);

    out
}

pub fn validate<T>(data: &Value, error_message: &str) -> anyhow::Result<T>
where
    T: DeserializeOwned + Validate,
{
    let parsed: T = serde_json::from_value(data.clone())
        .map_err(|e| dog_schema::unprocessable(error_message, json!({"_schema": [e.to_string()]})))?;

    parsed
        .validate()
        .map_err(|e| validator_errors_to_schema_errors(&e).into_unprocessable_anyhow(error_message))?;

    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use dog_core::errors::DogError;
    use serde::Deserialize;
    use serde_json::json;
    use validator::Validate;

    use super::validate;

    #[derive(Debug, Deserialize, Validate)]
    struct Profile {
        #[validate(length(min = 2, message = "display_name must be at least 2 chars"))]
        display_name: String,
    }

    #[derive(Debug, Deserialize, Validate)]
    struct Tag {
        #[validate(email(message = "tag email must be valid"))]
        email: String,
    }

    #[derive(Debug, Deserialize, Validate)]
    struct User {
        #[validate(nested)]
        profile: Profile,

        #[validate(nested)]
        tags: Vec<Tag>,
    }

    #[test]
    fn nested_and_list_errors_are_flattened_with_paths() {
        let data = json!({
            "profile": {"display_name": "x"},
            "tags": [{"email": "not-an-email"}]
        });

        let err = validate::<User>(&data, "Users schema validation failed").unwrap_err();
        let dog = DogError::from_anyhow(&err).expect("must be DogError");
        let errors = dog.errors.as_ref().unwrap();

        assert_eq!(errors["profile.display_name"][0], "display_name must be at least 2 chars");
        assert_eq!(errors["tags[0].email"][0], "tag email must be valid");
    }
}
