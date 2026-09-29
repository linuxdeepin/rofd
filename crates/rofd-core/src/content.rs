use std::collections::{BTreeMap, HashSet};
#[cfg(test)]
use std::sync::atomic::{AtomicUsize, Ordering};

#[cfg(test)]
static GLYPH_RANGE_NEIGHBOR_CHECKS: AtomicUsize = AtomicUsize::new(0);

use crate::paint::PaintParameters;
use crate::raw;
use crate::{
    CharacterGlyphMap, Color, Error, GlyphTransform, ImageObject, LineCap, LineJoin, PathData,
    Rect, ResourceKind, ResourceLimits, Result, StrokeStyle, TextCode, TextObject, Transform,
};

/// The stacking category assigned to a page layer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LayerType {
    /// Content painted behind the page body.
    Background,
    /// Ordinary page content.
    Body,
    /// Content painted in front of the page body.
    Foreground,
}

/// The package source that contributed an effective page layer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LayerSource {
    /// A layer loaded from the template with the given object identifier.
    Template(u64),
    /// A layer declared directly by the real page.
    Page,
}

/// An ordered layer of page objects.
#[derive(Clone, Debug, PartialEq)]
pub struct Layer {
    pub(crate) actions: crate::navigation::deferred::Actions,
    object_id: u64,
    kind: LayerType,
    source: LayerSource,
    objects: Vec<PageObject>,
}

impl Layer {
    /// Returns the OFD object identifier.
    pub fn object_id(&self) -> u64 {
        self.object_id
    }

    /// Returns the stacking category.
    pub fn kind(&self) -> LayerType {
        self.kind
    }

    /// Returns whether this layer came from the real page or a template.
    pub fn source(&self) -> LayerSource {
        self.source
    }

    /// Returns child objects in source order.
    pub fn objects(&self) -> &[PageObject] {
        &self.objects
    }
}

/// A page-level graphic object.
#[derive(Clone, Debug, PartialEq)]
pub enum PageObject {
    /// A supported vector path.
    Path(PathObject),
    /// A validated text object retained for later shaping and rendering.
    Text(TextObject),
    /// A validated encoded-image reference retained for later decoding and rendering.
    Image(ImageObject),
    /// An ordered group originating from a page block.
    Group(PageGroup),
    /// A composite object with the expanded content of its referenced vector
    /// graphic resource.
    Composite(CompositeObject),
    /// A known object kind not rendered by this version.
    Unsupported(UnsupportedObject),
}

impl PageObject {
    /// Returns the OFD object identifier.
    pub fn object_id(&self) -> u64 {
        match self {
            Self::Path(object) => object.object_id(),
            Self::Text(object) => object.object_id(),
            Self::Image(object) => object.object_id(),
            Self::Group(object) => object.object_id(),
            Self::Composite(object) => object.object_id(),
            Self::Unsupported(object) => object.object_id(),
        }
    }
}

/// A composite object whose graphic units come from a reusable vector
/// graphic resource.
///
/// Following ofdrw, the referenced content draws in the composite's local
/// coordinate system: the boundary translation and CTM position it on the
/// page, with no automatic scaling to the boundary size.
#[derive(Clone, Debug, PartialEq)]
pub struct CompositeObject {
    pub(crate) actions: crate::navigation::deferred::Actions,
    pub(crate) action_boundary: Option<Rect>,
    pub(crate) content_actions: crate::navigation::deferred::Actions,
    object_id: u64,
    boundary: Rect,
    transform: Transform,
    resource_id: u64,
    objects: Vec<PageObject>,
}

impl CompositeObject {
    /// Returns the OFD object identifier.
    pub fn object_id(&self) -> u64 {
        self.object_id
    }

    /// Returns the object boundary.
    pub fn boundary(&self) -> Rect {
        self.boundary
    }

    /// Returns the object transform.
    pub fn transform(&self) -> Transform {
        self.transform
    }

    /// Returns the referenced vector graphic resource identifier.
    pub fn resource_id(&self) -> u64 {
        self.resource_id
    }

    /// Returns the converted content of the referenced vector graphic.
    pub fn objects(&self) -> &[PageObject] {
        &self.objects
    }
}

/// An ordered group of page objects.
#[derive(Clone, Debug, PartialEq)]
pub struct PageGroup {
    pub(crate) actions: crate::navigation::deferred::Actions,
    object_id: u64,
    objects: Vec<PageObject>,
}

impl PageGroup {
    /// Returns the OFD object identifier.
    pub fn object_id(&self) -> u64 {
        self.object_id
    }

    /// Returns child objects in source order.
    pub fn objects(&self) -> &[PageObject] {
        &self.objects
    }
}

/// A known page object retained without interpreting its payload.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnsupportedObject {
    object_id: u64,
    kind: UnsupportedObjectKind,
}

impl UnsupportedObject {
    /// Returns the OFD object identifier.
    pub fn object_id(&self) -> u64 {
        self.object_id
    }

    /// Returns the unsupported object category.
    pub fn kind(&self) -> UnsupportedObjectKind {
        self.kind
    }
}

/// Known graphic object categories retained as explicit placeholders.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UnsupportedObjectKind {
    /// A text object not yet rendered by a downstream backend.
    Text,
    /// An image object not yet rendered by a downstream backend.
    Image,
}

/// The category of a page annotation (GB/T 33190-2016 table 62).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum AnnotationType {
    /// A hyperlink annotation.
    Link,
    /// A graphics annotation such as a rectangle or polygon.
    Path,
    /// A text highlight annotation.
    Highlight,
    /// A seal or watermark stamp annotation.
    Stamp,
    /// A watermark annotation.
    Watermark,
}

impl AnnotationType {
    pub(crate) fn from_type_name(value: &str) -> Option<Self> {
        match value {
            "Link" => Some(Self::Link),
            "Path" => Some(Self::Path),
            "Highlight" => Some(Self::Highlight),
            "Stamp" => Some(Self::Stamp),
            "Watermark" => Some(Self::Watermark),
            _ => None,
        }
    }
}

/// One page annotation with its inline appearance page block.
#[derive(Clone, Debug, PartialEq)]
pub struct PageAnnotation {
    pub(crate) actions: crate::navigation::deferred::Actions,
    pub(crate) action_boundary: Option<Rect>,
    pub(crate) appearance_actions: crate::navigation::deferred::Actions,
    pub(crate) page_ref: u64,
    object_id: u64,
    kind: AnnotationType,
    visible: bool,
    boundary: Rect,
    objects: Vec<PageObject>,
}

impl PageAnnotation {
    /// Returns the OFD object identifier of the annotation.
    pub fn object_id(&self) -> u64 {
        self.object_id
    }

    /// Returns the annotation category.
    pub fn kind(&self) -> AnnotationType {
        self.kind
    }

    /// Returns whether the annotation should be displayed.
    pub fn visible(&self) -> bool {
        self.visible
    }

    /// Returns the appearance boundary in page coordinates.
    pub fn boundary(&self) -> Rect {
        self.boundary
    }

    /// Returns the converted appearance graphic units.
    pub fn objects(&self) -> &[PageObject] {
        &self.objects
    }
}

/// Converts one raw annotation into the public model.
pub(crate) fn convert_annotation(
    annot: raw::AnnotEntry,
    document: &crate::Document,
    limits: &ResourceLimits,
    path: &str,
) -> Result<PageAnnotation> {
    let kind = match annot.kind.as_deref() {
        Some(value) => {
            AnnotationType::from_type_name(value).ok_or_else(|| Error::InvalidValue {
                field: "annotation type",
                value: value.to_owned(),
                path: None,
            })?
        }
        // The standard requires Type, but lenient parsing keeps untyped
        // annotations as stamps, matching how seal annotations are used.
        None => AnnotationType::Stamp,
    };
    let visible = match annot.visible.as_deref() {
        None | Some("true" | "1") => true,
        Some("false" | "0") => false,
        Some(value) => {
            return Err(invalid_value("annotation Visible", value));
        }
    };
    let mut context = ConversionContext {
        limits,
        document,
        path,
        object_ids: HashSet::new(),
        next_synthetic_id: u64::MAX,
        vector_graphics: Vec::new(),
        remaining_path_commands: limits.max_path_commands,
        remaining_text_characters: limits.max_text_characters_per_page,
        remaining_glyphs: limits.max_glyphs_per_page,
        remaining_text_expansion_entries: limits.max_text_expansion_entries,
    };
    let object_id = context.resolve_object_id(annot.id.as_deref())?;
    context.register_id(object_id)?;
    let appearance_present = annot.appearance.is_some();
    let appearance_actions = annot
        .appearance
        .as_ref()
        .and_then(|appearance| appearance.actions.clone());
    let (boundary, objects) = match annot.appearance {
        Some(appearance) => {
            let boundary = appearance
                .boundary
                .as_deref()
                .ok_or_else(|| {
                    object_error(
                        path,
                        object_id,
                        "Appearance.Boundary",
                        "required attribute is missing".to_owned(),
                    )
                })
                .and_then(|value| {
                    Rect::parse(value).map_err(|error| {
                        object_error(path, object_id, "Appearance.Boundary", error.to_string())
                    })
                })?;
            let objects = context.convert_objects(appearance.objects)?;
            (boundary, objects)
        }
        None => (
            crate::Rect {
                x: 0.0,
                y: 0.0,
                width: 0.0,
                height: 0.0,
            },
            Vec::new(),
        ),
    };
    Ok(PageAnnotation {
        actions: annot.actions,
        action_boundary: appearance_present.then_some(boundary),
        appearance_actions,
        page_ref: 0,
        object_id,
        kind,
        visible,
        boundary,
        objects,
    })
}

/// The algorithm used to determine the interior of a path.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FillRule {
    /// The non-zero winding rule.
    NonZero,
    /// The even-odd rule.
    EvenOdd,
}

/// A validated vector path object.
#[derive(Clone, Debug, PartialEq)]
pub struct PathObject {
    pub(crate) actions: crate::navigation::deferred::Actions,
    pub(crate) action_boundary: Option<Rect>,
    object_id: u64,
    boundary: Rect,
    transform: Transform,
    path_data: PathData,
    stroke: Option<Color>,
    fill: Option<Color>,
    line_width: f64,
    stroke_style: StrokeStyle,
    fill_rule: FillRule,
    clips: Vec<Clip>,
}

impl PathObject {
    /// Returns the OFD object identifier.
    pub fn object_id(&self) -> u64 {
        self.object_id
    }

    /// Returns the object boundary with a finite origin and positive dimensions.
    pub fn boundary(&self) -> Rect {
        self.boundary
    }

    /// Returns the object transform.
    pub fn transform(&self) -> Transform {
        self.transform
    }

    /// Returns the validated abbreviated path.
    pub fn path_data(&self) -> &PathData {
        &self.path_data
    }

    /// Returns the stroke color when stroking is enabled.
    pub fn stroke(&self) -> Option<Color> {
        self.stroke
    }

    /// Returns the fill color when filling is enabled.
    pub fn fill(&self) -> Option<Color> {
        self.fill
    }

    /// Returns the positive line width in millimetres.
    pub fn line_width(&self) -> f64 {
        self.line_width
    }

    /// Returns the effective validated stroke geometry.
    pub fn stroke_style(&self) -> &StrokeStyle {
        &self.stroke_style
    }

    /// Returns the path fill rule.
    pub fn fill_rule(&self) -> FillRule {
        self.fill_rule
    }

    /// Returns source-ordered clipping intersection operands.
    pub fn clips(&self) -> &[Clip] {
        &self.clips
    }
}

/// One clipping intersection operand containing unioned path areas.
#[derive(Clone, Debug, PartialEq)]
pub struct Clip {
    paths: Vec<ClipPath>,
    affected_by_object_transform: bool,
}

impl Clip {
    /// Returns path areas whose filled regions are unioned for this operand.
    pub fn paths(&self) -> &[ClipPath] {
        &self.paths
    }

    /// Returns whether the owning object's CTM affects this clip.
    pub fn affected_by_object_transform(&self) -> bool {
        self.affected_by_object_transform
    }
}

/// A validated fill-only path area used by a clipping operand.
#[derive(Clone, Debug, PartialEq)]
pub struct ClipPath {
    boundary: Rect,
    transform: Transform,
    area_transform: Transform,
    path_data: PathData,
    fill_rule: FillRule,
}

impl ClipPath {
    /// Returns the clip path boundary relative to its owning object.
    pub fn boundary(&self) -> Rect {
        self.boundary
    }

    /// Returns the clip path CTM, or identity when it was omitted.
    pub fn transform(&self) -> Transform {
        self.transform
    }

    /// Returns the containing Area CTM, or identity when it was omitted.
    pub fn area_transform(&self) -> Transform {
        self.area_transform
    }

    /// Returns the validated abbreviated clip path.
    pub fn path_data(&self) -> &PathData {
        &self.path_data
    }

    /// Returns the rule used to fill the clip path.
    pub fn fill_rule(&self) -> FillRule {
        self.fill_rule
    }
}

pub(crate) fn convert_layers(
    content: Option<raw::PageContent>,
    document: &crate::Document,
    limits: &ResourceLimits,
    path: &str,
    source: LayerSource,
) -> Result<(Vec<Layer>, ContentUsage)> {
    let Some(content) = content else {
        return Ok((Vec::new(), ContentUsage::default()));
    };
    let mut context = ConversionContext {
        limits,
        document,
        path,
        object_ids: HashSet::new(),
        next_synthetic_id: u64::MAX,
        vector_graphics: Vec::new(),
        remaining_path_commands: limits.max_path_commands,
        remaining_text_characters: limits.max_text_characters_per_page,
        remaining_glyphs: limits.max_glyphs_per_page,
        remaining_text_expansion_entries: limits.max_text_expansion_entries,
    };
    let mut layers = Vec::new();
    for layer in content.layers {
        let object_id = context.resolve_object_id(layer.id.as_deref())?;
        context.register_id(object_id)?;
        let kind = match layer.kind.as_deref() {
            None | Some("Body") => LayerType::Body,
            Some("Background") => LayerType::Background,
            Some("Foreground") => LayerType::Foreground,
            Some(value) => return Err(invalid_value("layer type", value)),
        };
        let objects = context.convert_objects(layer.objects)?;
        layers.push(Layer {
            actions: layer.actions,
            object_id,
            kind,
            source,
            objects,
        });
    }
    let usage = ContentUsage::from_layers(&layers);
    Ok((layers, usage))
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct ContentUsage {
    pub(crate) page_objects: usize,
    pub(crate) path_commands: usize,
    pub(crate) text_characters: usize,
    pub(crate) glyphs: usize,
    pub(crate) text_expansion_entries: usize,
}

impl ContentUsage {
    pub(crate) const fn template_reference() -> Self {
        Self {
            page_objects: 1,
            path_commands: 0,
            text_characters: 0,
            glyphs: 0,
            text_expansion_entries: 0,
        }
    }

    pub(crate) fn checked_add(self, other: Self) -> Option<Self> {
        Some(Self {
            page_objects: self.page_objects.checked_add(other.page_objects)?,
            path_commands: self.path_commands.checked_add(other.path_commands)?,
            text_characters: self.text_characters.checked_add(other.text_characters)?,
            glyphs: self.glyphs.checked_add(other.glyphs)?,
            text_expansion_entries: self
                .text_expansion_entries
                .checked_add(other.text_expansion_entries)?,
        })
    }

    fn from_layers(layers: &[Layer]) -> Self {
        let mut usage = Self::default();
        for layer in layers {
            usage.page_objects += 1;
            for object in &layer.objects {
                usage.add_object(object);
            }
        }
        usage
    }

    fn add_object(&mut self, object: &PageObject) {
        self.page_objects += 1;
        match object {
            PageObject::Path(path) => {
                self.path_commands += path.path_data.commands().len();
                for clip in &path.clips {
                    self.page_objects += 1;
                    for clip_path in &clip.paths {
                        self.page_objects += 2;
                        self.path_commands += clip_path.path_data.commands().len();
                    }
                }
            }
            PageObject::Group(group) => {
                for child in &group.objects {
                    self.add_object(child);
                }
            }
            PageObject::Text(text) => {
                let characters = text
                    .runs
                    .iter()
                    .map(|run| run.text.chars().count())
                    .sum::<usize>();
                self.text_characters += characters;
                let mapped_characters = text
                    .glyph_maps
                    .iter()
                    .map(|map| map.code_count)
                    .sum::<usize>();
                let mapped_glyphs = text
                    .glyph_maps
                    .iter()
                    .map(|map| map.glyphs.len())
                    .sum::<usize>();
                self.glyphs += characters - mapped_characters + mapped_glyphs;
                self.text_expansion_entries += text
                    .runs
                    .iter()
                    .map(|run| 1 + run.delta_x.len() + run.delta_y.len())
                    .sum::<usize>()
                    + text
                        .glyph_maps
                        .iter()
                        .map(|map| 1 + map.glyphs.len())
                        .sum::<usize>();
                self.add_clips(&text.clips);
            }
            PageObject::Image(image) => self.add_clips(&image.clips),
            PageObject::Composite(composite) => {
                for child in &composite.objects {
                    self.add_object(child);
                }
            }
            PageObject::Unsupported(_) => {}
        }
    }

    fn add_clips(&mut self, clips: &[Clip]) {
        for clip in clips {
            self.page_objects += 1;
            for clip_path in &clip.paths {
                self.page_objects += 2;
                self.path_commands += clip_path.path_data.commands().len();
            }
        }
    }
}

struct ConversionContext<'a> {
    limits: &'a ResourceLimits,
    document: &'a crate::Document,
    path: &'a str,
    object_ids: HashSet<u64>,
    next_synthetic_id: u64,
    vector_graphics: Vec<u64>,
    remaining_path_commands: usize,
    remaining_text_characters: usize,
    remaining_glyphs: usize,
    remaining_text_expansion_entries: usize,
}

impl ConversionContext<'_> {
    fn register_id(&mut self, id: u64) -> Result<()> {
        if self.object_ids.len() >= self.limits.max_page_objects {
            return Err(Error::LimitExceeded(format!(
                "page object count {} exceeds limit {}",
                self.object_ids.len().saturating_add(1),
                self.limits.max_page_objects
            )));
        }
        if !self.object_ids.insert(id) && self.document.strictness() == crate::Strictness::Strict {
            return Err(Error::InvalidStructure {
                path: self.path.to_owned(),
                message: format!("duplicate object ID {id}"),
            });
        }
        // Lenient: some producers reuse object IDs (several ofdrw converter
        // fixtures duplicate ID 15 inside one template); the ID only names
        // the object, so tolerate the collision and keep both objects.
        Ok(())
    }

    fn resolve_object_id(&mut self, value: Option<&str>) -> Result<u64> {
        match value {
            Some(value) => parse_object_id(value),
            None => {
                if self.document.strictness() == crate::Strictness::Strict {
                    return Err(Error::InvalidStructure {
                        path: self.path.to_owned(),
                        message: "page object ID is missing".to_owned(),
                    });
                }
                // Lenient: some producers omit object IDs (ofdrw's
                // converter/发票示例.ofd); synthesize unique IDs from the top
                // of the ID space, where real-world IDs never live.
                let id = self.next_synthetic_id;
                self.next_synthetic_id =
                    self.next_synthetic_id.checked_sub(1).ok_or_else(|| {
                        Error::LimitExceeded("synthetic object IDs exhausted".to_owned())
                    })?;
                Ok(id)
            }
        }
    }

    fn convert_objects(&mut self, objects: Vec<raw::GraphicUnit>) -> Result<Vec<PageObject>> {
        let mut converted = Vec::new();
        for object in objects {
            let object = match object {
                raw::GraphicUnit::Path(object) => {
                    let object_id = self.resolve_object_id(object.id.as_deref())?;
                    self.register_id(object_id)?;
                    let object = object.object.ok_or_else(|| Error::InvalidStructure {
                        path: self.path.to_owned(),
                        message: format!("PathObject {object_id} payload was not parsed"),
                    })?;
                    PageObject::Path(self.convert_path(*object, object_id)?)
                }
                raw::GraphicUnit::Group(group) => {
                    let object_id = self.resolve_object_id(group.id.as_deref())?;
                    self.register_id(object_id)?;
                    let objects = self.convert_objects(group.objects)?;
                    PageObject::Group(PageGroup {
                        actions: group.actions,
                        object_id,
                        objects,
                    })
                }
                raw::GraphicUnit::Text(object) => {
                    let object_id = self.resolve_object_id(object.id.as_deref())?;
                    self.register_id(object_id)?;
                    let object = object.object.ok_or_else(|| Error::InvalidStructure {
                        path: self.path.to_owned(),
                        message: format!("TextObject {object_id} payload was not parsed"),
                    })?;
                    PageObject::Text(self.convert_text(*object, object_id)?)
                }
                raw::GraphicUnit::Image(object) => {
                    let object_id = self.resolve_object_id(object.id.as_deref())?;
                    self.register_id(object_id)?;
                    let object = object.object.ok_or_else(|| Error::InvalidStructure {
                        path: self.path.to_owned(),
                        message: format!("ImageObject {object_id} payload was not parsed"),
                    })?;
                    if object.resource_id.is_none()
                        && self.document.strictness() != crate::Strictness::Strict
                    {
                        // Lenient: producers occasionally emit image objects
                        // without ResourceID (ofdrw's path_unstd.ofd page 2);
                        // ofdrw draws nothing for them, so skip the object.
                        continue;
                    }
                    PageObject::Image(self.convert_image(*object, object_id)?)
                }
                raw::GraphicUnit::Composite(object) => {
                    let object_id = self.resolve_object_id(object.id.as_deref())?;
                    self.register_id(object_id)?;
                    let strict = self.document.strictness() == crate::Strictness::Strict;
                    let boundary = match object.boundary.as_deref() {
                        Some(value) => parse_object_boundary(value, self.path, object_id, strict)?,
                        None if strict => {
                            return Err(object_error(
                                self.path,
                                object_id,
                                "Boundary",
                                "required attribute is missing".to_owned(),
                            ));
                        }
                        None => crate::Rect {
                            x: 0.0,
                            y: 0.0,
                            width: 0.0,
                            height: 0.0,
                        },
                    };
                    let transform =
                        parse_transform(object.transform.as_deref(), self.path, object_id)?;
                    let resource_id = parse_nonzero_id(
                        required_object_field(
                            object.resource_id.as_deref(),
                            "ResourceID",
                            self.path,
                            object_id,
                        )?,
                        "ResourceID",
                        self.path,
                        object_id,
                    )?;
                    self.require_resource_kind(
                        resource_id,
                        ResourceKind::VectorGraphic,
                        object_id,
                        "ResourceID",
                    )?;
                    if self.vector_graphics.contains(&resource_id) {
                        let mut cycle = self
                            .vector_graphics
                            .iter()
                            .map(u64::to_string)
                            .collect::<Vec<_>>();
                        cycle.push(resource_id.to_string());
                        return Err(Error::InvalidStructure {
                            path: self.path.to_owned(),
                            message: format!(
                                "vector graphic reference cycle: {}",
                                cycle.join(" -> ")
                            ),
                        });
                    }
                    if self.vector_graphics.len() >= self.limits.max_page_block_depth {
                        return Err(Error::LimitExceeded(format!(
                            "vector graphic reference depth exceeds limit {}",
                            self.limits.max_page_block_depth
                        )));
                    }
                    let content = self
                        .document
                        .vector_graphic_units(resource_id)
                        .map_err(|error| {
                            reference_error(error, self.path, object_id, "ResourceID")
                        })?
                        .to_vec();
                    self.vector_graphics.push(resource_id);
                    let objects = self.convert_objects(content);
                    self.vector_graphics.pop();
                    PageObject::Composite(CompositeObject {
                        actions: object.actions,
                        action_boundary: object.boundary.as_ref().map(|_| boundary),
                        content_actions: self.document.vector_graphic_actions(resource_id)?,
                        object_id,
                        boundary,
                        transform,
                        resource_id,
                        objects: objects?,
                    })
                }
            };
            converted.push(object);
        }
        Ok(converted)
    }

    fn convert_path(&mut self, path: raw::PathObject, object_id: u64) -> Result<PathObject> {
        let strict = self.document.strictness() == crate::Strictness::Strict;
        let boundary = match path.boundary.as_deref() {
            Some(value) => Rect::parse(value).map_err(|_| invalid_value("boundary", value))?,
            None if strict => {
                return Err(object_error(
                    self.path,
                    object_id,
                    "Boundary",
                    "required attribute is missing".to_owned(),
                ));
            }
            // Lenient: ofdrw draws paths without Boundary at the current
            // origin (converter/intro-数科.ofd pages 8-9).
            None => crate::Rect {
                x: 0.0,
                y: 0.0,
                width: 0.0,
                height: 0.0,
            },
        };
        if strict && (boundary.width <= 0.0 || boundary.height <= 0.0) {
            return Err(invalid_value(
                "boundary",
                path.boundary.as_deref().unwrap_or_default(),
            ));
        }
        let transform = path
            .transform
            .as_deref()
            .map(Transform::parse)
            .transpose()?
            .unwrap_or(Transform::IDENTITY);
        let stroke_enabled = parse_bool(path.stroke.as_deref(), true, "stroke")?;
        let fill_enabled = parse_bool(path.fill.as_deref(), false, "fill")?;
        let object_alpha = parse_alpha(path.alpha.as_deref())?;
        let mut parameters = self.resolve_draw_param(path.draw_param.as_deref(), object_id)?;
        apply_local_stroke_style(
            &mut parameters,
            path.line_width.as_deref(),
            path.line_join.as_deref(),
            path.line_cap.as_deref(),
            path.dash_offset.as_deref(),
            path.dash_pattern.as_deref(),
            path.miter_limit.as_deref(),
            self.path,
            object_id,
        )?;
        let stroke = if stroke_enabled {
            effective_paint_color(
                self.document,
                path.stroke_color.as_ref(),
                parameters.stroke_color,
                Color::BLACK,
                object_alpha,
                strict,
                self.path,
                object_id,
                "StrokeColor",
            )?
        } else {
            None
        };
        let fill = if fill_enabled {
            effective_paint_color(
                self.document,
                path.fill_color.as_ref(),
                parameters.fill_color,
                Color {
                    alpha: 0,
                    ..Color::BLACK
                },
                object_alpha,
                strict,
                self.path,
                object_id,
                "FillColor",
            )?
        } else {
            None
        };
        let stroke_style = parameters.stroke_style();
        let line_width = stroke_style.line_width();
        let fill_rule = match path.fill_rule.as_deref() {
            None | Some("NonZero") => FillRule::NonZero,
            Some("Even-Odd") => FillRule::EvenOdd,
            Some(value) => return Err(invalid_value("fill rule", value)),
        };
        let path_data =
            PathData::parse_with_limit(&path.abbreviated_data, self.remaining_path_commands)?;
        self.remaining_path_commands = self
            .remaining_path_commands
            .checked_sub(path_data.commands().len())
            .ok_or_else(|| Error::LimitExceeded("page path command budget exhausted".to_owned()))?;
        let clips = self.convert_clips(path.clips, object_id)?;

        Ok(PathObject {
            actions: path.actions,
            action_boundary: path.boundary.as_ref().map(|_| boundary),
            object_id,
            boundary,
            transform,
            path_data,
            stroke,
            fill,
            line_width,
            stroke_style,
            fill_rule,
            clips,
        })
    }

    fn resolve_draw_param(&self, value: Option<&str>, object_id: u64) -> Result<PaintParameters> {
        let Some(value) = value else {
            return Ok(PaintParameters::default());
        };
        let id = parse_nonzero_id(value, "DrawParam", self.path, object_id)?;
        self.document
            .draw_param(id)
            .map_err(|error| reference_error(error, self.path, object_id, "DrawParam"))
    }

    fn convert_text(&mut self, text: raw::TextObject, object_id: u64) -> Result<TextObject> {
        let boundary = parse_object_boundary(
            required_object_field(text.boundary.as_deref(), "Boundary", self.path, object_id)?,
            self.path,
            object_id,
            self.document.strictness() == crate::Strictness::Strict,
        )?;
        let transform = parse_transform(text.transform.as_deref(), self.path, object_id)?;
        let font_id = parse_nonzero_id(
            required_object_field(text.font.as_deref(), "Font", self.path, object_id)?,
            "Font",
            self.path,
            object_id,
        )?;
        self.require_resource_kind(font_id, ResourceKind::Font, object_id, "Font")?;
        let font_size = parse_positive_number(
            required_object_field(text.size.as_deref(), "Size", self.path, object_id)?,
            "Size",
            self.path,
            object_id,
        )?;
        let stroke_enabled = parse_object_bool(
            text.stroke.as_deref(),
            false,
            "Stroke",
            self.path,
            object_id,
        )?;
        let fill_enabled =
            parse_object_bool(text.fill.as_deref(), true, "Fill", self.path, object_id)?;
        let object_alpha =
            parse_object_alpha(text.alpha.as_deref(), self.path, object_id, "Alpha")?;
        let mut parameters = self.resolve_draw_param(text.draw_param.as_deref(), object_id)?;
        apply_local_stroke_style(
            &mut parameters,
            text.line_width.as_deref(),
            text.line_join.as_deref(),
            text.line_cap.as_deref(),
            text.dash_offset.as_deref(),
            text.dash_pattern.as_deref(),
            text.miter_limit.as_deref(),
            self.path,
            object_id,
        )?;

        if text.text_codes.is_empty() {
            return Err(object_error(
                self.path,
                object_id,
                "TextCode",
                "at least one text run is required".to_owned(),
            ));
        }

        let strict = self.document.strictness() == crate::Strictness::Strict;
        let stroke = if stroke_enabled {
            effective_paint_color(
                self.document,
                text.stroke_color.as_ref(),
                parameters.stroke_color,
                Color::BLACK,
                object_alpha,
                strict,
                self.path,
                object_id,
                "StrokeColor",
            )?
        } else {
            None
        };
        let fill = if fill_enabled {
            effective_paint_color(
                self.document,
                text.fill_color.as_ref(),
                parameters.fill_color,
                Color::BLACK,
                object_alpha,
                strict,
                self.path,
                object_id,
                "FillColor",
            )?
        } else {
            None
        };
        let clips = self.convert_clips(text.clips, object_id)?;
        let runs = self.convert_text_runs(text.text_codes, object_id)?;
        let character_count = runs
            .iter()
            .map(|run| run.text.chars().count())
            .sum::<usize>();
        let glyph_maps = self.convert_glyph_maps(text.cg_transforms, character_count, object_id)?;
        let mapped_characters = glyph_maps
            .iter()
            .try_fold(0usize, |total, map| total.checked_add(map.code_count))
            .ok_or_else(|| Error::LimitExceeded("mapped character count overflow".to_owned()))?;
        let mapped_glyphs = glyph_maps
            .iter()
            .try_fold(0usize, |total, map| total.checked_add(map.glyphs.len()))
            .ok_or_else(|| Error::LimitExceeded("mapped glyph count overflow".to_owned()))?;
        let glyph_count = character_count
            .checked_sub(mapped_characters)
            .and_then(|count| count.checked_add(mapped_glyphs))
            .ok_or_else(|| Error::LimitExceeded("effective glyph count overflow".to_owned()))?;
        consume(&mut self.remaining_glyphs, glyph_count, "page glyph")?;

        Ok(TextObject {
            actions: text.actions,
            object_id,
            boundary,
            transform,
            font_id,
            font_size,
            stroke,
            fill,
            stroke_style: parameters.stroke_style(),
            clips,
            runs,
            glyph_maps,
        })
    }

    fn convert_text_runs(
        &mut self,
        raw_runs: Vec<raw::TextCode>,
        object_id: u64,
    ) -> Result<Vec<TextCode>> {
        let mut runs = Vec::with_capacity(raw_runs.len());
        let mut inherited_x = None;
        let mut inherited_y = None;
        for (index, run) in raw_runs.into_iter().enumerate() {
            consume(
                &mut self.remaining_text_expansion_entries,
                1,
                "page text run",
            )?;
            let explicit_x =
                parse_optional_finite(run.x.as_deref(), "TextCode.X", self.path, object_id)?;
            let explicit_y =
                parse_optional_finite(run.y.as_deref(), "TextCode.Y", self.path, object_id)?;
            if index == 0 && (explicit_x.is_none() || explicit_y.is_none()) {
                if self.document.strictness() == crate::Strictness::Strict {
                    return Err(object_error(
                        self.path,
                        object_id,
                        "TextCode origin",
                        "the first run must specify both X and Y; later runs inherit each omitted coordinate".to_owned(),
                    ));
                }
                // Lenient: real-world producers omit a coordinate on the
                // first run (e.g. ofdrw's 发票监制章-数科.ofd has only X="0");
                // default it to 0 like ofdrw's ST_Base deserialization does.
                inherited_x.get_or_insert(0.0);
                inherited_y.get_or_insert(0.0);
            }
            if explicit_x.is_some() {
                inherited_x = explicit_x;
            }
            if explicit_y.is_some() {
                inherited_y = explicit_y;
            }
            let character_count = run.text.chars().count();
            consume(
                &mut self.remaining_text_characters,
                character_count,
                "page text character",
            )?;
            let delta_count = character_count;
            let has_delta_x = run
                .delta_x
                .as_deref()
                .is_some_and(|value| !value.trim().is_empty());
            let has_delta_y = run
                .delta_y
                .as_deref()
                .is_some_and(|value| !value.trim().is_empty());
            let strict = self.document.strictness() == crate::Strictness::Strict;
            let delta_x = parse_delta(
                run.delta_x.as_deref(),
                delta_count,
                &mut self.remaining_text_expansion_entries,
                self.path,
                object_id,
                "DeltaX",
                strict,
            )?;
            let delta_y = parse_delta(
                run.delta_y.as_deref(),
                delta_count,
                &mut self.remaining_text_expansion_entries,
                self.path,
                object_id,
                "DeltaY",
                strict,
            )?;
            runs.push(TextCode {
                text: run.text,
                x: inherited_x.expect("first run checked"),
                y: inherited_y.expect("first run checked"),
                delta_x,
                delta_y,
                has_delta_x,
                has_delta_y,
            });
        }
        Ok(runs)
    }

    fn convert_glyph_maps(
        &mut self,
        raw_maps: Vec<raw::CgTransform>,
        character_count: usize,
        object_id: u64,
    ) -> Result<Vec<CharacterGlyphMap>> {
        let mut maps = Vec::with_capacity(raw_maps.len());
        let mut ranges = BTreeMap::new();
        for raw in raw_maps {
            consume(
                &mut self.remaining_text_expansion_entries,
                1,
                "page glyph mapping",
            )?;
            let code_position = parse_usize(
                required_object_field(
                    raw.code_position.as_deref(),
                    "CodePosition",
                    self.path,
                    object_id,
                )?,
                "CodePosition",
                self.path,
                object_id,
                true,
            )?;
            let code_count = raw
                .code_count
                .as_deref()
                .map(|value| parse_usize(value, "CodeCount", self.path, object_id, false))
                .transpose()?
                .unwrap_or(1);
            let glyph_count = raw
                .glyph_count
                .as_deref()
                .map(|value| parse_usize(value, "GlyphCount", self.path, object_id, false))
                .transpose()?
                .unwrap_or(1);
            let glyphs_content = raw.glyphs.ok_or_else(|| {
                object_error(
                    self.path,
                    object_id,
                    "Glyphs",
                    "required child is missing".to_owned(),
                )
            })?;
            let (glyphs, transforms) = {
                let raw_glyphs = glyphs_content.children;
                let mut ids = Vec::new();
                let mut transforms = Vec::new();
                let mut text_parts: Vec<&str> = Vec::new();
                for entry in &raw_glyphs {
                    match entry {
                        raw::GlyphEntry::Text(text) => {
                            text_parts.extend(text.split_whitespace());
                        }
                        raw::GlyphEntry::Glyph(glyph) => {
                            let glyph_id = parse_u32(
                                required_object_field(
                                    glyph.glyph_id.as_deref(),
                                    "GlyphID",
                                    self.path,
                                    object_id,
                                )?,
                                "GlyphID",
                                self.path,
                                object_id,
                            )?;
                            ids.push(glyph_id);
                            let x = glyph
                                .x
                                .as_deref()
                                .map(|v| parse_finite_number(v, "Glyph.X", self.path, object_id))
                                .transpose()?
                                .unwrap_or(0.0);
                            let y = glyph
                                .y
                                .as_deref()
                                .map(|v| parse_finite_number(v, "Glyph.Y", self.path, object_id))
                                .transpose()?
                                .unwrap_or(0.0);
                            let matrix = parse_glyph_matrix(
                                glyph.m00.as_deref(),
                                glyph.m01.as_deref(),
                                glyph.m10.as_deref(),
                                glyph.m11.as_deref(),
                                self.path,
                                object_id,
                            )?;
                            transforms.push(Some(GlyphTransform::new(x, y, matrix)));
                        }
                    }
                }
                if ids.is_empty() {
                    // Legacy text form
                    let actual_glyph_count = text_parts.len();
                    if actual_glyph_count != glyph_count {
                        return Err(object_error(
                            self.path,
                            object_id,
                            "GlyphCount",
                            format!(
                                "declares {glyph_count} glyphs but Glyphs contains {actual_glyph_count}"
                            ),
                        ));
                    }
                    consume(
                        &mut self.remaining_text_expansion_entries,
                        actual_glyph_count,
                        "page text expansion",
                    )?;
                    let parsed_ids = text_parts
                        .iter()
                        .map(|value| {
                            value.parse::<u32>().map_err(|_| {
                                object_error(
                                    self.path,
                                    object_id,
                                    "Glyphs",
                                    format!("invalid glyph ID {value}"),
                                )
                            })
                        })
                        .collect::<Result<Vec<_>>>()?;
                    let parsed_transforms = vec![None; parsed_ids.len()];
                    (parsed_ids, parsed_transforms)
                } else {
                    // Structured Glyph form
                    let actual_glyph_count = ids.len();
                    if actual_glyph_count != glyph_count {
                        return Err(object_error(
                            self.path,
                            object_id,
                            "GlyphCount",
                            format!(
                                "declares {glyph_count} glyphs but Glyphs contains {actual_glyph_count} Glyph elements"
                            ),
                        ));
                    }
                    consume(
                        &mut self.remaining_text_expansion_entries,
                        actual_glyph_count,
                        "page text expansion",
                    )?;
                    (ids, transforms)
                }
            };
            let end = code_position.checked_add(code_count).ok_or_else(|| {
                object_error(
                    self.path,
                    object_id,
                    "CodeCount",
                    "range overflow".to_owned(),
                )
            })?;
            if end > character_count {
                return Err(object_error(
                    self.path,
                    object_id,
                    "CodePosition",
                    format!("range {code_position}..{end} exceeds {character_count} characters"),
                ));
            }
            if glyph_range_overlaps(&ranges, code_position, end) {
                return Err(object_error(
                    self.path,
                    object_id,
                    "CodePosition",
                    "CGTransform character ranges overlap".to_owned(),
                ));
            }
            ranges.insert(code_position, end);
            maps.push(CharacterGlyphMap {
                code_position,
                code_count,
                glyphs,
                transforms,
            });
        }
        Ok(maps)
    }

    fn convert_image(&mut self, image: raw::ImageObject, object_id: u64) -> Result<ImageObject> {
        let boundary = parse_object_boundary(
            required_object_field(image.boundary.as_deref(), "Boundary", self.path, object_id)?,
            self.path,
            object_id,
            self.document.strictness() == crate::Strictness::Strict,
        )?;
        let transform = parse_transform(image.transform.as_deref(), self.path, object_id)?;
        let resource_id = parse_nonzero_id(
            required_object_field(
                image.resource_id.as_deref(),
                "ResourceID",
                self.path,
                object_id,
            )?,
            "ResourceID",
            self.path,
            object_id,
        )?;
        let resource_format = self
            .document
            .image_resource_format(resource_id)
            .map_err(|error| reference_error(error, self.path, object_id, "ResourceID"))?;
        let substitution_id =
            self.optional_image_id(image.substitution.as_deref(), object_id, "Substitution")?;
        let image_mask_id =
            self.optional_image_id(image.image_mask.as_deref(), object_id, "ImageMask")?;
        let alpha = parse_object_alpha(image.alpha.as_deref(), self.path, object_id, "Alpha")?;
        let _ = self.resolve_draw_param(image.draw_param.as_deref(), object_id)?;
        let clips = self.convert_clips(image.clips, object_id)?;
        let border = image
            .border
            .map(|border| self.convert_image_border(border, object_id))
            .transpose()?;
        Ok(ImageObject {
            actions: image.actions,
            object_id,
            boundary,
            transform,
            resource_id,
            resource_format,
            alpha,
            clips,
            substitution_id,
            image_mask_id,
            border,
        })
    }

    fn convert_image_border(
        &self,
        border: raw::Border,
        object_id: u64,
    ) -> Result<crate::ImageBorder> {
        let strict = self.document.strictness() == crate::Strictness::Strict;
        let line_width = match border.line_width.as_deref() {
            None => 0.353,
            Some(value) => parse_finite_number(value, "Border.LineWidth", self.path, object_id)?,
        };
        if line_width < 0.0 {
            return Err(object_error(
                self.path,
                object_id,
                "Border.LineWidth",
                "must not be negative".to_owned(),
            ));
        }
        let corner = |value: Option<&str>, field: &'static str| -> Result<f64> {
            match value {
                None => Ok(0.0),
                Some(value) => {
                    let parsed = parse_finite_number(value, field, self.path, object_id)?;
                    if parsed < 0.0 {
                        return Err(object_error(
                            self.path,
                            object_id,
                            field,
                            "must not be negative".to_owned(),
                        ));
                    }
                    Ok(parsed)
                }
            }
        };
        let horizontal_corner_radius = corner(
            border.horizontal_corner_radius.as_deref(),
            "Border.HorizonalCornerRadius",
        )?;
        let vertical_corner_radius = corner(
            border.vertical_corner_radius.as_deref(),
            "Border.VerticalCornerRadius",
        )?;
        let mut parameters = crate::paint::PaintParameters::default();
        apply_local_stroke_style(
            &mut parameters,
            None,
            None,
            None,
            border.dash_offset.as_deref(),
            border.dash_pattern.as_deref(),
            None,
            self.path,
            object_id,
        )?;
        let color = match border.border_color.as_ref() {
            Some(color) => {
                match crate::content::effective_border_color(
                    self.document,
                    color,
                    strict,
                    self.path,
                    object_id,
                )? {
                    Some(color) => color,
                    // A BorderColor without Value paints nothing, but the
                    // border rectangle keeps its default black like ofdrw.
                    None => Color::BLACK,
                }
            }
            None => Color::BLACK,
        };
        Ok(crate::ImageBorder::new(
            line_width,
            horizontal_corner_radius,
            vertical_corner_radius,
            parameters.stroke_style(),
            color,
        ))
    }

    fn optional_image_id(
        &self,
        value: Option<&str>,
        object_id: u64,
        field: &'static str,
    ) -> Result<Option<u64>> {
        value
            .map(|value| {
                let id = parse_nonzero_id(value, field, self.path, object_id)?;
                self.require_resource_kind(id, ResourceKind::Image, object_id, field)?;
                Ok(id)
            })
            .transpose()
    }

    fn require_resource_kind(
        &self,
        id: u64,
        expected: ResourceKind,
        object_id: u64,
        field: &'static str,
    ) -> Result<()> {
        let actual = self
            .document
            .resource_kind(id)
            .map_err(|error| reference_error(error, self.path, object_id, field))?;
        if actual != expected {
            return Err(object_error(
                self.path,
                object_id,
                field,
                format!("resource {id} is {actual:?}, expected {expected:?}"),
            ));
        }
        Ok(())
    }

    fn convert_clips(&mut self, clips: Option<raw::Clips>, object_id: u64) -> Result<Vec<Clip>> {
        let Some(clips) = clips else {
            return Ok(Vec::new());
        };
        if clips.clips.is_empty() {
            // Invoice generators emit empty <Clips/> elements; in lenient mode
            // treat them as absent instead of rejecting the whole page.
            if self.document.strictness() == crate::Strictness::Strict {
                return Err(Error::InvalidStructure {
                    path: self.path.to_owned(),
                    message: "Clips must contain at least one Clip".to_owned(),
                });
            }
            return Ok(Vec::new());
        }
        let affected_by_object_transform = parse_object_bool(
            clips.trans_flag.as_deref(),
            false,
            "TransFlag",
            self.path,
            object_id,
        )?;
        clips
            .clips
            .into_iter()
            .filter_map(|clip| {
                self.convert_clip(clip, affected_by_object_transform, object_id)
                    .transpose()
            })
            .collect()
    }

    fn convert_clip(
        &mut self,
        clip: raw::Clip,
        affected_by_object_transform: bool,
        object_id: u64,
    ) -> Result<Option<Clip>> {
        if clip.areas.is_empty() {
            return Err(Error::InvalidStructure {
                path: self.path.to_owned(),
                message: "Clip must contain at least one Area".to_owned(),
            });
        }
        let strict = self.document.strictness() == crate::Strictness::Strict;
        let mut paths = Vec::with_capacity(clip.areas.len());
        for area in clip.areas {
            if area.children.len() != 1 {
                return Err(Error::InvalidStructure {
                    path: self.path.to_owned(),
                    message: "Area must contain exactly one Path or Text".to_owned(),
                });
            }
            let area_transform = area
                .transform
                .as_deref()
                .map(Transform::parse)
                .transpose()
                .map_err(|error| object_error(self.path, object_id, "Area.CTM", error.to_string()))?
                .unwrap_or(Transform::IDENTITY);
            let child = area.children.into_iter().next().expect("length checked");
            match child {
                raw::ClipAreaChild::Path(path) => {
                    paths.push(self.convert_clip_path(path, area_transform, object_id)?);
                }
                raw::ClipAreaChild::Text(_) => {
                    if strict {
                        return Err(Error::UnsupportedFeature(
                            "text clip areas are not supported".to_owned(),
                        ));
                    }
                }
            }
        }
        if paths.is_empty() {
            return Ok(None);
        }
        let fill_rule = paths[0].fill_rule;
        if paths.iter().any(|path| path.fill_rule != fill_rule) {
            return Err(Error::UnsupportedFeature(
                "mixed fill rules within one Clip are not supported in phase 2".to_owned(),
            ));
        }
        Ok(Some(Clip {
            paths,
            affected_by_object_transform,
        }))
    }

    fn convert_clip_path(
        &mut self,
        path: raw::ClipPath,
        area_transform: Transform,
        object_id: u64,
    ) -> Result<ClipPath> {
        let fill_enabled = parse_object_bool(
            path.fill.as_deref(),
            false,
            "Clip.Path.Fill",
            self.path,
            object_id,
        )?;
        let stroke_enabled = parse_object_bool(
            path.stroke.as_deref(),
            true,
            "Clip.Path.Stroke",
            self.path,
            object_id,
        )?;
        let strict = self.document.strictness() == crate::Strictness::Strict;
        if (!fill_enabled || stroke_enabled) && strict {
            return Err(Error::UnsupportedFeature(
                "clip paths must be fill-only (Fill=true and Stroke=false) in phase 2".to_owned(),
            ));
        }
        // Lenient: ofdrw's converter clips on the path geometry and ignores
        // the Fill/Stroke attributes entirely, so accept any combination.
        let boundary_value = required_object_field(
            path.boundary.as_deref(),
            "Clip.Path.Boundary",
            self.path,
            object_id,
        )?;
        let boundary = Rect::parse(boundary_value).map_err(|error| {
            object_error(
                self.path,
                object_id,
                "Clip.Path.Boundary",
                error.to_string(),
            )
        })?;
        if strict && (boundary.width <= 0.0 || boundary.height <= 0.0) {
            return Err(object_error(
                self.path,
                object_id,
                "Clip.Path.Boundary",
                "width and height must be positive".to_owned(),
            ));
        }
        let transform = path
            .transform
            .as_deref()
            .map(Transform::parse)
            .transpose()
            .map_err(|error| {
                object_error(self.path, object_id, "Clip.Path.CTM", error.to_string())
            })?
            .unwrap_or(Transform::IDENTITY);
        let fill_rule = match path.fill_rule.as_deref() {
            None | Some("NonZero") => FillRule::NonZero,
            Some("Even-Odd") => FillRule::EvenOdd,
            Some(value) => {
                return Err(object_error(
                    self.path,
                    object_id,
                    "Clip.Path.Rule",
                    format!("invalid fill rule {value}"),
                ))
            }
        };
        let abbreviated_data = required_object_field(
            path.abbreviated_data.as_deref(),
            "Clip.Path.AbbreviatedData",
            self.path,
            object_id,
        )?;
        let path_data = PathData::parse_with_limit(abbreviated_data, self.remaining_path_commands)
            .map_err(|error| match error {
                Error::LimitExceeded(_) => error,
                error => object_error(
                    self.path,
                    object_id,
                    "Clip.Path.AbbreviatedData",
                    error.to_string(),
                ),
            })?;
        self.remaining_path_commands = self
            .remaining_path_commands
            .checked_sub(path_data.commands().len())
            .ok_or_else(|| Error::LimitExceeded("page path command budget exhausted".to_owned()))?;
        Ok(ClipPath {
            boundary,
            transform,
            area_transform,
            path_data,
            fill_rule,
        })
    }
}

fn effective_border_color(
    document: &crate::Document,
    local: &raw::PaintColor,
    strict: bool,
    path: &str,
    object_id: u64,
) -> Result<Option<Color>> {
    if local.value.is_none() && local.index.is_none() {
        if strict {
            return Err(object_error(
                path,
                object_id,
                "BorderColor",
                "required attribute is missing".to_owned(),
            ));
        }
        return Ok(None);
    }
    document
        .resolve_paint_color(local, strict)
        .map_err(|error| object_error(path, object_id, "BorderColor", error.to_string()))
}

fn glyph_range_overlaps(ranges: &BTreeMap<usize, usize>, start: usize, end: usize) -> bool {
    let overlaps = |prior_start: usize, prior_end: usize| {
        #[cfg(test)]
        GLYPH_RANGE_NEIGHBOR_CHECKS.fetch_add(1, Ordering::Relaxed);
        start < prior_end && prior_start < end
    };
    ranges
        .range(..=start)
        .next_back()
        .is_some_and(|(&prior_start, &prior_end)| overlaps(prior_start, prior_end))
        || ranges
            .range(start..)
            .next()
            .is_some_and(|(&next_start, &next_end)| overlaps(next_start, next_end))
}

fn parse_bool(value: Option<&str>, default: bool, field: &'static str) -> Result<bool> {
    match value {
        None => Ok(default),
        Some("true" | "1") => Ok(true),
        Some("false" | "0") => Ok(false),
        Some(value) => Err(invalid_value(field, value)),
    }
}

fn parse_alpha(value: Option<&str>) -> Result<u8> {
    value
        .map(str::parse::<u8>)
        .transpose()
        .map_err(|_| invalid_value("alpha", value.unwrap_or_default()))
        .map(|alpha| alpha.unwrap_or(255))
}

fn required_object_field<'a>(
    value: Option<&'a str>,
    field: &'static str,
    path: &str,
    object_id: u64,
) -> Result<&'a str> {
    value.ok_or_else(|| {
        object_error(
            path,
            object_id,
            field,
            "required attribute is missing".to_owned(),
        )
    })
}

fn parse_object_bool(
    value: Option<&str>,
    default: bool,
    field: &'static str,
    path: &str,
    object_id: u64,
) -> Result<bool> {
    match value {
        None => Ok(default),
        Some("true" | "1") => Ok(true),
        Some("false" | "0") => Ok(false),
        Some(value) => Err(object_error(
            path,
            object_id,
            field,
            format!("invalid boolean {value}"),
        )),
    }
}

fn parse_object_alpha(
    value: Option<&str>,
    path: &str,
    object_id: u64,
    field: &'static str,
) -> Result<u8> {
    value
        .map(str::parse::<u8>)
        .transpose()
        .map_err(|_| {
            object_error(
                path,
                object_id,
                field,
                format!("invalid alpha {}", value.unwrap_or_default()),
            )
        })
        .map(|alpha| alpha.unwrap_or(255))
}

#[allow(clippy::too_many_arguments)]
fn effective_paint_color(
    document: &crate::Document,
    local: Option<&raw::PaintColor>,
    inherited: Option<Color>,
    default: Color,
    object_alpha: u8,
    strict: bool,
    path: &str,
    object_id: u64,
    field: &'static str,
) -> Result<Option<Color>> {
    let mut color = match local {
        Some(color) => {
            if color.value.is_none() && color.index.is_none() {
                if strict {
                    required_object_field(None, field, path, object_id)?;
                    unreachable!("required_object_field rejects None");
                }
                // Lenient: ofdrw paints nothing for a color element without a
                // Value attribute (gradient-only FillColor in ofdrw's
                // converter/intro-数科.ofd takes this path in OFD2IMG too).
                return Ok(None);
            }
            // A colour element that resolves to "no colour" paints nothing,
            // like the missing-Value case above.
            match document
                .resolve_paint_color(color, strict)
                .map_err(|error| object_error(path, object_id, field, error.to_string()))?
            {
                Some(color) => color,
                None => return Ok(None),
            }
        }
        None => inherited.unwrap_or(default),
    };
    color.alpha = ((u16::from(color.alpha) * u16::from(object_alpha) + 127) / 255) as u8;
    Ok(Some(color))
}

#[allow(clippy::too_many_arguments)]
fn apply_local_stroke_style(
    parameters: &mut PaintParameters,
    line_width: Option<&str>,
    line_join: Option<&str>,
    line_cap: Option<&str>,
    dash_offset: Option<&str>,
    dash_pattern: Option<&str>,
    miter_limit: Option<&str>,
    path: &str,
    object_id: u64,
) -> Result<()> {
    if let Some(value) = line_width {
        parameters.line_width = Some(parse_positive_number(value, "LineWidth", path, object_id)?);
    }
    if let Some(value) = line_join {
        parameters.line_join = Some(match value {
            "Miter" => LineJoin::Miter,
            "Round" => LineJoin::Round,
            "Bevel" => LineJoin::Bevel,
            _ => {
                return Err(object_error(
                    path,
                    object_id,
                    "Join",
                    format!("invalid value {value}"),
                ))
            }
        });
    }
    if let Some(value) = line_cap {
        parameters.line_cap = Some(match value {
            "Butt" => LineCap::Butt,
            "Round" => LineCap::Round,
            "Square" => LineCap::Square,
            _ => {
                return Err(object_error(
                    path,
                    object_id,
                    "Cap",
                    format!("invalid value {value}"),
                ))
            }
        });
    }
    if let Some(value) = dash_offset {
        let parsed = parse_finite_number(value, "DashOffset", path, object_id)?;
        if parsed < 0.0 {
            return Err(object_error(
                path,
                object_id,
                "DashOffset",
                "must be non-negative".to_owned(),
            ));
        }
        parameters.dash_offset = Some(parsed);
    }
    if let Some(value) = dash_pattern {
        let parsed = value
            .split_whitespace()
            .map(|item| parse_positive_number(item, "DashPattern", path, object_id))
            .collect::<Result<Vec<_>>>()?;
        if parsed.is_empty() {
            return Err(object_error(
                path,
                object_id,
                "DashPattern",
                "must not be empty".to_owned(),
            ));
        }
        parameters.dash_pattern = Some(parsed);
    }
    if let Some(value) = miter_limit {
        parameters.miter_limit = Some(parse_positive_number(value, "MiterLimit", path, object_id)?);
    }
    Ok(())
}

/// Parses an object boundary. Strict mode requires positive dimensions per
/// the specification; lenient mode tolerates zero or negative width/height,
/// which real-world producers emit for invisible or degenerate objects
/// (e.g. the ofdrw fixture keyword.ofd has `Boundary="269.5112 184.6262 0.529 0"`).
/// Non-finite values are always rejected by `Rect::parse`.
fn parse_object_boundary(value: &str, path: &str, object_id: u64, strict: bool) -> Result<Rect> {
    let boundary = Rect::parse(value)
        .map_err(|error| object_error(path, object_id, "Boundary", error.to_string()))?;
    if strict && (boundary.width <= 0.0 || boundary.height <= 0.0) {
        return Err(object_error(
            path,
            object_id,
            "Boundary",
            "width and height must be positive".to_owned(),
        ));
    }
    Ok(boundary)
}

fn parse_transform(value: Option<&str>, path: &str, object_id: u64) -> Result<Transform> {
    value
        .map(Transform::parse)
        .transpose()
        .map_err(|error| object_error(path, object_id, "CTM", error.to_string()))
        .map(|value| value.unwrap_or(Transform::IDENTITY))
}

fn parse_nonzero_id(value: &str, field: &'static str, path: &str, object_id: u64) -> Result<u64> {
    match value.parse::<u64>() {
        Ok(id) if id != 0 => Ok(id),
        _ => Err(object_error(
            path,
            object_id,
            field,
            format!("invalid nonzero ID {value}"),
        )),
    }
}

fn parse_finite_number(
    value: &str,
    field: &'static str,
    path: &str,
    object_id: u64,
) -> Result<f64> {
    let parsed = value
        .parse::<f64>()
        .map_err(|_| object_error(path, object_id, field, format!("invalid number {value}")))?;
    if !parsed.is_finite() {
        return Err(object_error(
            path,
            object_id,
            field,
            format!("non-finite number {value}"),
        ));
    }
    Ok(parsed)
}

fn parse_positive_number(
    value: &str,
    field: &'static str,
    path: &str,
    object_id: u64,
) -> Result<f64> {
    let parsed = parse_finite_number(value, field, path, object_id)?;
    if parsed <= 0.0 {
        return Err(object_error(
            path,
            object_id,
            field,
            "must be positive".to_owned(),
        ));
    }
    Ok(parsed)
}

fn parse_optional_finite(
    value: Option<&str>,
    field: &'static str,
    path: &str,
    object_id: u64,
) -> Result<Option<f64>> {
    value
        .map(|value| parse_finite_number(value, field, path, object_id))
        .transpose()
}

fn parse_usize(
    value: &str,
    field: &'static str,
    path: &str,
    object_id: u64,
    allow_zero: bool,
) -> Result<usize> {
    let parsed = value
        .parse::<usize>()
        .map_err(|_| object_error(path, object_id, field, format!("invalid integer {value}")))?;
    if !allow_zero && parsed == 0 {
        return Err(object_error(
            path,
            object_id,
            field,
            "must be positive".to_owned(),
        ));
    }
    Ok(parsed)
}

fn parse_u32(value: &str, field: &'static str, path: &str, object_id: u64) -> Result<u32> {
    value
        .parse::<u32>()
        .map_err(|_| object_error(path, object_id, field, format!("invalid integer {value}")))
}

/// Parses the optional M00/M01/M10/M11 glyph matrix attributes.
///
/// Returns `None` when all four are absent (identity).  When any are present,
/// all four must be finite numbers; the result is an affine `Transform` with
/// the translation components set to zero (the X/Y offsets are handled
/// separately).
fn parse_glyph_matrix(
    m00: Option<&str>,
    m01: Option<&str>,
    m10: Option<&str>,
    m11: Option<&str>,
    path: &str,
    object_id: u64,
) -> Result<Option<Transform>> {
    let (m00, m01, m10, m11) = match (m00, m01, m10, m11) {
        (None, None, None, None) => return Ok(None),
        _ => (
            m00.map(|v| parse_finite_number(v, "Glyph.M00", path, object_id))
                .transpose()?,
            m01.map(|v| parse_finite_number(v, "Glyph.M01", path, object_id))
                .transpose()?,
            m10.map(|v| parse_finite_number(v, "Glyph.M10", path, object_id))
                .transpose()?,
            m11.map(|v| parse_finite_number(v, "Glyph.M11", path, object_id))
                .transpose()?,
        ),
    };
    Ok(Some(Transform::new(
        m00.unwrap_or(1.0),
        m10.unwrap_or(0.0),
        m01.unwrap_or(0.0),
        m11.unwrap_or(1.0),
        0.0,
        0.0,
    )?))
}

fn parse_delta(
    value: Option<&str>,
    target_len: usize,
    remaining: &mut usize,
    path: &str,
    object_id: u64,
    field: &'static str,
    strict: bool,
) -> Result<Vec<f64>> {
    if target_len > *remaining {
        return Err(Error::LimitExceeded(format!(
            "page text expansion count exceeds limit while expanding object {object_id} {field} at {path}"
        )));
    }
    let mut values = Vec::with_capacity(target_len);
    let mut tokens = value.unwrap_or_default().split_whitespace();
    while let Some(token) = tokens.next() {
        if token == "g" {
            let count_text = tokens.next().ok_or_else(|| {
                object_error(
                    path,
                    object_id,
                    field,
                    "g repetition is missing its count".to_owned(),
                )
            })?;
            // Lenient mode tolerates `g 0` (a zero-length repetition), which
            // ofdrw-generated invoices such as 999.ofd emit; strict mode
            // still rejects it as non-positive.
            let count = parse_usize(count_text, field, path, object_id, !strict)?;
            let repetitions = count;
            let repeated_text = tokens.next().ok_or_else(|| {
                object_error(
                    path,
                    object_id,
                    field,
                    "g repetition is missing its value".to_owned(),
                )
            })?;
            let repeated = parse_finite_number(repeated_text, field, path, object_id)?;
            let new_len = values.len().checked_add(repetitions).ok_or_else(|| {
                object_error(
                    path,
                    object_id,
                    field,
                    "repetition count overflow".to_owned(),
                )
            })?;
            if new_len > target_len {
                if strict {
                    return Err(object_error(
                        path,
                        object_id,
                        field,
                        format!("contains more than {target_len} displacements"),
                    ));
                }
                // Lenient: producers sometimes pad the list past the
                // character count; ofdrw ignores the extras, so truncate.
                values.resize(target_len, repeated);
                break;
            }
            values.resize(new_len, repeated);
        } else {
            if values.len() == target_len {
                if strict {
                    return Err(object_error(
                        path,
                        object_id,
                        field,
                        format!("contains more than {target_len} displacements"),
                    ));
                }
                break;
            }
            values.push(parse_finite_number(token, field, path, object_id)?);
        }
    }
    values.resize(target_len, 0.0);
    *remaining -= target_len;
    Ok(values)
}

fn consume(remaining: &mut usize, amount: usize, label: &str) -> Result<()> {
    *remaining = remaining
        .checked_sub(amount)
        .ok_or_else(|| Error::LimitExceeded(format!("{label} limit exceeded")))?;
    Ok(())
}

fn object_error(path: &str, object_id: u64, field: &'static str, message: String) -> Error {
    Error::InvalidPageObject {
        path: path.to_owned(),
        object_id,
        field,
        message,
    }
}

fn reference_error(error: Error, path: &str, object_id: u64, field: &'static str) -> Error {
    match error {
        Error::UnknownResource { .. } | Error::ResourceKindMismatch { .. } => {
            object_error(path, object_id, field, error.to_string())
        }
        error => error,
    }
}

fn parse_object_id(value: &str) -> Result<u64> {
    match value.parse::<u64>() {
        Ok(id) if id != 0 => Ok(id),
        _ => Err(invalid_value("object ID", value)),
    }
}

fn invalid_value(field: &'static str, value: &str) -> Error {
    Error::InvalidValue {
        field,
        value: value.to_owned(),
        path: None,
    }
}

#[cfg(test)]
mod tests {
    use super::{glyph_range_overlaps, GLYPH_RANGE_NEIGHBOR_CHECKS};
    use std::sync::atomic::Ordering;

    #[test]
    fn glyph_range_overlap_checks_only_adjacent_intervals() {
        GLYPH_RANGE_NEIGHBOR_CHECKS.store(0, Ordering::Relaxed);
        let mut ranges = std::collections::BTreeMap::new();
        for start in 0..4_096 {
            assert!(!glyph_range_overlaps(&ranges, start, start + 1));
            ranges.insert(start, start + 1);
        }
        assert!(GLYPH_RANGE_NEIGHBOR_CHECKS.load(Ordering::Relaxed) <= 8_192);
    }
}
