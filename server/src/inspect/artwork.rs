//! Embedded and uploaded artwork checks (media_inspector.go
//! validateArtworkData, optional-artwork.go ValidateUploadedArtwork).

use super::AlbumArtwork;
use super::errors::{Code, Result, error};
use image::{ImageFormat, ImageReader};
use sha2::{Digest, Sha256};
use std::io::Cursor;

pub const MAX_ARTWORK_SIZE_BYTES: usize = 20 * 1024 * 1024;
const MAX_ARTWORK_PIXELS: u64 = 50_000_000;
const JPEG_SIGNATURE: &[u8] = b"\xff\xd8\xff";
const PNG_SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";
const FORMATS_DIFFER: &str = "declared and detected image formats differ or are unsupported";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArtworkFormat {
    Jpeg,
    Png,
    WebP,
}

impl ArtworkFormat {
    pub fn mime_type(self) -> &'static str {
        match self {
            ArtworkFormat::Jpeg => "image/jpeg",
            ArtworkFormat::Png => "image/png",
            ArtworkFormat::WebP => "image/webp",
        }
    }

    fn image_format(self) -> ImageFormat {
        match self {
            ArtworkFormat::Jpeg => ImageFormat::Jpeg,
            ArtworkFormat::Png => ImageFormat::Png,
            ArtworkFormat::WebP => ImageFormat::WebP,
        }
    }

    fn is_animated(self, data: &[u8]) -> bool {
        match self {
            ArtworkFormat::Jpeg => false,
            ArtworkFormat::Png => has_png_animation_chunk(data),
            ArtworkFormat::WebP => has_webp_animation(data),
        }
    }
}

pub fn detect_format(data: &[u8]) -> Option<ArtworkFormat> {
    if data.starts_with(JPEG_SIGNATURE) {
        Some(ArtworkFormat::Jpeg)
    } else if data.starts_with(PNG_SIGNATURE) {
        Some(ArtworkFormat::Png)
    } else if data.len() >= 12 && &data[..4] == b"RIFF" && &data[8..12] == b"WEBP" {
        Some(ArtworkFormat::WebP)
    } else {
        None
    }
}

fn invalid(message: impl std::fmt::Display) -> super::errors::Failure {
    error(Code::InvalidArtwork, "artwork", message)
}

/// `validateArtworkData`: the declared MIME type must match the bytes, the
/// image must be still, and it must decode completely within the limits.
pub fn validate(mime_type: &str, data: &[u8]) -> Result<AlbumArtwork> {
    if data.is_empty() || data.len() > MAX_ARTWORK_SIZE_BYTES {
        return Err(invalid("encoded artwork size is invalid"));
    }
    let format = match detect_format(data) {
        Some(format) if format.mime_type() == mime_type => format,
        _ => return Err(invalid(FORMATS_DIFFER)),
    };
    if format.is_animated(data) {
        return Err(invalid("animated artwork is not supported"));
    }
    let reader = || ImageReader::with_format(Cursor::new(data), format.image_format());
    let (width, height) = reader()
        .into_dimensions()
        .map_err(|cause| invalid(format!("decode image config: {cause}")))?;
    if width == 0 || height == 0 || u64::from(width) * u64::from(height) > MAX_ARTWORK_PIXELS {
        return Err(invalid("decoded artwork dimensions are invalid"));
    }
    let mut full = reader();
    full.no_limits();
    full.decode()
        .map_err(|cause| invalid(format!("decode image: {cause}")))?;
    Ok(AlbumArtwork {
        warning: String::new(),
        mime_type: format.mime_type().to_owned(),
        width: i64::from(width),
        height: i64::from(height),
        sha256: hex::encode(Sha256::digest(data)),
        data: data.to_vec(),
    })
}

/// `ValidateUploadedArtwork`: uploads are checked by their bytes and must be
/// JPEG or PNG.
pub fn validate_uploaded(data: &[u8]) -> Result<AlbumArtwork> {
    match detect_format(data) {
        Some(format @ (ArtworkFormat::Jpeg | ArtworkFormat::Png)) => {
            validate(format.mime_type(), data)
        }
        _ => Err(invalid("uploaded artwork must be JPEG or PNG")),
    }
}

fn has_png_animation_chunk(data: &[u8]) -> bool {
    let mut offset = PNG_SIGNATURE.len();
    while offset + 12 <= data.len() {
        let length = u32::from_be_bytes(data[offset..offset + 4].try_into().unwrap()) as usize;
        if length > data.len() - offset - 12 {
            return false;
        }
        if &data[offset + 4..offset + 8] == b"acTL" {
            return true;
        }
        offset += 12 + length;
    }
    false
}

fn has_webp_animation(data: &[u8]) -> bool {
    let mut offset = 12;
    while offset + 8 <= data.len() {
        let length = u32::from_le_bytes(data[offset + 4..offset + 8].try_into().unwrap()) as usize;
        let data_offset = offset + 8;
        if length > data.len() - data_offset {
            return false;
        }
        let chunk = &data[offset..offset + 4];
        if chunk == b"ANIM"
            || chunk == b"ANMF"
            || (chunk == b"VP8X" && length > 0 && data[data_offset] & 0x02 != 0)
        {
            return true;
        }
        offset = data_offset + length + length % 2;
    }
    false
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use image::{ImageEncoder, RgbImage};

    pub fn png(width: u32, height: u32) -> Vec<u8> {
        let mut out = Vec::new();
        image::codecs::png::PngEncoder::new(&mut out)
            .write_image(
                RgbImage::new(width, height).as_raw(),
                width,
                height,
                image::ExtendedColorType::Rgb8,
            )
            .unwrap();
        out
    }

    pub fn jpeg(width: u32, height: u32) -> Vec<u8> {
        let mut out = Vec::new();
        image::codecs::jpeg::JpegEncoder::new(&mut out)
            .encode_image(&RgbImage::new(width, height))
            .unwrap();
        out
    }

    #[test]
    fn accepts_matching_still_images() {
        let artwork = validate("image/png", &png(3, 2)).unwrap();
        assert_eq!((artwork.width, artwork.height), (3, 2));
        assert_eq!(artwork.mime_type, "image/png");
        assert_eq!(artwork.sha256.len(), 64);
        assert!(validate("image/jpeg", &jpeg(4, 4)).is_ok());
    }

    #[test]
    fn rejects_mismatched_animated_and_broken_images() {
        let reason = |result: Result<AlbumArtwork>| result.unwrap_err().0[0].to_string();
        assert!(reason(validate("image/jpeg", &png(1, 1))).contains(FORMATS_DIFFER));
        let mut animated = png(1, 1);
        animated.splice(33..33, [0, 0, 0, 0, b'a', b'c', b'T', b'L', 0, 0, 0, 0]);
        assert!(reason(validate("image/png", &animated)).contains("animated"));
        let truncated = &png(8, 8)[..40];
        assert!(validate("image/png", truncated).is_err());
        assert!(reason(validate("image/png", &[])).contains("size is invalid"));
    }

    #[test]
    fn uploads_must_be_jpeg_or_png() {
        let webp = b"RIFF\x00\x00\x00\x00WEBPVP8 ";
        let failure = validate_uploaded(webp).unwrap_err();
        assert!(
            failure
                .to_string()
                .contains("uploaded artwork must be JPEG or PNG")
        );
        assert!(validate_uploaded(&png(2, 2)).is_ok());
    }
}
