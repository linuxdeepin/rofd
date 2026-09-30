use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct OfdRoot {
    #[serde(rename = "DocBody", default)]
    pub(crate) doc_bodies: Vec<DocBody>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct DocBody {
    pub(crate) doc_info: DocInfo,
    pub(crate) doc_root: String,
    #[serde(rename = "Signatures")]
    pub(crate) signatures: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct SignaturesRoot {
    #[serde(rename = "Signature", default)]
    pub(crate) signatures: Vec<SignatureEntry>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct SignatureEntry {
    #[serde(rename = "ID")]
    pub(crate) id: Option<String>,
    #[serde(rename = "BaseLoc")]
    pub(crate) base_loc: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct SignatureRoot {
    #[serde(rename = "SignedInfo")]
    pub(crate) signed_info: SignedInfo,
    #[serde(rename = "SignedValue")]
    pub(crate) signed_value: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct SignedInfo {
    #[serde(rename = "StampAnnot", default)]
    pub(crate) stamp_annots: Vec<StampAnnotRaw>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct StampAnnotRaw {
    #[serde(rename = "PageRef")]
    pub(crate) page_ref: String,
    #[serde(rename = "ID")]
    pub(crate) id: Option<String>,
    #[serde(rename = "Boundary")]
    pub(crate) boundary: String,
    #[serde(rename = "Clip")]
    pub(crate) clip: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct DocInfo {
    #[serde(rename = "DocID")]
    pub(crate) document_id: Option<String>,
    pub(crate) title: Option<String>,
    pub(crate) author: Option<String>,
    pub(crate) subject: Option<String>,
    #[serde(rename = "Abstract")]
    pub(crate) abstract_: Option<String>,
    pub(crate) creator: Option<String>,
    pub(crate) creator_version: Option<String>,
    pub(crate) creation_date: Option<String>,
    pub(crate) mod_date: Option<String>,
    #[serde(default)]
    pub(crate) keywords: Keywords,
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct Keywords {
    #[serde(rename = "Keyword", default)]
    pub(crate) entries: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct DocumentRoot {
    pub(crate) common_data: CommonData,
    pub(crate) pages: PageList,
    #[serde(rename = "Annotations")]
    pub(crate) annotations: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct CommonData {
    /// Optional because real-world producers omit it; documents without it
    /// are only usable when every page declares its own Area.
    pub(crate) page_area: Option<PageArea>,
    pub(crate) public_res: Option<String>,
    pub(crate) document_res: Option<String>,
    #[serde(rename = "TemplatePage", default)]
    pub(crate) template_pages: Vec<TemplatePage>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ResourceRoot {
    #[serde(rename = "BaseLoc")]
    pub(crate) base_loc: Option<String>,
    /// Real-world producers occasionally emit several Fonts blocks (ofdrw's
    /// ano.ofd); every block contributes entries to the catalog.
    #[serde(rename = "Fonts", default)]
    pub(crate) fonts: Vec<Fonts>,
    /// Real-world producers occasionally emit several MultiMedias blocks
    /// (invoice generators); every block contributes entries to the catalog.
    #[serde(rename = "MultiMedias", default)]
    pub(crate) multi_medias: Vec<MultiMedias>,
    #[serde(rename = "DrawParams")]
    pub(crate) draw_params: Option<DrawParams>,
    #[serde(rename = "ColorSpaces")]
    pub(crate) color_spaces: Option<ColorSpaces>,
    #[serde(rename = "CompositeGraphicUnits")]
    pub(crate) composite_graphic_units: Option<CompositeGraphicUnits>,
}

/// A reusable vector graphic (`CompositeGraphicUnit`) resource.
#[derive(Debug, Deserialize)]
pub(crate) struct CompositeGraphicUnits {
    #[serde(rename = "CompositeGraphicUnit", default)]
    pub(crate) entries: Vec<CompositeGraphicUnit>,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct CompositeGraphicUnit {
    #[serde(rename = "ID")]
    pub(crate) id: String,
    #[serde(rename = "Width")]
    pub(crate) width: Option<String>,
    #[serde(rename = "Height")]
    pub(crate) height: Option<String>,
    #[serde(rename = "Content")]
    pub(crate) content: Option<CompositeGraphicContent>,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct CompositeGraphicContent {
    #[serde(skip)]
    pub(crate) actions: crate::navigation::deferred::Actions,
    #[serde(rename = "ID")]
    #[allow(dead_code)]
    pub(crate) id: Option<String>,
    #[serde(rename = "$value", default)]
    pub(crate) objects: Vec<GraphicUnit>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct Fonts {
    #[serde(rename = "Font", default)]
    pub(crate) entries: Vec<FontEntry>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct FontEntry {
    #[serde(rename = "ID")]
    pub(crate) id: String,
    #[serde(rename = "FontName")]
    pub(crate) font_name: Option<String>,
    #[serde(rename = "FamilyName")]
    pub(crate) family_name: Option<String>,
    #[serde(rename = "Charset")]
    pub(crate) charset: Option<String>,
    #[serde(rename = "FontFile")]
    pub(crate) font_file: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct MultiMedias {
    #[serde(rename = "MultiMedia", default)]
    pub(crate) entries: Vec<MultiMediaEntry>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct MultiMediaEntry {
    #[serde(rename = "ID")]
    pub(crate) id: String,
    #[serde(rename = "Type")]
    pub(crate) kind: Option<String>,
    #[serde(rename = "Format")]
    pub(crate) format: Option<String>,
    #[serde(rename = "MediaFile")]
    pub(crate) media_file: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct DrawParams {
    #[serde(rename = "DrawParam", default)]
    pub(crate) entries: Vec<DrawParamEntry>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct DrawParamEntry {
    #[serde(rename = "ID")]
    pub(crate) id: String,
    #[serde(rename = "Relative")]
    pub(crate) relative: Option<String>,
    #[serde(rename = "LineWidth")]
    pub(crate) line_width: Option<String>,
    #[serde(rename = "Join")]
    pub(crate) line_join: Option<String>,
    #[serde(rename = "Cap")]
    pub(crate) line_cap: Option<String>,
    #[serde(rename = "DashOffset")]
    pub(crate) dash_offset: Option<String>,
    #[serde(rename = "DashPattern")]
    pub(crate) dash_pattern: Option<String>,
    #[serde(rename = "MiterLimit")]
    pub(crate) miter_limit: Option<String>,
    #[serde(rename = "FillColor")]
    pub(crate) fill_color: Option<PaintColor>,
    #[serde(rename = "StrokeColor")]
    pub(crate) stroke_color: Option<PaintColor>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ColorSpaces {
    #[serde(rename = "ColorSpace", default)]
    pub(crate) entries: Vec<ColorSpaceEntry>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ColorSpaceEntry {
    #[serde(rename = "ID")]
    pub(crate) id: String,
    #[serde(rename = "Type")]
    pub(crate) kind: Option<String>,
    #[serde(rename = "BitsPerComponent")]
    pub(crate) bits_per_component: Option<String>,
    #[serde(rename = "Palette")]
    pub(crate) palette: Option<Palette>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct Palette {
    #[serde(rename = "CV", default)]
    pub(crate) colors: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct TemplatePage {
    #[serde(rename = "ID")]
    pub(crate) id: String,
    #[serde(rename = "BaseLoc")]
    pub(crate) base_loc: String,
    #[serde(rename = "ZOrder")]
    pub(crate) z_order: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct PageArea {
    pub(crate) physical_box: Option<String>,
    pub(crate) application_box: Option<String>,
    pub(crate) content_box: Option<String>,
    pub(crate) bleed_box: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct PageList {
    #[serde(rename = "Page")]
    pub(crate) pages: Vec<PageEntry>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct PageEntry {
    #[serde(rename = "ID")]
    pub(crate) id: u64,
    #[serde(rename = "BaseLoc")]
    pub(crate) base_loc: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct PageRoot {
    #[serde(skip)]
    pub(crate) actions: crate::navigation::deferred::Actions,
    pub(crate) area: Option<PageArea>,
    #[serde(rename = "Template", default)]
    pub(crate) templates: Vec<TemplateReference>,
    pub(crate) content: Option<PageContent>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct TemplateReference {
    #[serde(rename = "TemplateID")]
    pub(crate) template_id: String,
    #[serde(rename = "ZOrder")]
    pub(crate) z_order: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct PageContent {
    #[serde(rename = "Layer", default)]
    pub(crate) layers: Vec<Layer>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct Layer {
    #[serde(skip)]
    pub(crate) actions: crate::navigation::deferred::Actions,
    #[serde(rename = "ID")]
    pub(crate) id: Option<String>,
    #[serde(rename = "Type")]
    pub(crate) kind: Option<String>,
    #[serde(rename = "$value", default)]
    pub(crate) objects: Vec<GraphicUnit>,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) enum GraphicUnit {
    #[serde(rename = "PathObject")]
    Path(PathObjectEnvelope),
    #[serde(rename = "PageBlock")]
    Group(PageBlock),
    #[serde(rename = "TextObject")]
    Text(TextObjectEnvelope),
    #[serde(rename = "ImageObject")]
    Image(ImageObjectEnvelope),
    #[serde(rename = "CompositeObject")]
    Composite(CompositeObject),
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct PageBlock {
    #[serde(skip)]
    pub(crate) actions: crate::navigation::deferred::Actions,
    #[serde(rename = "ID")]
    pub(crate) id: Option<String>,
    #[serde(rename = "$value", default)]
    pub(crate) objects: Vec<GraphicUnit>,
}

/// Attributes-only payload, so it parses through the main serde pass.
#[derive(Clone, Debug, Deserialize)]
pub(crate) struct CompositeObject {
    #[serde(skip)]
    pub(crate) actions: crate::navigation::deferred::Actions,
    #[serde(rename = "ID")]
    pub(crate) id: Option<String>,
    #[serde(rename = "Boundary")]
    pub(crate) boundary: Option<String>,
    #[serde(rename = "CTM")]
    pub(crate) transform: Option<String>,
    #[serde(rename = "ResourceID")]
    pub(crate) resource_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct TextObjectEnvelope {
    #[serde(rename = "ID")]
    pub(crate) id: Option<String>,
    #[serde(skip)]
    pub(crate) object: Option<Box<TextObject>>,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct ImageObjectEnvelope {
    #[serde(rename = "ID")]
    pub(crate) id: Option<String>,
    #[serde(skip)]
    pub(crate) object: Option<Box<ImageObject>>,
}

/// PathObject is parsed standalone like TextObject/ImageObject: serde-xml-rs
/// 0.6 mishandles its nested Clips vectors when a sibling graphic unit
/// follows, so the streaming pass extracts the payload separately.
#[derive(Clone, Debug, Deserialize)]
pub(crate) struct PathObjectEnvelope {
    #[serde(rename = "ID")]
    pub(crate) id: Option<String>,
    #[serde(skip)]
    pub(crate) object: Option<Box<PathObject>>,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct PathObject {
    #[serde(skip)]
    pub(crate) actions: crate::navigation::deferred::Actions,
    #[serde(rename = "ID")]
    pub(crate) id: Option<String>,
    /// Optional because real-world producers omit it (ofdrw's
    /// converter/intro-数科.ofd); ofdrw draws such paths without the boundary
    /// translation.
    #[serde(rename = "Boundary")]
    pub(crate) boundary: Option<String>,
    #[serde(rename = "CTM")]
    pub(crate) transform: Option<String>,
    #[serde(rename = "Stroke")]
    pub(crate) stroke: Option<String>,
    #[serde(rename = "Fill")]
    pub(crate) fill: Option<String>,
    #[serde(rename = "LineWidth")]
    pub(crate) line_width: Option<String>,
    #[serde(rename = "Rule")]
    pub(crate) fill_rule: Option<String>,
    #[serde(rename = "Alpha")]
    pub(crate) alpha: Option<String>,
    #[serde(rename = "DrawParam")]
    pub(crate) draw_param: Option<String>,
    #[serde(rename = "Join")]
    pub(crate) line_join: Option<String>,
    #[serde(rename = "Cap")]
    pub(crate) line_cap: Option<String>,
    #[serde(rename = "DashOffset")]
    pub(crate) dash_offset: Option<String>,
    #[serde(rename = "DashPattern")]
    pub(crate) dash_pattern: Option<String>,
    #[serde(rename = "MiterLimit")]
    pub(crate) miter_limit: Option<String>,
    #[serde(rename = "AbbreviatedData")]
    pub(crate) abbreviated_data: String,
    #[serde(rename = "StrokeColor")]
    pub(crate) stroke_color: Option<PaintColor>,
    #[serde(rename = "FillColor")]
    pub(crate) fill_color: Option<PaintColor>,
    #[serde(rename = "Clips")]
    pub(crate) clips: Option<Clips>,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct TextObject {
    #[serde(skip)]
    pub(crate) actions: crate::navigation::deferred::Actions,
    #[serde(rename = "ID")]
    pub(crate) id: Option<String>,
    #[serde(rename = "Boundary")]
    pub(crate) boundary: Option<String>,
    #[serde(rename = "CTM")]
    pub(crate) transform: Option<String>,
    #[serde(rename = "Font")]
    pub(crate) font: Option<String>,
    #[serde(rename = "Size")]
    pub(crate) size: Option<String>,
    #[serde(rename = "Stroke")]
    pub(crate) stroke: Option<String>,
    #[serde(rename = "Fill")]
    pub(crate) fill: Option<String>,
    #[serde(rename = "Alpha")]
    pub(crate) alpha: Option<String>,
    #[serde(rename = "DrawParam")]
    pub(crate) draw_param: Option<String>,
    #[serde(rename = "LineWidth")]
    pub(crate) line_width: Option<String>,
    #[serde(rename = "Join")]
    pub(crate) line_join: Option<String>,
    #[serde(rename = "Cap")]
    pub(crate) line_cap: Option<String>,
    #[serde(rename = "DashOffset")]
    pub(crate) dash_offset: Option<String>,
    #[serde(rename = "DashPattern")]
    pub(crate) dash_pattern: Option<String>,
    #[serde(rename = "MiterLimit")]
    pub(crate) miter_limit: Option<String>,
    #[serde(rename = "FillColor")]
    pub(crate) fill_color: Option<PaintColor>,
    #[serde(rename = "StrokeColor")]
    pub(crate) stroke_color: Option<PaintColor>,
    #[serde(rename = "Clips")]
    pub(crate) clips: Option<Clips>,
    #[serde(rename = "TextCode", default)]
    pub(crate) text_codes: Vec<TextCode>,
    #[serde(rename = "CGTransform", default)]
    pub(crate) cg_transforms: Vec<CgTransform>,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct TextCode {
    #[serde(rename = "$value", default)]
    pub(crate) text: String,
    #[serde(rename = "X")]
    pub(crate) x: Option<String>,
    #[serde(rename = "Y")]
    pub(crate) y: Option<String>,
    #[serde(rename = "DeltaX")]
    pub(crate) delta_x: Option<String>,
    #[serde(rename = "DeltaY")]
    pub(crate) delta_y: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct CgTransform {
    #[serde(rename = "CodePosition")]
    pub(crate) code_position: Option<String>,
    #[serde(rename = "CodeCount")]
    pub(crate) code_count: Option<String>,
    #[serde(rename = "GlyphCount")]
    pub(crate) glyph_count: Option<String>,
    #[serde(rename = "Glyphs")]
    pub(crate) glyphs: Option<GlyphsContent>,
}

/// The `Glyphs` element may contain either a whitespace-separated list of
/// glyph IDs (legacy form) or structured `Glyph` child elements with
/// per-glyph transforms (GB/T 33190-2016 form).
#[derive(Clone, Debug, Deserialize)]
pub(crate) struct GlyphsContent {
    #[serde(rename = "$value", default)]
    pub(crate) children: Vec<GlyphEntry>,
}

/// One entry inside `Glyphs`: either text content or a structured `Glyph`.
#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub(crate) enum GlyphEntry {
    /// Legacy text content: `"3 2"`.
    Text(String),
    /// Structured `Glyph` element with per-glyph attributes.
    Glyph(RawGlyph),
}

/// One structured `Glyph` child of `Glyphs` with optional per-glyph transform.
#[derive(Clone, Debug, Deserialize)]
pub(crate) struct RawGlyph {
    #[serde(rename = "GlyphID")]
    pub(crate) glyph_id: Option<String>,
    #[serde(rename = "X")]
    pub(crate) x: Option<String>,
    #[serde(rename = "Y")]
    pub(crate) y: Option<String>,
    #[serde(rename = "M00")]
    pub(crate) m00: Option<String>,
    #[serde(rename = "M01")]
    pub(crate) m01: Option<String>,
    #[serde(rename = "M10")]
    pub(crate) m10: Option<String>,
    #[serde(rename = "M11")]
    pub(crate) m11: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct ImageObject {
    #[serde(skip)]
    pub(crate) actions: crate::navigation::deferred::Actions,
    #[serde(rename = "ID")]
    pub(crate) id: Option<String>,
    #[serde(rename = "Boundary")]
    pub(crate) boundary: Option<String>,
    #[serde(rename = "CTM")]
    pub(crate) transform: Option<String>,
    #[serde(rename = "ResourceID")]
    pub(crate) resource_id: Option<String>,
    #[serde(rename = "Alpha")]
    pub(crate) alpha: Option<String>,
    #[serde(rename = "DrawParam")]
    pub(crate) draw_param: Option<String>,
    #[serde(rename = "Substitution")]
    pub(crate) substitution: Option<String>,
    #[serde(rename = "ImageMask")]
    pub(crate) image_mask: Option<String>,
    #[serde(rename = "Clips")]
    pub(crate) clips: Option<Clips>,
    #[serde(rename = "Border")]
    pub(crate) border: Option<Border>,
}

/// Image border settings (GB/T 33190-2016 table 43).
#[derive(Clone, Debug, Deserialize)]
pub(crate) struct Border {
    #[serde(rename = "LineWidth")]
    pub(crate) line_width: Option<String>,
    #[serde(rename = "HorizonalCornerRadius")]
    pub(crate) horizontal_corner_radius: Option<String>,
    #[serde(rename = "VerticalCornerRadius")]
    pub(crate) vertical_corner_radius: Option<String>,
    #[serde(rename = "DashOffset")]
    pub(crate) dash_offset: Option<String>,
    #[serde(rename = "DashPattern")]
    pub(crate) dash_pattern: Option<String>,
    #[serde(rename = "BorderColor")]
    pub(crate) border_color: Option<PaintColor>,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct Clips {
    #[serde(rename = "TransFlag")]
    pub(crate) trans_flag: Option<String>,
    #[serde(rename = "Clip", default)]
    pub(crate) clips: Vec<Clip>,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct Clip {
    #[serde(rename = "Area", default)]
    pub(crate) areas: Vec<ClipArea>,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct ClipArea {
    #[serde(rename = "CTM")]
    pub(crate) transform: Option<String>,
    #[serde(rename = "$value", default)]
    pub(crate) children: Vec<ClipAreaChild>,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) enum ClipAreaChild {
    #[serde(rename = "Path")]
    Path(ClipPath),
    #[serde(rename = "Text")]
    Text(ClipText),
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct ClipPath {
    #[serde(rename = "Boundary")]
    pub(crate) boundary: Option<String>,
    #[serde(rename = "CTM")]
    pub(crate) transform: Option<String>,
    #[serde(rename = "Stroke")]
    pub(crate) stroke: Option<String>,
    #[serde(rename = "Fill")]
    pub(crate) fill: Option<String>,
    #[serde(rename = "Rule")]
    pub(crate) fill_rule: Option<String>,
    #[serde(rename = "AbbreviatedData")]
    pub(crate) abbreviated_data: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct ClipText {}

/// Entry file `Annotations.xml`: one `Page` element per annotated page.
#[derive(Debug, Deserialize)]
pub(crate) struct AnnotationsRoot {
    #[serde(rename = "Page", default)]
    pub(crate) pages: Vec<AnnotationPageEntry>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct AnnotationPageEntry {
    #[serde(rename = "PageID")]
    pub(crate) page_id: String,
    #[serde(rename = "FileLoc")]
    pub(crate) file_loc: Option<String>,
    #[serde(rename = "Annot", default)]
    pub(crate) inline_annots: Vec<AnnotEntry>,
}

/// One `PageAnnot` file holding the annotations of a single page.
#[derive(Debug, Deserialize)]
pub(crate) struct PageAnnotRoot {
    #[serde(rename = "Annot", default)]
    pub(crate) annots: Vec<AnnotEntry>,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct AnnotEntry {
    #[serde(skip)]
    pub(crate) actions: crate::navigation::deferred::Actions,
    #[serde(rename = "ID")]
    pub(crate) id: Option<String>,
    #[serde(rename = "Type")]
    pub(crate) kind: Option<String>,
    #[serde(rename = "Visible")]
    pub(crate) visible: Option<String>,
    #[serde(rename = "Appearance")]
    pub(crate) appearance: Option<AppearanceRaw>,
}

/// The inline `Appearance` page block of an annotation.
#[derive(Clone, Debug, Deserialize)]
pub(crate) struct AppearanceRaw {
    #[serde(skip)]
    pub(crate) actions: crate::navigation::deferred::Actions,
    #[serde(rename = "Boundary")]
    pub(crate) boundary: Option<String>,
    #[serde(rename = "$value", default)]
    pub(crate) objects: Vec<GraphicUnit>,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct PaintColor {
    #[serde(rename = "Value")]
    pub(crate) value: Option<String>,
    #[serde(rename = "Alpha")]
    pub(crate) alpha: Option<String>,
    #[serde(rename = "Index")]
    pub(crate) index: Option<String>,
    #[serde(rename = "ColorSpace")]
    pub(crate) color_space: Option<String>,
}
