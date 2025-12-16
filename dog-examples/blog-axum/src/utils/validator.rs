use anyhow::Result;

pub fn require_non_empty(field: &str, v: &str) -> Result<()> {
    if v.trim().is_empty() {
        anyhow::bail!("'{field}' must not be empty");
    }
    Ok(())
}
