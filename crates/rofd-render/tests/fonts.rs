use std::io::{Cursor, Write};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;

use fontdb::Database;
use rofd_core::{Document, LoadOptions, PageObject, ResourceLimits, TextMetricsProvider};
use rofd_render::{
    position_glyph_runs, Error, FontDiagnostic, FontIdentity, FontResolver, FontSource,
    ResolvedFont, SystemFontResolver,
};
use zip::{write::SimpleFileOptions, ZipWriter};

const FONT: &[u8] = include_bytes!("fixtures/fonts/phase3-subset.ttf");
const LATIN_FONT: &[u8] = include_bytes!("fixtures/fonts/phase3-latin-subset.ttf");
const COLLECTION: &[u8] = include_bytes!("fixtures/fonts/phase3-subsets.ttc");

fn package(font_file: Option<&[u8]>, text: &str, extra: &str) -> Document {
    package_named(font_file, text, extra, "Noto Sans CJK SC")
}

fn package_named(font_file: Option<&[u8]>, text: &str, extra: &str, font_name: &str) -> Document {
    package_with_names(font_file, text, extra, font_name, Some(font_name))
}

fn package_with_names(
    font_file: Option<&[u8]>,
    text: &str,
    extra: &str,
    font_name: &str,
    family_name: Option<&str>,
) -> Document {
    let font_file_element = font_file.map_or(String::new(), |_| {
        "<ofd:FontFile>Fonts/phase3-subset.ttf</ofd:FontFile>".to_owned()
    });
    let family_name =
        family_name.map_or(String::new(), |family| format!(r#" FamilyName="{family}""#));
    let resource = format!(
        r#"<ofd:Res xmlns:ofd="http://www.ofdspec.org/2016"><ofd:Fonts>
          <ofd:Font ID="10" FontName="{font_name}"{family_name}>{font_file_element}</ofd:Font>
          <ofd:Font ID="11" FontName="Other"/>
        </ofd:Fonts></ofd:Res>"#
    );
    let page = format!(
        r#"<ofd:Page xmlns:ofd="http://www.ofdspec.org/2016"><ofd:Area><ofd:PhysicalBox>0 0 30 30</ofd:PhysicalBox></ofd:Area><ofd:Content><ofd:Layer ID="1"><ofd:TextObject ID="2" Boundary="0 0 30 10" Font="10" Size="4">{text}{extra}</ofd:TextObject></ofd:Layer></ofd:Content></ofd:Page>"#
    );
    let entries = [
        (
            "OFD.xml",
            br#"<ofd:OFD xmlns:ofd="http://www.ofdspec.org/2016"><ofd:DocBody><ofd:DocInfo><ofd:DocID>fonts</ofd:DocID></ofd:DocInfo><ofd:DocRoot>Doc_0/Document.xml</ofd:DocRoot></ofd:DocBody></ofd:OFD>"#.as_slice(),
        ),
        (
            "Doc_0/Document.xml",
            br#"<ofd:Document xmlns:ofd="http://www.ofdspec.org/2016"><ofd:CommonData><ofd:PageArea><ofd:PhysicalBox>0 0 30 30</ofd:PhysicalBox></ofd:PageArea><ofd:DocumentRes>Res.xml</ofd:DocumentRes></ofd:CommonData><ofd:Pages><ofd:Page ID="3" BaseLoc="Pages/Page.xml"/></ofd:Pages></ofd:Document>"#.as_slice(),
        ),
        ("Doc_0/Res.xml", resource.as_bytes()),
        ("Doc_0/Pages/Page.xml", page.as_bytes()),
    ];
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in entries {
        writer
            .start_file(name, SimpleFileOptions::default())
            .unwrap();
        writer.write_all(bytes).unwrap();
    }
    if let Some(bytes) = font_file {
        writer
            .start_file(
                "Doc_0/Fonts/phase3-subset.ttf",
                SimpleFileOptions::default(),
            )
            .unwrap();
        writer.write_all(bytes).unwrap();
    }
    Document::from_bytes(
        writer.finish().unwrap().into_inner(),
        LoadOptions::default(),
    )
    .unwrap()
}

fn text_object(document: &Document) -> rofd_core::TextObject {
    let page = document.page(0).unwrap();
    let PageObject::Text(text) = &page.layers()[0].objects()[0] else {
        panic!("expected text object");
    };
    text.clone()
}

#[test]
fn embedded_font_has_priority_and_absent_deltas_use_font_advance() {
    let document = package(
        Some(FONT),
        r#"<ofd:TextCode X="1" Y="2">A中</ofd:TextCode>"#,
        "",
    );
    let font = document.font_resource(10).unwrap();
    let text = text_object(&document);
    let mut database = Database::new();
    database.load_font_data(FONT.to_vec());
    let resolver = SystemFontResolver::from_database(database, Vec::new(), 1 << 20);

    let runs = position_glyph_runs(&resolver, &font, &text, &ResourceLimits::default()).unwrap();

    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].text(), "A中");
    assert_eq!(runs[0].source_range(), 0..2);
    assert_eq!(runs[0].glyphs().len(), 2);
    assert_eq!(
        (runs[0].glyphs()[0].x(), runs[0].glyphs()[0].y()),
        (1.0, 2.0)
    );
    assert!(runs[0].glyphs()[1].x() > 1.0);
    assert_eq!(runs[0].glyphs()[1].y(), 2.0);
    assert_eq!(runs[0].glyphs()[1].source_range(), 1..4);
    assert_eq!(
        runs[0].glyphs()[0].font_source(),
        &FontSource::Embedded { resource_id: 10 }
    );
    assert!(runs[0].diagnostics().is_empty());
}

#[test]
fn explicit_zero_deltas_stay_zero_and_each_text_code_keeps_its_origin() {
    let document = package(
        Some(FONT),
        r#"<ofd:TextCode X="1" Y="2" DeltaX="0 0">AB</ofd:TextCode><ofd:TextCode X="8" Y="9">C</ofd:TextCode>"#,
        "",
    );
    let text = text_object(&document);
    assert!(text.runs()[0].has_explicit_delta_x());
    assert!(!text.runs()[1].has_explicit_delta_x());
    let runs = position_glyph_runs(
        &SystemFontResolver::from_database(Database::new(), Vec::new(), 1 << 20),
        &document.font_resource(10).unwrap(),
        &text,
        &ResourceLimits::default(),
    )
    .unwrap();

    assert_eq!(runs[0].glyphs()[0].x(), 1.0);
    assert_eq!(runs[0].glyphs()[1].x(), 1.0);
    assert_eq!(
        (runs[1].glyphs()[0].x(), runs[1].glyphs()[0].y()),
        (8.0, 9.0)
    );
}

#[test]
fn empty_delta_axes_infer_advance_but_numeric_zero_and_short_arrays_are_explicit() {
    fn glyph_positions(delta_attributes: &str, text_value: &str) -> Vec<(f64, f64)> {
        let text_code =
            format!(r#"<ofd:TextCode X="1" Y="2" {delta_attributes}>{text_value}</ofd:TextCode>"#);
        let document = package(Some(FONT), &text_code, "");
        position_glyph_runs(
            &SystemFontResolver::empty(Vec::new(), 1 << 20),
            &document.font_resource(10).unwrap(),
            &text_object(&document),
            &ResourceLimits::default(),
        )
        .unwrap()[0]
            .glyphs()
            .iter()
            .map(|glyph| (glyph.x(), glyph.y()))
            .collect()
    }

    let absent = glyph_positions("", "AB");
    let empty = glyph_positions(r#"DeltaX="" DeltaY="""#, "AB");
    let whitespace = glyph_positions(r#"DeltaX="   " DeltaY=" 	 ""#, "AB");
    assert_eq!(empty, absent);
    assert_eq!(whitespace, absent);
    assert!(absent[1].0 > absent[0].0);
    assert_eq!(absent[1].1, absent[0].1);

    let explicit_zero = glyph_positions(r#"DeltaX="0" DeltaY="0""#, "AB");
    assert_eq!(explicit_zero, [(1.0, 2.0), (1.0, 2.0)]);

    let short = glyph_positions(r#"DeltaX="2" DeltaY="1""#, "ABC");
    assert_eq!(short, [(1.0, 2.0), (3.0, 3.0), (3.0, 3.0)]);
}

#[test]
fn controlled_database_supports_declared_family_then_ordered_fallback() {
    let mut database = Database::new();
    database.load_font_data(FONT.to_vec());
    let resolver =
        SystemFontResolver::from_database(database, vec!["Noto Sans CJK SC".to_owned()], 1 << 20);
    let declared = package(None, r#"<ofd:TextCode X="0" Y="0">A</ofd:TextCode>"#, "");
    let runs = position_glyph_runs(
        &resolver,
        &declared.font_resource(10).unwrap(),
        &text_object(&declared),
        &ResourceLimits::default(),
    )
    .unwrap();
    assert!(matches!(
        runs[0].glyphs()[0].font_source(),
        FontSource::System { .. }
    ));

    let name_only = package_with_names(
        None,
        r#"<ofd:TextCode X="0" Y="0">A</ofd:TextCode>"#,
        "",
        "NotoSansCJKsc-Regular",
        None,
    );
    let runs = position_glyph_runs(
        &resolver,
        &name_only.font_resource(10).unwrap(),
        &text_object(&name_only),
        &ResourceLimits::default(),
    )
    .unwrap();
    assert!(matches!(
        runs[0].glyphs()[0].font_source(),
        FontSource::System { .. }
    ));

    let missing_family = package_named(
        None,
        r#"<ofd:TextCode X="0" Y="0">A</ofd:TextCode>"#,
        "",
        "Definitely Missing",
    );
    let mut fallback_database = Database::new();
    fallback_database.load_font_data(FONT.to_vec());
    let fallback = SystemFontResolver::from_database(
        fallback_database,
        vec![
            "Unavailable First".to_owned(),
            "Noto Sans CJK SC".to_owned(),
        ],
        1 << 20,
    );
    let runs = position_glyph_runs(
        &fallback,
        &missing_family.font_resource(10).unwrap(),
        &text_object(&missing_family),
        &ResourceLimits::default(),
    )
    .unwrap();
    assert!(matches!(
        runs[0].diagnostics()[0],
        FontDiagnostic::FamilyFallback { .. }
    ));
}

#[test]
fn fallback_is_selected_per_character_and_missing_uses_a_synthetic_box() {
    let document = package(
        Some(LATIN_FONT),
        r#"<ofd:TextCode X="0" Y="0">A中</ofd:TextCode>"#,
        "",
    );
    let mut database = Database::new();
    database.load_font_data(FONT.to_vec());
    let resolver =
        SystemFontResolver::from_database(database, vec!["Noto Sans CJK SC".to_owned()], 1 << 20);
    let runs = position_glyph_runs(
        &resolver,
        &document.font_resource(10).unwrap(),
        &text_object(&document),
        &ResourceLimits::default(),
    )
    .unwrap();
    assert!(matches!(
        runs[0].glyphs()[0].font_source(),
        FontSource::Embedded { .. }
    ));
    assert!(matches!(
        runs[0].glyphs()[1].font_source(),
        FontSource::ConfiguredFallback { .. }
    ));
    assert_eq!(
        runs[0].glyphs()[1].font().unwrap().source(),
        runs[0].glyphs()[1].font_source()
    );

    let visible_replacement = package(
        Some(LATIN_FONT),
        r#"<ofd:TextCode X="0" Y="0">🦄</ofd:TextCode>"#,
        "",
    );
    let runs = position_glyph_runs(
        &SystemFontResolver::empty(Vec::new(), 1 << 20),
        &visible_replacement.font_resource(10).unwrap(),
        &text_object(&visible_replacement),
        &ResourceLimits::default(),
    )
    .unwrap();
    assert!(!runs[0].glyphs()[0].is_synthetic_box());
    assert_ne!(runs[0].glyphs()[0].glyph_id(), 0);
    assert!(matches!(
        runs[0].diagnostics()[0],
        FontDiagnostic::MissingGlyph {
            used_visible_replacement: true,
            ..
        }
    ));

    let missing = package_named(
        None,
        r#"<ofd:TextCode X="0" Y="0">🦄</ofd:TextCode>"#,
        "",
        "Definitely Missing",
    );
    let runs = position_glyph_runs(
        &SystemFontResolver::empty(Vec::new(), 1 << 20),
        &missing.font_resource(10).unwrap(),
        &text_object(&missing),
        &ResourceLimits::default(),
    )
    .unwrap();
    assert_eq!(runs[0].glyphs()[0].font_source(), &FontSource::Missing);
    assert!(runs[0].glyphs()[0].is_synthetic_box());
    assert!(runs[0].glyphs()[0].font().is_none());
    assert!(matches!(
        runs[0].diagnostics()[0],
        FontDiagnostic::MissingGlyph {
            used_visible_replacement: false,
            ..
        }
    ));
}

#[test]
fn cgtransform_glyphs_override_charmap_and_validate_font_range() {
    let document = package(
        Some(FONT),
        r#"<ofd:TextCode X="0" Y="0">AB</ofd:TextCode>"#,
        r#"<ofd:CGTransform CodePosition="0" CodeCount="2" GlyphCount="2"><ofd:Glyphs>3 2</ofd:Glyphs></ofd:CGTransform>"#,
    );
    let resolver = SystemFontResolver::from_database(Database::new(), Vec::new(), 1 << 20);
    let runs = position_glyph_runs(
        &resolver,
        &document.font_resource(10).unwrap(),
        &text_object(&document),
        &ResourceLimits::default(),
    )
    .unwrap();
    assert_eq!(
        runs[0]
            .glyphs()
            .iter()
            .map(|glyph| glyph.glyph_id())
            .collect::<Vec<_>>(),
        [3, 2]
    );
    assert_eq!(runs[0].glyphs()[0].source_scalar_range(), 0..2);
    assert!(runs[0].glyphs()[1].x() > runs[0].glyphs()[0].x());

    let one_to_many = package(
        Some(FONT),
        r#"<ofd:TextCode X="0" Y="0">AB</ofd:TextCode>"#,
        r#"<ofd:CGTransform CodePosition="0" CodeCount="1" GlyphCount="2"><ofd:Glyphs>2 3</ofd:Glyphs></ofd:CGTransform>"#,
    );
    let runs = position_glyph_runs(
        &resolver,
        &one_to_many.font_resource(10).unwrap(),
        &text_object(&one_to_many),
        &ResourceLimits::default(),
    )
    .unwrap();
    assert_eq!(runs[0].glyphs().len(), 3);
    assert_eq!(runs[0].glyphs()[0].character(), Some('A'));
    assert_eq!(runs[0].glyphs()[1].character(), None);
    assert_eq!(runs[0].glyphs()[2].character(), Some('B'));

    let invalid = package(
        Some(FONT),
        r#"<ofd:TextCode X="0" Y="0">A</ofd:TextCode>"#,
        r#"<ofd:CGTransform CodePosition="0" CodeCount="1" GlyphCount="1"><ofd:Glyphs>99999</ofd:Glyphs></ofd:CGTransform>"#,
    );
    assert!(matches!(
        position_glyph_runs(
            &resolver,
            &invalid.font_resource(10).unwrap(),
            &text_object(&invalid),
            &ResourceLimits::default()
        ),
        Err(Error::InvalidGlyph {
            glyph_id: 99999,
            ..
        })
    ));
}

#[test]
fn cgtransform_glyphs_apply_to_the_substitute_face_when_primary_is_unresolved() {
    // ofdrw's z.ofd/y.ofd declare external fonts (e.g. Tahoma) with explicit
    // glyph IDs; ofdrw applies the IDs to its substitute face instead of
    // failing the object. Mirror that with the configured fallback chain.
    let document = package_named(
        None,
        r#"<ofd:TextCode X="0" Y="0">AB</ofd:TextCode>"#,
        r#"<ofd:CGTransform CodePosition="0" CodeCount="2" GlyphCount="2"><ofd:Glyphs>3 2</ofd:Glyphs></ofd:CGTransform>"#,
        "External Missing",
    );
    let mut database = Database::new();
    database.load_font_data(FONT.to_vec());
    let resolver =
        SystemFontResolver::from_database(database, vec!["Noto Sans CJK SC".to_owned()], 1 << 20);
    let runs = position_glyph_runs(
        &resolver,
        &document.font_resource(10).unwrap(),
        &text_object(&document),
        &ResourceLimits::default(),
    )
    .unwrap();
    assert_eq!(
        runs[0]
            .glyphs()
            .iter()
            .map(|glyph| glyph.glyph_id())
            .collect::<Vec<_>>(),
        [3, 2]
    );
    assert!(runs[0]
        .glyphs()
        .iter()
        .all(|glyph| matches!(glyph.font_source(), FontSource::ConfiguredFallback { .. })));
    assert!(matches!(
        runs[0].diagnostics()[0],
        FontDiagnostic::FamilyFallback { .. }
    ));
}

#[test]
fn cgtransform_substitute_skips_out_of_range_glyph_ids_like_ofdrw() {
    let document = package_named(
        None,
        r#"<ofd:TextCode X="0" Y="0">AB</ofd:TextCode>"#,
        r#"<ofd:CGTransform CodePosition="0" CodeCount="2" GlyphCount="2"><ofd:Glyphs>3 99999</ofd:Glyphs></ofd:CGTransform>"#,
        "External Missing",
    );
    let mut database = Database::new();
    database.load_font_data(FONT.to_vec());
    let resolver =
        SystemFontResolver::from_database(database, vec!["Noto Sans CJK SC".to_owned()], 1 << 20);
    let runs = position_glyph_runs(
        &resolver,
        &document.font_resource(10).unwrap(),
        &text_object(&document),
        &ResourceLimits::default(),
    )
    .unwrap();
    assert_eq!(
        runs[0]
            .glyphs()
            .iter()
            .map(|glyph| glyph.glyph_id())
            .collect::<Vec<_>>(),
        [3]
    );
    assert!(runs[0].diagnostics().iter().any(|diagnostic| matches!(
        diagnostic,
        FontDiagnostic::MissingGlyph {
            used_visible_replacement: false,
            ..
        }
    )));
}

#[test]
fn cgtransform_without_any_face_reports_missing_glyphs_instead_of_failing() {
    let document = package_named(
        None,
        r#"<ofd:TextCode X="0" Y="0">AB</ofd:TextCode>"#,
        r#"<ofd:CGTransform CodePosition="0" CodeCount="2" GlyphCount="1"><ofd:Glyphs>3</ofd:Glyphs></ofd:CGTransform>"#,
        "External Missing",
    );
    let resolver = SystemFontResolver::empty(Vec::new(), 1 << 20);
    let runs = position_glyph_runs(
        &resolver,
        &document.font_resource(10).unwrap(),
        &text_object(&document),
        &ResourceLimits::default(),
    )
    .unwrap();
    assert!(runs[0].glyphs().is_empty());
    assert_eq!(runs[0].diagnostics().len(), 2);
    assert!(runs[0].diagnostics().iter().all(|diagnostic| matches!(
        diagnostic,
        FontDiagnostic::MissingGlyph {
            used_visible_replacement: false,
            ..
        }
    )));
}

#[test]
fn cumulative_deltas_cross_run_cg_ranges_and_renderer_limits_are_exact() {
    let deltas = package(
        Some(FONT),
        r#"<ofd:TextCode X="1" Y="2" DeltaX="2 3 4">ABC</ofd:TextCode>"#,
        "",
    );
    let resolver = SystemFontResolver::empty(Vec::new(), 1 << 20);
    let runs = position_glyph_runs(
        &resolver,
        &deltas.font_resource(10).unwrap(),
        &text_object(&deltas),
        &ResourceLimits::default(),
    )
    .unwrap();
    assert_eq!(
        runs[0]
            .glyphs()
            .iter()
            .map(|glyph| glyph.x())
            .collect::<Vec<_>>(),
        [1.0, 3.0, 6.0]
    );

    let crossing = package(
        Some(FONT),
        r#"<ofd:TextCode X="0" Y="0">A</ofd:TextCode><ofd:TextCode X="10" Y="10">B</ofd:TextCode>"#,
        r#"<ofd:CGTransform CodePosition="0" CodeCount="2"><ofd:Glyphs>2</ofd:Glyphs></ofd:CGTransform>"#,
    );
    let runs = position_glyph_runs(
        &resolver,
        &crossing.font_resource(10).unwrap(),
        &text_object(&crossing),
        &ResourceLimits::default(),
    )
    .unwrap();
    assert_eq!(runs[0].glyphs().len(), 1);
    assert_eq!(runs[0].glyphs()[0].source_scalar_range(), 0..2);
    assert!(runs[1].glyphs().is_empty());

    let mut exact = ResourceLimits {
        max_glyphs_per_page: 3,
        max_text_expansion_entries: 4,
        ..ResourceLimits::default()
    };
    position_glyph_runs(
        &resolver,
        &deltas.font_resource(10).unwrap(),
        &text_object(&deltas),
        &exact,
    )
    .unwrap();
    exact.max_glyphs_per_page = 2;
    assert!(matches!(
        position_glyph_runs(
            &resolver,
            &deltas.font_resource(10).unwrap(),
            &text_object(&deltas),
            &exact
        ),
        Err(Error::InvalidTextLayout {
            field: "max_glyphs_per_page",
            ..
        })
    ));
    exact.max_glyphs_per_page = 3;
    exact.max_text_expansion_entries = 3;
    assert!(matches!(
        position_glyph_runs(
            &resolver,
            &deltas.font_resource(10).unwrap(),
            &text_object(&deltas),
            &exact
        ),
        Err(Error::InvalidTextLayout {
            field: "max_text_expansion_entries",
            ..
        })
    ));
}

#[test]
fn system_font_bytes_are_bounded_before_ownership_and_position_math_fails_closed() {
    let document = package(None, r#"<ofd:TextCode X="0" Y="0">A</ofd:TextCode>"#, "");
    let mut database = Database::new();
    database.load_font_data(FONT.to_vec());
    let resolver = SystemFontResolver::from_database(
        database,
        vec!["Noto Sans CJK SC".to_owned()],
        (FONT.len() - 1) as u64,
    );
    assert!(matches!(
        position_glyph_runs(
            &resolver,
            &document.font_resource(10).unwrap(),
            &text_object(&document),
            &ResourceLimits::default()
        ),
        Err(Error::FontBytesExceeded { .. })
    ));

    let huge = package(
        Some(FONT),
        r#"<ofd:TextCode X="1.7976931348623157e308" Y="0" DeltaX="1.7976931348623157e308 0">AA</ofd:TextCode>"#,
        "",
    );
    assert!(matches!(
        position_glyph_runs(
            &SystemFontResolver::empty(Vec::new(), 1 << 20),
            &huge.font_resource(10).unwrap(),
            &text_object(&huge),
            &ResourceLimits::default()
        ),
        Err(Error::InvalidTextLayout {
            field: "glyph x",
            ..
        })
    ));
}

#[test]
fn invalid_embedded_data_and_face_index_are_structured_errors() {
    let document = package(
        Some(b"not a font"),
        r#"<ofd:TextCode X="0" Y="0">A</ofd:TextCode>"#,
        "",
    );
    let resolver = SystemFontResolver::from_database(Database::new(), Vec::new(), 1 << 20);
    assert!(matches!(
        position_glyph_runs(
            &resolver,
            &document.font_resource(10).unwrap(),
            &text_object(&document),
            &ResourceLimits::default()
        ),
        Err(Error::InvalidFont { .. })
    ));
    assert!(matches!(
        ResolvedFont::from_system_bytes(Arc::from(FONT), 99, FontIdentity::new("fixture").unwrap(),),
        Err(Error::InvalidFont { .. })
    ));
}

#[test]
fn controlled_collection_selects_face_index_and_exposes_lookup_and_advance() {
    let owned: Arc<[u8]> = Arc::from(COLLECTION.to_vec());
    let latin = ResolvedFont::from_system_bytes(
        Arc::clone(&owned),
        0,
        FontIdentity::new("collection-face-0").unwrap(),
    )
    .unwrap();
    let cjk = ResolvedFont::from_system_bytes(
        Arc::clone(&owned),
        1,
        FontIdentity::new("collection-face-1").unwrap(),
    )
    .unwrap();

    assert!(latin.glyph_index('A').unwrap().is_some());
    assert_eq!(latin.glyph_index('中').unwrap(), None);
    let cjk_glyph = cjk.glyph_index('中').unwrap().unwrap();
    let advance = cjk.glyph_advance_mm(cjk_glyph, 4.0).unwrap();
    assert!(advance.0.is_finite() && advance.0 > 0.0);
    assert_eq!(advance.1, 0.0);
    assert_eq!(cjk.face_index(), 1);
    assert!(Arc::ptr_eq(&cjk.encoded_bytes_arc(), &owned));

    assert!(matches!(
        ResolvedFont::from_system_bytes(owned, 2, FontIdentity::new("collection-face-2").unwrap(),),
        Err(Error::InvalidFont { .. })
    ));
}

#[test]
fn constrained_font_constructors_keep_sources_consistent_and_reject_path_identities() {
    assert!(FontIdentity::new("").is_err());
    assert!(FontIdentity::new("/host/font.ttf").is_err());
    assert!(FontIdentity::new(r"C:\\host\\font.ttf").is_err());

    let document = package(
        Some(FONT),
        r#"<ofd:TextCode X="0" Y="0">A</ofd:TextCode>"#,
        "",
    );
    let embedded =
        ResolvedFont::from_embedded_resource(&document.font_resource(10).unwrap(), 0).unwrap();
    assert_eq!(embedded.source(), &FontSource::Embedded { resource_id: 10 });
    assert!(ResolvedFont::from_embedded_resource(&document.font_resource(11).unwrap(), 0).is_err());
    let system = ResolvedFont::from_system_bytes(
        Arc::from(FONT),
        0,
        FontIdentity::new("controlled-system").unwrap(),
    )
    .unwrap();
    assert!(matches!(system.source(), FontSource::System { .. }));
    let fallback = ResolvedFont::from_configured_fallback_bytes(
        Arc::from(FONT),
        0,
        FontIdentity::new("controlled-fallback").unwrap(),
    )
    .unwrap();
    assert!(matches!(
        fallback.source(),
        FontSource::ConfiguredFallback { .. }
    ));
    assert!(SystemFontResolver::from_database_with_cache_capacity(
        Database::new(),
        Vec::new(),
        1 << 20,
        0,
    )
    .is_err());
}

#[test]
fn mismatched_resource_and_budgets_fail_before_resolver_invocation() {
    struct CountingResolver(AtomicUsize);
    impl FontResolver for CountingResolver {
        fn resolve_primary(
            &self,
            _resource: &rofd_core::FontResource,
        ) -> rofd_render::Result<Option<ResolvedFont>> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(None)
        }

        fn resolve_fallback(&self, _character: char) -> rofd_render::Result<Option<ResolvedFont>> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(None)
        }
    }

    let document = package(
        Some(FONT),
        r#"<ofd:TextCode X="0" Y="0">A</ofd:TextCode>"#,
        "",
    );
    let resolver = CountingResolver(AtomicUsize::new(0));
    assert!(matches!(
        position_glyph_runs(
            &resolver,
            &document.font_resource(11).unwrap(),
            &text_object(&document),
            &ResourceLimits::default(),
        ),
        Err(Error::FontResourceMismatch {
            expected_font_id: 10,
            actual_resource_id: 11,
            ..
        })
    ));
    assert_eq!(resolver.0.load(Ordering::SeqCst), 0);

    let limits = ResourceLimits {
        max_text_characters_per_page: 0,
        ..ResourceLimits::default()
    };
    assert!(matches!(
        position_glyph_runs(
            &resolver,
            &document.font_resource(10).unwrap(),
            &text_object(&document),
            &limits,
        ),
        Err(Error::InvalidTextLayout {
            field: "max_text_characters_per_page",
            ..
        })
    ));
    assert_eq!(resolver.0.load(Ordering::SeqCst), 0);
}

#[test]
fn concurrent_resolution_reuses_the_same_stable_font_identity() {
    let document = Arc::new(package(
        Some(FONT),
        r#"<ofd:TextCode X="0" Y="0">A</ofd:TextCode>"#,
        "",
    ));
    let resolver = Arc::new(SystemFontResolver::from_database(
        Database::new(),
        Vec::new(),
        1 << 20,
    ));
    let handles = (0..8)
        .map(|_| {
            let document = Arc::clone(&document);
            let resolver = Arc::clone(&resolver);
            thread::spawn(move || {
                let font = document.font_resource(10).unwrap();
                let runs = position_glyph_runs(
                    resolver.as_ref(),
                    &font,
                    &text_object(&document),
                    &ResourceLimits::default(),
                )
                .unwrap();
                let glyph = &runs[0].glyphs()[0];
                (
                    glyph.font_identity().to_owned(),
                    glyph.font().unwrap().encoded_bytes_arc(),
                )
            })
        })
        .collect::<Vec<_>>();
    let resolved = handles
        .into_iter()
        .map(|handle| handle.join().unwrap())
        .collect::<Vec<_>>();
    assert!(resolved
        .iter()
        .all(|(identity, _)| identity == &resolved[0].0));
    assert!(resolved
        .iter()
        .all(|(_, bytes)| Arc::ptr_eq(bytes, &resolved[0].1)));
}

#[test]
fn equal_resource_ids_from_distinct_documents_do_not_share_cache_entries() {
    let latin_document = package(
        Some(LATIN_FONT),
        r#"<ofd:TextCode X="0" Y="0">中</ofd:TextCode>"#,
        "",
    );
    let cjk_document = package(
        Some(FONT),
        r#"<ofd:TextCode X="0" Y="0">中</ofd:TextCode>"#,
        "",
    );
    let latin_resource = latin_document.font_resource(10).unwrap();
    let cjk_resource = cjk_document.font_resource(10).unwrap();
    assert_ne!(latin_resource.identity(), cjk_resource.identity());

    let resolver = SystemFontResolver::empty(Vec::new(), 1 << 20);
    let latin = position_glyph_runs(
        &resolver,
        &latin_resource,
        &text_object(&latin_document),
        &ResourceLimits::default(),
    )
    .unwrap();
    let cjk = position_glyph_runs(
        &resolver,
        &cjk_resource,
        &text_object(&cjk_document),
        &ResourceLimits::default(),
    )
    .unwrap();

    assert!(matches!(
        latin[0].diagnostics()[0],
        FontDiagnostic::MissingGlyph {
            character: '中',
            ..
        }
    ));
    assert!(cjk[0].diagnostics().is_empty());
    assert_ne!(
        latin[0].glyphs()[0].glyph_id(),
        cjk[0].glyphs()[0].glyph_id()
    );
}

/// The provider rofd-core asks for must answer exactly the geometry the positioner
/// used, so page character boxes land on the glyphs a renderer draws - horizontally
/// on their advance and vertically on their painted extents.
#[test]
fn font_text_metrics_match_the_positioner_and_follow_font_metrics() {
    // A, B and Z carry proportional advances in the fixture subset; most other
    // Latin scalars map to a full-width face advance equal to the font size, which
    // would not distinguish metrics from the size-based fallback. 中 adds a CJK glyph
    // whose ink reaches below the baseline.
    let document = package_named(
        Some(FONT),
        r#"<ofd:TextCode X="0" Y="4">ABZ中</ofd:TextCode>"#,
        "",
        "Fixture",
    );
    let text = text_object(&document);
    let page = document.page(0).unwrap();
    let resource = document.font_resource(text.font_id()).unwrap();
    let mut database = Database::new();
    database.load_font_data(FONT.to_vec());
    let resolver = SystemFontResolver::from_database(database, Vec::new(), 1 << 20);

    let runs = position_glyph_runs(&resolver, &resource, &text, page.resource_limits()).unwrap();
    let provider = rofd_render::FontTextMetrics::new(Arc::new(resolver));
    let metrics = provider
        .metrics_mm(&page, &text, text.font_size())
        .expect("embedded font resolves");

    assert!(
        !runs[0].glyphs()[0].is_synthetic_box(),
        "the subset font must own these glyphs: {:?}",
        runs[0].diagnostics()
    );

    // One entry per scalar, equal to the metrics of the glyph that owns it.
    let positioned: Vec<rofd_render::GlyphMetrics> = runs
        .iter()
        .flat_map(|run| run.glyphs())
        .map(|glyph| glyph.metrics())
        .collect();
    let advances: Vec<f64> = metrics.iter().map(|metrics| metrics.advance_mm).collect();
    assert_eq!(metrics.len(), positioned.len());
    assert_eq!(metrics.len(), 4);
    for (index, entry) in metrics.iter().enumerate() {
        assert!(
            (entry.advance_mm - positioned[index].advance().0).abs() < 1e-9,
            "scalar {index}: provider {} vs positioner {}",
            entry.advance_mm,
            positioned[index].advance().0
        );
        assert_eq!(entry.ascent_mm, positioned[index].ascent(), "{index}");
        assert_eq!(entry.descent_mm, positioned[index].descent(), "{index}");
    }

    // Font metrics, not a uniform font-size footprint: the Latin scalars are narrower
    // than the size, while the full-width 中 fills its em.
    assert!(advances[0] < advances[1], "{advances:?}");
    assert!(
        (advances[3] - text.font_size()).abs() < 1e-9,
        "{advances:?}"
    );
    assert!(advances[..3]
        .iter()
        .all(|advance| *advance < text.font_size()));
    assert!(advances.iter().all(|advance| *advance > 0.0));

    // The vertical extents come from the face's own metrics (hhea 1160/-288 over the
    // subset's 1000-unit em), scaled to Size: every glyph reports the same pair, which
    // is what keeps the boxes of Latin, CJK and punctuation aligned.
    let size = text.font_size();
    let per_em = |units: f64| units / 1000.0 * size;
    for entry in &metrics {
        assert!(
            (entry.ascent_mm - per_em(1160.0)).abs() < 1e-6,
            "{metrics:?}"
        );
        assert!(
            (entry.descent_mm - per_em(288.0)).abs() < 1e-6,
            "{metrics:?}"
        );
    }
    // 中 paints below the baseline and A does not, yet both boxes span the same band.
    assert!(metrics[3].descent_mm > 0.0);

    // Both layers agree on where the run ends.
    let semantic_end: f64 = advances.iter().sum();
    let positioned_end: f64 = positioned.iter().map(|metrics| metrics.advance().0).sum();
    assert!((semantic_end - positioned_end).abs() < 1e-9);
}
