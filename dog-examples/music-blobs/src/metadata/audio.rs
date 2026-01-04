use base64::{Engine as _, engine::general_purpose};
use dog_blob::BlobMetadata;

/// Audio metadata extractor for various formats
pub struct AudioMetadataExtractor;

impl AudioMetadataExtractor {
    /// Extract metadata from audio file content
    pub fn extract(data: &[u8], filename: Option<&str>) -> Option<BlobMetadata> {
        let mut metadata = BlobMetadata::default();
        let mut has_metadata = false;

        if let Some(filename) = filename {
            let filename_lower = filename.to_lowercase();
            
            if filename_lower.ends_with(".mp3") {
                if let Some(mp3_metadata) = Self::extract_mp3_metadata(data) {
                    metadata = mp3_metadata;
                    has_metadata = true;
                }
            } else {
                // Handle other audio formats
                if let Some((encoding, mime_type)) = Self::get_audio_format_info(&filename_lower) {
                    metadata.encoding = Some(encoding);
                    metadata.mime_type = Some(mime_type);
                    has_metadata = true;
                }
            }
        }

        has_metadata.then_some(metadata)
    }

    /// Extract MP3 ID3 metadata including album art
    fn extract_mp3_metadata(data: &[u8]) -> Option<BlobMetadata> {
        let tag = id3::Tag::read_from2(std::io::Cursor::new(data)).ok()?;
        let mut metadata = BlobMetadata::default();

        println!("🎵 Found ID3 tag with {} frames", tag.frames().count());

        // Extract basic metadata
        for frame in tag.frames() {
            println!("🔍 Processing frame: {}", frame.id());
            match frame.id() {
                "TIT2" => metadata.title = frame.content().text().map(String::from),
                "TPE1" => metadata.artist = frame.content().text().map(String::from),
                "TALB" => metadata.album = frame.content().text().map(String::from),
                "TCON" => metadata.genre = frame.content().text().map(String::from),
                "TYER" | "TDRC" => {
                    metadata.year = frame.content().text()
                        .and_then(|text| text.chars().take(4).collect::<String>().parse().ok());
                },
                "TLEN" => {
                    metadata.duration = frame.content().text()
                        .and_then(|text| text.parse::<u32>().ok())
                        .map(|ms| ms / 1000);
                },
                "APIC" => {
                    if let Some(album_art) = Self::extract_album_art(frame) {
                        Self::set_album_art(&mut metadata, album_art);
                    }
                },
                _ => {}
            }
        }

        // Set format info
        metadata.encoding = Some("MP3".to_string());
        metadata.mime_type = Some("audio/mpeg".to_string());

        Some(metadata)
    }

    /// Extract album art from APIC frame
    fn extract_album_art(frame: &id3::Frame) -> Option<AlbumArt> {
        let picture = frame.content().picture()?;
        
        println!("🖼️ Found APIC frame");
        println!("📸 Picture data: {} bytes, mime: {}, type: {:?}", 
            picture.data.len(), picture.mime_type, picture.picture_type);
        
        let base64_data = general_purpose::STANDARD.encode(&picture.data);
        let data_url = format!("data:{};base64,{}", picture.mime_type, base64_data);
        
        println!("🔗 Generated data URL (first 100 chars): {}", 
            if data_url.len() > 100 { &data_url[..100] } else { &data_url });

        Some(AlbumArt {
            data_url,
            picture_type: picture.picture_type,
        })
    }

    /// Set album art based on picture type
    fn set_album_art(metadata: &mut BlobMetadata, album_art: AlbumArt) {
        match album_art.picture_type {
            id3::frame::PictureType::CoverFront => {
                println!("✅ Setting as album_art_url (CoverFront)");
                metadata.album_art_url = Some(album_art.data_url);
            },
            id3::frame::PictureType::Other => {
                if metadata.album_art_url.is_none() {
                    println!("✅ Setting as thumbnail_url (Other, no album art yet)");
                    metadata.thumbnail_url = Some(album_art.data_url);
                }
            },
            _ => {
                if metadata.thumbnail_url.is_none() {
                    println!("✅ Setting as thumbnail_url (other type: {:?})", album_art.picture_type);
                    metadata.thumbnail_url = Some(album_art.data_url);
                }
            }
        }
    }

    /// Get audio format info from filename extension
    fn get_audio_format_info(filename_lower: &str) -> Option<(String, String)> {
        match filename_lower {
            name if name.ends_with(".flac") => Some(("FLAC".to_string(), "audio/flac".to_string())),
            name if name.ends_with(".wav") => Some(("WAV".to_string(), "audio/wav".to_string())),
            name if name.ends_with(".aac") => Some(("AAC".to_string(), "audio/aac".to_string())),
            name if name.ends_with(".ogg") => Some(("OGG".to_string(), "audio/ogg".to_string())),
            name if name.ends_with(".m4a") => Some(("M4A".to_string(), "audio/mp4".to_string())),
            _ => None,
        }
    }
}

/// Extracted album art data
struct AlbumArt {
    data_url: String,
    picture_type: id3::frame::PictureType,
}
