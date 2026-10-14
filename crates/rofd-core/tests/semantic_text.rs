mod support;

use std::sync::Arc;

use rofd_core::{
    Document, Error, FindOptions, LoadOptions, Rect, ResourceLimits, SelectionStyle, TextCharFlags,
    TextGeometryPrecision, TextObject, TextSelection,
};

fn rect(x: f64, y: f64, width: f64, height: f64) -> Rect {
    Rect {
        x,
        y,
        width,
        height,
    }
}

#[test]
fn literal_search_defaults_to_case_insensitive_original_utf8_ranges() {
    let page = page_with_text(&text_object(2, "中Foo foo发票🙂发票"));
    let text = page.text().unwrap();
    assert_eq!(
        FindOptions::default(),
        FindOptions {
            case_sensitive: false,
            whole_words: false,
            max_results: 10_000,
        }
    );
    let matches = text.find("foo", FindOptions::default()).unwrap();
    assert_eq!(
        matches
            .iter()
            .map(|hit| hit.utf8_range())
            .collect::<Vec<_>>(),
        vec![3..6, 7..10]
    );
    assert_eq!(&text.as_str()[matches[0].utf8_range()], "Foo");
    assert_eq!(&text.as_str()[matches[1].utf8_range()], "foo");
    let cjk = text.find("发票", FindOptions::default()).unwrap();
    assert_eq!(
        cjk.iter().map(|hit| hit.utf8_range()).collect::<Vec<_>>(),
        vec![10..16, 20..26]
    );
    assert!(text
        .find("missing", FindOptions::default())
        .unwrap()
        .is_empty());
}

#[test]
fn search_respects_case_and_original_unicode_word_boundaries() {
    let page = page_with_text(&text_object(
        2,
        "Foo foo foobar _foo foo_ 中foo foo中 (foo)",
    ));
    let text = page.text().unwrap();
    let case = FindOptions {
        case_sensitive: true,
        whole_words: true,
        ..FindOptions::default()
    };
    let hits = text.find("foo", case).unwrap();
    assert_eq!(
        hits.iter().map(|hit| hit.utf8_range()).collect::<Vec<_>>(),
        vec![4..7, 40..43]
    );
    assert_eq!(
        text.find(
            "foo",
            FindOptions {
                case_sensitive: false,
                ..case
            }
        )
        .unwrap()
        .len(),
        3
    );
}

#[test]
fn rejected_whole_word_candidate_does_not_hide_later_overlapping_candidate() {
    let page = page_with_text(&text_object(2, "xfoo foo foo"));
    let text = page.text().unwrap();
    for case_sensitive in [false, true] {
        let hits = text
            .find(
                "foo foo",
                FindOptions {
                    case_sensitive,
                    whole_words: true,
                    ..FindOptions::default()
                },
            )
            .unwrap();
        assert_eq!(
            hits.iter().map(|hit| hit.utf8_range()).collect::<Vec<_>>(),
            vec![5..12]
        );
    }
}

#[test]
fn lowercase_expansions_map_partial_matches_to_complete_source_scalars() {
    let page = page_with_text(&text_object(2, "中İiİ"));
    let text = page.text().unwrap();
    for (query, expected) in [
        ("i", vec![3..5, 5..6, 6..8]),
        ("\u{307}", vec![3..5, 6..8]),
        ("İ", vec![3..5, 6..8]),
        ("\u{307}i", std::iter::once(3..6).collect()),
    ] {
        let hits = text.find(query, FindOptions::default()).unwrap();
        assert_eq!(
            hits.iter().map(|hit| hit.utf8_range()).collect::<Vec<_>>(),
            expected
        );
        for hit in hits {
            assert!(text.as_str().get(hit.utf8_range()).is_some());
        }
    }
    assert!(text
        .find(
            "i",
            FindOptions {
                whole_words: true,
                ..FindOptions::default()
            }
        )
        .unwrap()
        .is_empty());
}

#[test]
fn search_is_nonoverlapping_left_to_right_and_stops_at_exact_cap() {
    let page = page_with_text(&text_object(2, "aaaaa"));
    let text = page.text().unwrap();
    for case_sensitive in [false, true] {
        let options = FindOptions {
            case_sensitive,
            ..FindOptions::default()
        };
        let hits = text.find("aa", options).unwrap();
        assert_eq!(
            hits.iter().map(|hit| hit.utf8_range()).collect::<Vec<_>>(),
            vec![0..2, 2..4]
        );
        for cap in [1, 2, 3, usize::MAX] {
            let hits = text
                .find(
                    "aa",
                    FindOptions {
                        max_results: cap,
                        ..options
                    },
                )
                .unwrap();
            assert_eq!(hits.len(), cap.min(2));
            assert_eq!(hits[0].utf8_range(), 0..2);
        }
    }
}

#[test]
fn invalid_search_options_are_rejected_even_on_empty_pages() {
    for page in [page_with_text(""), page_with_text(&text_object(2, "a"))] {
        let text = page.text().unwrap();
        assert!(matches!(
            text.find("", FindOptions::default()),
            Err(Error::InvalidOption { field: "query", .. })
        ));
        assert!(matches!(
            text.find(
                "a",
                FindOptions {
                    max_results: 0,
                    ..FindOptions::default()
                }
            ),
            Err(Error::InvalidOption {
                field: "max_results",
                ..
            })
        ));
    }
}

#[test]
fn match_rectangles_union_visible_characters_and_allow_separator_only_matches() {
    let page = page_with_text(&format!("{}{}", text_object(2, "ab"), text_object(3, "中")));
    let text = page.text().unwrap();
    assert_eq!(
        text.find("ab", FindOptions::default()).unwrap()[0].rect_mm(),
        Some(rect(1.0, 1.0, 8.0, 4.0))
    );
    assert_eq!(
        text.find("b\n中", FindOptions::default()).unwrap()[0].rect_mm(),
        Some(rect(1.0, 1.0, 8.0, 4.0))
    );
    assert_eq!(
        text.find("\n", FindOptions::default()).unwrap()[0].rect_mm(),
        None
    );
}

fn selection_page() -> rofd_core::Page {
    page_with_text(
        r#"<ofd:TextObject ID="2" Boundary="0 0 100 8" Font="10" Size="4"><ofd:TextCode X="1" Y="5">ab_中, cd</ofd:TextCode></ofd:TextObject>
<ofd:TextObject ID="3" Boundary="0 10 100 8" Font="10" Size="4"><ofd:TextCode X="1" Y="5">ef gh</ofd:TextCode></ofd:TextObject>"#,
    )
}

#[test]
fn area_extraction_and_glyph_selection_require_positive_intersection() {
    let page = selection_page();
    let text = page.text().unwrap();
    let area = rect(5.0, 2.0, 4.0, 12.0);
    assert_eq!(text.text_for_area(area).unwrap(), "bf");
    let selection = text.select(area, SelectionStyle::Glyph).unwrap();
    assert_eq!(selection.text(), "bf");
    assert_eq!(
        selection.regions(),
        &[rect(5.0, 1.0, 4.0, 4.0), rect(5.0, 11.0, 4.0, 4.0)]
    );
    assert_eq!(text.text_for_area(rect(8.9, 2.0, 0.2, 1.0)).unwrap(), "b_");
    assert_eq!(text.text_for_area(rect(1.0, 5.0, 4.0, 1.0)).unwrap(), "");
}

#[test]
fn word_selection_expands_unicode_words_without_crossing_punctuation_or_space() {
    let page = selection_page();
    let text = page.text().unwrap();
    let selection = text
        .select(rect(6.0, 2.0, 8.0, 1.0), SelectionStyle::Word)
        .unwrap();
    assert_eq!(selection.text(), "ab_中");
    assert_eq!(selection.regions(), &[rect(1.0, 1.0, 16.0, 4.0)]);
    assert_eq!(
        text.select(rect(18.0, 2.0, 1.0, 1.0), SelectionStyle::Word)
            .unwrap()
            .text(),
        ","
    );
    assert_eq!(
        text.select(rect(22.0, 2.0, 1.0, 1.0), SelectionStyle::Word)
            .unwrap()
            .text(),
        " "
    );
    assert_eq!(
        text.select(rect(26.0, 2.0, 1.0, 12.0), SelectionStyle::Word)
            .unwrap()
            .text(),
        "cd"
    );
}

#[test]
fn line_selection_preserves_internal_synthesized_newlines_and_region_order() {
    let page = selection_page();
    let text = page.text().unwrap();
    let one = text
        .select(rect(6.0, 2.0, 1.0, 1.0), SelectionStyle::Line)
        .unwrap();
    assert_eq!(one.text(), "ab_中, cd");
    assert_eq!(one.regions(), &[rect(1.0, 1.0, 32.0, 4.0)]);
    let both = text
        .select(rect(6.0, 2.0, 1.0, 12.0), SelectionStyle::Line)
        .unwrap();
    assert_eq!(both.text(), "ab_中, cd\nef gh");
    assert_eq!(
        both.regions(),
        &[rect(1.0, 1.0, 32.0, 4.0), rect(1.0, 11.0, 20.0, 4.0)]
    );
}

#[test]
fn selection_preserves_canonical_order_when_geometry_runs_backwards() {
    let page = page_with_text(
        r#"<ofd:TextObject ID="2" Boundary="0 0 100 8" Font="10" Size="4"><ofd:TextCode X="13" Y="5" DeltaX="-4 -4 -4">abc</ofd:TextCode></ofd:TextObject>"#,
    );
    let text = page.text().unwrap();
    let area = rect(1.0, 1.0, 12.0, 4.0);
    assert_eq!(text.text_for_area(area).unwrap(), "abc");
    let selection = text.select(area, SelectionStyle::Glyph).unwrap();
    assert_eq!(selection.text(), "abc");
    assert_eq!(selection.regions(), &[rect(1.0, 1.0, 12.0, 4.0)]);
}

#[test]
fn invalid_rectangles_error_while_empty_or_missed_rectangles_return_empty_results() {
    let page = selection_page();
    let text = page.text().unwrap();
    for area in [
        rect(0.0, 0.0, -1.0, 20.0),
        rect(0.0, 0.0, 20.0, -1.0),
        rect(f64::NAN, 0.0, 20.0, 20.0),
        rect(0.0, f64::INFINITY, 20.0, 20.0),
        rect(0.0, 0.0, f64::INFINITY, 20.0),
        rect(0.0, 0.0, 20.0, f64::NAN),
        rect(f64::MAX, 0.0, f64::MAX, 20.0),
    ] {
        assert!(matches!(
            text.text_for_area(area),
            Err(Error::InvalidOption { field: "area", .. })
        ));
        for style in [
            SelectionStyle::Glyph,
            SelectionStyle::Word,
            SelectionStyle::Line,
        ] {
            assert!(matches!(
                text.select(area, style),
                Err(Error::InvalidOption { field: "area", .. })
            ));
        }
    }
    for area in [
        rect(0.0, 0.0, 0.0, 20.0),
        rect(0.0, 0.0, 20.0, 0.0),
        rect(100.0, 100.0, 1.0, 1.0),
    ] {
        assert_eq!(text.text_for_area(area).unwrap(), "");
        for style in [
            SelectionStyle::Glyph,
            SelectionStyle::Word,
            SelectionStyle::Line,
        ] {
            assert_eq!(text.select(area, style).unwrap(), TextSelection::default());
        }
    }
    // Negative positions are valid; only negative dimensions are invalid.
    assert_eq!(text.text_for_area(rect(-1.0, -1.0, 3.0, 3.0)).unwrap(), "a");
}

#[test]
fn unrepresentable_geometry_unions_fail_without_nonfinite_output() {
    let page = page_with_text(
        r#"<ofd:TextObject ID="2" Boundary="0 0 100 8" Font="10" Size="1e307"><ofd:TextCode X="-1e308" Y="1e307">a</ofd:TextCode><ofd:TextCode X="1e308" Y="1e307">b</ofd:TextCode></ofd:TextObject>"#,
    );
    let text = page.text().unwrap();
    assert!(matches!(
        text.find("ab", FindOptions::default()),
        Err(Error::InvalidValue { .. })
    ));
    let area = rect(-1e308, 0.0, 1e307, 1e307);
    assert_eq!(text.text_for_area(area).unwrap(), "a");
    assert!(matches!(
        text.select(area, SelectionStyle::Line),
        Err(Error::InvalidValue { .. })
    ));
}

const FONT_CATALOG: &str = r#"<ofd:Res xmlns:ofd="http://www.ofdspec.org/2016"><ofd:Fonts><ofd:Font ID="10" FontName="Fixture"/></ofd:Fonts></ofd:Res>"#;

fn document_with_page(
    page_xml: &str,
    common: &str,
    tail: &str,
    entries: &[(&str, &[u8])],
) -> Document {
    document_with_page_options(page_xml, common, tail, entries, LoadOptions::default())
}

fn document_with_page_options(
    page_xml: &str,
    common: &str,
    tail: &str,
    entries: &[(&str, &[u8])],
    options: LoadOptions,
) -> Document {
    let document_xml = format!(
        r#"<ofd:Document xmlns:ofd="http://www.ofdspec.org/2016"><ofd:CommonData>
<ofd:PageArea><ofd:PhysicalBox>0 0 210 297</ofd:PhysicalBox></ofd:PageArea>
<ofd:PublicRes>Res.xml</ofd:PublicRes>{common}</ofd:CommonData>
<ofd:Pages><ofd:Page ID="900" BaseLoc="Pages/Page_0/Content.xml"/></ofd:Pages>{tail}</ofd:Document>"#
    );
    let entries = [("Doc_0/Res.xml", FONT_CATALOG.as_bytes())]
        .into_iter()
        .chain(entries.iter().copied())
        .collect::<Vec<_>>();
    Document::from_bytes(
        support::ofd_with_document_page_and_entries(&document_xml, page_xml, &entries),
        options,
    )
    .unwrap()
}

fn page_with_text(content: &str) -> rofd_core::Page {
    let page_xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<ofd:Page xmlns:ofd="http://www.ofdspec.org/2016">
  <ofd:Content><ofd:Layer ID="1">{content}</ofd:Layer></ofd:Content>
</ofd:Page>"#
    );
    document_with_page(&page_xml, "", "", &[]).page(0).unwrap()
}

/// A provider that advances every scalar by half the font size, so the geometry it
/// produces is distinguishable from both the font-size footprint and explicit deltas.
struct HalfSizeAdvances {
    resolution_failure: bool,
}

impl rofd_core::TextAdvanceProvider for HalfSizeAdvances {
    fn advances_mm(
        &self,
        _page: &rofd_core::Page,
        text: &TextObject,
        size_mm: f64,
    ) -> Option<Vec<f64>> {
        if self.resolution_failure {
            return None;
        }
        Some(
            text.runs()
                .iter()
                .flat_map(|run| run.text().chars().map(|_| size_mm / 2.0))
                .collect(),
        )
    }
}

fn document_with_page_and_advances(
    page_xml: &str,
    advances: Option<Arc<dyn rofd_core::TextAdvanceProvider>>,
) -> Document {
    let document_xml = r#"<ofd:Document xmlns:ofd="http://www.ofdspec.org/2016"><ofd:CommonData>
<ofd:PageArea><ofd:PhysicalBox>0 0 210 297</ofd:PhysicalBox></ofd:PageArea>
<ofd:PublicRes>Res.xml</ofd:PublicRes></ofd:CommonData>
<ofd:Pages><ofd:Page ID="900" BaseLoc="Pages/Page_0/Content.xml"/></ofd:Pages></ofd:Document>"#;
    let entries = [("Doc_0/Res.xml", FONT_CATALOG.as_bytes())];
    Document::from_bytes_with_advances(
        support::ofd_with_document_page_and_entries(document_xml, page_xml, &entries),
        LoadOptions::default(),
        advances,
    )
    .unwrap()
}

#[test]
fn advance_provider_supplies_scalar_steps_where_deltas_are_absent() {
    let page_xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<ofd:Page xmlns:ofd="http://www.ofdspec.org/2016">
  <ofd:Content><ofd:Layer ID="1">{}</ofd:Layer></ofd:Content>
</ofd:Page>"#,
        text_object(2, "ABCD")
    );

    let without = document_with_page_and_advances(&page_xml, None)
        .page(0)
        .unwrap()
        .text()
        .unwrap()
        .characters()
        .to_vec();
    let with = document_with_page_and_advances(
        &page_xml,
        Some(Arc::new(HalfSizeAdvances {
            resolution_failure: false,
        })),
    )
    .page(0)
    .unwrap()
    .text()
    .unwrap()
    .characters()
    .to_vec();

    // The fixture places run X at 1 and uses Size 4, so the second scalar starts at
    // 5 without a provider and at 3 with one, and the same holds for every step.
    let first = without[0].rect_mm().unwrap();
    assert_eq!(without[1].rect_mm().unwrap().x - first.x, 4.0);
    assert_eq!(with[1].rect_mm().unwrap().x - first.x, 2.0);
    assert_eq!(with[3].rect_mm().unwrap().x - first.x, 6.0);
    // The footprint stays conservative: every box is at least Size wide.
    assert!(with[3].rect_mm().unwrap().width >= 4.0);

    // A provider that cannot resolve a font leaves the font-size footprint alone.
    let failed = document_with_page_and_advances(
        &page_xml,
        Some(Arc::new(HalfSizeAdvances {
            resolution_failure: true,
        })),
    )
    .page(0)
    .unwrap()
    .text()
    .unwrap()
    .characters()
    .to_vec();
    assert_eq!(failed[1].rect_mm().unwrap().x - first.x, 4.0);
}

#[test]
fn explicit_deltas_stay_authoritative_with_a_provider() {
    let page_xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<ofd:Page xmlns:ofd="http://www.ofdspec.org/2016">
  <ofd:Content><ofd:Layer ID="1">{}</ofd:Layer></ofd:Content>
</ofd:Page>"#,
        r#"<ofd:TextObject ID="2" Boundary="0 0 20 8" Font="10" Size="4"><ofd:TextCode X="1" Y="5" DeltaX="9 9 9 9">ABCD</ofd:TextCode></ofd:TextObject>"#
    );
    let text = document_with_page_and_advances(
        &page_xml,
        Some(Arc::new(HalfSizeAdvances {
            resolution_failure: false,
        })),
    )
    .page(0)
    .unwrap()
    .text()
    .unwrap()
    .characters()
    .to_vec();

    let first = text[0].rect_mm().unwrap();
    assert_eq!(text[1].rect_mm().unwrap().x - first.x, 9.0);
}

#[test]
fn page_text_exposes_utf8_ranges_source_ids_and_conservative_geometry() {
    const SYNTHESIZED_SEPARATOR_BITS: u32 = TextCharFlags::SYNTHESIZED_SEPARATOR.bits();
    const HAS_SYNTHESIZED_SEPARATOR: bool =
        TextCharFlags::SYNTHESIZED_SEPARATOR.contains(TextCharFlags::SYNTHESIZED_SEPARATOR);

    assert_eq!(SYNTHESIZED_SEPARATOR_BITS, 1);
    const { assert!(HAS_SYNTHESIZED_SEPARATOR) };

    let page = page_with_text(
        r#"<ofd:TextObject ID="2" Boundary="10 20 30 8" Font="10" Size="4">
  <ofd:TextCode X="1" Y="5" DeltaX="3 4">A中B</ofd:TextCode>
</ofd:TextObject>
<ofd:TextObject ID="3" Boundary="10 35 30 8" Font="10" Size="4">
  <ofd:TextCode X="1" Y="5">尾</ofd:TextCode>
</ofd:TextObject>"#,
    );

    let text = page.text().unwrap();
    assert_eq!(text.as_str(), "A中B\n尾");

    let characters = text.characters();
    assert_eq!(characters.len(), 5);
    assert_eq!(characters[0].utf8_range(), 0..1);
    assert_eq!(characters[1].utf8_range(), 1..4);
    assert_eq!(characters[2].utf8_range(), 4..5);
    assert_eq!(characters[3].utf8_range(), 5..6);
    assert_eq!(characters[4].utf8_range(), 6..9);

    assert_eq!(characters[1].object_id(), Some(2));
    assert_eq!(
        characters[1].geometry_precision(),
        TextGeometryPrecision::Conservative
    );
    let middle_rect = characters[1].rect_mm().expect("middle character geometry");
    assert!(middle_rect.width > 0.0);
    assert!(middle_rect.height > 0.0);

    let separator = &characters[3];
    assert!(separator
        .flags()
        .contains(TextCharFlags::SYNTHESIZED_SEPARATOR));
    assert_eq!(separator.object_id(), None);
    assert_eq!(separator.rect_mm(), None);
}

fn text_object(id: u64, text: &str) -> String {
    format!(
        r#"<ofd:TextObject ID="{id}" Boundary="0 0 20 8" Font="10" Size="4"><ofd:TextCode X="1" Y="5">{text}</ofd:TextCode></ofd:TextObject>"#
    )
}

#[test]
fn semantic_text_recurses_in_effective_paint_order_and_maps_to_page_mm() {
    let page = page_with_text(
        r#"<ofd:PageBlock ID="2"><ofd:TextObject ID="3" Boundary="10 20 20 8" CTM="1 0 0 1 2 3" Font="10" Size="4"><ofd:TextCode X="1" Y="5" DeltaX="4">甲乙</ofd:TextCode></ofd:TextObject></ofd:PageBlock>"#,
    );
    let text = page.text().unwrap();
    assert_eq!(text.as_str(), "甲乙");
    let rect = text.characters()[0].rect_mm().unwrap();
    assert_eq!((rect.x, rect.y), (13.0, 24.0));
    assert!(rect.width > 0.0 && rect.height > 0.0);
}

#[test]
fn repeated_page_handles_share_one_published_semantic_index() {
    let page = page_with_text(&text_object(2, "cache"));
    let clone = page.clone();
    assert!(std::ptr::eq(page.text().unwrap(), clone.text().unwrap()));
    assert_eq!(page.text().unwrap().as_str(), "cache");
}

#[test]
fn concurrent_cold_page_handles_publish_one_complete_index() {
    let page = page_with_text(&text_object(2, "并发 cache"));
    let barrier = std::sync::Barrier::new(8);
    std::thread::scope(|scope| {
        let handles = (0..8)
            .map(|_| {
                let page = &page;
                let barrier = &barrier;
                scope.spawn(move || {
                    barrier.wait();
                    let text = page.text().unwrap();
                    assert_eq!(text.as_str(), "并发 cache");
                    assert_eq!(text.characters().len(), 8);
                    text
                })
            })
            .collect::<Vec<_>>();
        for handle in handles {
            assert!(std::ptr::eq(handle.join().unwrap(), page.text().unwrap()));
        }
    });
}

#[test]
fn explicit_zero_negative_and_vertical_deltas_preserve_source_run_positions() {
    let page = page_with_text(
        r#"<ofd:TextObject ID="2" Boundary="0 0 30 20" Font="10" Size="4">
<ofd:TextCode X="10" Y="5" DeltaX="0 -3 2" DeltaY="2 -1 0">ABC</ofd:TextCode>
<ofd:TextCode X="2" Y="10">DE</ofd:TextCode></ofd:TextObject>"#,
    );
    let text = page.text().unwrap();
    assert_eq!(text.as_str(), "ABCDE");
    let rects = text
        .characters()
        .iter()
        .map(|ch| ch.rect_mm().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        rects,
        vec![
            Rect {
                x: 10.0,
                y: 1.0,
                width: 4.0,
                height: 6.0
            },
            Rect {
                x: 7.0,
                y: 2.0,
                width: 4.0,
                height: 5.0
            },
            Rect {
                x: 7.0,
                y: 2.0,
                width: 4.0,
                height: 4.0
            },
            Rect {
                x: 2.0,
                y: 6.0,
                width: 4.0,
                height: 4.0
            },
            Rect {
                x: 6.0,
                y: 6.0,
                width: 4.0,
                height: 4.0
            },
        ]
    );
}

#[test]
fn source_whitespace_is_flagged_and_empty_objects_do_not_add_separators() {
    let page = page_with_text(
        &[
            text_object(2, ""),
            text_object(3, "A &#x9;&#xA;中"),
            text_object(4, ""),
            text_object(5, "B"),
            text_object(6, ""),
        ]
        .join(""),
    );
    let text = page.text().unwrap();
    assert_eq!(text.as_str(), "A \t\n中\nB");
    for (index, ch) in text.characters().iter().enumerate() {
        assert_eq!(
            ch.flags().contains(TextCharFlags::WHITESPACE),
            (1..=3).contains(&index)
        );
        assert_eq!(
            ch.flags().contains(TextCharFlags::SYNTHESIZED_SEPARATOR),
            index == 5
        );
        assert_eq!(ch.geometry_precision(), TextGeometryPrecision::Conservative);
    }
}

fn composite_page(transform: &str, inner_transform: &str) -> rofd_core::Page {
    let catalog = format!(
        r#"<ofd:Res xmlns:ofd="http://www.ofdspec.org/2016"><ofd:CompositeGraphicUnits>
<ofd:CompositeGraphicUnit ID="20" Width="20" Height="20"><ofd:Content ID="21">
<ofd:CompositeObject ID="22" ResourceID="30" Boundary="3 4 20 20" CTM="{inner_transform}"/>
</ofd:Content></ofd:CompositeGraphicUnit>
<ofd:CompositeGraphicUnit ID="30" Width="20" Height="20"><ofd:Content ID="31">
<ofd:PageBlock ID="32"><ofd:TextObject ID="33" Boundary="5 6 20 8" CTM="1 0 0 1 2 3" Font="10" Size="4"><ofd:TextCode X="1" Y="5">X</ofd:TextCode></ofd:TextObject></ofd:PageBlock>
</ofd:Content></ofd:CompositeGraphicUnit></ofd:CompositeGraphicUnits></ofd:Res>"#
    );
    let page = format!(
        r#"<ofd:Page xmlns:ofd="http://www.ofdspec.org/2016"><ofd:Content><ofd:Layer ID="1">
<ofd:CompositeObject ID="2" ResourceID="20" Boundary="10 20 40 40" CTM="{transform}"/>
<ofd:TextObject ID="3" Boundary="0 0 20 8" CTM="1 0 0 0 0 0" Font="10" Size="4"><ofd:TextCode X="1" Y="5">hidden</ofd:TextCode></ofd:TextObject>
{}</ofd:Layer></ofd:Content></ofd:Page>"#,
        text_object(4, "tail")
    );
    document_with_page(
        &page,
        "<ofd:DocumentRes>Composite.xml</ofd:DocumentRes>",
        "",
        &[("Doc_0/Composite.xml", catalog.as_bytes())],
    )
    .page(0)
    .unwrap()
}

#[test]
fn nested_composites_compose_child_then_local_then_parent_transforms() {
    let page = composite_page("2 0 0 3 1 2", "0 1 -1 0 7 8");
    let text = page.text().unwrap();
    assert_eq!(text.as_str(), "X\ntail");
    assert_eq!(text.characters()[0].object_id(), Some(33));
    assert_eq!(
        text.characters()[0].rect_mm(),
        Some(Rect {
            x: 3.0,
            y: 82.0,
            width: 8.0,
            height: 12.0
        })
    );
}

#[test]
fn singular_composites_and_text_are_omitted_without_separators() {
    for (outer, inner) in [
        ("1 0 0 0 0 0", "1 0 0 1 0 0"),
        ("1 0 0 1 0 0", "0 0 0 1 0 0"),
    ] {
        assert_eq!(
            composite_page(outer, inner).text().unwrap().as_str(),
            "tail"
        );
    }
}

#[test]
fn effective_template_and_layer_order_precedes_visible_annotation_appearances() {
    let layer = |id: u64, kind, text| {
        format!(
            r#"<ofd:Layer ID="{id}" Type="{kind}">{}</ofd:Layer>"#,
            text_object(id + 100, text)
        )
    };
    let page = format!(
        r#"<ofd:Page xmlns:ofd="http://www.ofdspec.org/2016"><ofd:Template TemplateID="20"/><ofd:Template TemplateID="30"/><ofd:Content>{}{}{}{}</ofd:Content></ofd:Page>"#,
        layer(1, "Foreground", "front"),
        layer(2, "Body", "body1"),
        layer(3, "Background", "back"),
        layer(4, "Body", "body2")
    );
    let background = format!(
        r#"<ofd:Page xmlns:ofd="http://www.ofdspec.org/2016"><ofd:Content>{}</ofd:Content></ofd:Page>"#,
        layer(5, "Body", "template back")
    );
    let foreground = format!(
        r#"<ofd:Page xmlns:ofd="http://www.ofdspec.org/2016"><ofd:Content>{}</ofd:Content></ofd:Page>"#,
        layer(6, "Body", "template front")
    );
    let annotations = r#"<ofd:Annotations xmlns:ofd="http://www.ofdspec.org/2016"><ofd:Page PageID="900">
<ofd:FileLoc>Annots/Page_0/Annotation.xml</ofd:FileLoc>
</ofd:Page></ofd:Annotations>"#;
    let page_annotations = format!(
        r#"<ofd:PageAnnot xmlns:ofd="http://www.ofdspec.org/2016">
<ofd:Annot ID="40" Type="Stamp"><ofd:Appearance Boundary="100 200 30 20"><ofd:PageBlock ID="41">{}</ofd:PageBlock></ofd:Appearance></ofd:Annot>
<ofd:Annot ID="50" Type="Stamp" Visible="false"><ofd:Appearance Boundary="0 0 30 20">{}</ofd:Appearance></ofd:Annot>
<ofd:Annot ID="60" Type="Stamp"><ofd:Appearance Boundary="0 0 30 20">{}</ofd:Appearance></ofd:Annot>
</ofd:PageAnnot>"#,
        text_object(42, "annotation1"),
        text_object(51, "invisible"),
        text_object(61, "annotation2")
    );
    let document = document_with_page(
        &page,
        r#"<ofd:TemplatePage ID="20" BaseLoc="Back.xml"/><ofd:TemplatePage ID="30" BaseLoc="Front.xml" ZOrder="Foreground"/>"#,
        "<ofd:Annotations>Annotations.xml</ofd:Annotations>",
        &[
            ("Doc_0/Back.xml", background.as_bytes()),
            ("Doc_0/Front.xml", foreground.as_bytes()),
            ("Doc_0/Annotations.xml", annotations.as_bytes()),
            (
                "Doc_0/Annots/Page_0/Annotation.xml",
                page_annotations.as_bytes(),
            ),
        ],
    );
    let page = document.page(0).unwrap();
    let text = page.text().unwrap();
    assert_eq!(
        text.as_str(),
        "template back\nback\nbody1\nbody2\nfront\ntemplate front\nannotation1\nannotation2"
    );
    let annotation = text
        .characters()
        .iter()
        .find(|ch| ch.object_id() == Some(42))
        .unwrap();
    assert_eq!(
        annotation.rect_mm(),
        Some(Rect {
            x: 101.0,
            y: 201.0,
            width: 4.0,
            height: 4.0
        })
    );
    assert!(document
        .warnings()
        .iter()
        .all(|warning| warning.code != rofd_core::WarningCode::AnnotationSkipped));
}

#[test]
fn overflowing_coordinates_fail_repeatedly_without_partial_cache_or_poisoning() {
    for attributes in [
        r#"CTM="1e308 0 0 1 0 0""#,
        r#"Boundary="1e308 0 20 8" CTM="1 0 0 1 1e308 0""#,
    ] {
        let boundary = if attributes.starts_with("Boundary") {
            ""
        } else {
            r#"Boundary="0 0 20 8""#
        };
        let page = page_with_text(&format!(
            r#"{}<ofd:TextObject ID="3" {boundary} {attributes} Font="10" Size="4"><ofd:TextCode X="10" Y="5">overflow</ofd:TextCode></ofd:TextObject>"#,
            text_object(2, "valid prefix")
        ));
        for handle in [&page, &page.clone(), &page] {
            assert!(matches!(handle.text(), Err(Error::InvalidValue { .. })));
        }
    }
}

#[test]
fn collapsed_character_geometry_fails_repeatedly_without_cache_publication() {
    let page = page_with_text(
        r#"<ofd:TextObject ID="2" Boundary="0 0 20 8" Font="10" Size="1">
<ofd:TextCode X="10000000000000000" Y="10000000000000000" DeltaX="0">X</ofd:TextCode>
</ofd:TextObject>"#,
    );
    let clone = page.clone();

    for handle in [&page, &clone, &page] {
        let error = handle.text().expect_err("collapsed geometry must fail");
        assert!(
            matches!(
                error,
                Error::InvalidValue {
                    field: "semantic text geometry",
                    ..
                }
            ),
            "{error:?}"
        );
    }
}

fn page_with_aggregate_semantic_text(limits: ResourceLimits) -> rofd_core::Page {
    let page_xml = format!(
        r#"<ofd:Page xmlns:ofd="http://www.ofdspec.org/2016"><ofd:Content><ofd:Layer ID="1">{}</ofd:Layer></ofd:Content></ofd:Page>"#,
        text_object(2, "A")
    );
    let annotations = r#"<ofd:Annotations xmlns:ofd="http://www.ofdspec.org/2016"><ofd:Page PageID="900"><ofd:FileLoc>Annots/Page_0/Annotation.xml</ofd:FileLoc></ofd:Page></ofd:Annotations>"#;
    let page_annotations = format!(
        r#"<ofd:PageAnnot xmlns:ofd="http://www.ofdspec.org/2016">
<ofd:Annot ID="10" Type="Stamp"><ofd:Appearance Boundary="0 10 20 8">{}</ofd:Appearance></ofd:Annot>
<ofd:Annot ID="20" Type="Stamp"><ofd:Appearance Boundary="0 20 20 8">{}</ofd:Appearance></ofd:Annot>
<ofd:Annot ID="30" Type="Stamp" Visible="false"><ofd:Appearance Boundary="0 30 20 8">{}</ofd:Appearance></ofd:Annot>
</ofd:PageAnnot>"#,
        text_object(11, "B"),
        text_object(21, "C"),
        text_object(31, "D")
    );
    document_with_page_options(
        &page_xml,
        "",
        "<ofd:Annotations>Annotations.xml</ofd:Annotations>",
        &[
            ("Doc_0/Annotations.xml", annotations.as_bytes()),
            (
                "Doc_0/Annots/Page_0/Annotation.xml",
                page_annotations.as_bytes(),
            ),
        ],
        LoadOptions {
            limits,
            ..LoadOptions::default()
        },
    )
    .page(0)
    .unwrap()
}

#[test]
fn semantic_source_scalar_budget_is_aggregate_across_visible_page_content() {
    let exact_limits = ResourceLimits {
        max_text_characters_per_page: 3,
        ..ResourceLimits::default()
    };
    assert_eq!(
        page_with_aggregate_semantic_text(exact_limits)
            .text()
            .unwrap()
            .as_str(),
        "A\nB\nC"
    );

    let exceeded_limits = ResourceLimits {
        max_text_characters_per_page: 2,
        ..ResourceLimits::default()
    };
    let page = page_with_aggregate_semantic_text(exceeded_limits);
    let clone = page.clone();
    for handle in [&page, &clone, &page] {
        assert!(matches!(
            handle.text(),
            Err(Error::LimitExceeded(message))
                if message == "semantic page source character limit exceeded: 3 > 2"
        ));
    }
}

#[test]
fn semantic_metadata_budget_counts_synthesized_separators_at_exact_boundary() {
    let exact_limits = ResourceLimits {
        max_text_characters_per_page: 3,
        max_text_expansion_entries: 5,
        ..ResourceLimits::default()
    };
    assert_eq!(
        page_with_aggregate_semantic_text(exact_limits)
            .text()
            .unwrap()
            .characters()
            .len(),
        5
    );

    let exceeded_limits = ResourceLimits {
        max_text_characters_per_page: 3,
        max_text_expansion_entries: 4,
        ..ResourceLimits::default()
    };
    let page = page_with_aggregate_semantic_text(exceeded_limits);
    let clone = page.clone();
    for handle in [&page, &clone, &page] {
        assert!(matches!(
            handle.text(),
            Err(Error::LimitExceeded(message))
                if message == "semantic text metadata entry limit exceeded: 5 > 4"
        ));
    }
}
