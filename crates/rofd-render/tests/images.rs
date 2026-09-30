use std::io::{Cursor, Write};
use std::sync::{Arc, Barrier};
use std::thread;

use rofd_core::{Document, LoadOptions, ResourceLimits};
use rofd_render::{Error, ImageDecoder};
use zip::{write::SimpleFileOptions, ZipWriter};

const PNG: &[u8] = include_bytes!("fixtures/images/asymmetric-rgba.png");
const JPEG: &[u8] = include_bytes!("fixtures/images/asymmetric-rgb.jpg");
const BMP: &[u8] = include_bytes!("fixtures/images/asymmetric-rgb.bmp");
const GIF: &[u8] = include_bytes!("fixtures/images/asymmetric-rgb.gif");
const TIFF: &[u8] = include_bytes!("fixtures/images/asymmetric-rgb.tiff");

fn package(bytes: &[u8], format: &str, limits: ResourceLimits) -> Document {
    package_many(&[(10, format, "image.bin", bytes)], limits)
}

fn package_many(entries: &[(u64, &str, &str, &[u8])], limits: ResourceLimits) -> Document {
    let resources = entries
        .iter()
        .map(|(id, format, path, _)| {
            format!(
                r#"<ofd:MultiMedia ID="{id}" Type="Image" Format="{format}"><ofd:MediaFile>{path}</ofd:MediaFile></ofd:MultiMedia>"#,
            )
        })
        .collect::<String>();
    let resource_xml = format!(
        r#"<ofd:Res xmlns:ofd="http://www.ofdspec.org/2016"><ofd:MultiMedias>{resources}</ofd:MultiMedias></ofd:Res>"#
    );
    let files = [
        (
            "OFD.xml",
            br#"<ofd:OFD xmlns:ofd="http://www.ofdspec.org/2016"><ofd:DocBody><ofd:DocInfo><ofd:DocID>images</ofd:DocID></ofd:DocInfo><ofd:DocRoot>Doc_0/Document.xml</ofd:DocRoot></ofd:DocBody></ofd:OFD>"#.as_slice(),
        ),
        (
            "Doc_0/Document.xml",
            br#"<ofd:Document xmlns:ofd="http://www.ofdspec.org/2016"><ofd:CommonData><ofd:PageArea><ofd:PhysicalBox>0 0 30 30</ofd:PhysicalBox></ofd:PageArea><ofd:DocumentRes>Res.xml</ofd:DocumentRes></ofd:CommonData><ofd:Pages><ofd:Page ID="1" BaseLoc="Page.xml"/></ofd:Pages></ofd:Document>"#.as_slice(),
        ),
        ("Doc_0/Res.xml", resource_xml.as_bytes()),
        (
            "Doc_0/Page.xml",
            br#"<ofd:Page xmlns:ofd="http://www.ofdspec.org/2016"><ofd:Area><ofd:PhysicalBox>0 0 30 30</ofd:PhysicalBox></ofd:Area><ofd:Content/></ofd:Page>"#.as_slice(),
        ),
    ];
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    for (name, contents) in files {
        writer
            .start_file(name, SimpleFileOptions::default())
            .unwrap();
        writer.write_all(contents).unwrap();
    }
    for (_, _, path, contents) in entries {
        writer
            .start_file(format!("Doc_0/{path}"), SimpleFileOptions::default())
            .unwrap();
        writer.write_all(contents).unwrap();
    }
    Document::from_bytes(
        writer.finish().unwrap().into_inner(),
        LoadOptions {
            limits,
            ..LoadOptions::default()
        },
    )
    .unwrap()
}

fn decode(bytes: &[u8], format: &str) -> rofd_render::Result<rofd_render::DecodedImage> {
    let document = package(bytes, format, ResourceLimits::default());
    ImageDecoder::default().decode(
        &document.image_resource(10).unwrap(),
        &ResourceLimits::default(),
    )
}

fn png_header(width: u32, height: u32) -> Vec<u8> {
    let mut bytes = Vec::from(&b"\x89PNG\r\n\x1a\n\x00\x00\x00\x0dIHDR"[..]);
    bytes.extend_from_slice(&width.to_be_bytes());
    bytes.extend_from_slice(&height.to_be_bytes());
    bytes.extend_from_slice(&[8, 6, 0, 0, 0]);
    let crc = crc32(&bytes[12..]);
    bytes.extend_from_slice(&crc.to_be_bytes());
    bytes
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb8_8320 & 0u32.wrapping_sub(crc & 1));
        }
    }
    !crc
}

#[test]
fn png_decodes_to_native_rgba_with_orientation_and_transparency() {
    let image = decode(PNG, "PNG").unwrap();
    assert_eq!(image.resource_id(), 10);
    assert_eq!(image.width(), 3);
    assert_eq!(image.height(), 2);
    assert_eq!(image.dimensions(), (3, 2));
    assert_eq!(image.stride(), 12);
    assert_eq!(image.rgba().len(), 24);
    assert_eq!(image.pixel(0, 0), Some([255, 0, 0, 255]));
    assert_eq!(image.pixel(1, 0), Some([0, 255, 0, 128]));
    assert_eq!(image.pixel(2, 0), Some([0, 0, 255, 255]));
    assert_eq!(image.pixel(0, 1), Some([255, 255, 0, 255]));
    assert_eq!(image.pixel(2, 1), Some([0, 255, 255, 255]));
    assert_eq!(image.pixel(3, 0), None);
    assert_eq!(image.pixel(0, 2), None);
}

#[test]
fn jpeg_decodes_in_source_orientation_with_opaque_rgba_channels() {
    let image = decode(JPEG, "JPEG").unwrap();
    assert_eq!(image.dimensions(), (3, 2));
    for ((x, y), expected) in [
        ((0, 0), [223, 34, 28]),
        ((1, 0), [31, 225, 32]),
        ((2, 0), [30, 35, 223]),
        ((0, 1), [226, 222, 29]),
        ((1, 1), [222, 32, 230]),
        ((2, 1), [35, 223, 232]),
    ] {
        let actual = image.pixel(x, y).unwrap();
        for channel in 0..3 {
            assert!(actual[channel].abs_diff(expected[channel]) <= 12);
        }
        assert_eq!(actual[3], 255);
    }
}

#[test]
fn lossless_formats_decode_in_source_orientation_with_opaque_rgba_channels() {
    for (bytes, format) in [(BMP, "bmp"), (GIF, "gif"), (TIFF, "tiff")] {
        let image = decode(bytes, format).unwrap();
        assert_eq!(image.dimensions(), (3, 2), "{format}");
        for ((x, y), expected) in [
            ((0, 0), [224, 32, 32, 255]),
            ((1, 0), [32, 224, 32, 255]),
            ((2, 0), [32, 32, 224, 255]),
            ((0, 1), [224, 224, 32, 255]),
            ((1, 1), [224, 32, 224, 255]),
            ((2, 1), [32, 224, 224, 255]),
        ] {
            assert_eq!(image.pixel(x, y), Some(expected), "{format} at ({x}, {y})");
        }
    }
}

#[test]
fn magic_is_required_and_overrides_the_declared_format() {
    // Real-world producers misdeclare formats (invoices declaring PNG for
    // JPEG data); the byte signature is authoritative when the two disagree.
    let png_declared_jpeg = decode(PNG, "JPEG").unwrap();
    assert_eq!(png_declared_jpeg.resource_id(), 10);
    assert_eq!(png_declared_jpeg.dimensions(), (3, 2));
    let jpeg_declared_png = decode(JPEG, "PNG").unwrap();
    assert_eq!(jpeg_declared_png.dimensions(), (3, 2));
    assert!(matches!(
        decode(b"WEBP-not-supported", "PNG"),
        Err(Error::UnsupportedImageFormat {
            resource_id: 10,
            ..
        })
    ));
}

#[test]
fn corrupt_and_truncated_images_are_structured_and_retryable() {
    for (bytes, format) in [
        (&PNG[..20], "PNG"),
        (&PNG[..PNG.len() / 2], "PNG"),
        (&JPEG[..12], "JPEG"),
    ] {
        let document = package(bytes, format, ResourceLimits::default());
        let resource = document.image_resource(10).unwrap();
        let decoder = ImageDecoder::default();
        let first = decoder.decode(&resource, &ResourceLimits::default());
        let second = decoder.decode(&resource, &ResourceLimits::default());
        assert!(matches!(
            first,
            Err(Error::ImageDecode {
                resource_id: 10,
                ..
            })
        ));
        assert!(matches!(
            second,
            Err(Error::ImageDecode {
                resource_id: 10,
                ..
            })
        ));
    }
}

#[test]
fn decoded_limits_have_exact_and_one_over_boundaries() {
    let document = package(PNG, "PNG", ResourceLimits::default());
    let resource = document.image_resource(10).unwrap();
    let decoder = ImageDecoder::default();
    let exact = ResourceLimits {
        max_decoded_image_pixels: 6,
        max_decoded_image_bytes: 24,
        ..ResourceLimits::default()
    };
    assert!(decoder.decode(&resource, &exact).is_ok());
    for limits in [
        ResourceLimits {
            max_decoded_image_pixels: 5,
            ..exact.clone()
        },
        ResourceLimits {
            max_decoded_image_bytes: 23,
            ..exact.clone()
        },
    ] {
        assert!(matches!(
            decoder.decode(&resource, &limits),
            Err(Error::ImageLimitExceeded {
                resource_id: 10,
                ..
            })
        ));
    }
}

#[test]
fn huge_and_overflowing_headers_fail_before_pixel_allocation() {
    let huge = png_header(100_001, 100_001);
    assert!(matches!(
        decode(&huge, "PNG"),
        Err(Error::ImageLimitExceeded {
            resource_id: 10,
            ..
        })
    ));

    let overflow = package(
        &png_header(u32::MAX, u32::MAX),
        "PNG",
        ResourceLimits {
            max_decoded_image_pixels: u64::MAX,
            max_decoded_image_bytes: u64::MAX,
            ..ResourceLimits::default()
        },
    );
    assert!(matches!(
        ImageDecoder::default().decode(
            &overflow.image_resource(10).unwrap(),
            &ResourceLimits {
                max_decoded_image_pixels: u64::MAX,
                max_decoded_image_bytes: u64::MAX,
                ..ResourceLimits::default()
            }
        ),
        Err(Error::ImageDimensionsOverflow {
            resource_id: 10,
            ..
        })
    ));

    assert!(matches!(
        decode(&png_header(0, 2), "PNG"),
        Err(Error::InvalidImageDimensions {
            resource_id: 10,
            ..
        })
    ));
}

#[test]
fn encoded_limit_is_enforced_by_core_and_again_at_decode_boundary() {
    let exact_document = package(
        PNG,
        "PNG",
        ResourceLimits {
            max_encoded_image_bytes: PNG.len() as u64,
            ..ResourceLimits::default()
        },
    );
    assert!(exact_document.image_resource(10).is_ok());

    let core_limits = ResourceLimits {
        max_encoded_image_bytes: (PNG.len() - 1) as u64,
        ..ResourceLimits::default()
    };
    let document = package(PNG, "PNG", core_limits);
    assert!(document.image_resource(10).is_err());

    let document = package(PNG, "PNG", ResourceLimits::default());
    let resource = document.image_resource(10).unwrap();
    assert!(matches!(
        ImageDecoder::default().decode(
            &resource,
            &ResourceLimits {
                max_encoded_image_bytes: (PNG.len() - 1) as u64,
                ..ResourceLimits::default()
            }
        ),
        Err(Error::ImageLimitExceeded {
            resource_id: 10,
            field: "encoded bytes",
            ..
        })
    ));
}

#[test]
fn cache_hits_never_bypass_the_current_callers_limits() {
    let document = package(PNG, "PNG", ResourceLimits::default());
    let resource = document.image_resource(10).unwrap();
    let decoder = ImageDecoder::default();
    decoder
        .decode(&resource, &ResourceLimits::default())
        .unwrap();
    let restrictive = ResourceLimits {
        max_decoded_image_pixels: 5,
        ..ResourceLimits::default()
    };
    assert!(matches!(
        decoder.decode(&resource, &restrictive),
        Err(Error::ImageLimitExceeded {
            field: "decoded pixels",
            ..
        })
    ));

    let other_decoder = ImageDecoder::default();
    assert!(other_decoder.decode(&resource, &restrictive).is_err());
    assert!(other_decoder
        .decode(&resource, &ResourceLimits::default())
        .is_ok());
}

#[test]
fn cache_reuses_bytes_evicts_by_decoded_bytes_and_returns_oversize_uncached() {
    let document = package_many(
        &[(10, "PNG", "a.png", PNG), (11, "JPEG", "b.jpg", JPEG)],
        ResourceLimits::default(),
    );
    let first = document.image_resource(10).unwrap();
    let second = document.image_resource(11).unwrap();
    let decoder = ImageDecoder::with_cache_byte_budget(24).unwrap();
    let a1 = decoder.decode(&first, &ResourceLimits::default()).unwrap();
    let a2 = decoder.decode(&first, &ResourceLimits::default()).unwrap();
    assert!(Arc::ptr_eq(&a1.rgba_arc(), &a2.rgba_arc()));
    decoder.decode(&second, &ResourceLimits::default()).unwrap();
    let a3 = decoder.decode(&first, &ResourceLimits::default()).unwrap();
    assert!(!Arc::ptr_eq(&a1.rgba_arc(), &a3.rgba_arc()));

    let uncached = ImageDecoder::with_cache_byte_budget(23).unwrap();
    let first_decode = uncached.decode(&first, &ResourceLimits::default()).unwrap();
    let second_decode = uncached.decode(&first, &ResourceLimits::default()).unwrap();
    assert!(!Arc::ptr_eq(
        &first_decode.rgba_arc(),
        &second_decode.rgba_arc()
    ));
    assert!(ImageDecoder::with_cache_byte_budget(0).is_err());
}

#[test]
fn concurrent_same_resource_is_single_flight_and_shares_pixels() {
    const WORKERS: usize = 12;
    let document = Arc::new(package(PNG, "PNG", ResourceLimits::default()));
    let decoder = Arc::new(ImageDecoder::default());
    let start = Arc::new(Barrier::new(WORKERS + 1));
    let threads = (0..WORKERS)
        .map(|_| {
            let document = Arc::clone(&document);
            let decoder = Arc::clone(&decoder);
            let start = Arc::clone(&start);
            thread::spawn(move || {
                let resource = document.image_resource(10).unwrap();
                start.wait();
                decoder
                    .decode(&resource, &ResourceLimits::default())
                    .unwrap()
                    .rgba_arc()
            })
        })
        .collect::<Vec<_>>();
    start.wait();
    let results = threads
        .into_iter()
        .map(|thread| thread.join().unwrap())
        .collect::<Vec<_>>();
    assert!(results
        .iter()
        .all(|pixels| Arc::ptr_eq(pixels, &results[0])));
}

#[test]
fn equal_ids_from_distinct_documents_never_share_cached_pixels() {
    let png = package(PNG, "PNG", ResourceLimits::default());
    let jpeg = package(JPEG, "JPEG", ResourceLimits::default());
    let png_resource = png.image_resource(10).unwrap();
    let jpeg_resource = jpeg.image_resource(10).unwrap();
    assert_ne!(png_resource.identity(), jpeg_resource.identity());

    let decoder = ImageDecoder::default();
    let first = decoder
        .decode(&png_resource, &ResourceLimits::default())
        .unwrap();
    let second = decoder
        .decode(&jpeg_resource, &ResourceLimits::default())
        .unwrap();
    assert_eq!(first.pixel(0, 0), Some([255, 0, 0, 255]));
    assert_ne!(first.pixel(0, 0), second.pixel(0, 0));
    assert!(!Arc::ptr_eq(&first.rgba_arc(), &second.rgba_arc()));
}

#[test]
fn errors_include_safe_package_local_asset_provenance() {
    let document = package(b"not-an-image", "PNG", ResourceLimits::default());
    let resource = document.image_resource(10).unwrap();
    let error = ImageDecoder::default()
        .decode(&resource, &ResourceLimits::default())
        .unwrap_err();
    assert!(error.to_string().contains("Doc_0/image.bin"));
}

#[cfg(feature = "jbig2")]
#[test]
fn standalone_jbig2_images_decode_through_the_system_jbig2dec() {
    // image_78.jb2 is the GBIG2-encoded image embedded in ofdrw's
    // converter/1.ofd fixture.
    let bytes = std::fs::read(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/image_78.jb2"),
    )
    .unwrap();
    assert!(bytes.starts_with(b"\x97JB2\r\n\x1a\n"));
    let decoded = decode(&bytes, "GBIG2").unwrap();
    assert!(decoded.width() > 0 && decoded.height() > 0);
    assert_eq!(
        decoded.rgba().len(),
        decoded.width() as usize * decoded.height() as usize * 4
    );
    // A bi-level scan must expand to black or white pixels only.
    assert!(decoded
        .rgba()
        .chunks_exact(4)
        .all(|pixel| { alpha_is_opaque_and_channels_are_bi_level(pixel) }));
    // The scan contains ink.
    let black = decoded.rgba().chunks_exact(4).filter(|p| p[0] == 0).count();
    assert!(black > 0, "expected black pixels in the decoded scan");
}

#[cfg(feature = "jbig2")]
fn alpha_is_opaque_and_channels_are_bi_level(pixel: &[u8]) -> bool {
    let [red, green, blue, alpha] = [pixel[0], pixel[1], pixel[2], pixel[3]];
    alpha == 255
        && ((red == 0 && green == 0 && blue == 0) || (red == 255 && green == 255 && blue == 255))
}
