use std::io::Cursor;

use canvas_core::{EmbeddedImage, MAX_IMAGE_BYTES, MAX_IMAGE_DIMENSION, MAX_IMAGE_PIXELS};
use image::{DynamicImage, GenericImageView, ImageFormat, RgbaImage};
use thiserror::Error;

/// Errors raised while converting a dropped file into durable image state.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum ImageImportError {
    /// The source bytes exceed the document image bound.
    #[error("image exceeds the 4 MiB embedded image limit")]
    TooLarge,
    /// The source is not a supported PNG or JPEG image.
    #[error("only PNG and JPEG images are supported")]
    UnsupportedFormat,
    /// The source bytes do not decode as an image.
    #[error("image data could not be decoded")]
    Decode,
    /// The clipboard pixel buffer could not be encoded as PNG.
    #[error("clipboard image could not be encoded")]
    Encode,
    /// The decoded image dimensions exceed the document image bound.
    #[error("image dimensions exceed the 8192 pixel or 16 megapixel limit")]
    InvalidDimensions,
}

fn decode_bounded_image(bytes: &[u8]) -> Result<(&'static str, DynamicImage), ImageImportError> {
    if bytes.len() > MAX_IMAGE_BYTES {
        return Err(ImageImportError::TooLarge);
    }
    let format = image::guess_format(bytes).map_err(|_| ImageImportError::UnsupportedFormat)?;
    let mime_type = match format {
        ImageFormat::Png => "image/png",
        ImageFormat::Jpeg => "image/jpeg",
        _ => return Err(ImageImportError::UnsupportedFormat),
    };
    let decoded = image::load_from_memory(bytes).map_err(|_| ImageImportError::Decode)?;
    let (width, height) = decoded.dimensions();
    let pixel_count = u64::from(width) * u64::from(height);
    if width == 0
        || height == 0
        || width > MAX_IMAGE_DIMENSION
        || height > MAX_IMAGE_DIMENSION
        || pixel_count > MAX_IMAGE_PIXELS
    {
        return Err(ImageImportError::InvalidDimensions);
    }
    Ok((mime_type, decoded))
}

/// Decodes a dropped PNG or JPEG into a bounded embedded document payload.
pub fn embedded_image_from_bytes(bytes: Vec<u8>) -> Result<EmbeddedImage, ImageImportError> {
    let (mime_type, decoded) = decode_bounded_image(&bytes)?;
    let (width, height) = decoded.dimensions();
    Ok(EmbeddedImage::new(mime_type, width, height, bytes))
}

/// Decodes a bounded dropped image once for both the document payload and a
/// hover preview texture.
pub fn embedded_image_with_rgba(
    bytes: Vec<u8>,
) -> Result<(EmbeddedImage, Vec<u8>), ImageImportError> {
    let (mime_type, decoded) = decode_bounded_image(&bytes)?;
    let (width, height) = decoded.dimensions();
    let rgba = decoded.to_rgba8().into_raw();
    Ok((EmbeddedImage::new(mime_type, width, height, bytes), rgba))
}

/// Converts clipboard RGBA pixels into the same bounded embedded PNG payload
/// used by dropped files.
pub fn embedded_image_from_rgba(
    width: usize,
    height: usize,
    bytes: Vec<u8>,
) -> Result<EmbeddedImage, ImageImportError> {
    let width = u32::try_from(width).map_err(|_| ImageImportError::InvalidDimensions)?;
    let height = u32::try_from(height).map_err(|_| ImageImportError::InvalidDimensions)?;
    let pixel_count = u64::from(width)
        .checked_mul(u64::from(height))
        .ok_or(ImageImportError::InvalidDimensions)?;
    let expected_bytes = pixel_count
        .checked_mul(4)
        .and_then(|count| usize::try_from(count).ok())
        .ok_or(ImageImportError::InvalidDimensions)?;
    if bytes.len() != expected_bytes {
        return Err(ImageImportError::Decode);
    }
    let rgba = RgbaImage::from_raw(width, height, bytes).ok_or(ImageImportError::Decode)?;
    let mut encoded = Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(rgba)
        .write_to(&mut encoded, ImageFormat::Png)
        .map_err(|_| ImageImportError::Encode)?;
    embedded_image_from_bytes(encoded.into_inner())
}

#[cfg(test)]
mod tests {
    use super::{ImageImportError, embedded_image_from_bytes, embedded_image_from_rgba};

    const ONE_BY_ONE_PNG: &[u8] = &[
        0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, b'I', b'H', b'D',
        b'R', 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F,
        0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0D, b'I', b'D', b'A', b'T', 0x78, 0x9C, 0x63, 0xF8,
        0xCF, 0xC0, 0xF0, 0x1F, 0x00, 0x05, 0x00, 0x01, 0xFF, 0x89, 0x99, 0x3D, 0x1D, 0x00, 0x00,
        0x00, 0x00, b'I', b'E', b'N', b'D', 0xAE, 0x42, 0x60, 0x82,
    ];

    #[test]
    fn imports_a_png_with_metadata_and_original_bytes() {
        let result = embedded_image_from_bytes(ONE_BY_ONE_PNG.to_vec());
        assert!(result.is_ok());
        let Some(image) = result.ok() else {
            return;
        };
        assert_eq!(image.mime_type, "image/png");
        assert_eq!((image.width, image.height), (1, 1));
        assert_eq!(image.bytes, ONE_BY_ONE_PNG);
    }

    #[test]
    fn rejects_an_image_before_decoding_when_it_exceeds_the_bound() {
        assert!(matches!(
            embedded_image_from_bytes(vec![0; canvas_core::MAX_IMAGE_BYTES + 1]),
            Err(ImageImportError::TooLarge)
        ));
    }

    #[test]
    fn imports_clipboard_rgba_pixels_as_an_embedded_png() {
        let result = embedded_image_from_rgba(1, 1, vec![255, 0, 0, 255]);
        assert!(result.is_ok());
        let Some(image) = result.ok() else {
            return;
        };
        assert_eq!(image.mime_type, "image/png");
        assert_eq!((image.width, image.height), (1, 1));
        assert!(!image.bytes.is_empty());
    }
}
