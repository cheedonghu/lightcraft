//! Complete pixel validation for integrity-sensitive consumers, separate from
//! the tolerant photo renderer. No colour/profile conversion is needed.
use crate::{DecodeOptions, Error, Format, Header, Result, convert::check_size};

pub(super) fn validate(bytes: &[u8], format: Format, max_pixels: u64) -> Result<Header> {
    if !matches!(format, Format::Jpeg | Format::Png) {
        return Err(Error::Unsupported(format, "complete-image validation only supports JPEG and PNG"));
    }
    let header = crate::read_header(bytes)?;
    let options = DecodeOptions { max_pixels, ..Default::default() };
    check_size(format, header.width as u64, header.height as u64, &options)?;
    match format {
        Format::Jpeg => crate::jpeg::validate_complete(bytes)?,
        Format::Png => {
            let error = |e: png::DecodingError| Error::Malformed(format, e.to_string());
            let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
            decoder.set_transformations(png::Transformations::EXPAND);
            decoder.set_limits(png::Limits { bytes: max_pixels.saturating_mul(8).min(usize::MAX as u64) as usize });
            let mut reader = decoder.read_info().map_err(error)?;
            let size = reader.output_buffer_size().ok_or_else(|| Error::Malformed(format, "buffer size overflow".into()))?;
            let mut pixels = vec![0u8; size];
            reader.next_frame(&mut pixels).map_err(error)?;
            reader.finish().map_err(error)?;
        }
        _ => return Err(Error::Unsupported(format, "unsupported complete-image validation")),
    }
    Ok(header)
}
