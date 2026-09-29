mod support;

use std::sync::{Arc, Barrier};

use rofd_core::{Document, Error, ImageFormat, LoadOptions, ResourceKind, ResourceLimits};

const DOCUMENT_PREFIX: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<ofd:Document xmlns:ofd="http://www.ofdspec.org/2016"><ofd:CommonData>
<ofd:PageArea><ofd:PhysicalBox>0 0 210 297</ofd:PhysicalBox></ofd:PageArea>"#;
const DOCUMENT_SUFFIX: &str = r#"</ofd:CommonData><ofd:Pages>
<ofd:Page ID="2" BaseLoc="Pages/Page_0/Content.xml"/></ofd:Pages></ofd:Document>"#;

fn package(declarations: &str, entries: &[(&str, &[u8])]) -> Vec<u8> {
    support::ofd_with_entries(
        &format!("{DOCUMENT_PREFIX}{declarations}{DOCUMENT_SUFFIX}"),
        entries,
    )
}

fn open(declarations: &str, entries: &[(&str, &[u8])]) -> Document {
    Document::from_bytes(package(declarations, entries), LoadOptions::default()).unwrap()
}

#[test]
fn no_resource_declarations_keep_open_and_page_access_compatible() {
    let document = open("", &[]);
    assert_eq!(document.page(0).unwrap().object_id(), 2);
    assert!(matches!(
        document.font_resource(7),
        Err(Error::UnknownResource { object_id: 7 })
    ));
}

#[test]
fn resource_catalog_is_lazy_and_failed_initialization_is_retryable() {
    let document = open("<ofd:PublicRes>Res/Missing.xml</ofd:PublicRes>", &[]);
    assert_eq!(document.page_count(), 1);
    for _ in 0..2 {
        assert!(matches!(
            document.font_resource(1),
            Err(Error::MissingEntry(ref path)) if path == "Doc_0/Res/Missing.xml"
        ));
    }

    let malformed = open(
        "<ofd:DocumentRes>Res/Bad.xml</ofd:DocumentRes>",
        &[("Doc_0/Res/Bad.xml", b"<ofd:Res")],
    );
    assert!(matches!(
        malformed.image_resource(1),
        Err(Error::Xml { .. })
    ));
    assert!(matches!(malformed.font_resource(1), Err(Error::Xml { .. })));
}

#[test]
fn resolves_normalized_catalog_base_and_asset_paths() {
    let catalog = br#"<ofd:Res xmlns:ofd="http://www.ofdspec.org/2016" BaseLoc="./Assets/tmp/..">
      <ofd:Fonts>
        <ofd:Font ID="1" FontName="Embedded" FamilyName="Family" Charset="unicode"><ofd:FontFile>Fonts/a.otf</ofd:FontFile></ofd:Font>
        <ofd:Font ID="2" FontName="System"/>
      </ofd:Fonts>
      <ofd:MultiMedias>
        <ofd:MultiMedia ID="3" Type="Image" Format="PNG"><ofd:MediaFile>Images/a.png</ofd:MediaFile></ofd:MultiMedia>
        <ofd:MultiMedia ID="4" Type="Image" Format="JPG"><ofd:MediaFile>Images/b.jpg</ofd:MediaFile></ofd:MultiMedia>
      </ofd:MultiMedias>
    </ofd:Res>"#;
    let document = open(
        "<ofd:PublicRes>./Res/x/../Public.xml</ofd:PublicRes>",
        &[
            ("Doc_0/Res/Public.xml", catalog),
            ("Doc_0/Res/Assets/Fonts/a.otf", b"font"),
            ("Doc_0/Res/Assets/Images/a.png", b"png"),
            ("Doc_0/Res/Assets/Images/b.jpg", b"jpeg"),
        ],
    );

    let embedded = document.font_resource(1).unwrap();
    let embedded_again = document.font_resource(1).unwrap();
    assert_eq!(embedded.identity(), embedded_again.identity());
    let other_document = open(
        "<ofd:PublicRes>Res/Public.xml</ofd:PublicRes>",
        &[
            ("Doc_0/Res/Public.xml", catalog),
            ("Doc_0/Res/Assets/Fonts/a.otf", b"font"),
            ("Doc_0/Res/Assets/Images/a.png", b"png"),
            ("Doc_0/Res/Assets/Images/b.jpg", b"jpeg"),
        ],
    );
    assert_ne!(
        embedded.identity(),
        other_document.font_resource(1).unwrap().identity()
    );
    assert_eq!(embedded.id(), 1);
    assert_eq!(embedded.font_name(), "Embedded");
    assert_eq!(embedded.family_name(), Some("Family"));
    assert_eq!(embedded.charset(), Some("unicode"));
    assert_eq!(embedded.encoded_bytes(), Some(b"font".as_slice()));
    assert_eq!(document.font_resource(2).unwrap().encoded_bytes(), None);
    let png = document.image_resource(3).unwrap();
    assert_eq!(png.id(), 3);
    assert_eq!(png.format(), ImageFormat::Png);
    assert_eq!(png.encoded_bytes(), b"png");
    assert_eq!(
        document.image_resource(4).unwrap().format(),
        ImageFormat::Jpeg
    );
}

#[test]
fn jbig2_declarations_are_indexed_as_a_recognized_image_format() {
    // JB2/GBIG2 are valid OFD image encodings (ofdrw decodes them through a
    // JBIG2 plugin); rofd-core indexes them so pages load, and renderers
    // report them as undecodable.
    let document = open(
        "<ofd:PublicRes>Res/Public.xml</ofd:PublicRes>",
        &[
            (
                "Doc_0/Res/Public.xml",
                br#"<Res><MultiMedias>
              <MultiMedia ID="3" Type="Image" Format="GBIG2"><MediaFile>a.jb2</MediaFile></MultiMedia>
              <MultiMedia ID="4" Type="Image" Format="JB2"><MediaFile>b.jb2</MediaFile></MultiMedia>
              <MultiMedia ID="5" Type="Image"><MediaFile>c.jbig2</MediaFile></MultiMedia>
            </MultiMedias></Res>"#,
            ),
            ("Doc_0/Res/a.jb2", b"a"),
            ("Doc_0/Res/b.jb2", b"b"),
            ("Doc_0/Res/c.jbig2", b"c"),
        ],
    );
    for id in [3, 4, 5] {
        assert_eq!(
            document.image_resource(id).unwrap().format(),
            ImageFormat::Jbig2
        );
    }
}

#[test]
fn resource_ids_are_one_atomic_document_wide_space() {
    let public = br#"<Res><Fonts><Font ID="9" FontName="one"/></Fonts></Res>"#;
    let document = br#"<Res><MultiMedias><MultiMedia ID="9" Type="Image" Format="PNG"><MediaFile>x</MediaFile></MultiMedia></MultiMedias></Res>"#;
    let doc = open(
        "<ofd:PublicRes>Res/Public.xml</ofd:PublicRes><ofd:DocumentRes>Res/Document.xml</ofd:DocumentRes>",
        &[("Doc_0/Res/Public.xml", public), ("Doc_0/Res/Document.xml", document)],
    );
    for id in [9, 999] {
        assert!(matches!(
            doc.font_resource(id),
            Err(Error::DuplicateResourceId {
                object_id: 9,
                ref first_path,
                first_kind: ResourceKind::Font,
                ref duplicate_path,
                duplicate_kind: ResourceKind::Image,
            }) if first_path == "Doc_0/Res/Public.xml"
                && duplicate_path == "Doc_0/Res/Document.xml"
        ));
    }

    let zero = open(
        "<ofd:PublicRes>Res/Public.xml</ofd:PublicRes>",
        &[(
            "Doc_0/Res/Public.xml",
            br#"<Res><Fonts><Font ID="0" FontName="bad"/></Fonts></Res>"#,
        )],
    );
    assert!(matches!(
        zero.font_resource(0),
        Err(Error::InvalidValue {
            field: "resource ID",
            ..
        })
    ));

    let duplicate = open(
        "<ofd:PublicRes>Res/Public.xml</ofd:PublicRes>",
        &[(
            "Doc_0/Res/Public.xml",
            br#"<Res><Fonts><Font ID="4" FontName="a"/><Font ID="4" FontName="b"/></Fonts></Res>"#,
        )],
    );
    assert!(matches!(
        duplicate.font_resource(4),
        Err(Error::DuplicateResourceId {
            object_id: 4,
            ref first_path,
            first_kind: ResourceKind::Font,
            ref duplicate_path,
            duplicate_kind: ResourceKind::Font,
        }) if first_path == "Doc_0/Res/Public.xml"
            && duplicate_path == "Doc_0/Res/Public.xml"
    ));

    let draw_param_duplicate = open(
        "<ofd:PublicRes>Res/Public.xml</ofd:PublicRes>",
        &[(
            "Doc_0/Res/Public.xml",
            br#"<Res><Fonts><Font ID="6" FontName="font"/></Fonts><DrawParams><DrawParam ID="6"/></DrawParams></Res>"#,
        )],
    );
    assert!(matches!(
        draw_param_duplicate.font_resource(6),
        Err(Error::DuplicateResourceId {
            object_id: 6,
            first_kind: ResourceKind::Font,
            duplicate_kind: ResourceKind::DrawParam,
            ..
        })
    ));
}

#[test]
fn unknown_ids_kind_mismatches_and_invalid_image_declarations_are_structured() {
    let catalog = br#"<Res><Fonts><Font ID="1" FontName="system"/></Fonts><MultiMedias>
      <MultiMedia ID="2" Type="Image" Format="PNG"><MediaFile>x.png</MediaFile></MultiMedia>
    </MultiMedias></Res>"#;
    let doc = open(
        "<ofd:PublicRes>Res/r.xml</ofd:PublicRes>",
        &[("Doc_0/Res/r.xml", catalog)],
    );
    assert!(matches!(
        doc.image_resource(1),
        Err(Error::ResourceKindMismatch {
            object_id: 1,
            expected: ResourceKind::Image,
            actual: ResourceKind::Font
        })
    ));
    assert!(matches!(
        doc.font_resource(2),
        Err(Error::ResourceKindMismatch {
            object_id: 2,
            expected: ResourceKind::Font,
            actual: ResourceKind::Image
        })
    ));
    assert!(matches!(
        doc.font_resource(99),
        Err(Error::UnknownResource { object_id: 99 })
    ));

    for xml in [
        br#"<Res><MultiMedias><MultiMedia ID="3" Type="Video" Format="PNG"><MediaFile>x</MediaFile></MultiMedia></MultiMedias></Res>"#.as_slice(),
        br#"<Res><MultiMedias><MultiMedia ID="3" Type="Image" Format="WEBP"><MediaFile>x</MediaFile></MultiMedia></MultiMedias></Res>"#.as_slice(),
        br#"<Res><MultiMedias><MultiMedia ID="3" Type="Image" Format="PNG"/></MultiMedias></Res>"#.as_slice(),
        br#"<Res><MultiMedias><MultiMedia ID="3" Type="Image"><MediaFile>x</MediaFile></MultiMedia></MultiMedias></Res>"#.as_slice(),
    ] {
        let invalid = open("<ofd:PublicRes>Res/r.xml</ofd:PublicRes>", &[("Doc_0/Res/r.xml", xml)]);
        assert!(matches!(invalid.image_resource(3), Err(Error::InvalidResource { object_id: Some(3), .. })));
    }
}

#[test]
fn resource_xml_depth_has_an_exact_pre_deserialization_boundary() {
    let xml = br#"<Res><Ignored><A><B><C/></B></A></Ignored><Fonts><Font ID="1" FontName="a"/></Fonts></Res>"#;
    let declaration = "<ofd:PublicRes>Res/r.xml</ofd:PublicRes>";
    let exact = ResourceLimits {
        max_xml_depth: 5,
        ..ResourceLimits::default()
    };
    let doc = Document::from_bytes(
        package(declaration, &[("Doc_0/Res/r.xml", xml)]),
        LoadOptions {
            limits: exact,
            ..LoadOptions::default()
        },
    )
    .unwrap();
    assert!(doc.font_resource(1).is_ok());

    let one_over = ResourceLimits {
        max_xml_depth: 4,
        ..ResourceLimits::default()
    };
    let doc = Document::from_bytes(
        package(declaration, &[("Doc_0/Res/r.xml", xml)]),
        LoadOptions {
            limits: one_over,
            ..LoadOptions::default()
        },
    )
    .unwrap();
    assert!(matches!(doc.font_resource(1), Err(Error::LimitExceeded(_))));
}

#[test]
fn unsafe_resource_paths_are_rejected_at_the_appropriate_lazy_boundary() {
    let error = Document::from_bytes(
        package("<ofd:PublicRes>../../escape.xml</ofd:PublicRes>", &[]),
        LoadOptions::default(),
    )
    .unwrap_err();
    assert!(matches!(error, Error::InvalidValue { .. }));

    let xml = br#"<Res BaseLoc="../../../escape"><Fonts><Font ID="1" FontName="bad"><FontFile>x</FontFile></Font></Fonts></Res>"#;
    let doc = open(
        "<ofd:PublicRes>Res/r.xml</ofd:PublicRes>",
        &[("Doc_0/Res/r.xml", xml)],
    );
    assert!(
        matches!(doc.font_resource(1), Err(Error::InvalidValue { .. }))
            || matches!(doc.image_resource(1), Err(Error::InvalidValue { .. }))
    );

    // A leading slash is a package-root-absolute path, not an unsafe one:
    // `/abs` normalizes to `abs` and then simply misses at lookup time.
    let xml = br#"<Res><MultiMedias><MultiMedia ID="1" Type="Image" Format="PNG"><MediaFile>/abs</MediaFile></MultiMedia></MultiMedias></Res>"#;
    let doc = open(
        "<ofd:PublicRes>Res/r.xml</ofd:PublicRes>",
        &[("Doc_0/Res/r.xml", xml)],
    );
    assert!(matches!(
        doc.image_resource(1),
        Err(Error::MissingEntry(_) | Error::InvalidResource { .. })
    ));
}

#[test]
fn resource_file_and_count_limits_have_exact_boundaries() {
    let declarations =
        "<ofd:PublicRes>Res/a.xml</ofd:PublicRes><ofd:DocumentRes>Res/b.xml</ofd:DocumentRes>";
    let entries = [
        (
            "Doc_0/Res/a.xml",
            br#"<Res><Fonts><Font ID="1" FontName="a"/></Fonts></Res>"#.as_slice(),
        ),
        (
            "Doc_0/Res/b.xml",
            br#"<Res><Fonts><Font ID="2" FontName="b"/></Fonts></Res>"#.as_slice(),
        ),
    ];
    let exact = ResourceLimits {
        max_resource_files: 2,
        max_resources: 2,
        ..ResourceLimits::default()
    };
    assert!(Document::from_bytes(
        package(declarations, &entries),
        LoadOptions {
            limits: exact,
            ..LoadOptions::default()
        }
    )
    .unwrap()
    .font_resource(2)
    .is_ok());
    let files_over = ResourceLimits {
        max_resource_files: 1,
        ..ResourceLimits::default()
    };
    assert!(matches!(
        Document::from_bytes(
            package(declarations, &entries),
            LoadOptions {
                limits: files_over,
                ..LoadOptions::default()
            }
        ),
        Err(Error::LimitExceeded(_))
    ));
    let resources_over = ResourceLimits {
        max_resources: 1,
        ..ResourceLimits::default()
    };
    let doc = Document::from_bytes(
        package(declarations, &entries),
        LoadOptions {
            limits: resources_over,
            ..LoadOptions::default()
        },
    )
    .unwrap();
    assert!(matches!(doc.font_resource(1), Err(Error::LimitExceeded(_))));
}

#[test]
fn encoded_asset_limits_are_enforced_before_returning_bytes() {
    let catalog = br#"<Res><Fonts><Font ID="1" FontName="a"><FontFile>f</FontFile></Font></Fonts><MultiMedias><MultiMedia ID="2" Type="Image" Format="JPEG"><MediaFile>i</MediaFile></MultiMedia></MultiMedias></Res>"#;
    let entries = [
        ("Doc_0/Res/r.xml", catalog.as_slice()),
        ("Doc_0/Res/f", b"1234".as_slice()),
        ("Doc_0/Res/i", b"5678".as_slice()),
    ];
    let decl = "<ofd:PublicRes>Res/r.xml</ofd:PublicRes>";
    let exact = ResourceLimits {
        max_font_bytes: 4,
        max_encoded_image_bytes: 4,
        ..ResourceLimits::default()
    };
    let doc = Document::from_bytes(
        package(decl, &entries),
        LoadOptions {
            limits: exact,
            ..LoadOptions::default()
        },
    )
    .unwrap();
    assert_eq!(
        doc.font_resource(1).unwrap().encoded_bytes().unwrap().len(),
        4
    );
    assert_eq!(doc.image_resource(2).unwrap().encoded_bytes().len(), 4);
    let over = ResourceLimits {
        max_font_bytes: 3,
        max_encoded_image_bytes: 3,
        ..ResourceLimits::default()
    };
    let doc = Document::from_bytes(
        package(decl, &entries),
        LoadOptions {
            limits: over,
            ..LoadOptions::default()
        },
    )
    .unwrap();
    assert!(matches!(doc.font_resource(1), Err(Error::LimitExceeded(_))));
    assert!(matches!(
        doc.image_resource(2),
        Err(Error::LimitExceeded(_))
    ));
}

#[test]
fn asset_bytes_are_lazy_and_concurrent_results_are_consistent() {
    let catalog = br#"<Res><Fonts><Font ID="1" FontName="system"/><Font ID="2" FontName="missing"><FontFile>missing</FontFile></Font></Fonts></Res>"#;
    let doc = Arc::new(open(
        "<ofd:PublicRes>Res/r.xml</ofd:PublicRes>",
        &[("Doc_0/Res/r.xml", catalog)],
    ));
    assert!(doc.font_resource(1).unwrap().encoded_bytes().is_none());
    let barrier = Arc::new(Barrier::new(4));
    let handles = (0..4)
        .map(|_| {
            let doc = Arc::clone(&doc);
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                doc.font_resource(2).unwrap_err().to_string()
            })
        })
        .collect::<Vec<_>>();
    let errors = handles
        .into_iter()
        .map(|h| h.join().unwrap())
        .collect::<Vec<_>>();
    assert!(errors.windows(2).all(|pair| pair[0] == pair[1]));
}

#[test]
fn concurrent_successful_catalog_and_asset_lookups_agree() {
    let catalog = br#"<Res><MultiMedias><MultiMedia ID="8" Type="Image" Format="PNG"><MediaFile>pixel.png</MediaFile></MultiMedia></MultiMedias></Res>"#;
    let doc = Arc::new(open(
        "<ofd:PublicRes>Res/r.xml</ofd:PublicRes>",
        &[
            ("Doc_0/Res/r.xml", catalog),
            ("Doc_0/Res/pixel.png", b"same"),
        ],
    ));
    let barrier = Arc::new(Barrier::new(4));
    let handles = (0..4)
        .map(|_| {
            let doc = Arc::clone(&doc);
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                doc.image_resource(8).unwrap().encoded_bytes().to_vec()
            })
        })
        .collect::<Vec<_>>();
    for handle in handles {
        assert_eq!(handle.join().unwrap(), b"same");
    }
}

#[test]
fn concurrent_failed_catalog_initialization_is_consistent_and_unpublished() {
    let doc = Arc::new(open(
        "<ofd:PublicRes>Res/bad.xml</ofd:PublicRes>",
        &[("Doc_0/Res/bad.xml", b"<Res><Fonts>")],
    ));
    let barrier = Arc::new(Barrier::new(4));
    let handles = (0..4)
        .map(|id| {
            let doc = Arc::clone(&doc);
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                doc.font_resource(id).unwrap_err().to_string()
            })
        })
        .collect::<Vec<_>>();
    let errors = handles
        .into_iter()
        .map(|handle| handle.join().unwrap())
        .collect::<Vec<_>>();
    assert!(errors.windows(2).all(|pair| pair[0] == pair[1]));
    assert!(matches!(doc.image_resource(99), Err(Error::Xml { .. })));
}

#[test]
fn new_resource_limit_defaults_are_stable() {
    let limits = ResourceLimits::default();
    assert_eq!(limits.max_resource_files, 32);
    assert_eq!(limits.max_resources, 100_000);
    assert_eq!(limits.max_font_bytes, 64 * 1024 * 1024);
    assert_eq!(limits.max_encoded_image_bytes, 64 * 1024 * 1024);
    assert_eq!(limits.max_decoded_image_pixels, 100_000_000);
    assert_eq!(limits.max_decoded_image_bytes, 400 * 1024 * 1024);
    assert_eq!(limits.max_text_characters_per_page, 1_000_000);
    assert_eq!(limits.max_glyphs_per_page, 1_000_000);
    assert_eq!(limits.max_text_expansion_entries, 2_000_000);
}

const DUPLICATE_FONTS_CATALOG: &[u8] = br#"<ofd:Res xmlns:ofd="http://www.ofdspec.org/2016">
  <ofd:Fonts><ofd:Font ID="1" FontName="First"/></ofd:Fonts>
  <ofd:Fonts><ofd:Font ID="2" FontName="Second"/></ofd:Fonts>
</ofd:Res>"#;

#[test]
fn duplicate_fonts_blocks_are_merged_in_lenient_mode() {
    // ofdrw's converter/ano.ofd and converter/透明度文字.ofd split fonts over
    // two Fonts blocks in one catalog.
    let document = open(
        "<ofd:PublicRes>Res/Dup.xml</ofd:PublicRes>",
        &[("Doc_0/Res/Dup.xml", DUPLICATE_FONTS_CATALOG)],
    );
    assert_eq!(document.font_resource(1).unwrap().font_name(), "First");
    assert_eq!(document.font_resource(2).unwrap().font_name(), "Second");
}

#[test]
fn duplicate_fonts_blocks_are_rejected_in_strict_mode() {
    let bytes = package(
        "<ofd:PublicRes>Res/Dup.xml</ofd:PublicRes>",
        &[("Doc_0/Res/Dup.xml", DUPLICATE_FONTS_CATALOG)],
    );
    let document = Document::from_bytes(
        bytes,
        LoadOptions {
            strictness: rofd_core::Strictness::Strict,
            ..LoadOptions::default()
        },
    )
    .unwrap();
    assert!(matches!(
        document.font_resource(1),
        Err(Error::InvalidStructure { .. })
    ));
}

const DUPLICATE_MULTI_MEDIAS_CATALOG: &[u8] = br#"<ofd:Res xmlns:ofd="http://www.ofdspec.org/2016">
  <ofd:MultiMedias><ofd:MultiMedia ID="1" Type="Image" Format="PNG"><ofd:MediaFile>x.png</ofd:MediaFile></ofd:MultiMedia></ofd:MultiMedias>
  <ofd:MultiMedias><ofd:MultiMedia ID="2" Type="Image" Format="PNG"><ofd:MediaFile>y.png</ofd:MediaFile></ofd:MultiMedia></ofd:MultiMedias>
</ofd:Res>"#;

const FAKE_PNG: &[u8] = b"\x89PNG\r\n\x1a\nfake";

#[test]
fn duplicate_multi_medias_blocks_are_merged_in_lenient_mode() {
    // Invoice generators emit several MultiMedias blocks in one catalog.
    let document = open(
        "<ofd:PublicRes>Res/DupMedia.xml</ofd:PublicRes>",
        &[
            ("Doc_0/Res/DupMedia.xml", DUPLICATE_MULTI_MEDIAS_CATALOG),
            ("Doc_0/Res/x.png", FAKE_PNG),
            ("Doc_0/Res/y.png", FAKE_PNG),
        ],
    );
    assert!(document.image_resource(1).is_ok());
    assert!(document.image_resource(2).is_ok());
}

#[test]
fn duplicate_multi_medias_blocks_are_rejected_in_strict_mode() {
    let bytes = package(
        "<ofd:PublicRes>Res/DupMedia.xml</ofd:PublicRes>",
        &[("Doc_0/Res/DupMedia.xml", DUPLICATE_MULTI_MEDIAS_CATALOG)],
    );
    let document = Document::from_bytes(
        bytes,
        LoadOptions {
            strictness: rofd_core::Strictness::Strict,
            ..LoadOptions::default()
        },
    )
    .unwrap();
    assert!(matches!(
        document.image_resource(1),
        Err(Error::InvalidStructure { .. })
    ));
}
