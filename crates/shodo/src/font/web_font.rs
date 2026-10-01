#[cfg(all(test, feature = "web-fonts"))]
mod tests {
    use super::*;
    use crate::font::{FontCollection, FontFaceDescriptor, FontOptions, FontQuery, FontSource};
    use crate::limits::{LimitKind, Limits};
    use std::io::Write;

    fn tables(sfnt: &[u8]) -> Vec<([u8; 4], Vec<u8>)> {
        let count = u16::from_be_bytes(sfnt[4..6].try_into().unwrap()) as usize;
        (0..count)
            .map(|n| {
                let at = 12 + 16 * n;
                let start = u32::from_be_bytes(sfnt[at + 8..at + 12].try_into().unwrap()) as usize;
                let len = u32::from_be_bytes(sfnt[at + 12..at + 16].try_into().unwrap()) as usize;
                (
                    sfnt[at..at + 4].try_into().unwrap(),
                    sfnt[start..start + len].to_vec(),
                )
            })
            .collect()
    }
    fn font() -> Vec<u8> {
        crate::font::browser_tests::test_font("Web", &['0', '水'], 600)
    }
    fn woff1(sfnt: &[u8]) -> Vec<u8> {
        let tables = tables(sfnt);
        let mut header = vec![0; 44];
        header[..4].copy_from_slice(b"wOFF");
        header[4..8].copy_from_slice(&sfnt[..4]);
        header[12..14].copy_from_slice(&(tables.len() as u16).to_be_bytes());
        header[16..20].copy_from_slice(&(sfnt.len() as u32).to_be_bytes());
        let mut directory = Vec::new();
        let mut body = Vec::new();
        for (tag, raw) in &tables {
            let mut encoder =
                flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
            encoder.write_all(raw).unwrap();
            let compressed = encoder.finish().unwrap();
            let data = if compressed.len() < raw.len() {
                &compressed
            } else {
                raw
            };
            directory.extend_from_slice(tag);
            for value in [
                44 + 20 * tables.len() + body.len(),
                data.len(),
                raw.len(),
                0,
            ] {
                directory.extend_from_slice(&(value as u32).to_be_bytes());
            }
            body.extend(data);
            while !body.len().is_multiple_of(4) {
                body.push(0);
            }
        }
        header.extend(directory);
        header.extend(body);
        let len = header.len() as u32;
        header[8..12].copy_from_slice(&len.to_be_bytes());
        header
    }
    fn ub128(mut value: usize) -> Vec<u8> {
        let mut out = vec![(value & 127) as u8];
        value >>= 7;
        while value > 0 {
            out.push((value & 127) as u8 | 128);
            value >>= 7;
        }
        out.reverse();
        out
    }
    fn woff2(sfnt: &[u8]) -> Vec<u8> {
        let tables = tables(sfnt);
        let mut header = vec![0; 48];
        header[..4].copy_from_slice(b"wOF2");
        header[4..8].copy_from_slice(&sfnt[..4]);
        header[12..14].copy_from_slice(&(tables.len() as u16).to_be_bytes());
        header[16..20].copy_from_slice(&(sfnt.len() as u32).to_be_bytes());
        let mut directory = Vec::new();
        let mut raw = Vec::new();
        for (tag, data) in tables {
            directory.push(63);
            directory.extend(tag);
            directory.extend(ub128(data.len()));
            raw.extend(data);
        }
        let mut compressed = Vec::new();
        {
            let mut writer = brotli::CompressorWriter::new(&mut compressed, 4096, 5, 22);
            writer.write_all(&raw).unwrap();
        }
        header[20..24].copy_from_slice(&(compressed.len() as u32).to_be_bytes());
        header.extend(directory);
        header.extend(compressed);
        while !header.len().is_multiple_of(4) {
            header.push(0);
        }
        let len = header.len() as u32;
        header[8..12].copy_from_slice(&len.to_be_bytes());
        header
    }

    #[test]
    fn woff_and_woff2_preserve_cmap_and_metrics_through_ordered_sources() {
        for data in [woff1(&font()), woff2(&font())] {
            let decoded = decode_web_font(&data, &Limits::default()).unwrap();
            assert!(crate::font::check::check_font(&decoded, &Limits::default()).is_ok());
            let fonts = FontCollection::with_options(
                &Limits::default(),
                FontOptions {
                    system_fonts: false,
                    ..Default::default()
                },
            );
            let id = fonts
                .register_sources(
                    FontFaceDescriptor {
                        family: "Alias".into(),
                        ..Default::default()
                    },
                    vec![FontSource::Data(data, 0)],
                )
                .unwrap();
            let query = FontQuery {
                families: vec![crate::style::FontFamily::Named("Alias".into())],
                ..Default::default()
            };
            assert_eq!(fonts.resolve_ch(&query, 10.).id, Some(id));
            assert!((fonts.resolve_ch(&query, 10.).advance - 6.).abs() < 0.0001);
            assert_eq!(fonts.metrics(id, 10.).ascent, 7.5);
        }
    }

    #[test]
    fn invalid_web_font_axes_are_rejected_and_sources_fall_through() {
        let invalid = crate::font::browser_tests::font_with_axes(&[(*b"opsz", [72, 12, 8])]);
        for bytes in [invalid.clone(), woff1(&invalid), woff2(&invalid)] {
            assert!(matches!(
                decode_web_font(&bytes, &Limits::default()),
                Err(FontError::Malformed(_))
            ));
            let fonts = FontCollection::with_options(
                &Limits::default(),
                FontOptions {
                    system_fonts: false,
                    ..Default::default()
                },
            );
            let id = fonts
                .register_sources(
                    FontFaceDescriptor {
                        family: "Alias".into(),
                        ..Default::default()
                    },
                    vec![FontSource::Data(bytes, 0), FontSource::Data(font(), 0)],
                )
                .unwrap();
            assert_eq!(fonts.generation(), 1);
            assert_eq!(
                fonts.font_data(id).unwrap().data.as_ref(),
                font().as_slice()
            );
        }
    }

    #[test]
    fn table_directory_sizes_are_budgeted_independently_of_sfnt_header_size() {
        let mut data = woff2(&font());
        data[16..20].copy_from_slice(&1u32.to_be_bytes());
        let limits = Limits {
            max_font_blob_bytes: Some(font().len() as u64 - 1),
            ..Default::default()
        };
        assert!((data.len() as u64) < limits.max_font_blob_bytes.unwrap());
        assert!(
            matches!(decode_web_font(&data,&limits),Err(crate::font::FontError::Limit(e)) if e.kind==LimitKind::FontBlobBytes)
        );
    }

    #[test]
    fn truncated_and_zero_table_web_fonts_return_errors_without_panics() {
        for mut data in [woff1(&font()), woff2(&font())] {
            for length in 0..data.len() {
                assert!(decode_web_font(&data[..length], &Limits::default()).is_err());
            }
            data[12..14].fill(0);
            assert!(decode_web_font(&data, &Limits::default()).is_err());
        }
    }
}

use super::{FontError, check};
use crate::limits::{LimitKind, Limits};

/// Converts WOFF/WOFF2 into structurally checked sfnt/TTC bytes. Plain
/// sfnt/TTC is accepted unchanged. Compressed formats require `web-fonts`.
/// Checks the input and directory-derived decoded sizes before decompression,
/// and the final reconstructed size before returning. wuff additionally caps
/// reconstructed output above 128 MiB. Transformed-table reconstruction
/// can allocate transient buffers larger than `max_font_blob_bytes`.
pub fn decode_web_font(data: &[u8], limits: &Limits) -> Result<Vec<u8>, FontError> {
    Limits::check(
        limits.max_font_blob_bytes,
        LimitKind::FontBlobBytes,
        data.len() as u64,
    )?;
    if !matches!(data.get(..4), Some(b"wOFF" | b"wOF2")) {
        check::check_font(data, limits)?;
        return Ok(data.to_vec());
    }
    #[cfg(not(feature = "web-fonts"))]
    {
        Err(FontError::Malformed("web-fonts feature is disabled"))
    }
    #[cfg(feature = "web-fonts")]
    {
        let woff2 = data.get(..4) == Some(b"wOF2");
        check_web_directory(data, woff2, limits)?;
        use std::io::Read;
        let mut callback =
            |compressed: &[u8], size: usize| -> Result<Vec<u8>, Box<dyn std::error::Error>> {
                Limits::check(
                    limits.max_font_blob_bytes,
                    LimitKind::FontBlobBytes,
                    size as u64,
                )?;
                let decoder: Box<dyn Read + '_> = if woff2 {
                    Box::new(brotli_decompressor::Decompressor::new(compressed, 4096))
                } else {
                    Box::new(flate2::read::ZlibDecoder::new(compressed))
                };
                let mut result = Vec::with_capacity(size);
                decoder.take(size as u64 + 1).read_to_end(&mut result)?;
                if result.len() != size {
                    return Err(Box::new(FontError::Malformed(
                        "decoded stream length mismatch",
                    )));
                }
                Ok(result)
            };
        let output = if woff2 {
            wuff::decompress_woff2_with_custom_brotli(data, &mut callback)
        } else {
            wuff::decompress_woff1_with_custom_z(data, &mut callback)
        }
        .map_err(|_| FontError::Malformed("invalid WOFF/WOFF2"))?;
        check::check_font(&output, limits)?;
        Ok(output)
    }
}

#[cfg(feature = "web-fonts")]
struct WebCursor<'a> {
    bytes: &'a [u8],
    index: usize,
}
#[cfg(feature = "web-fonts")]
impl WebCursor<'_> {
    fn byte(&mut self) -> Result<u8, FontError> {
        let b = *self.bytes.get(self.index).ok_or(check::TRUNCATED)?;
        self.index += 1;
        Ok(b)
    }
    fn word(&mut self) -> Result<u32, FontError> {
        let value = check::read_u32(self.bytes, self.index)?;
        self.index += 4;
        Ok(value)
    }
    fn base128(&mut self) -> Result<u32, FontError> {
        let mut value = 0u32;
        for index in 0..5 {
            let byte = self.byte()?;
            if (index == 0 && byte == 128) || value > 0x01ff_ffff {
                return Err(check::TRUNCATED);
            }
            value = (value << 7) | u32::from(byte & 127);
            if byte & 128 == 0 {
                return Ok(value);
            }
        }
        Err(check::TRUNCATED)
    }
    fn uint255(&mut self) -> Result<u16, FontError> {
        Ok(match self.byte()? {
            253 => {
                let high = self.byte()?;
                u16::from_be_bytes([high, self.byte()?])
            }
            254 => 506 + u16::from(self.byte()?),
            255 => 253 + u16::from(self.byte()?),
            b => u16::from(b),
        })
    }
}

#[cfg(feature = "web-fonts")]
fn check_web_directory(data: &[u8], woff2: bool, limits: &Limits) -> Result<(), FontError> {
    if check::read_u32(data, 8)? as usize != data.len() || check::read_u16(data, 14)? != 0 {
        return Err(FontError::Malformed("invalid web font header"));
    }
    let count = usize::from(check::read_u16(data, 12)?);
    if count == 0 || count > 4095 {
        return Err(FontError::Malformed("invalid web font table count"));
    }
    let mut header = 12u64 + 16 * count as u64;
    let mut tables = 0u64;
    if !woff2 {
        for n in 0..count {
            let length = check::read_u32(data, 44 + 20 * n + 12)?;
            tables += (u64::from(length) + 3) & !3;
            Limits::check(
                limits.max_font_blob_bytes,
                LimitKind::FontBlobBytes,
                header + tables,
            )?;
        }
    } else {
        let mut cursor = WebCursor {
            bytes: data,
            index: 48,
        };
        let mut stream = 0u64;
        for _ in 0..count {
            let flags = cursor.byte()?;
            let tag = match flags & 63 {
                63 => Some(cursor.word()?.to_be_bytes()),
                10 => Some(*b"glyf"),
                11 => Some(*b"loca"),
                _ => None,
            };
            let original = cursor.base128()?;
            tables += (u64::from(original) + 3) & !3;
            let transformed = if matches!(tag,Some(t) if t==*b"glyf" || t==*b"loca") {
                flags >> 6 != 3
            } else {
                flags >> 6 != 0
            };
            stream += u64::from(if transformed {
                cursor.base128()?
            } else {
                original
            });
            Limits::check(limits.max_font_blob_bytes, LimitKind::FontBlobBytes, tables)?;
            Limits::check(limits.max_font_blob_bytes, LimitKind::FontBlobBytes, stream)?;
        }
        if data.get(4..8) == Some(b"ttcf") {
            let version = cursor.word()?;
            let faces = cursor.uint255()?;
            if faces == 0 {
                return Err(FontError::Malformed("empty WOFF2 collection"));
            }
            Limits::check(limits.max_ttc_faces, LimitKind::TtcFaces, u64::from(faces))?;
            header = 12 + 4 * u64::from(faces) + if version == 0x0002_0000 { 12 } else { 0 };
            for _ in 0..faces {
                let count_in_face = cursor.uint255()?;
                cursor.word()?;
                if count_in_face == 0 || count_in_face > 4095 {
                    return Err(FontError::Malformed("invalid WOFF2 face directory"));
                }
                header += 12 + 16 * u64::from(count_in_face);
                Limits::check(
                    limits.max_font_blob_bytes,
                    LimitKind::FontBlobBytes,
                    header + tables,
                )?;
                for _ in 0..count_in_face {
                    if usize::from(cursor.uint255()?) >= count {
                        return Err(check::TRUNCATED);
                    }
                }
            }
        }
    }
    Limits::check(
        limits.max_font_blob_bytes,
        LimitKind::FontBlobBytes,
        header + tables,
    )?;
    Ok(())
}
