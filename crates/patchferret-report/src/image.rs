//! Embedding a logo into the PDF, without an image-decoding dependency.
//!
//! Both supported formats are handled by *passing the compressed data straight
//! through* to a PDF image XObject, because PDF happens to speak both codecs
//! natively:
//!
//! - **JPEG** → `/DCTDecode`. The bytes are copied verbatim; only the SOF
//!   marker is read, for the dimensions and component count.
//! - **PNG** → `/FlateDecode` with a PNG predictor. A PNG's IDAT stream is
//!   already zlib, and PDF's `/Predictor 15` *is* PNG's per-scanline filtering,
//!   so the IDAT chunks concatenated together are a valid PDF stream as-is.
//!
//! No pixels are ever decoded, which is what keeps this dependency-free and
//! small enough for the WASM build.
//!
//! # What is deliberately refused
//!
//! PNGs with an alpha channel (colour types 4 and 6) and interlaced PNGs.
//! Alpha would need a separate `/SMask`, which means actually inflating the
//! image and splitting the channels — an inflate implementation this crate does
//! not have. Rather than silently flatten or emit a broken XObject, those are
//! rejected with a reason.
//!
//! The browser front end sidesteps this entirely by re-encoding whatever the
//! user picks to JPEG on a canvas first, so a transparent PNG logo still works
//! there; it is only the CLI that sees the restriction.

/// A logo prepared for embedding.
#[derive(Debug, Clone, PartialEq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    /// PDF colour space, as written into the XObject dictionary. Owned because
    /// an indexed PNG's palette is inlined here.
    pub colour_space: String,
    pub bits_per_component: u8,
    /// PDF filter name.
    pub filter: &'static str,
    /// `/DecodeParms` dictionary body, if the filter needs one.
    pub decode_parms: Option<String>,
    /// Stream payload, already in the filter's encoding.
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImageError {
    Unrecognised,
    Truncated,
    /// Understood, but cannot be embedded without decoding pixels.
    Unsupported(&'static str),
}

impl std::fmt::Display for ImageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ImageError::Unrecognised => {
                write!(f, "not a JPEG or PNG image")
            }
            ImageError::Truncated => write!(f, "image data is truncated"),
            ImageError::Unsupported(why) => write!(f, "{why}"),
        }
    }
}

impl Image {
    pub fn parse(bytes: &[u8]) -> Result<Image, ImageError> {
        if bytes.starts_with(&[0xFF, 0xD8]) {
            jpeg(bytes)
        } else if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
            png(bytes)
        } else {
            Err(ImageError::Unrecognised)
        }
    }

    /// Height in points for a given printed width, preserving aspect ratio.
    pub fn height_for_width(&self, width: f32) -> f32 {
        if self.width == 0 {
            return 0.0;
        }
        width * self.height as f32 / self.width as f32
    }
}

/// Read a JPEG's SOF segment for dimensions and component count.
fn jpeg(bytes: &[u8]) -> Result<Image, ImageError> {
    let mut i = 2usize;
    while i + 3 < bytes.len() {
        if bytes[i] != 0xFF {
            i += 1;
            continue;
        }
        let marker = bytes[i + 1];
        // Standalone markers carry no length.
        if marker == 0xD8 || marker == 0x01 || (0xD0..=0xD7).contains(&marker) {
            i += 2;
            continue;
        }
        let len = u16::from_be_bytes([bytes[i + 2], bytes[i + 3]]) as usize;
        if len < 2 || i + 2 + len > bytes.len() {
            return Err(ImageError::Truncated);
        }
        // Any SOF except the arithmetic-coded and hierarchical oddities.
        let is_sof = matches!(marker, 0xC0..=0xCF) && !matches!(marker, 0xC4 | 0xC8 | 0xCC);
        if is_sof {
            let seg = &bytes[i + 4..i + 2 + len];
            if seg.len() < 6 {
                return Err(ImageError::Truncated);
            }
            let bpc = seg[0];
            let height = u16::from_be_bytes([seg[1], seg[2]]) as u32;
            let width = u16::from_be_bytes([seg[3], seg[4]]) as u32;
            let components = seg[5];
            let colour_space = match components {
                1 => "/DeviceGray",
                3 => "/DeviceRGB",
                4 => "/DeviceCMYK",
                _ => return Err(ImageError::Unsupported("unsupported JPEG component count")),
            }
            .to_string();
            if width == 0 || height == 0 {
                return Err(ImageError::Unsupported("JPEG reports a zero dimension"));
            }
            return Ok(Image {
                width,
                height,
                colour_space,
                bits_per_component: bpc,
                filter: "/DCTDecode",
                decode_parms: None,
                data: bytes.to_vec(),
            });
        }
        i += 2 + len;
    }
    Err(ImageError::Truncated)
}

/// Concatenate a PNG's IDAT chunks and describe them as a Flate stream.
fn png(bytes: &[u8]) -> Result<Image, ImageError> {
    let mut i = 8usize;
    let mut width = 0u32;
    let mut height = 0u32;
    let mut bit_depth = 0u8;
    let mut colour_type = 0u8;
    let mut idat: Vec<u8> = Vec::new();
    let mut palette: Option<Vec<u8>> = None;
    let mut seen_ihdr = false;

    while i + 8 <= bytes.len() {
        let len =
            u32::from_be_bytes([bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]]) as usize;
        let kind = &bytes[i + 4..i + 8];
        let start = i + 8;
        let end = start.checked_add(len).ok_or(ImageError::Truncated)?;
        if end + 4 > bytes.len() {
            return Err(ImageError::Truncated);
        }
        let body = &bytes[start..end];

        match kind {
            b"IHDR" => {
                if body.len() < 13 {
                    return Err(ImageError::Truncated);
                }
                width = u32::from_be_bytes([body[0], body[1], body[2], body[3]]);
                height = u32::from_be_bytes([body[4], body[5], body[6], body[7]]);
                bit_depth = body[8];
                colour_type = body[9];
                if body[12] != 0 {
                    return Err(ImageError::Unsupported(
                        "interlaced PNG cannot be embedded without decoding; save it \
                         non-interlaced or use a JPEG",
                    ));
                }
                seen_ihdr = true;
            }
            b"PLTE" => palette = Some(body.to_vec()),
            b"IDAT" => idat.extend_from_slice(body),
            b"IEND" => break,
            _ => {}
        }
        i = end + 4; // skip CRC
    }

    if !seen_ihdr || width == 0 || height == 0 {
        return Err(ImageError::Truncated);
    }
    if idat.is_empty() {
        return Err(ImageError::Truncated);
    }

    let (colour_space, colours) = match colour_type {
        0 => ("/DeviceGray".to_string(), 1),
        2 => ("/DeviceRGB".to_string(), 3),
        3 => {
            let Some(pal) = palette else {
                return Err(ImageError::Truncated);
            };
            // An indexed image needs its palette inline; hand it back as a
            // hex-encoded /Indexed colour space.
            let hex: String = pal.iter().map(|b| format!("{b:02X}")).collect();
            let n = pal.len() / 3;
            if n == 0 {
                return Err(ImageError::Truncated);
            }
            let space = format!("[/Indexed /DeviceRGB {} <{}>]", n - 1, hex);
            return Ok(Image {
                width,
                height,
                colour_space: space,
                bits_per_component: bit_depth,
                filter: "/FlateDecode",
                decode_parms: Some(format!(
                    "<< /Predictor 15 /Colors 1 /BitsPerComponent {bit_depth} /Columns {width} >>"
                )),
                data: idat,
            });
        }
        4 | 6 => return Err(ImageError::Unsupported(
            "PNG has an alpha channel, which needs the pixels decoded to build a soft mask; \
                 use a JPEG or a PNG without transparency",
        )),
        _ => return Err(ImageError::Unsupported("unknown PNG colour type")),
    };

    Ok(Image {
        width,
        height,
        colour_space,
        bits_per_component: bit_depth,
        filter: "/FlateDecode",
        decode_parms: Some(format!(
            "<< /Predictor 15 /Colors {colours} /BitsPerComponent {bit_depth} /Columns {width} >>"
        )),
        data: idat,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal JPEG: SOI, an SOF0 describing the size, EOI.
    fn fake_jpeg(w: u16, h: u16, components: u8) -> Vec<u8> {
        let mut v = vec![0xFF, 0xD8];
        let seg_len: u16 = 8 + 3 * (components as u16 - 1);
        v.extend_from_slice(&[0xFF, 0xC0]);
        v.extend_from_slice(&seg_len.to_be_bytes());
        v.push(8); // bits per component
        v.extend_from_slice(&h.to_be_bytes());
        v.extend_from_slice(&w.to_be_bytes());
        v.push(components);
        for c in 0..components {
            v.extend_from_slice(&[c + 1, 0x11, 0]);
        }
        v.extend_from_slice(&[0xFF, 0xD9]);
        v
    }

    fn png_chunk(kind: &[u8], body: &[u8]) -> Vec<u8> {
        let mut v = (body.len() as u32).to_be_bytes().to_vec();
        v.extend_from_slice(kind);
        v.extend_from_slice(body);
        v.extend_from_slice(&[0, 0, 0, 0]); // CRC, not checked
        v
    }

    fn fake_png(w: u32, h: u32, colour_type: u8, interlace: u8) -> Vec<u8> {
        let mut ihdr = Vec::new();
        ihdr.extend_from_slice(&w.to_be_bytes());
        ihdr.extend_from_slice(&h.to_be_bytes());
        ihdr.extend_from_slice(&[8, colour_type, 0, 0, interlace]);

        let mut v = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        v.extend_from_slice(&png_chunk(b"IHDR", &ihdr));
        if colour_type == 3 {
            v.extend_from_slice(&png_chunk(b"PLTE", &[255, 0, 0, 0, 255, 0]));
        }
        v.extend_from_slice(&png_chunk(b"IDAT", &[0x78, 0x9C, 0x00]));
        v.extend_from_slice(&png_chunk(b"IEND", &[]));
        v
    }

    #[test]
    fn reads_jpeg_dimensions_and_colour_space() {
        let img = Image::parse(&fake_jpeg(640, 480, 3)).expect("jpeg");
        assert_eq!((img.width, img.height), (640, 480));
        assert_eq!(img.colour_space, "/DeviceRGB");
        assert_eq!(img.filter, "/DCTDecode");
        // The whole file is the stream — DCTDecode takes the JPEG verbatim.
        assert_eq!(img.data.len(), fake_jpeg(640, 480, 3).len());
    }

    #[test]
    fn reads_greyscale_and_cmyk_jpeg() {
        assert_eq!(Image::parse(&fake_jpeg(8, 8, 1)).unwrap().colour_space, "/DeviceGray");
        assert_eq!(Image::parse(&fake_jpeg(8, 8, 4)).unwrap().colour_space, "/DeviceCMYK");
    }

    #[test]
    fn reads_png_rgb_and_sets_a_predictor() {
        let img = Image::parse(&fake_png(100, 50, 2, 0)).expect("png");
        assert_eq!((img.width, img.height), (100, 50));
        assert_eq!(img.filter, "/FlateDecode");
        let parms = img.decode_parms.unwrap();
        // Predictor 15 is what makes PNG's own filtering legal in a PDF stream.
        assert!(parms.contains("/Predictor 15"), "{parms}");
        assert!(parms.contains("/Colors 3"), "{parms}");
        assert!(parms.contains("/Columns 100"), "{parms}");
    }

    #[test]
    fn reads_indexed_png_with_its_palette() {
        let img = Image::parse(&fake_png(4, 4, 3, 0)).expect("indexed png");
        assert!(
            img.colour_space.starts_with("[/Indexed /DeviceRGB 1 <"),
            "{}",
            img.colour_space
        );
    }

    #[test]
    fn refuses_alpha_png_with_a_reason() {
        for ct in [4u8, 6] {
            match Image::parse(&fake_png(10, 10, ct, 0)) {
                Err(ImageError::Unsupported(why)) => assert!(why.contains("alpha")),
                other => panic!("expected an alpha refusal, got {other:?}"),
            }
        }
    }

    #[test]
    fn refuses_interlaced_png() {
        match Image::parse(&fake_png(10, 10, 2, 1)) {
            Err(ImageError::Unsupported(why)) => assert!(why.contains("interlaced")),
            other => panic!("expected an interlace refusal, got {other:?}"),
        }
    }

    #[test]
    fn rejects_other_formats_and_junk() {
        assert_eq!(Image::parse(b"GIF89a").unwrap_err(), ImageError::Unrecognised);
        assert_eq!(Image::parse(b"").unwrap_err(), ImageError::Unrecognised);
    }

    #[test]
    fn truncated_images_do_not_panic() {
        let j = fake_jpeg(64, 64, 3);
        let p = fake_png(64, 64, 2, 0);
        for cut in 0..j.len() {
            let _ = Image::parse(&j[..cut]);
        }
        for cut in 0..p.len() {
            let _ = Image::parse(&p[..cut]);
        }
    }

    #[test]
    fn aspect_ratio_is_preserved() {
        let img = Image::parse(&fake_jpeg(200, 100, 3)).unwrap();
        assert_eq!(img.height_for_width(50.0), 25.0);
    }
}
