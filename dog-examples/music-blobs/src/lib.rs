mod app;
mod channels;
mod hooks;
mod metadata;
mod rustfs;
mod rustfs_store;
mod services;

use std::sync::Arc;

use dog_axum::{
    middlewares::{FileEncoding, MultipartConfig, MultipartToJson},
    AxumApp,
};
use serde_json::Value;
use axum::{
    http::HeaderMap,
    response::Response,
    body::Body,
};

pub use services::MusicParams;

struct MusicMultipartDefaults;

impl MusicMultipartDefaults {
    const MAX_FILE_SIZE_MB: usize = 200;
    const MAX_TOTAL_SIZE_MB: usize = 500;
    const ALLOWED_TYPES: &'static str = "audio/mpeg,audio/wav,audio/flac,audio/aac,audio/ogg,application/octet-stream";
    const INCLUDE_METADATA: bool = true;
    const FILE_ENCODING: &'static str = "base64";
}

pub async fn build() -> anyhow::Result<AxumApp<Value, MusicParams>> {
    let ax = app::music_app()?;
    rustfs::RustFsState::setup_store(ax.app.as_ref()).await?;

    let state = ax
        .app
        .get::<Arc<rustfs::RustFsState>>("rustfs")
        .ok_or(anyhow::anyhow!("RustFsState not found"))?;

    hooks::global_hooks(ax.app.as_ref());
    channels::configure(ax.app.as_ref())?;

    let svcs = services::configure(ax.app.as_ref(), Arc::clone(&state))?;

    let config = multipart_config();
  
    let mut ax = ax
        .use_service_with("/music", svcs.music, MultipartToJson::with_config(config))
        .service("/health", || async { "ok" });

    // Add other middleware layers to router
    ax.router = ax
        .router
        .layer(axum::extract::DefaultBodyLimit::max(100 * 1024 * 1024)) // 100MB to match dog-blob config
        .layer(
            tower_http::cors::CorsLayer::new()
                .allow_origin(tower_http::cors::Any)
                .allow_methods(tower_http::cors::Any)
                .allow_headers(tower_http::cors::Any),
        )
        .fallback_service(tower_http::services::ServeDir::new(
            std::env::var("STATIC_DIR").unwrap_or_else(|_| "static".to_string()),
        ));

    Ok(ax)
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

fn process_audio_file(
    ctx: &mut dog_axum::middlewares::FieldContext,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Audio file processed successfully

    if let Some(filename) = &ctx.filename {
        let format = if filename.ends_with(".mp3") {
            "MP3"
        } else if filename.ends_with(".wav") {
            "WAV"
        } else {
            "Audio"
        };
        ctx.metadata
            .insert("format".to_string(), serde_json::json!(format));
    }

    Ok(())
}

fn multipart_config() -> MultipartConfig {
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
        "metadata" => FileEncoding::Metadata,
        "skip" => FileEncoding::Skip,
        _ => FileEncoding::Base64,
    };

    let mut config = MultipartConfig::new()
        .max_file_size(max_file_mb * 1024 * 1024)
        .max_total_size(max_total_mb * 1024 * 1024)
        .file_field("file")
        .file_encoding(encoding)
        .include_metadata(include_metadata)
        .field_processor("file", process_audio_file);

    // Add each allowed content type
    for content_type in MusicMultipartDefaults::ALLOWED_TYPES.split(',') {
        config = config.allow_content_type(content_type.trim());
    }

    config
}
