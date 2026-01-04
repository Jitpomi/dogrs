use base64::{Engine as _, engine::general_purpose};
use dog_blob::BlobMetadata;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use std::io::Cursor;

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

        // Extract technical audio properties using symphonia
        if let Some(audio_props) = Self::extract_audio_properties_with_symphonia(data) {
            if metadata.duration.is_none() {
                metadata.duration = audio_props.duration;
            }
            metadata.bitrate = audio_props.bitrate;
            metadata.sample_rate = audio_props.sample_rate;
            metadata.channels = audio_props.channels;
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

    /// Extract technical audio properties using symphonia
    fn extract_audio_properties_with_symphonia(data: &[u8]) -> Option<AudioProperties> {
        println!("🎼 Using symphonia to extract audio properties from {} bytes", data.len());
        
        // Create a media source from the byte data
        let cursor = Cursor::new(data.to_vec());
        let media_source = MediaSourceStream::new(Box::new(cursor), Default::default());
        
        // Create a probe hint (symphonia will auto-detect format)
        let mut hint = Hint::new();
        hint.with_extension("mp3");
        
        // Probe the media source
        let format_opts = FormatOptions::default();
        let metadata_opts = MetadataOptions::default();
        
        let probed = match symphonia::default::get_probe()
            .format(&hint, media_source, &format_opts, &metadata_opts) {
            Ok(probed) => {
                println!("✅ Symphonia successfully probed the audio format");
                probed
            },
            Err(e) => {
                println!("❌ Symphonia probe failed: {:?}", e);
                return None;
            }
        };
        
        let format = probed.format;
        
        // Extract track information and calculate proper duration
        let (sample_rate, channels, duration, bitrate) = {
            let track = format.tracks().iter().find(|t| t.codec_params.codec != symphonia::core::codecs::CODEC_TYPE_NULL)?;
            
            println!("🎵 Found track with codec: {:?}", track.codec_params.codec);
            
            let codec_params = &track.codec_params;
            let sample_rate = codec_params.sample_rate;
            let channels = codec_params.channels.map(|ch| ch.count() as u32);
            
            println!("📊 Codec params - Sample rate: {:?}, Channels: {:?}", sample_rate, channels);
            println!("📊 Time base: {:?}, N frames: {:?}", codec_params.time_base, codec_params.n_frames);
            
            // Calculate duration from symphonia's codec parameters
            let duration = if let (Some(time_base), Some(n_frames)) = (codec_params.time_base, codec_params.n_frames) {
                let duration_seconds = (n_frames as f64 * time_base.numer as f64) / time_base.denom as f64;
                println!("🕐 Calculated duration from symphonia time base: {:.2}s", duration_seconds);
                Some(duration_seconds as u32)
            } else if let Some(sample_rate) = sample_rate {
                // Fallback: estimate from file size and sample rate
                let bytes_per_sample = 2; // 16-bit samples
                let estimated_samples = data.len() / (bytes_per_sample * channels.unwrap_or(2) as usize);
                let duration_seconds = estimated_samples as f64 / sample_rate as f64;
                println!("🕐 Estimated duration from sample rate: {:.2}s", duration_seconds);
                Some(duration_seconds as u32)
            } else {
                println!("❌ Could not calculate duration - no time base or sample rate");
                None
            };
            
            // Calculate bitrate from file size and duration
            let bitrate = if let Some(dur) = duration {
                if dur > 0 {
                    let calculated_bitrate = ((data.len() * 8) / (dur as usize * 1000)) as u32;
                    println!("📈 Calculated bitrate from duration: {} kbps", calculated_bitrate);
                    Some(calculated_bitrate)
                } else {
                    None
                }
            } else {
                None
            };
            
            (sample_rate, channels, duration, bitrate)
        };
        
        let result = AudioProperties {
            sample_rate,
            channels,
            duration,
            bitrate,
        };
        
        println!("🎯 Final symphonia result: {:?}", result);
        Some(result)
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

/// Technical audio properties extracted from file header
#[derive(Debug)]
struct AudioProperties {
    bitrate: Option<u32>,
    sample_rate: Option<u32>,
    channels: Option<u32>,
    duration: Option<u32>,
}

/// Extracted album art data
struct AlbumArt {
    data_url: String,
    picture_type: id3::frame::PictureType,
}
