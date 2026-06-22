pub mod multipart;
mod app;
mod channels;
mod hooks;
mod metadata;
mod rustfs;
mod rustfs_store;
mod services;

use std::sync::Arc;

use dog_core::DogApp;
use dog_transport::{IntoDogService, http::DogHttpService};
use serde_json::Value;

pub use services::MusicParams;

struct MusicMultipartDefaults;

impl MusicMultipartDefaults {
    const MAX_FILE_SIZE_MB: usize = 200;
    const MAX_TOTAL_SIZE_MB: usize = 500;
    const ALLOWED_TYPES: &'static str =
        "audio/mpeg,audio/wav,audio/flac,audio/aac,audio/ogg,application/octet-stream";
    const INCLUDE_METADATA: bool = true;
    const FILE_ENCODING: &'static str = "base64";
}

pub async fn build() -> anyhow::Result<(DogApp<Value, MusicParams>, DogHttpService<Value, MusicParams>)> {
    let mut builder = app::build_builder().await?;

    let state = builder
        .get::<Arc<rustfs::RustFsState>>("rustfs")
        .ok_or(anyhow::anyhow!("RustFsState not found"))?;

    services::configure(&mut builder, Arc::clone(&state))?;

    let dog = builder.build();
    let http_service = dog.clone().into_service(
        dog_transport::HttpOptions::default().route("/music", "music")
    );

    Ok((dog, http_service))
}

fn env_var_or<T>(key: &str, default: T) -> T
where
    T: std::str::FromStr + std::fmt::Display,
    T::Err: std::fmt::Debug,
{
    std::env::var(key)
        .unwrap_or_else(|_| default.to_string())
        .parse()
        .unwrap_or(default)
}


pub fn multipart_config() -> multipart::MultipartConfig {
    let max_file_mb = env_var_or(
        "MUSIC_MAX_FILE_SIZE_MB",
        MusicMultipartDefaults::MAX_FILE_SIZE_MB,
    );
    let max_total_mb = env_var_or(
        "MUSIC_MAX_TOTAL_SIZE_MB",
        MusicMultipartDefaults::MAX_TOTAL_SIZE_MB,
    );
    let include_metadata = env_var_or(
        "MUSIC_INCLUDE_METADATA",
        MusicMultipartDefaults::INCLUDE_METADATA,
    );
    let encoding = match env_var_or(
        "MUSIC_FILE_ENCODING",
        MusicMultipartDefaults::FILE_ENCODING.to_string(),
    )
    .to_lowercase()
    .as_str()
    {
        "metadata" => multipart::FileEncoding::Metadata,
        "skip" => multipart::FileEncoding::Skip,
        _ => multipart::FileEncoding::Base64,
    };

    let mut config = multipart::MultipartConfig::new()
        .max_file_size(max_file_mb * 1024 * 1024)
        .max_total_size(max_total_mb * 1024 * 1024)
        .file_field("file")
        .file_encoding(encoding)
        .include_metadata(include_metadata);

    // Add each allowed content type
    for content_type in MusicMultipartDefaults::ALLOWED_TYPES.split(',') {
        config = config.allow_content_type(content_type.trim());
    }

    config
}
