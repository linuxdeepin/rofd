mod support;

use rofd_core::{
    Document, Error, FillRule, LoadOptions, PageObject, PathData, Rect, ResourceLimits, Transform,
};
use support::minimal_ofd;

fn page_with(content: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<ofd:Page xmlns:ofd="http://www.ofdspec.org/2016">
  <ofd:Area><ofd:PhysicalBox>0 0 210 297</ofd:PhysicalBox></ofd:Area>
  {content}
</ofd:Page>"#
    )
}

fn open_page_with_limits(
    content: &str,
    limits: ResourceLimits,
) -> rofd_core::Result<rofd_core::Page> {
    let document = Document::from_bytes(
        minimal_ofd(&page_with(content)),
        LoadOptions {
            limits,
            ..LoadOptions::default()
        },
    )?;
    document.page(0)
}

fn open_page(content: &str) -> rofd_core::Result<rofd_core::Page> {
    open_page_with_limits(content, ResourceLimits::default())
}

fn open_page_strict(content: &str) -> rofd_core::Result<rofd_core::Page> {
    let document = Document::from_bytes(
        minimal_ofd(&page_with(content)),
        LoadOptions {
            strictness: rofd_core::Strictness::Strict,
            ..LoadOptions::default()
        },
    )?;
    document.page(0)
}

fn path_with_clips(clips: &str, data: &str) -> String {
    format!(
        r#"<ofd:Content><ofd:Layer ID="1"><ofd:PathObject ID="2" Boundary="100 200 30 40">
  {clips}
  <ofd:AbbreviatedData>{data}</ofd:AbbreviatedData>
</ofd:PathObject></ofd:Layer></ofd:Content>"#
    )
}

fn path_object(page: &rofd_core::Page) -> &rofd_core::PathObject {
    let PageObject::Path(path) = &page.layers()[0].objects()[0] else {
        panic!("expected path object");
    };
    path
}

#[test]
fn exposes_validated_path_clip_model() {
    let page = open_page(&path_with_clips(
        r#"<ofd:Clips TransFlag="false"><ofd:Clip><ofd:Area CTM="0 1 -1 0 7 8">
    <ofd:Path Boundary="10 20 30 40" CTM="2 0 0 3 1 2" Stroke="false" Fill="true" Rule="Even-Odd">
      <ofd:AbbreviatedData>M 1 2 L 3 4 C</ofd:AbbreviatedData>
    </ofd:Path>
  </ofd:Area></ofd:Clip></ofd:Clips>"#,
        "M 0 0",
    ))
    .unwrap();

    let clip = &path_object(&page).clips()[0];
    assert!(!clip.affected_by_object_transform());
    assert_eq!(clip.paths().len(), 1);
    let path = &clip.paths()[0];
    assert_eq!(
        path.boundary(),
        Rect {
            x: 10.0,
            y: 20.0,
            width: 30.0,
            height: 40.0,
        }
    );
    assert_eq!(
        path.transform(),
        Transform::new(2.0, 0.0, 0.0, 3.0, 1.0, 2.0).unwrap()
    );
    assert_eq!(
        path.area_transform(),
        Transform::new(0.0, 1.0, -1.0, 0.0, 7.0, 8.0).unwrap()
    );
    assert_eq!(path.path_data(), &PathData::parse("M 1 2 L 3 4 C").unwrap());
    assert_eq!(path.fill_rule(), FillRule::EvenOdd);
}

#[test]
fn trans_flag_defaults_false_and_accepts_xml_schema_boolean_spellings() {
    for (attribute, expected) in [
        ("", false),
        (r#" TransFlag="false""#, false),
        (r#" TransFlag="0""#, false),
        (r#" TransFlag="true""#, true),
        (r#" TransFlag="1""#, true),
    ] {
        let clips = format!(
            r#"<ofd:Clips{attribute}><ofd:Clip><ofd:Area><ofd:Path Boundary="0 0 1 1" Fill="true" Stroke="false"><ofd:AbbreviatedData>M 0 0</ofd:AbbreviatedData></ofd:Path></ofd:Area></ofd:Clip></ofd:Clips>"#
        );
        let page = open_page(&path_with_clips(&clips, "M 0 0")).unwrap();
        let clip_path = &path_object(&page).clips()[0].paths()[0];
        assert_eq!(clip_path.transform(), Transform::IDENTITY);
        assert_eq!(clip_path.area_transform(), Transform::IDENTITY);
        assert_eq!(
            path_object(&page).clips()[0].affected_by_object_transform(),
            expected,
            "attribute {attribute:?}"
        );
    }
}

#[test]
fn empty_clips_element_is_tolerated_in_lenient_mode() {
    // Invoice generators emit empty <Clips/> elements; lenient mode treats
    // them as absent rather than rejecting the whole page.
    let page = open_page(&path_with_clips("<ofd:Clips/>", "M 0 0")).unwrap();
    assert_eq!(path_object(&page).clips().len(), 0);
}

#[test]
fn empty_clips_element_is_rejected_in_strict_mode() {
    let error = open_page_strict(&path_with_clips("<ofd:Clips/>", "M 0 0")).unwrap_err();
    assert!(matches!(
        error,
        Error::InvalidStructure { ref message, .. } if message.contains("Clips must contain at least one Clip")
    ), "got {error:?}");
}

#[test]
fn rejects_missing_or_ambiguous_clip_structure() {
    let cases = [
        (
            "<ofd:Clips><ofd:Clip/></ofd:Clips>",
            "Clip must contain at least one Area",
        ),
        (
            "<ofd:Clips><ofd:Clip><ofd:Area/></ofd:Clip></ofd:Clips>",
            "Area must contain exactly one Path or Text",
        ),
        (
            r#"<ofd:Clips><ofd:Clip><ofd:Area><ofd:Path Boundary="0 0 1 1" Fill="true" Stroke="false"><ofd:AbbreviatedData>M 0 0</ofd:AbbreviatedData></ofd:Path><ofd:Text/></ofd:Area></ofd:Clip></ofd:Clips>"#,
            "Area must contain exactly one Path or Text",
        ),
    ];

    for (clips, expected) in cases {
        let error = open_page(&path_with_clips(clips, "M 0 0")).unwrap_err();
        assert!(
            matches!(error, Error::InvalidStructure { ref message, .. } if message.contains(expected)),
            "expected {expected:?}, got {error:?}"
        );
    }
}

#[test]
fn text_clip_areas_are_skipped_in_lenient_mode() {
    // Lenient mode silently drops text clip areas rather than failing.
    let page = open_page(&path_with_clips(
        "<ofd:Clips><ofd:Clip><ofd:Area><ofd:Text/></ofd:Area></ofd:Clip></ofd:Clips>",
        "M 0 0",
    ))
    .unwrap();
    assert_eq!(path_object(&page).clips().len(), 0);
}

#[test]
fn text_clip_areas_are_rejected_in_strict_mode() {
    let error = open_page_strict(&path_with_clips(
        "<ofd:Clips><ofd:Clip><ofd:Area><ofd:Text/></ofd:Area></ofd:Clip></ofd:Clips>",
        "M 0 0",
    ))
    .unwrap_err();
    assert!(
        matches!(error, Error::UnsupportedFeature(ref message) if message.contains("text clip areas are not supported")),
        "{error:?}"
    );
}

#[test]
fn mixed_path_and_text_clip_areas_drop_only_text_in_lenient_mode() {
    // A Clip with two Areas—one Path, one Text—should keep the Path and drop
    // only the Text in lenient mode.
    let clips = r#"<ofd:Clips><ofd:Clip>
        <ofd:Area><ofd:Path Boundary="0 0 1 1" Fill="true" Stroke="false"><ofd:AbbreviatedData>M 0 0</ofd:AbbreviatedData></ofd:Path></ofd:Area>
        <ofd:Area><ofd:Text/></ofd:Area>
    </ofd:Clip></ofd:Clips>"#;
    let page = open_page(&path_with_clips(clips, "M 0 0")).unwrap();
    assert_eq!(path_object(&page).clips().len(), 1);
    assert_eq!(path_object(&page).clips()[0].paths().len(), 1);
}

#[test]
fn rejects_clip_forms_that_cannot_be_rendered_correctly() {
    // Non-fill-only clip paths are only rejected in strict mode; lenient
    // mode matches ofdrw and clips on the path geometry alone.
    let cases = [
        r#"<ofd:Clips><ofd:Clip><ofd:Area><ofd:Path Boundary="0 0 1 1"><ofd:AbbreviatedData>M 0 0</ofd:AbbreviatedData></ofd:Path></ofd:Area></ofd:Clip></ofd:Clips>"#,
        r#"<ofd:Clips><ofd:Clip><ofd:Area><ofd:Path Boundary="0 0 1 1" Fill="true" Stroke="true"><ofd:AbbreviatedData>M 0 0</ofd:AbbreviatedData></ofd:Path></ofd:Area></ofd:Clip></ofd:Clips>"#,
    ];

    for clips in cases {
        let error = open_page_strict(&path_with_clips(clips, "M 0 0")).unwrap_err();
        assert!(
            matches!(error, Error::UnsupportedFeature(ref message) if message.contains("clip paths must be fill-only")),
            "{error:?}"
        );
    }
}

#[test]
fn lenient_mode_clips_on_path_geometry_regardless_of_fill_and_stroke() {
    let cases = [
        r#"<ofd:Clips><ofd:Clip><ofd:Area><ofd:Path Boundary="0 0 1 1"><ofd:AbbreviatedData>M 0 0 L 1 0 C</ofd:AbbreviatedData></ofd:Path></ofd:Area></ofd:Clip></ofd:Clips>"#,
        r#"<ofd:Clips><ofd:Clip><ofd:Area><ofd:Path Boundary="0 0 1 1" Fill="true" Stroke="true"><ofd:AbbreviatedData>M 0 0 L 1 0 C</ofd:AbbreviatedData></ofd:Path></ofd:Area></ofd:Clip></ofd:Clips>"#,
    ];
    for clips in cases {
        let page = open_page(&path_with_clips(clips, "M 0 0")).unwrap();
        assert_eq!(path_object(&page).clips().len(), 1);
    }
}

#[test]
fn rejects_mixed_fill_rules_within_one_clip() {
    let area = |rule: &str| {
        format!(
            r#"<ofd:Area><ofd:Path Boundary="0 0 1 1" Fill="true" Stroke="false" Rule="{rule}"><ofd:AbbreviatedData>M 0 0</ofd:AbbreviatedData></ofd:Path></ofd:Area>"#
        )
    };
    let clips = format!(
        "<ofd:Clips><ofd:Clip>{}{}</ofd:Clip></ofd:Clips>",
        area("NonZero"),
        area("Even-Odd")
    );

    let error = open_page(&path_with_clips(&clips, "M 0 0")).unwrap_err();
    assert!(
        matches!(error, Error::UnsupportedFeature(message) if message.contains("mixed fill rules"))
    );
}

#[test]
fn rejects_invalid_clip_values() {
    let cases = [
        (
            r#"<ofd:Clips TransFlag="yes"><ofd:Clip><ofd:Area><ofd:Path Boundary="0 0 1 1" Fill="true" Stroke="false"><ofd:AbbreviatedData>M 0 0</ofd:AbbreviatedData></ofd:Path></ofd:Area></ofd:Clip></ofd:Clips>"#,
            "TransFlag",
        ),
        (
            r#"<ofd:Clips><ofd:Clip><ofd:Area CTM="bad"><ofd:Path Boundary="0 0 1 1" Fill="true" Stroke="false"><ofd:AbbreviatedData>M 0 0</ofd:AbbreviatedData></ofd:Path></ofd:Area></ofd:Clip></ofd:Clips>"#,
            "Area.CTM",
        ),
        (
            r#"<ofd:Clips><ofd:Clip><ofd:Area><ofd:Path Fill="true" Stroke="false"><ofd:AbbreviatedData>M 0 0</ofd:AbbreviatedData></ofd:Path></ofd:Area></ofd:Clip></ofd:Clips>"#,
            "Clip.Path.Boundary",
        ),
        (
            r#"<ofd:Clips><ofd:Clip><ofd:Area><ofd:Path Boundary="bad" Fill="true" Stroke="false"><ofd:AbbreviatedData>M 0 0</ofd:AbbreviatedData></ofd:Path></ofd:Area></ofd:Clip></ofd:Clips>"#,
            "Clip.Path.Boundary",
        ),
        (
            r#"<ofd:Clips><ofd:Clip><ofd:Area><ofd:Path Boundary="0 0 1 1" CTM="bad" Fill="true" Stroke="false"><ofd:AbbreviatedData>M 0 0</ofd:AbbreviatedData></ofd:Path></ofd:Area></ofd:Clip></ofd:Clips>"#,
            "Clip.Path.CTM",
        ),
        (
            r#"<ofd:Clips><ofd:Clip><ofd:Area><ofd:Path Boundary="0 0 1 1" Fill="true" Stroke="false" Rule="bad"><ofd:AbbreviatedData>M 0 0</ofd:AbbreviatedData></ofd:Path></ofd:Area></ofd:Clip></ofd:Clips>"#,
            "Clip.Path.Rule",
        ),
        (
            r#"<ofd:Clips><ofd:Clip><ofd:Area><ofd:Path Boundary="0 0 1 1" Fill="true" Stroke="false"/></ofd:Area></ofd:Clip></ofd:Clips>"#,
            "Clip.Path.AbbreviatedData",
        ),
        (
            r#"<ofd:Clips><ofd:Clip><ofd:Area><ofd:Path Boundary="0 0 1 1" Fill="true" Stroke="false"><ofd:AbbreviatedData>M nope</ofd:AbbreviatedData></ofd:Path></ofd:Area></ofd:Clip></ofd:Clips>"#,
            "Clip.Path.AbbreviatedData",
        ),
        (
            r#"<ofd:Clips><ofd:Clip><ofd:Area><ofd:Path Boundary="0 0 1 1" Fill="yes" Stroke="false"><ofd:AbbreviatedData>M 0 0</ofd:AbbreviatedData></ofd:Path></ofd:Area></ofd:Clip></ofd:Clips>"#,
            "Clip.Path.Fill",
        ),
        (
            r#"<ofd:Clips><ofd:Clip><ofd:Area><ofd:Path Boundary="0 0 1 1" Fill="true" Stroke="yes"><ofd:AbbreviatedData>M 0 0</ofd:AbbreviatedData></ofd:Path></ofd:Area></ofd:Clip></ofd:Clips>"#,
            "Clip.Path.Stroke",
        ),
    ];

    for (clips, expected_field) in cases {
        let error = open_page(&path_with_clips(clips, "M 0 0")).unwrap_err();
        assert!(
            matches!(error, Error::InvalidPageObject { object_id: 2, field, ref path, .. } if field == expected_field && path.ends_with("Content.xml")),
            "expected {expected_field} for {clips}, got {error:?}"
        );
    }
}

#[test]
fn clip_commands_share_the_cumulative_page_path_budget() {
    let content = path_with_clips(
        r#"<ofd:Clips><ofd:Clip><ofd:Area><ofd:Path Boundary="0 0 1 1" Fill="true" Stroke="false"><ofd:AbbreviatedData>M 0 0 L 1 1</ofd:AbbreviatedData></ofd:Path></ofd:Area></ofd:Clip></ofd:Clips>"#,
        "M 0 0 L 1 1",
    );
    let exact = ResourceLimits {
        max_path_commands: 4,
        ..ResourceLimits::default()
    };
    assert!(open_page_with_limits(&content, exact).is_ok());

    let exceed = ResourceLimits {
        max_path_commands: 3,
        ..ResourceLimits::default()
    };
    assert!(matches!(
        open_page_with_limits(&content, exceed),
        Err(Error::LimitExceeded(message)) if message.contains("path command count")
    ));
}

#[test]
fn clip_elements_count_toward_the_pre_serde_page_object_budget() {
    let content = path_with_clips(
        r#"<ofd:Clips><ofd:Clip><ofd:Area><ofd:Path Boundary="0 0 1 1" Fill="true" Stroke="false"><ofd:AbbreviatedData>M 0 0</ofd:AbbreviatedData></ofd:Path></ofd:Area></ofd:Clip></ofd:Clips>"#,
        "M 0 0",
    );
    let exact = ResourceLimits {
        max_page_objects: 5,
        ..ResourceLimits::default()
    };
    assert!(open_page_with_limits(&content, exact).is_ok());

    let exceed = ResourceLimits {
        max_page_objects: 4,
        ..ResourceLimits::default()
    };
    assert!(matches!(
        open_page_with_limits(&content, exceed),
        Err(Error::LimitExceeded(message)) if message.contains("page object count 5 exceeds limit 4")
    ));
}

#[test]
fn clip_text_children_count_toward_the_pre_serde_page_object_budget() {
    let content = path_with_clips(
        "<ofd:Clips><ofd:Clip><ofd:Area><ofd:Text/><ofd:Text/></ofd:Area></ofd:Clip></ofd:Clips>",
        "M 0 0",
    );
    let exact = ResourceLimits {
        max_page_objects: 6,
        ..ResourceLimits::default()
    };
    let error = open_page_with_limits(&content, exact).unwrap_err();
    assert!(
        matches!(error, Error::InvalidStructure { ref message, .. } if message.contains("Area must contain exactly one Path or Text")),
        "the exact budget must reach structural validation, got {error:?}"
    );

    let one_over = ResourceLimits {
        max_page_objects: 5,
        ..ResourceLimits::default()
    };
    let error = open_page_with_limits(&content, one_over).unwrap_err();
    assert!(
        matches!(error, Error::LimitExceeded(ref message) if message.contains("page object count 6 exceeds limit 5")),
        "the preflight limit must win, got {error:?}"
    );
}
