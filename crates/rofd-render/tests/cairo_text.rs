use std::io::{Cursor, Write};
use std::sync::Arc;

use fontdb::Database;

use cairo::{
    Antialias, Context, FillRule, FontOptions, Format, HintMetrics, HintStyle, ImageSurface,
    Matrix, Operator, PathSegment, SubpixelOrder,
};
use rofd_core::{Document, LoadOptions};
use rofd_render::{
    CairoRenderer, Error, FontTextMetrics, PixelRect, RenderOptions, SystemFontResolver,
};
use zip::{write::SimpleFileOptions, ZipWriter};

const FONT: &[u8] = include_bytes!("fixtures/fonts/phase3-subset.ttf");

fn package(content: &str) -> Vec<u8> {
    package_with_font(content, FONT)
}

fn package_with_font(content: &str, font: &[u8]) -> Vec<u8> {
    let page = format!(
        r#"<ofd:Page xmlns:ofd="http://www.ofdspec.org/2016"><ofd:Area><ofd:PhysicalBox>0 0 30 30</ofd:PhysicalBox></ofd:Area><ofd:Content><ofd:Layer ID="1">{content}</ofd:Layer></ofd:Content></ofd:Page>"#
    );
    let files = [
        (
            "OFD.xml",
            br#"<ofd:OFD xmlns:ofd="http://www.ofdspec.org/2016"><ofd:DocBody><ofd:DocInfo><ofd:DocID>cairo-text</ofd:DocID></ofd:DocInfo><ofd:DocRoot>Doc_0/Document.xml</ofd:DocRoot></ofd:DocBody></ofd:OFD>"#.as_slice(),
        ),
        (
            "Doc_0/Document.xml",
            br#"<ofd:Document xmlns:ofd="http://www.ofdspec.org/2016"><ofd:CommonData><ofd:PageArea><ofd:PhysicalBox>0 0 30 30</ofd:PhysicalBox></ofd:PageArea><ofd:DocumentRes>Res.xml</ofd:DocumentRes></ofd:CommonData><ofd:Pages><ofd:Page ID="1" BaseLoc="Page.xml"/></ofd:Pages></ofd:Document>"#.as_slice(),
        ),
        (
            "Doc_0/Res.xml",
            br#"<ofd:Res xmlns:ofd="http://www.ofdspec.org/2016"><ofd:Fonts><ofd:Font ID="10" FontName="Fixture"><ofd:FontFile>font.ttf</ofd:FontFile></ofd:Font></ofd:Fonts></ofd:Res>"#.as_slice(),
        ),
        ("Doc_0/Page.xml", page.as_bytes()),
        ("Doc_0/font.ttf", font),
    ];
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    for (path, bytes) in files {
        writer
            .start_file(path, SimpleFileOptions::default())
            .unwrap();
        writer.write_all(bytes).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

fn surface_bytes(surface: &ImageSurface) -> Vec<u8> {
    surface.flush();
    let mut bytes = Vec::new();
    surface
        .with_data(|data| bytes.extend_from_slice(data))
        .unwrap();
    bytes
}

fn matching_pixels(
    surface: &mut ImageSurface,
    region: (i32, i32, i32, i32),
    predicate: impl Fn([u8; 4]) -> bool,
) -> usize {
    surface.flush();
    let stride = surface.stride() as usize;
    let data = surface.data().unwrap();
    let mut count = 0;
    for y in region.1..=region.3 {
        for x in region.0..=region.2 {
            let offset = y as usize * stride + x as usize * 4;
            let native = u32::from_ne_bytes(data[offset..offset + 4].try_into().unwrap());
            let pixel = [
                ((native >> 16) & 0xff) as u8,
                ((native >> 8) & 0xff) as u8,
                (native & 0xff) as u8,
                ((native >> 24) & 0xff) as u8,
            ];
            count += usize::from(predicate(pixel));
        }
    }
    count
}

fn path_segments(context: &Context) -> Vec<(u8, Vec<f64>)> {
    context
        .copy_path()
        .unwrap()
        .iter()
        .map(|segment| match segment {
            PathSegment::MoveTo((x, y)) => (0, vec![x, y]),
            PathSegment::LineTo((x, y)) => (1, vec![x, y]),
            PathSegment::CurveTo((x1, y1), (x2, y2), (x3, y3)) => (2, vec![x1, y1, x2, y2, x3, y3]),
            PathSegment::ClosePath => (3, Vec::new()),
        })
        .collect()
}

fn page(content: &str) -> rofd_core::Page {
    Document::from_bytes(package(content), LoadOptions::default())
        .unwrap()
        .page(0)
        .unwrap()
}

fn options(rotation_degrees: u16) -> RenderOptions {
    RenderOptions {
        dpi: 254.0,
        rotation_degrees,
        ..RenderOptions::default()
    }
}

fn render(page: &rofd_core::Page, rotation_degrees: u16) -> ImageSurface {
    let options = options(rotation_degrees);
    let (width, height) = CairoRenderer::pixel_size(page, &options).unwrap();
    let surface = ImageSurface::create(Format::ARgb32, width, height).unwrap();
    let context = Context::new(&surface).unwrap();
    CairoRenderer.render_page(page, &context, &options).unwrap();
    drop(context);
    surface
}

fn ink_bounds(surface: &mut ImageSurface) -> Option<(i32, i32, i32, i32)> {
    surface.flush();
    let width = surface.width();
    let height = surface.height();
    let stride = surface.stride() as usize;
    let data = surface.data().unwrap();
    let mut bounds: Option<(i32, i32, i32, i32)> = None;
    for y in 0..height {
        for x in 0..width {
            let offset = y as usize * stride + x as usize * 4;
            let pixel = &data[offset..offset + 4];
            if pixel[0..3] == [255, 255, 255] {
                continue;
            }
            bounds = Some(match bounds {
                Some((left, top, right, bottom)) => {
                    (left.min(x), top.min(y), right.max(x), bottom.max(y))
                }
                None => (x, y, x, y),
            });
        }
    }
    bounds
}

#[test]
fn embedded_latin_and_cjk_glyphs_render_at_positioned_baselines() {
    let page = page(
        r#"<ofd:TextObject ID="2" Boundary="2 3 24 12" Font="10" Size="4" Fill="true" Stroke="false"><ofd:FillColor Value="0 0 0"/><ofd:TextCode X="1" Y="5">A</ofd:TextCode><ofd:TextCode X="8" Y="5">中</ofd:TextCode></ofd:TextObject>"#,
    );
    let mut surface = render(&page, 0);
    let bounds = ink_bounds(&mut surface).expect("controlled glyphs must paint");
    assert_eq!(bounds, (30, 46, 136, 83));
}

#[test]
fn semantic_character_boxes_cover_the_rendered_ink() {
    let content = r#"<ofd:TextObject ID="2" Boundary="2 3 24 12" Font="10" Size="4" Fill="true" Stroke="false"><ofd:FillColor Value="0 0 0"/><ofd:TextCode X="1" Y="5">A中</ofd:TextCode></ofd:TextObject>"#;
    let mut database = Database::new();
    database.load_font_data(FONT.to_vec());
    let resolver: Arc<dyn rofd_render::FontResolver> = Arc::new(SystemFontResolver::from_database(
        database,
        Vec::new(),
        1 << 20,
    ));
    let metrics: Arc<dyn rofd_core::TextMetricsProvider> = Arc::new(FontTextMetrics::new(resolver));
    let page =
        Document::from_bytes_with_metrics(package(content), LoadOptions::default(), Some(metrics))
            .unwrap()
            .page(0)
            .unwrap();

    let mut surface = render(&page, 0);
    let (left, top, right, bottom) =
        ink_bounds(&mut surface).expect("controlled glyphs must paint");

    // 254 dpi renders exactly ten pixels per millimetre.
    let boxes: Vec<(f64, f64, f64, f64)> = page
        .text()
        .unwrap()
        .characters()
        .iter()
        .map(|character| {
            let rect = character.rect_mm().expect("both glyphs carry geometry");
            (
                rect.x * 10.0,
                rect.y * 10.0,
                (rect.x + rect.width) * 10.0,
                (rect.y + rect.height) * 10.0,
            )
        })
        .collect();
    assert_eq!(boxes.len(), 2);
    // One band for the whole text object: the boxes of A and 中 share their top and
    // bottom, because both come from the face's metrics rather than from each glyph's
    // ink.
    for index in 1..boxes.len() {
        assert_eq!(boxes[index].1, boxes[0].1, "tops differ: {boxes:?}");
        assert_eq!(boxes[index].3, boxes[0].3, "bottoms differ: {boxes:?}");
    }
    let box_left = boxes
        .iter()
        .map(|box_| box_.0)
        .fold(f64::INFINITY, f64::min);
    let box_top = boxes
        .iter()
        .map(|box_| box_.1)
        .fold(f64::INFINITY, f64::min);
    let box_right = boxes
        .iter()
        .map(|box_| box_.2)
        .fold(f64::NEG_INFINITY, f64::max);
    let box_bottom = boxes
        .iter()
        .map(|box_| box_.3)
        .fold(f64::NEG_INFINITY, f64::max);

    // The band contains the painted ink and sticks out above and below it, the way a
    // PDF text box built from the same font metrics does.
    let slack = 1.0;
    assert!(f64::from(left) >= box_left - slack, "{left} vs {box_left}");
    assert!(
        f64::from(right) <= box_right + slack,
        "{right} vs {box_right}"
    );
    assert!(box_top < f64::from(top), "{box_top} vs {top}");
    assert!(box_bottom > f64::from(bottom), "{box_bottom} vs {bottom}");
    // 中 paints below the baseline, which is where the font-size footprint stopped:
    // the band's bottom is below the baseline even without the ink.
    assert!(box_bottom > 80.0, "{box_bottom} vs the baseline");
}

#[test]
fn embedded_text_regions_preserve_fractional_glyph_positions_at_every_rotation() {
    let page = page(
        r#"<ofd:TextObject ID="2" Boundary="2 3 24 12" Font="10" Size="4" Fill="true" Stroke="false"><ofd:FillColor Value="0 0 0"/><ofd:TextCode X="1" Y="5">A</ofd:TextCode><ofd:TextCode X="8" Y="5">中</ofd:TextCode></ofd:TextObject>"#,
    );
    let mut changed = 0;
    let mut total = 0;
    let mut maximum_delta = 0;
    for rotation in [0, 90, 180, 270] {
        let options = RenderOptions {
            dpi: 87.3,
            scale: 1.13,
            rotation_degrees: rotation,
            ..RenderOptions::default()
        };
        let (width, height) = CairoRenderer::pixel_size(&page, &options).unwrap();
        let full = ImageSurface::create(Format::ARgb32, width, height).unwrap();
        let context = Context::new(&full).unwrap();
        let full_report = CairoRenderer
            .render_page(&page, &context, &options)
            .unwrap();
        drop(context);
        let full_bytes = surface_bytes(&full);
        for y in (0..height).step_by(23) {
            for x in (0..width).step_by(19) {
                let viewport = PixelRect {
                    x,
                    y,
                    width: 19.min(width - x),
                    height: 23.min(height - y),
                };
                let tile =
                    ImageSurface::create(Format::ARgb32, viewport.width, viewport.height).unwrap();
                let context = Context::new(&tile).unwrap();
                assert_eq!(
                    CairoRenderer
                        .render_page_region(&page, &context, &options, viewport)
                        .unwrap(),
                    full_report
                );
                drop(context);
                let tile_bytes = surface_bytes(&tile);
                for ty in 0..viewport.height {
                    for tx in 0..viewport.width {
                        let tile_offset = (ty * tile.stride() + tx * 4) as usize;
                        let full_offset = ((y + ty) * full.stride() + (x + tx) * 4) as usize;
                        for channel in 0..4 {
                            let actual = tile_bytes[tile_offset + channel];
                            let expected = full_bytes[full_offset + channel];
                            maximum_delta = maximum_delta.max(actual.abs_diff(expected));
                            changed += usize::from(actual != expected);
                            total += 1;
                        }
                    }
                }
            }
        }
    }
    // Cairo may cull a very faint edge pixel differently on a smaller target.
    // The fixture has two such pixels; geometry and every other channel agree.
    assert!(
        maximum_delta <= 13,
        "{changed}/{total} channels differ; max {maximum_delta}"
    );
    assert!(changed <= 6, "{changed}/{total} channels differ");
}

#[test]
fn explicit_and_inferred_deltas_change_exact_glyph_placement() {
    let inferred = page(
        r#"<ofd:TextObject ID="2" Boundary="2 2 24 8" Font="10" Size="4"><ofd:TextCode X="1" Y="5">AAA</ofd:TextCode></ofd:TextObject>"#,
    );
    let explicit = page(
        r#"<ofd:TextObject ID="2" Boundary="2 2 24 8" Font="10" Size="4"><ofd:TextCode X="1" Y="5" DeltaX="0 6">AAA</ofd:TextCode></ofd:TextObject>"#,
    );
    let mut inferred_surface = render(&inferred, 0);
    let mut explicit_surface = render(&explicit, 0);
    let inferred_bounds = ink_bounds(&mut inferred_surface).unwrap();
    let explicit_bounds = ink_bounds(&mut explicit_surface).unwrap();
    assert_eq!(inferred_bounds, (30, 40, 102, 69));
    assert_eq!(explicit_bounds, (30, 40, 114, 69));
}

#[test]
fn synthetic_missing_glyph_draws_a_deterministic_visible_box() {
    let page = page(
        r#"<ofd:TextObject ID="2" Boundary="4 4 12 10" Font="10" Size="5" Fill="false" Stroke="true" LineWidth="0.4"><ofd:StrokeColor Value="0 0 0"/><ofd:TextCode X="1" Y="6">🦄</ofd:TextCode></ofd:TextObject>"#,
    );
    let mut surface = render(&page, 0);
    let bounds = ink_bounds(&mut surface).expect("missing glyph box must paint");
    assert_eq!(bounds, (53, 59, 96, 102));
}

#[test]
fn fill_stroke_alpha_and_extended_stroke_style_are_applied() {
    let page = page(
        r#"<ofd:TextObject ID="2" Boundary="1 1 8 8" Font="10" Size="5" Fill="true" Stroke="false"><ofd:FillColor Value="255 0 0"/><ofd:TextCode X="1" Y="6">A</ofd:TextCode></ofd:TextObject><ofd:TextObject ID="3" Boundary="10 1 8 8" Font="10" Size="5" Fill="false" Stroke="true" LineWidth="0.6" Join="Round" Cap="Square" DashOffset="0.2" DashPattern="1 0.5" MiterLimit="5"><ofd:StrokeColor Value="0 0 255"/><ofd:TextCode X="1" Y="6">A</ofd:TextCode></ofd:TextObject><ofd:TextObject ID="4" Boundary="19 1 8 8" Font="10" Size="5" Fill="true" Stroke="true" Alpha="128" LineWidth="0.3"><ofd:FillColor Value="0 255 0"/><ofd:StrokeColor Value="255 0 0"/><ofd:TextCode X="1" Y="6">A</ofd:TextCode></ofd:TextObject>"#,
    );
    let mut surface = render(&page, 0);
    assert!(
        matching_pixels(&mut surface, (15, 15, 80, 75), |[r, g, b, _]| {
            r > 180 && g < 100 && b < 100
        }) > 300
    );
    assert!(
        matching_pixels(&mut surface, (105, 15, 175, 75), |[r, g, b, _]| {
            r < 120 && g < 120 && b > 150
        }) > 100
    );
    let translucent_green = matching_pixels(&mut surface, (195, 15, 265, 75), |[r, g, b, _]| {
        (80..=200).contains(&r) && g > 210 && (80..=200).contains(&b)
    });
    let translucent_red = matching_pixels(&mut surface, (195, 15, 265, 75), |[r, g, b, _]| {
        r > 210 && (80..=210).contains(&g) && b < 210
    });
    assert!(translucent_green > 40);
    assert!(translucent_red > 20);
}

#[test]
fn caller_font_rasterization_and_fill_rule_do_not_change_text_or_clipped_output() {
    let content = r#"<ofd:TextObject ID="2" Boundary="1 1 8 8" Font="10" Size="5" Fill="true" Stroke="true" LineWidth="0.3"><ofd:FillColor Value="0 180 0"/><ofd:StrokeColor Value="180 0 0"/><ofd:TextCode X="1" Y="6">A</ofd:TextCode></ofd:TextObject>"#;
    let unclipped_page = page(content);
    let clean = render(&unclipped_page, 0);

    let clipped = page(
        r#"<ofd:TextObject ID="2" Boundary="1 1 8 8" Font="10" Size="5" Fill="true" Stroke="true" LineWidth="0.3"><ofd:Clips><ofd:Clip><ofd:Area><ofd:Path Boundary="0 0 8 8" Fill="true" Stroke="false"><ofd:AbbreviatedData>M 0 0 L 8 0 L 8 8 L 0 8 C</ofd:AbbreviatedData></ofd:Path></ofd:Area></ofd:Clip></ofd:Clips><ofd:FillColor Value="0 180 0"/><ofd:StrokeColor Value="180 0 0"/><ofd:TextCode X="1" Y="6">A</ofd:TextCode></ofd:TextObject>"#,
    );
    assert_eq!(surface_bytes(&clean), surface_bytes(&render(&clipped, 0)));

    let surface = ImageSurface::create(Format::ARgb32, 300, 300).unwrap();
    let context = Context::new(&surface).unwrap();
    let font_matrix = Matrix::new(7.0, 0.0, 0.0, 9.0, 0.0, 0.0);
    let mut font_options = FontOptions::new().unwrap();
    font_options.set_antialias(Antialias::Subpixel);
    font_options.set_subpixel_order(SubpixelOrder::Bgr);
    font_options.set_hint_style(HintStyle::Full);
    font_options.set_hint_metrics(HintMetrics::On);
    context.set_font_matrix(font_matrix);
    context.set_font_options(&font_options);
    context.set_fill_rule(FillRule::EvenOdd);

    CairoRenderer
        .render_page(&unclipped_page, &context, &options(0))
        .unwrap();
    assert_eq!(context.font_matrix(), font_matrix);
    assert_eq!(context.font_options().unwrap(), font_options);
    assert_eq!(context.fill_rule(), FillRule::EvenOdd);
    drop(context);
    assert_eq!(surface_bytes(&surface), surface_bytes(&clean));
}

#[test]
fn object_transform_and_a8_clip_mask_apply_to_glyphs() {
    let transformed = page(
        r#"<ofd:TextObject ID="2" Boundary="2 2 20 12" CTM="1 0.4 0.3 1 3 1" Font="10" Size="5"><ofd:TextCode X="1" Y="6">A中</ofd:TextCode></ofd:TextObject>"#,
    );
    let mut transformed_surface = render(&transformed, 0);
    assert_eq!(
        ink_bounds(&mut transformed_surface),
        Some((78, 62, 150, 120))
    );

    let clipped = page(
        r#"<ofd:TextObject ID="2" Boundary="2 2 20 12" Font="10" Size="5"><ofd:Clips><ofd:Clip><ofd:Area><ofd:Path Boundary="0 0 2 10" Fill="true" Stroke="false"><ofd:AbbreviatedData>M 0 0 L 2 0 L 2 10 L 0 10 C</ofd:AbbreviatedData></ofd:Path></ofd:Area></ofd:Clip></ofd:Clips><ofd:TextCode X="1" Y="6">A</ofd:TextCode></ofd:TextObject>"#,
    );
    let mut clipped_surface = render(&clipped, 0);
    assert_eq!(ink_bounds(&mut clipped_surface), Some((30, 51, 39, 79)));
}

#[test]
fn page_quarter_turns_and_half_turn_map_text_pixels_exactly() {
    let page = page(
        r#"<ofd:TextObject ID="2" Boundary="2 3 10 8" Font="10" Size="4"><ofd:TextCode X="1" Y="5">A</ofd:TextCode></ofd:TextObject>"#,
    );
    let mut bounds = Vec::new();
    for rotation in [0, 90, 180, 270] {
        bounds.push(ink_bounds(&mut render(&page, rotation)).unwrap());
    }
    assert_eq!(
        bounds,
        [
            (30, 50, 54, 79),
            (220, 30, 249, 54),
            (245, 220, 269, 249),
            (50, 245, 79, 269),
        ]
    );
}

#[test]
fn cgtransform_uses_explicit_glyph_ids_without_shaping() {
    let ordinary = page(
        r#"<ofd:TextObject ID="2" Boundary="2 2 20 8" Font="10" Size="5"><ofd:TextCode X="1" Y="6">AB</ofd:TextCode></ofd:TextObject>"#,
    );
    let mapped = page(
        r#"<ofd:TextObject ID="2" Boundary="2 2 20 8" Font="10" Size="5"><ofd:TextCode X="1" Y="6">AB</ofd:TextCode><ofd:CGTransform CodePosition="0" CodeCount="2" GlyphCount="2"><ofd:Glyphs>3 2</ofd:Glyphs></ofd:CGTransform></ofd:TextObject>"#,
    );
    let ordinary_surface = render(&ordinary, 0);
    let mapped_surface = render(&mapped, 0);
    assert_ne!(
        surface_bytes(&ordinary_surface),
        surface_bytes(&mapped_surface)
    );
}

#[test]
fn invalid_embedded_font_preserves_every_caller_cairo_state_component() {
    let content = r#"<ofd:TextObject ID="2" Boundary="2 2 20 8" Font="10" Size="5"><ofd:TextCode X="1" Y="6">A</ofd:TextCode></ofd:TextObject>"#;
    let page = Document::from_bytes(
        package_with_font(content, b"not-a-font"),
        LoadOptions::default(),
    )
    .unwrap()
    .page(0)
    .unwrap();
    let surface = ImageSurface::create(Format::ARgb32, 300, 300).unwrap();
    let context = Context::new(&surface).unwrap();
    let matrix = Matrix::new(2.0, 0.25, 0.5, 3.0, 4.0, 5.0);
    let font_matrix = Matrix::new(7.0, 0.0, 0.0, 9.0, 0.0, 0.0);
    let mut font_options = FontOptions::new().unwrap();
    font_options.set_antialias(Antialias::None);
    context.set_matrix(matrix);
    context.set_font_matrix(font_matrix);
    context.set_font_options(&font_options);
    context.set_operator(Operator::Xor);
    context.set_antialias(Antialias::Gray);
    context.set_tolerance(3.0);
    context.set_line_width(7.0);
    context.move_to(4.0, 5.0);
    context.line_to(6.0, 7.0);
    let path = path_segments(&context);
    let font_face = context.font_face();
    let font_face_type = font_face.type_();
    let font_family = font_face.toy_get_family();

    assert!(matches!(
        CairoRenderer.render_page(&page, &context, &options(0)),
        Err(Error::ObjectResourceProcessing { object_id: 2, .. })
    ));
    assert_eq!(context.matrix(), matrix);
    assert_eq!(context.font_matrix(), font_matrix);
    assert_eq!(context.font_options().unwrap(), font_options);
    assert_eq!(context.font_face().type_(), font_face_type);
    assert_eq!(context.font_face().toy_get_family(), font_family);
    assert_eq!(context.operator(), Operator::Xor);
    assert_eq!(context.antialias(), Antialias::Gray);
    assert_eq!(context.tolerance(), 3.0);
    assert_eq!(context.line_width(), 7.0);
    assert_eq!(path_segments(&context), path);
    drop(context);
    assert!(surface_bytes(&surface).iter().all(|byte| *byte == 0));
}
