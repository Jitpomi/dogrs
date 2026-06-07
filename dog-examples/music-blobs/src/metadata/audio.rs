use dog_blob::BlobMetadata;
use std::io::Cursor;
use symphonia::core::formats::FormatOptions;
use symphonia::core::formats::probe::Hint;
use symphonia::core::codecs::CodecParameters;
use symphonia::core::codecs::audio::CODEC_ID_NULL_AUDIO;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;

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
            }
        }

        has_metadata.then_some(metadata)
    }

    /// Extract MP3 ID3 metadata including album art
    fn extract_mp3_metadata(data: &[u8]) -> Option<BlobMetadata> {
        let mut metadata = BlobMetadata::default();

        if let Ok(tag) = id3::Tag::read_from2(std::io::Cursor::new(data)) {
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
                        metadata.year = frame
                            .content()
                            .text()
                            .and_then(|text| text.chars().take(4).collect::<String>().parse().ok());
                    }
                    "TLEN" => {
                        metadata.duration = frame
                            .content()
                            .text()
                            .and_then(|text| text.parse::<u32>().ok())
                            .map(|ms| ms / 1000);
                    }
                    "APIC" => {
                        // Album art is extracted separately using extract_raw_album_art
                        // But we still need to set the metadata flag so the UI knows there is cover art!
                        metadata.album_art_url = Some("true".to_string());
                    }
                    _ => {}
                }
            }
        }

        // Extract technical audio properties using symphonia
        if let Some(audio_props) = Self::extract_audio_properties_with_symphonia(data) {
            metadata.bitrate = audio_props.bitrate;
            metadata.sample_rate = audio_props.sample_rate;
            metadata.channels = audio_props.channels;
        }

        // Set format info
        metadata.encoding = Some("MP3".to_string());
        metadata.mime_type = Some("audio/mpeg".to_string());

        Some(metadata)
    }

    /// Extract raw album art bytes from MP3 data
    pub fn extract_raw_album_art(data: &[u8]) -> Option<(String, Vec<u8>)> {
        let tag = id3::Tag::read_from2(std::io::Cursor::new(data)).ok()?;
        for frame in tag.frames() {
            if frame.id() == "APIC" {
                if let Some(picture) = frame.content().picture() {
                    return Some((picture.mime_type.clone(), picture.data.clone()));
                }
            }
        }
        None
    }

    /// Extract technical audio properties using symphonia
    fn extract_audio_properties_with_symphonia(data: &[u8]) -> Option<AudioProperties> {
        println!(
            "🎼 Using symphonia to extract audio properties from {} bytes",
            data.len()
        );

        // Create a media source from the byte data
        let cursor = Cursor::new(data.to_vec());
        let media_source = MediaSourceStream::new(Box::new(cursor), Default::default());

        // Create a probe hint (symphonia will auto-detect format)
        let mut hint = Hint::new();
        hint.with_extension("mp3");

        // Probe the media source
        let format_opts = FormatOptions::default();
        let metadata_opts = MetadataOptions::default();

        let format = match symphonia::default::get_probe().probe(
            &hint,
            media_source,
            format_opts,
            metadata_opts,
        ) {
            Ok(fmt) => {
                println!("✅ Symphonia successfully probed the audio format");
                fmt
            }
            Err(e) => {
                println!("❌ Symphonia probe failed: {:?}", e);
                return None;
            }
        };

        // Extract track information and calculate proper duration
        let (sample_rate, channels, bitrate) = {
            // symphonia 0.6: codec_params is Option<CodecParameters> enum;
            // time_base and num_frames are on Track, not codec_params
            let track = format
                .tracks()
                .iter()
                .find(|t| {
                    matches!(&t.codec_params, Some(CodecParameters::Audio(p)) if p.codec != CODEC_ID_NULL_AUDIO)
                })?;

            let audio_params = match &track.codec_params {
                Some(CodecParameters::Audio(params)) => params,
                _ => return None,
            };

            println!("🎵 Found audio track with codec: {:?}", audio_params.codec);

            let sample_rate = audio_params.sample_rate;
            let channels = audio_params.channels.as_ref().map(|ch| ch.count() as u32);

            println!(
                "📊 Codec params - Sample rate: {:?}, Channels: {:?}",
                sample_rate, channels
            );
            println!(
                "📊 Time base: {:?}, Num frames: {:?}",
                track.time_base, track.num_frames
            );

            // Calculate duration from track-level time_base and num_frames (symphonia 0.6)
            let duration = if let (Some(time_base), Some(num_frames)) =
                (track.time_base, track.num_frames)
            {
                // symphonia 0.6: time_base.numer/denom are NonZero<u32> — call .get()
                let duration_seconds =
                    (num_frames as f64 * time_base.numer.get() as f64) / time_base.denom.get() as f64;
                println!(
                    "🕐 Calculated duration from symphonia time base: {:.2}s",
                    duration_seconds
                );
                Some(duration_seconds as u32)
            } else {
                println!("❌ Could not calculate duration - no time base or num_frames");
                None
            };

            // Calculate bitrate from file size and duration
            let bitrate = if let Some(dur) = duration {
                if dur > 0 {
                    let calculated_bitrate = ((data.len() * 8) / (dur as usize * 1000)) as u32;
                    println!(
                        "📈 Calculated bitrate from duration: {} kbps",
                        calculated_bitrate
                    );
                    Some(calculated_bitrate)
                } else {
                    None
                }
            } else {
                None
            };

            (sample_rate, channels, bitrate)
        };

        let result = AudioProperties {
            sample_rate,
            channels,
            bitrate,
        };

        println!("🎯 Final symphonia result: {:?}", result);
        Some(result)
    }
}

/// Technical audio properties extracted from file header
#[derive(Debug)]
struct AudioProperties {
    bitrate: Option<u32>,
    sample_rate: Option<u32>,
    channels: Option<u32>,
}
