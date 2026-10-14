#[path = "../../rofd-core/tests/support/mod.rs"]
mod support;

use rofd_ffi::*;
use std::ffi::{CStr, CString};
use std::mem::{size_of, MaybeUninit};
use std::ptr;
use std::sync::atomic::{AtomicUsize, Ordering};

fn dangling_mut<T>() -> *mut T {
    std::ptr::NonNull::dangling().as_ptr()
}

fn page_with_layer(layer: &str) -> *mut rofd_page_t {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let xml = format!(
        r#"<ofd:Page xmlns:ofd="http://www.ofdspec.org/2016"><ofd:Content><ofd:Layer ID="1">{layer}</ofd:Layer></ofd:Content></ofd:Page>"#
    );
    let document = r#"<ofd:Document xmlns:ofd="http://www.ofdspec.org/2016"><ofd:CommonData><ofd:PageArea><ofd:PhysicalBox>0 0 210 297</ofd:PhysicalBox></ofd:PageArea><ofd:PublicRes>Res.xml</ofd:PublicRes></ofd:CommonData><ofd:Pages><ofd:Page ID="900" BaseLoc="Pages/Page_0/Content.xml"/></ofd:Pages></ofd:Document>"#;
    let font = br#"<ofd:Res xmlns:ofd="http://www.ofdspec.org/2016"><ofd:Fonts><ofd:Font ID="10" FontName="Fixture"/></ofd:Fonts></ofd:Res>"#;
    let bytes =
        support::ofd_with_document_page_and_entries(document, &xml, &[("Doc_0/Res.xml", font)]);
    let path = std::env::temp_dir().join(format!(
        "rofd-ffi-semantic-{}-{}.ofd",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::write(&path, bytes).unwrap();
    let cpath = CString::new(path.to_str().unwrap()).unwrap();
    // SAFETY: All input and output storage is valid and distinct; document is freed once.
    unsafe {
        let mut document = ptr::null_mut();
        let mut page = ptr::null_mut();
        assert_eq!(
            rofd_document_open(cpath.as_ptr(), ptr::null(), &mut document, ptr::null_mut()),
            ROFD_STATUS_OK
        );
        std::fs::remove_file(path).unwrap();
        assert_eq!(
            rofd_document_get_page(document, 0, &mut page, ptr::null_mut()),
            ROFD_STATUS_OK
        );
        rofd_document_free(document);
        page
    }
}

/// A page whose text object references an embedded subset font and declares no
/// `DeltaX`, so its geometry can only come from the font's advances.
fn page_with_embedded_font() -> *mut rofd_page_t {
    const LATIN_FONT: &[u8] =
        include_bytes!("../../rofd-render/tests/fixtures/fonts/phase3-latin-subset.ttf");
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    // Most Latin scalars in this subset are full-width, so A/B/Z are the ones that
    // tell a font advance apart from the font-size footprint.
    let xml = r#"<ofd:Page xmlns:ofd="http://www.ofdspec.org/2016"><ofd:Content><ofd:Layer ID="1"><ofd:TextObject ID="2" Boundary="0 0 30 10" Font="10" Size="4"><ofd:TextCode X="1" Y="4">ABZ</ofd:TextCode></ofd:TextObject></ofd:Layer></ofd:Content></ofd:Page>"#;
    let document = r#"<ofd:Document xmlns:ofd="http://www.ofdspec.org/2016"><ofd:CommonData><ofd:PageArea><ofd:PhysicalBox>0 0 210 297</ofd:PhysicalBox></ofd:PageArea><ofd:DocumentRes>Res.xml</ofd:DocumentRes></ofd:CommonData><ofd:Pages><ofd:Page ID="900" BaseLoc="Pages/Page_0/Content.xml"/></ofd:Pages></ofd:Document>"#;
    let font = br#"<ofd:Res xmlns:ofd="http://www.ofdspec.org/2016"><ofd:Fonts><ofd:Font ID="10" FontName="Fixture"><ofd:FontFile>Fonts/latin.ttf</ofd:FontFile></ofd:Font></ofd:Fonts></ofd:Res>"#;
    let bytes = support::ofd_with_document_page_and_entries(
        document,
        xml,
        &[
            ("Doc_0/Res.xml", font),
            ("Doc_0/Fonts/latin.ttf", LATIN_FONT),
        ],
    );
    let path = std::env::temp_dir().join(format!(
        "rofd-ffi-semantic-font-{}-{}.ofd",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::write(&path, bytes).unwrap();
    let cpath = CString::new(path.to_str().unwrap()).unwrap();
    // SAFETY: All input and output storage is valid and distinct; document is freed once.
    unsafe {
        let mut document = ptr::null_mut();
        let mut page = ptr::null_mut();
        assert_eq!(
            rofd_document_open(cpath.as_ptr(), ptr::null(), &mut document, ptr::null_mut()),
            ROFD_STATUS_OK
        );
        std::fs::remove_file(path).unwrap();
        assert_eq!(
            rofd_document_get_page(document, 0, &mut page, ptr::null_mut()),
            ROFD_STATUS_OK
        );
        rofd_document_free(document);
        page
    }
}

fn page(two_objects: bool) -> *mut rofd_page_t {
    let tail = if two_objects {
        r#"<ofd:TextObject ID="3" Boundary="10 35 30 8" Font="10" Size="4"><ofd:TextCode X="1" Y="5">尾</ofd:TextCode></ofd:TextObject>"#
    } else {
        ""
    };
    page_with_layer(&format!(
        r#"<ofd:TextObject ID="2" Boundary="10 20 30 8" Font="10" Size="4"><ofd:TextCode X="1" Y="5" DeltaX="3 4">A中B</ofd:TextCode></ofd:TextObject>{tail}"#
    ))
}

fn page_with_text(text: &str) -> *mut rofd_page_t {
    page_with_layer(&format!(
        r#"<ofd:TextObject ID="2" Boundary="10 20 100 8" Font="10" Size="4"><ofd:TextCode X="1" Y="5">{text}</ofd:TextCode></ofd:TextObject>"#
    ))
}

unsafe fn character(layout: *const rofd_text_layout_t, index: usize) -> rofd_text_char_t {
    let mut value = MaybeUninit::<rofd_text_char_t>::zeroed();
    // SAFETY: A complete initialized output is live and distinct from the borrowed layout.
    unsafe {
        (*value.as_mut_ptr()).struct_size = size_of::<rofd_text_char_t>() as u32;
        assert_eq!(
            rofd_text_layout_get_char(layout, index, value.as_mut_ptr(), ptr::null_mut()),
            ROFD_STATUS_OK
        );
        value.assume_init()
    }
}

unsafe fn bytes(string: *const rofd_string_t) -> Vec<u8> {
    // SAFETY: The returned data is borrowed only while its owning string is live.
    unsafe {
        CStr::from_ptr(rofd_string_get_data(string))
            .to_bytes()
            .to_vec()
    }
}

unsafe fn search_count(search: *const rofd_text_search_t) -> usize {
    let mut count = usize::MAX;
    // SAFETY: The output is valid and distinct from the live immutable search handle.
    unsafe {
        assert_eq!(
            rofd_text_search_get_count(search, &mut count, ptr::null_mut()),
            ROFD_STATUS_OK
        );
    }
    count
}

unsafe fn text_match(search: *const rofd_text_search_t, index: usize) -> rofd_text_match_t {
    let mut value = MaybeUninit::<rofd_text_match_t>::zeroed();
    // SAFETY: A complete initialized output is live and distinct from the search handle.
    unsafe {
        (*value.as_mut_ptr()).struct_size = size_of::<rofd_text_match_t>() as u32;
        assert_eq!(
            rofd_text_search_get_match(search, index, value.as_mut_ptr(), ptr::null_mut()),
            ROFD_STATUS_OK
        );
        value.assume_init()
    }
}

unsafe fn selection_bytes(selection: *const rofd_text_selection_t) -> Vec<u8> {
    // SAFETY: The returned data is borrowed only while its owning selection is live.
    unsafe {
        CStr::from_ptr(rofd_text_selection_get_text(selection))
            .to_bytes()
            .to_vec()
    }
}

unsafe fn selection_region(selection: *const rofd_text_selection_t, index: usize) -> rofd_rect_t {
    let mut region = rofd_rect_t {
        x_mm: f64::NAN,
        y_mm: f64::NAN,
        width_mm: f64::NAN,
        height_mm: f64::NAN,
    };
    // SAFETY: The output is valid and distinct from the live immutable selection handle.
    unsafe {
        assert_eq!(
            rofd_text_selection_get_region(selection, index, &mut region, ptr::null_mut()),
            ROFD_STATUS_OK
        );
    }
    region
}

#[test]
fn owned_text_and_layout_keep_exact_utf8_after_page_and_document_free() {
    // SAFETY: Each handle is live for its accesses and freed once after all borrows.
    unsafe {
        let page = page(false);
        let mut text = ptr::null_mut();
        let mut layout = ptr::null_mut();
        assert_eq!(
            rofd_page_get_text(page, &mut text, ptr::null_mut()),
            ROFD_STATUS_OK
        );
        assert_eq!(
            rofd_page_get_text_layout(page, &mut layout, ptr::null_mut()),
            ROFD_STATUS_OK
        );
        rofd_page_free(page);
        assert_eq!(bytes(text), "A中B".as_bytes());
        assert_eq!(rofd_string_get_length(text), 5);
        let mut count = 0;
        assert_eq!(
            rofd_text_layout_get_count(layout, &mut count, ptr::null_mut()),
            ROFD_STATUS_OK
        );
        assert_eq!(count, 3);
        let c = character(layout, 1);
        assert_eq!((c.utf8_offset, c.utf8_length, c.object_id), (1, 3, 2));
        assert_eq!(c.flags, ROFD_TEXT_CHAR_CONSERVATIVE_GEOMETRY);
        assert!(c.rect_mm.x_mm.is_finite() && c.rect_mm.y_mm.is_finite());
        assert!(c.rect_mm.width_mm > 0.0 && c.rect_mm.width_mm.is_finite());
        assert!(c.rect_mm.height_mm > 0.0 && c.rect_mm.height_mm.is_finite());
        rofd_string_free(text);
        rofd_text_layout_free(layout);
    }
}

#[test]
fn area_uses_glyph_intersections_and_separator_has_no_geometry() {
    // SAFETY: All live handles and records are disjoint and freed exactly once.
    unsafe {
        let page = page(true);
        let mut layout = ptr::null_mut();
        let mut text = ptr::null_mut();
        assert_eq!(
            rofd_page_get_text_layout(page, &mut layout, ptr::null_mut()),
            ROFD_STATUS_OK
        );
        let separator = character(layout, 3);
        assert_eq!(
            (
                separator.utf8_offset,
                separator.utf8_length,
                separator.object_id
            ),
            (5, 1, 0)
        );
        assert_eq!(
            separator.flags,
            ROFD_TEXT_CHAR_SYNTHESIZED_SEPARATOR | ROFD_TEXT_CHAR_CONSERVATIVE_GEOMETRY
        );
        assert_eq!(
            (
                separator.rect_mm.x_mm,
                separator.rect_mm.y_mm,
                separator.rect_mm.width_mm,
                separator.rect_mm.height_mm
            ),
            (0.0, 0.0, 0.0, 0.0)
        );
        let c = character(layout, 1);
        let area = rofd_rect_t {
            x_mm: c.rect_mm.x_mm + c.rect_mm.width_mm - 0.02,
            y_mm: c.rect_mm.y_mm + 0.01,
            width_mm: 0.01,
            height_mm: 0.01,
        };
        assert_eq!(
            rofd_page_get_text_for_area(page, &area, &mut text, ptr::null_mut()),
            ROFD_STATUS_OK
        );
        assert_eq!(bytes(text), "中".as_bytes());
        rofd_string_free(text);
        assert_eq!(
            rofd_page_get_text(page, &mut text, ptr::null_mut()),
            ROFD_STATUS_OK
        );
        assert_eq!(bytes(text), "A中B\n尾".as_bytes());
        rofd_string_free(text);
        rofd_text_layout_free(layout);
        rofd_page_free(page);
    }
}

#[test]
fn null_and_invalid_arguments_publish_transactional_defaults() {
    // SAFETY: Invalid handles are null; sentinel outputs are never dereferenced.
    unsafe {
        assert!(rofd_string_get_data(ptr::null()).is_null());
        assert_eq!(rofd_string_get_length(ptr::null()), 0);
        rofd_string_free(ptr::null_mut());
        rofd_text_layout_free(ptr::null_mut());
        let mut text = dangling_mut();
        let mut layout = dangling_mut();
        let mut error = ptr::null_mut();
        assert_eq!(
            rofd_page_get_text(ptr::null(), &mut text, &mut error),
            ROFD_STATUS_INVALID_ARGUMENT
        );
        assert!(text.is_null());
        assert_eq!(rofd_error_get_status(error), ROFD_STATUS_INVALID_ARGUMENT);
        rofd_error_free(error);
        assert_eq!(
            rofd_page_get_text_layout(ptr::null(), &mut layout, ptr::null_mut()),
            ROFD_STATUS_INVALID_ARGUMENT
        );
        assert!(layout.is_null());
        let mut count = 99;
        assert_eq!(
            rofd_text_layout_get_count(ptr::null(), &mut count, ptr::null_mut()),
            ROFD_STATUS_INVALID_ARGUMENT
        );
        assert_eq!(count, 0);
        let page = page(false);
        assert_eq!(
            rofd_page_get_text(page, ptr::null_mut(), ptr::null_mut()),
            ROFD_STATUS_INVALID_ARGUMENT
        );
        assert_eq!(
            rofd_page_get_text_layout(page, ptr::null_mut(), ptr::null_mut()),
            ROFD_STATUS_INVALID_ARGUMENT
        );
        assert_eq!(
            rofd_page_get_text_for_area(page, ptr::null(), &mut text, ptr::null_mut()),
            ROFD_STATUS_INVALID_ARGUMENT
        );
        assert!(text.is_null());
        for area in [
            rofd_rect_t {
                x_mm: f64::NAN,
                y_mm: 0.0,
                width_mm: 1.0,
                height_mm: 1.0,
            },
            rofd_rect_t {
                x_mm: 0.0,
                y_mm: 0.0,
                width_mm: -1.0,
                height_mm: 1.0,
            },
        ] {
            assert_eq!(
                rofd_page_get_text_for_area(page, &area, &mut text, ptr::null_mut()),
                ROFD_STATUS_INVALID_ARGUMENT
            );
            assert!(text.is_null());
        }
        assert_eq!(
            rofd_page_get_text_layout(page, &mut layout, ptr::null_mut()),
            ROFD_STATUS_OK
        );
        assert_eq!(
            rofd_text_layout_get_count(layout, ptr::null_mut(), ptr::null_mut()),
            ROFD_STATUS_INVALID_ARGUMENT
        );
        assert_eq!(
            rofd_text_layout_get_char(layout, 0, ptr::null_mut(), ptr::null_mut()),
            ROFD_STATUS_INVALID_ARGUMENT
        );
        let mut c = character(layout, 1);
        assert_eq!(
            rofd_text_layout_get_char(layout, 99, &mut c, ptr::null_mut()),
            ROFD_STATUS_PAGE_OUT_OF_RANGE
        );
        assert_eq!(
            (c.utf8_offset, c.utf8_length, c.object_id, c.flags),
            (0, 0, 0, 0)
        );
        assert_eq!(c.struct_size as usize, size_of::<rofd_text_char_t>());
        rofd_text_layout_free(layout);
        rofd_page_free(page);
    }
}

#[test]
fn record_versioning_preserves_small_record_and_unknown_tail() {
    #[repr(C)]
    struct Extended {
        record: rofd_text_char_t,
        tail: [u8; 16],
    }
    // SAFETY: Byte-initialized records are readable and writable at their declared boundary.
    unsafe {
        let page = page(false);
        let mut layout = ptr::null_mut();
        assert_eq!(
            rofd_page_get_text_layout(page, &mut layout, ptr::null_mut()),
            ROFD_STATUS_OK
        );
        let mut extended = MaybeUninit::<Extended>::uninit();
        ptr::write_bytes(
            extended.as_mut_ptr().cast::<u8>(),
            0xa5,
            size_of::<Extended>(),
        );
        (*extended.as_mut_ptr()).record.struct_size = size_of::<Extended>() as u32;
        let mut extended = extended.assume_init();
        assert_eq!(
            rofd_text_layout_get_char(layout, 1, &mut extended.record, ptr::null_mut()),
            ROFD_STATUS_OK
        );
        assert_eq!(extended.tail, [0xa5; 16]);
        assert_eq!(extended.record.struct_size as usize, size_of::<Extended>());
        let mut small = 4_u32;
        let mut error = ptr::null_mut();
        assert_eq!(
            rofd_text_layout_get_char(layout, 0, (&mut small as *mut u32).cast(), &mut error),
            ROFD_STATUS_INVALID_ARGUMENT
        );
        assert_eq!(small, 4);
        assert_eq!(rofd_error_get_status(error), ROFD_STATUS_INVALID_ARGUMENT);
        rofd_error_free(error);
        rofd_text_layout_free(layout);
        rofd_page_free(page);
    }
}

#[test]
fn preflight_rejects_output_aliases_and_input_overlap_without_corruption() {
    // SAFETY: Aliased slots are valid allocations, deliberately rejected before any conflicting write.
    unsafe {
        let page = page(false);
        let mut layout = ptr::null_mut();
        assert_eq!(
            rofd_page_get_text_layout(page, &mut layout, ptr::null_mut()),
            ROFD_STATUS_OK
        );
        let mut slot: *mut rofd_string_t = dangling_mut();
        let sentinel = slot;
        let raw = &mut slot as *mut *mut rofd_string_t;
        assert_eq!(
            rofd_page_get_text(page, raw, raw.cast()),
            ROFD_STATUS_INVALID_ARGUMENT
        );
        assert_eq!(slot, sentinel);
        let mut area = rofd_rect_t {
            x_mm: 1.0,
            y_mm: 2.0,
            width_mm: 3.0,
            height_mm: 4.0,
        };
        let area_ptr = &mut area as *mut rofd_rect_t;
        let mut error = ptr::null_mut();
        assert_eq!(
            rofd_page_get_text_for_area(page, area_ptr, area_ptr.cast(), &mut error),
            ROFD_STATUS_INVALID_ARGUMENT
        );
        assert_eq!(area.x_mm, 1.0);
        rofd_error_free(error);
        assert_eq!(
            rofd_page_get_text(page, page.cast(), &mut error),
            ROFD_STATUS_INVALID_ARGUMENT
        );
        rofd_error_free(error);
        assert_eq!(
            rofd_page_get_text_layout(page, page.cast(), &mut error),
            ROFD_STATUS_INVALID_ARGUMENT
        );
        rofd_error_free(error);
        assert_eq!(
            rofd_text_layout_get_count(layout, layout.cast(), &mut error),
            ROFD_STATUS_INVALID_ARGUMENT
        );
        rofd_error_free(error);
        let mut c = character(layout, 1);
        let cptr = &mut c as *mut rofd_text_char_t;
        assert_eq!(
            rofd_text_layout_get_char(layout, 1, cptr, cptr.cast()),
            ROFD_STATUS_INVALID_ARGUMENT
        );
        assert_eq!(c.utf8_length, 3);
        assert_eq!(character(layout, 1).utf8_length, 3);
        rofd_text_layout_free(layout);
        rofd_page_free(page);
    }
}

#[test]
fn concurrent_readers_share_live_page_and_snapshot() {
    // SAFETY: Read-only calls share live handles, each uses its own outputs, and joins precede free.
    unsafe {
        let page = page(false);
        let mut layout = ptr::null_mut();
        assert_eq!(
            rofd_page_get_text_layout(page, &mut layout, ptr::null_mut()),
            ROFD_STATUS_OK
        );
        let page_address = page as usize;
        let layout_address = layout as usize;
        std::thread::scope(|scope| {
            for _ in 0..8 {
                scope.spawn(move || {
                    let mut text = ptr::null_mut();
                    assert_eq!(
                        rofd_page_get_text(
                            page_address as *const rofd_page_t,
                            &mut text,
                            ptr::null_mut()
                        ),
                        ROFD_STATUS_OK
                    );
                    assert_eq!(bytes(text), "A中B".as_bytes());
                    assert_eq!(
                        character(layout_address as *const rofd_text_layout_t, 1).utf8_length,
                        3
                    );
                    rofd_string_free(text);
                });
            }
        });
        rofd_text_layout_free(layout);
        rofd_page_free(page);
    }
}

#[test]
fn search_supports_defaults_flags_limits_and_independent_results() {
    // SAFETY: Inputs remain readable for each call; every result is owned and freed once.
    unsafe {
        let page = page(false);
        let query = CString::new("a中").unwrap();
        let mut search = ptr::null_mut();
        assert_eq!(
            rofd_page_find_text(page, query.as_ptr(), &mut search, ptr::null_mut()),
            ROFD_STATUS_OK
        );
        rofd_page_free(page);
        assert_eq!(search_count(search), 1);
        let found = text_match(search, 0);
        assert_eq!((found.utf8_offset, found.utf8_length), (0, 4));
        assert!(found.rect_mm.x_mm.is_finite() && found.rect_mm.y_mm.is_finite());
        assert!(found.rect_mm.width_mm > 0.0 && found.rect_mm.height_mm > 0.0);
        rofd_text_search_free(search);

        let page = page_with_text("A a A");
        let query = CString::new("a").unwrap();
        let mut options = MaybeUninit::<rofd_find_options_t>::zeroed();
        rofd_find_options_init(options.as_mut_ptr(), size_of::<rofd_find_options_t>());
        let mut options = options.assume_init();
        options.flags = ROFD_FIND_CASE_SENSITIVE;
        options.max_results = 10;
        search = ptr::null_mut();
        assert_eq!(
            rofd_page_find_text_with_options(
                page,
                query.as_ptr(),
                &options,
                &mut search,
                ptr::null_mut()
            ),
            ROFD_STATUS_OK
        );
        assert_eq!(search_count(search), 1);
        assert_eq!(text_match(search, 0).utf8_offset, 2);
        rofd_text_search_free(search);

        options.flags = 0;
        options.max_results = 2;
        assert_eq!(
            rofd_page_find_text_with_options(
                page,
                query.as_ptr(),
                &options,
                &mut search,
                ptr::null_mut()
            ),
            ROFD_STATUS_OK
        );
        assert_eq!(search_count(search), 2);
        rofd_text_search_free(search);
        rofd_page_free(page);

        let page = page_with_text("cat scatter cat");
        let query = CString::new("cat").unwrap();
        options.flags = 0;
        options.max_results = 10;
        assert_eq!(
            rofd_page_find_text_with_options(
                page,
                query.as_ptr(),
                &options,
                &mut search,
                ptr::null_mut()
            ),
            ROFD_STATUS_OK
        );
        assert_eq!(search_count(search), 3);
        rofd_text_search_free(search);
        options.flags = ROFD_FIND_WHOLE_WORDS;
        assert_eq!(
            rofd_page_find_text_with_options(
                page,
                query.as_ptr(),
                &options,
                &mut search,
                ptr::null_mut()
            ),
            ROFD_STATUS_OK
        );
        assert_eq!(search_count(search), 2);
        rofd_text_search_free(search);
        rofd_page_free(page);
    }
}

#[test]
fn styled_selection_owns_text_and_regions_after_page_free() {
    // SAFETY: All handles and outputs are valid, disjoint, and freed exactly once.
    unsafe {
        let page = page(true);
        let mut layout = ptr::null_mut();
        assert_eq!(
            rofd_page_get_text_layout(page, &mut layout, ptr::null_mut()),
            ROFD_STATUS_OK
        );
        let middle = character(layout, 1).rect_mm;
        rofd_text_layout_free(layout);
        let area = rofd_rect_t {
            x_mm: middle.x_mm + middle.width_mm - 0.02,
            y_mm: middle.y_mm + 0.01,
            width_mm: 0.01,
            height_mm: 0.01,
        };
        let mut glyph = ptr::null_mut();
        let mut word = ptr::null_mut();
        let mut line = ptr::null_mut();
        assert_eq!(
            rofd_page_get_selected_text(
                page,
                ROFD_SELECTION_GLYPH,
                &area,
                &mut glyph,
                ptr::null_mut()
            ),
            ROFD_STATUS_OK
        );
        assert_eq!(
            rofd_page_get_selected_text(
                page,
                ROFD_SELECTION_WORD,
                &area,
                &mut word,
                ptr::null_mut()
            ),
            ROFD_STATUS_OK
        );
        assert_eq!(
            rofd_page_get_selected_text(
                page,
                ROFD_SELECTION_LINE,
                &area,
                &mut line,
                ptr::null_mut()
            ),
            ROFD_STATUS_OK
        );
        rofd_page_free(page);

        assert_eq!(selection_bytes(glyph), "中".as_bytes());
        assert_eq!(selection_bytes(word), "A中B".as_bytes());
        assert_eq!(selection_bytes(line), "A中B".as_bytes());
        assert_eq!(rofd_text_selection_get_text_length(glyph), 3);
        let mut count = usize::MAX;
        assert_eq!(
            rofd_text_selection_get_region_count(glyph, &mut count, ptr::null_mut()),
            ROFD_STATUS_OK
        );
        assert_eq!(count, 1);
        let region = selection_region(glyph, 0);
        assert!(region.x_mm.is_finite() && region.y_mm.is_finite());
        assert!(region.width_mm > 0.0 && region.height_mm > 0.0);

        rofd_text_selection_free(glyph);
        rofd_text_selection_free(word);
        rofd_text_selection_free(line);
    }
}

#[test]
fn search_rejects_invalid_inputs_and_preserves_overlap_storage() {
    #[repr(C)]
    struct ExtendedOptions {
        options: rofd_find_options_t,
        tail: [u8; 16],
    }

    // SAFETY: Invalid inputs satisfy the documented readable-prefix requirements; every
    // successful result and error handle is freed exactly once.
    unsafe {
        rofd_text_search_free(ptr::null_mut());
        let page = page(false);
        let query = CString::new("A").unwrap();
        let empty = CString::new("").unwrap();
        let invalid_utf8 = [0xff_u8, 0];
        for query in [ptr::null(), empty.as_ptr(), invalid_utf8.as_ptr().cast()] {
            let mut search = dangling_mut();
            assert_eq!(
                rofd_page_find_text(page, query, &mut search, ptr::null_mut()),
                ROFD_STATUS_INVALID_ARGUMENT
            );
            assert!(search.is_null());
        }
        assert_eq!(
            rofd_page_find_text(page, query.as_ptr(), ptr::null_mut(), ptr::null_mut()),
            ROFD_STATUS_INVALID_ARGUMENT
        );

        let mut options = MaybeUninit::<rofd_find_options_t>::zeroed();
        rofd_find_options_init(options.as_mut_ptr(), size_of::<rofd_find_options_t>());
        let mut options = options.assume_init();
        for (flags, max_results) in [(1_u32 << 31, 1), (0, 0)] {
            options.flags = flags;
            options.max_results = max_results;
            let mut search = dangling_mut();
            assert_eq!(
                rofd_page_find_text_with_options(
                    page,
                    query.as_ptr(),
                    &options,
                    &mut search,
                    ptr::null_mut()
                ),
                ROFD_STATUS_INVALID_ARGUMENT
            );
            assert!(search.is_null());
        }

        options.struct_size = 4;
        let mut search = dangling_mut();
        assert_eq!(
            rofd_page_find_text_with_options(
                page,
                query.as_ptr(),
                &options,
                &mut search,
                ptr::null_mut()
            ),
            ROFD_STATUS_INVALID_ARGUMENT
        );
        assert!(search.is_null());

        let mut extended = MaybeUninit::<ExtendedOptions>::zeroed();
        rofd_find_options_init(extended.as_mut_ptr().cast(), size_of::<ExtendedOptions>());
        (*extended.as_mut_ptr()).options.struct_size = size_of::<ExtendedOptions>() as u32;
        (*extended.as_mut_ptr()).tail = [0xa5; 16];
        let extended = extended.assume_init();
        assert_eq!(
            rofd_page_find_text_with_options(
                page,
                query.as_ptr(),
                &extended.options,
                &mut search,
                ptr::null_mut()
            ),
            ROFD_STATUS_OK
        );
        assert_eq!(extended.tail, [0xa5; 16]);
        rofd_text_search_free(search);

        let mut query_storage = [0_usize; 2];
        query_storage[0] = usize::from(b'A');
        let query_snapshot = query_storage;
        assert_eq!(
            rofd_page_find_text(
                page,
                query_storage.as_ptr().cast(),
                query_storage.as_mut_ptr().cast(),
                ptr::null_mut()
            ),
            ROFD_STATUS_INVALID_ARGUMENT
        );
        assert_eq!(query_storage, query_snapshot);

        rofd_find_options_init(&mut options, size_of::<rofd_find_options_t>());
        let options_ptr = &mut options as *mut rofd_find_options_t;
        assert_eq!(
            rofd_page_find_text_with_options(
                page,
                query.as_ptr(),
                options_ptr,
                options_ptr.cast(),
                ptr::null_mut()
            ),
            ROFD_STATUS_INVALID_ARGUMENT
        );
        assert_eq!(
            options.struct_size as usize,
            size_of::<rofd_find_options_t>()
        );

        let mut alias: *mut rofd_text_search_t = dangling_mut();
        let sentinel = alias;
        let alias_ptr = &mut alias as *mut *mut rofd_text_search_t;
        assert_eq!(
            rofd_page_find_text(page, query.as_ptr(), alias_ptr, alias_ptr.cast()),
            ROFD_STATUS_INVALID_ARGUMENT
        );
        assert_eq!(alias, sentinel);
        rofd_page_free(page);
    }
}

#[test]
fn search_and_selection_accessors_are_transactional() {
    #[repr(C)]
    struct ExtendedMatch {
        found: rofd_text_match_t,
        tail: [u8; 16],
    }

    // SAFETY: All sentinel outputs are valid allocations and every owned handle is freed once.
    unsafe {
        let mut count = 99;
        assert_eq!(
            rofd_text_search_get_count(ptr::null(), &mut count, ptr::null_mut()),
            ROFD_STATUS_INVALID_ARGUMENT
        );
        assert_eq!(count, 0);
        let page = page(false);
        let query = CString::new("A").unwrap();
        let mut search = ptr::null_mut();
        assert_eq!(
            rofd_page_find_text(page, query.as_ptr(), &mut search, ptr::null_mut()),
            ROFD_STATUS_OK
        );
        let mut extended = MaybeUninit::<ExtendedMatch>::uninit();
        ptr::write_bytes(
            extended.as_mut_ptr().cast::<u8>(),
            0xa5,
            size_of::<ExtendedMatch>(),
        );
        (*extended.as_mut_ptr()).found.struct_size = size_of::<ExtendedMatch>() as u32;
        let mut extended = extended.assume_init();
        assert_eq!(
            rofd_text_search_get_match(search, 0, &mut extended.found, ptr::null_mut()),
            ROFD_STATUS_OK
        );
        assert_eq!(extended.tail, [0xa5; 16]);
        assert_eq!(
            extended.found.struct_size as usize,
            size_of::<ExtendedMatch>()
        );
        let mut found = text_match(search, 0);
        assert_eq!(
            rofd_text_search_get_match(search, 99, &mut found, ptr::null_mut()),
            ROFD_STATUS_PAGE_OUT_OF_RANGE
        );
        assert_eq!((found.utf8_offset, found.utf8_length), (0, 0));
        assert_eq!(found.struct_size as usize, size_of::<rofd_text_match_t>());
        let mut small = 4_u32;
        assert_eq!(
            rofd_text_search_get_match(search, 0, (&mut small as *mut u32).cast(), ptr::null_mut()),
            ROFD_STATUS_INVALID_ARGUMENT
        );
        assert_eq!(small, 4);
        rofd_text_search_free(search);

        assert!(rofd_text_selection_get_text(ptr::null()).is_null());
        assert_eq!(rofd_text_selection_get_text_length(ptr::null()), 0);
        rofd_text_selection_free(ptr::null_mut());
        let area = rofd_rect_t {
            x_mm: 0.0,
            y_mm: 0.0,
            width_mm: 100.0,
            height_mm: 100.0,
        };
        let mut selection = dangling_mut();
        assert_eq!(
            rofd_page_get_selected_text(
                page,
                ROFD_SELECTION_GLYPH,
                ptr::null(),
                &mut selection,
                ptr::null_mut()
            ),
            ROFD_STATUS_INVALID_ARGUMENT
        );
        assert!(selection.is_null());
        assert_eq!(
            rofd_page_get_selected_text(page, 99, &area, &mut selection, ptr::null_mut()),
            ROFD_STATUS_INVALID_ARGUMENT
        );
        assert!(selection.is_null());
        for area in [
            rofd_rect_t {
                x_mm: f64::INFINITY,
                ..area
            },
            rofd_rect_t {
                width_mm: -1.0,
                ..area
            },
        ] {
            assert_eq!(
                rofd_page_get_selected_text(
                    page,
                    ROFD_SELECTION_GLYPH,
                    &area,
                    &mut selection,
                    ptr::null_mut()
                ),
                ROFD_STATUS_INVALID_ARGUMENT
            );
            assert!(selection.is_null());
        }
        assert_eq!(
            rofd_page_get_selected_text(
                page,
                ROFD_SELECTION_GLYPH,
                &area,
                ptr::null_mut(),
                ptr::null_mut()
            ),
            ROFD_STATUS_INVALID_ARGUMENT
        );
        assert_eq!(
            rofd_page_get_selected_text(
                page,
                ROFD_SELECTION_GLYPH,
                &area,
                &mut selection,
                ptr::null_mut()
            ),
            ROFD_STATUS_OK
        );
        count = 99;
        assert_eq!(
            rofd_text_selection_get_region_count(ptr::null(), &mut count, ptr::null_mut()),
            ROFD_STATUS_INVALID_ARGUMENT
        );
        assert_eq!(count, 0);
        let mut region = rofd_rect_t {
            x_mm: 1.0,
            y_mm: 2.0,
            width_mm: 3.0,
            height_mm: 4.0,
        };
        assert_eq!(
            rofd_text_selection_get_region(selection, 99, &mut region, ptr::null_mut()),
            ROFD_STATUS_PAGE_OUT_OF_RANGE
        );
        assert_eq!(
            (region.x_mm, region.y_mm, region.width_mm, region.height_mm),
            (0.0, 0.0, 0.0, 0.0)
        );

        let mut overlap_area = area;
        let overlap_ptr = &mut overlap_area as *mut rofd_rect_t;
        assert_eq!(
            rofd_page_get_selected_text(
                page,
                ROFD_SELECTION_GLYPH,
                overlap_ptr,
                overlap_ptr.cast(),
                ptr::null_mut()
            ),
            ROFD_STATUS_INVALID_ARGUMENT
        );
        assert_eq!(overlap_area.width_mm, area.width_mm);
        let mut alias: *mut rofd_text_selection_t = dangling_mut();
        let sentinel = alias;
        let alias_ptr = &mut alias as *mut *mut rofd_text_selection_t;
        assert_eq!(
            rofd_page_get_selected_text(
                page,
                ROFD_SELECTION_GLYPH,
                &area,
                alias_ptr,
                alias_ptr.cast()
            ),
            ROFD_STATUS_INVALID_ARGUMENT
        );
        assert_eq!(alias, sentinel);

        rofd_text_selection_free(selection);
        rofd_page_free(page);
    }
}

/// Page geometry and search rectangles resolve the advances of the font a renderer
/// would use, instead of the font-size footprint, so they land on the drawn glyphs.
#[test]
fn text_geometry_and_search_rectangles_use_font_advances() {
    // SAFETY: Every handle is live for its accesses and freed once after all borrows.
    unsafe {
        let page = page_with_embedded_font();
        let mut layout = ptr::null_mut();
        assert_eq!(
            rofd_page_get_text_layout(page, &mut layout, ptr::null_mut()),
            ROFD_STATUS_OK
        );
        let mut count = 0;
        assert_eq!(
            rofd_text_layout_get_count(layout, &mut count, ptr::null_mut()),
            ROFD_STATUS_OK
        );
        assert_eq!(count, 3);

        let a = character(layout, 0);
        let b = character(layout, 1);
        let z = character(layout, 2);
        assert_eq!(a.rect_mm.x_mm, 1.0);
        // The subset font's own advances: A 2.432 mm, B 2.628 mm, Z 2.412 mm at
        // size 4. The font-size footprint would step by 4 mm each time.
        let ab = b.rect_mm.x_mm - a.rect_mm.x_mm;
        let bz = z.rect_mm.x_mm - b.rect_mm.x_mm;
        assert!(
            (ab - 2.432).abs() < 1e-3 && (bz - 2.628).abs() < 1e-3,
            "advances {ab} and {bz}"
        );
        assert!(a.rect_mm.width_mm >= 4.0, "geometry stays conservative");

        // Search rectangles come from the same geometry.
        let query = CString::new("BZ").unwrap();
        let mut search = ptr::null_mut();
        assert_eq!(
            rofd_page_find_text(page, query.as_ptr(), &mut search, ptr::null_mut()),
            ROFD_STATUS_OK
        );
        assert_eq!(search_count(search), 1);
        let matched = text_match(search, 0);
        assert!((matched.rect_mm.x_mm - (1.0 + 2.432)).abs() < 1e-3);
        rofd_text_search_free(search);
        rofd_text_layout_free(layout);
        rofd_page_free(page);
    }
}
