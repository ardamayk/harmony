//! Album covers scaled down to a requested longest edge, generated on first
//! use and stored by the source cover's hash (library/cover_variants.go).
//! Like the Go server, only JPEG and PNG covers are scaled; anything else,
//! or a cover already within the size, is answered as is.

use super::repository::{AlbumCover, LibraryError, Repository};
use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use image::{ImageFormat, ImageReader};
use std::io::Cursor;

/// The longest-edge sizes a scaled-down cover can be asked for.
pub const COVER_SIZES: [i64; 5] = [64, 128, 256, 512, 1024];
/// The size large views ask for, the one a sharper cover can stand in at.
pub const LARGEST_COVER_SIZE: i64 = 1024;
const JPEG_QUALITY: u8 = 82;

pub async fn album_cover(
    repo: &Repository,
    album_id: &str,
    size: i64,
) -> Result<AlbumCover, LibraryError> {
    let original = repo.get_album_cover_source(album_id).await?;
    if size == 0 {
        return Ok(original);
    }
    if let Some(stored) = repo.get_album_cover_variant(album_id, size).await?
        && stored.source_sha256 == original.source_sha256
    {
        return Ok(stored);
    }
    let data = original.data.clone();
    let scaled = tokio::task::spawn_blocking(move || scale(&data, size))
        .await
        .ok()
        .flatten();
    let Some(scaled) = scaled else {
        return Ok(original);
    };
    let variant = AlbumCover {
        media_type: "image/jpeg".to_owned(),
        data: scaled,
        source_sha256: original.source_sha256,
    };
    repo.put_album_cover_variant(album_id, size, &variant)
        .await?;
    Ok(variant)
}

/// Fits the image into size×size keeping its aspect ratio, as JPEG; `None`
/// when it cannot be read or already fits.
fn scale(data: &[u8], size: i64) -> Option<Vec<u8>> {
    let reader = ImageReader::new(Cursor::new(data))
        .with_guessed_format()
        .ok()?;
    if !matches!(
        reader.format(),
        Some(ImageFormat::Jpeg | ImageFormat::Png | ImageFormat::WebP)
    ) {
        return None;
    }
    let (width, height) = reader.into_dimensions().ok()?;
    let (width, height) = (i64::from(width), i64::from(height));
    if width.max(height) <= size {
        return None;
    }
    let source = image::load_from_memory(data).ok()?;
    let (target_width, target_height) = if width >= height {
        (size, (height * size / width).max(1))
    } else {
        ((width * size / height).max(1), size)
    };
    let scaled = source
        .resize_exact(
            target_width as u32,
            target_height as u32,
            FilterType::CatmullRom,
        )
        .to_rgb8();
    let mut encoded = Vec::new();
    JpegEncoder::new_with_quality(&mut encoded, JPEG_QUALITY)
        .encode_image(&scaled)
        .ok()?;
    Some(encoded)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(width: u32, height: u32) -> Vec<u8> {
        let image = image::RgbImage::from_pixel(width, height, image::Rgb([200, 40, 10]));
        let mut encoded = Vec::new();
        image
            .write_to(&mut Cursor::new(&mut encoded), ImageFormat::Png)
            .expect("encode png");
        encoded
    }

    #[test]
    fn scales_to_the_longest_edge_keeping_the_aspect_ratio() {
        let scaled = scale(&png(400, 200), 128).expect("scaled");
        let decoded = image::load_from_memory(&scaled).expect("decode");
        assert_eq!((decoded.width(), decoded.height()), (128, 64));
        assert_eq!(
            image::guess_format(&scaled).expect("format"),
            ImageFormat::Jpeg
        );
    }

    #[test]
    fn leaves_small_and_unreadable_covers_alone() {
        assert_eq!(scale(&png(100, 100), 128), None);
        assert_eq!(scale(b"GIF89a not really", 64), None);
    }
}
