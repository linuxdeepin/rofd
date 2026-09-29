use std::collections::HashMap;
use std::fmt;
use std::hash::{Hash, Hasher};
#[cfg(test)]
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use serde::Deserialize;

use crate::container::Container;
use crate::paint::ColorSpaceKind;
use crate::paint::PaintParameters;
use crate::path::PackagePath;
use crate::raw::{
    ColorSpaceEntry, CompositeGraphicUnit, DrawParamEntry, FontEntry, MultiMediaEntry, ResourceRoot,
};
use crate::{Color, Error, LineCap, LineJoin, ResourceLimits, Result};

/// A document resource category.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ResourceKind {
    /// A font resource.
    Font,
    /// An encoded raster image resource.
    Image,
    /// A reusable set of drawing parameters.
    DrawParam,
    /// A colour space resource with an optional indexed palette.
    ColorSpace,
    /// A reusable vector graphic (`CompositeGraphicUnit`) resource.
    VectorGraphic,
}

/// An encoded image format supported by the resource index.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ImageFormat {
    /// Portable Network Graphics.
    Png,
    /// JPEG image data.
    Jpeg,
    /// Windows bitmap image data.
    Bmp,
    /// Graphics Interchange Format image data.
    Gif,
    /// Tagged Image File Format image data.
    Tiff,
    /// JBIG2 bi-level image data (`JB2`, `GBIG2`, or `JBIG2` declarations).
    Jbig2,
}

/// Opaque process-local identity of one validated resource declaration.
///
/// Clones of a resource preserve this identity. Equal numeric OFD identifiers
/// from different documents receive distinct identities. The token has no
/// package-path or backend meaning and is suitable for constant-size cache keys.
#[derive(Clone)]
pub struct ResourceIdentity(Arc<ResourceIdentityMarker>);

#[derive(Debug)]
struct ResourceIdentityMarker {
    _nonce: u8,
}

impl ResourceIdentity {
    fn new() -> Self {
        Self(Arc::new(ResourceIdentityMarker { _nonce: 0 }))
    }
}

impl fmt::Debug for ResourceIdentity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ResourceIdentity(..)")
    }
}

impl PartialEq for ResourceIdentity {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for ResourceIdentity {}

impl Hash for ResourceIdentity {
    fn hash<H: Hasher>(&self, state: &mut H) {
        Arc::as_ptr(&self.0).hash(state);
    }
}

/// Immutable metadata and optional embedded bytes for an OFD font resource.
#[derive(Clone, Debug)]
pub struct FontResource {
    identity: ResourceIdentity,
    id: u64,
    font_name: String,
    family_name: Option<String>,
    charset: Option<String>,
    bytes: Option<Arc<[u8]>>,
    asset_path: Option<String>,
}

impl FontResource {
    /// Returns the opaque identity of this resource declaration.
    pub fn identity(&self) -> ResourceIdentity {
        self.identity.clone()
    }

    /// Returns the document-wide OFD object identifier.
    pub fn id(&self) -> u64 {
        self.id
    }

    /// Returns the declared font name.
    pub fn font_name(&self) -> &str {
        &self.font_name
    }

    /// Returns the declared font family name, when present.
    pub fn family_name(&self) -> Option<&str> {
        self.family_name.as_deref()
    }

    /// Returns the declared character set, when present.
    pub fn charset(&self) -> Option<&str> {
        self.charset.as_deref()
    }

    /// Returns the bounded encoded font file, when the catalog embeds one.
    pub fn encoded_bytes(&self) -> Option<&[u8]> {
        self.bytes.as_deref()
    }

    /// Returns a shared owner for the bounded encoded font file, when embedded.
    pub fn encoded_bytes_arc(&self) -> Option<Arc<[u8]>> {
        self.bytes.as_ref().map(Arc::clone)
    }

    /// Returns the safe package-local embedded font path, when declared.
    pub fn asset_path(&self) -> Option<&str> {
        self.asset_path.as_deref()
    }
}

/// Immutable metadata and encoded bytes for an OFD image resource.
#[derive(Clone, Debug)]
pub struct ImageResource {
    identity: ResourceIdentity,
    id: u64,
    format: ImageFormat,
    bytes: Arc<[u8]>,
    asset_path: String,
}

impl ImageResource {
    /// Returns the opaque identity of this resource declaration.
    pub fn identity(&self) -> ResourceIdentity {
        self.identity.clone()
    }

    /// Returns the document-wide OFD object identifier.
    pub fn id(&self) -> u64 {
        self.id
    }

    /// Returns the declared and supported encoded image format.
    pub fn format(&self) -> ImageFormat {
        self.format
    }

    /// Returns the bounded encoded image bytes without decoding them.
    pub fn encoded_bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Returns the safe package-local path of the encoded asset.
    pub fn asset_path(&self) -> &str {
        &self.asset_path
    }
}

#[derive(Debug)]
pub(crate) struct ResourceCatalog {
    entries: HashMap<u64, ResourceEntry>,
    strictness: crate::Strictness,
    draw_param_initialization: Mutex<()>,
    #[cfg(test)]
    draw_param_visits: AtomicUsize,
}

#[derive(Debug)]
enum ResourceEntry {
    Font(FontRecord),
    Image(ImageRecord),
    DrawParam(DrawParamRecord),
    ColorSpace(ColorSpaceRecord),
    VectorGraphic(VectorGraphicRecord),
}

/// A reusable vector graphic with its already-injected graphic-unit content.
///
/// The declared `Width`/`Height` are validated on insert but not retained:
/// like ofdrw, rendering positions content by the referencing object's
/// boundary and CTM instead of scaling to the declared size.
#[derive(Debug)]
struct VectorGraphicRecord {
    content: Vec<crate::raw::GraphicUnit>,
    actions: crate::navigation::deferred::Actions,
    declaration_path: String,
}

/// A colour space declaration with its optional palette entries.
///
/// Palette entries are retained as raw channel-array text and validated when
/// a colour actually references them.
#[derive(Debug)]
struct ColorSpaceRecord {
    kind: ColorSpaceKind,
    palette: Option<Vec<String>>,
    declaration_path: String,
}

#[derive(Debug)]
struct FontRecord {
    identity: ResourceIdentity,
    id: u64,
    font_name: String,
    family_name: Option<String>,
    charset: Option<String>,
    file: Option<Asset>,
    declaration_path: String,
}

#[derive(Debug)]
struct ImageRecord {
    identity: ResourceIdentity,
    id: u64,
    format: ImageFormat,
    file: Asset,
    declaration_path: String,
}

#[derive(Debug)]
struct DrawParamRecord {
    id: u64,
    relative: Option<u64>,
    /// Line geometry only; colours are retained raw because resolving them
    /// may reference colour spaces declared elsewhere in the catalog.
    values: PaintParameters,
    fill_color: Option<Box<crate::raw::PaintColor>>,
    stroke_color: Option<Box<crate::raw::PaintColor>>,
    declaration_path: String,
    resolved: OnceLock<PaintParameters>,
}

#[derive(Debug)]
struct Asset {
    path: PackagePath,
    bytes: OnceLock<Arc<[u8]>>,
    initialization: Mutex<()>,
}

impl Asset {
    fn new(path: PackagePath) -> Self {
        Self {
            path,
            bytes: OnceLock::new(),
            initialization: Mutex::new(()),
        }
    }

    fn load(&self, container: &Container, limit: u64, label: &str) -> Result<Arc<[u8]>> {
        self.load_with(|| container.read_with_limit(&self.path, limit, label))
    }

    fn load_with<F>(&self, loader: F) -> Result<Arc<[u8]>>
    where
        F: FnOnce() -> Result<Vec<u8>>,
    {
        if let Some(bytes) = self.bytes.get() {
            return Ok(Arc::clone(bytes));
        }
        let _initialization = self
            .initialization
            .lock()
            .map_err(|_| Error::Internal("asset initialization lock is poisoned".to_owned()))?;
        if let Some(bytes) = self.bytes.get() {
            return Ok(Arc::clone(bytes));
        }
        let bytes: Arc<[u8]> = loader()?.into();
        Ok(Arc::clone(self.bytes.get_or_init(|| Arc::clone(&bytes))))
    }
}

impl ResourceCatalog {
    pub(crate) fn empty() -> Self {
        Self {
            entries: HashMap::new(),
            strictness: crate::Strictness::Lenient,
            draw_param_initialization: Mutex::new(()),
            #[cfg(test)]
            draw_param_visits: AtomicUsize::new(0),
        }
    }

    pub(crate) fn load(
        container: &Container,
        paths: &[PackagePath],
        limits: &ResourceLimits,
        strictness: crate::Strictness,
    ) -> Result<(Self, Vec<(String, crate::document::SkippedGraphicUnit)>)> {
        let mut count = 0usize;
        let mut documents = Vec::with_capacity(paths.len());
        for path in paths {
            let bytes = container.read(path)?;
            preflight(&bytes, path, limits, &mut count)?;
            documents.push((path, bytes));
        }

        let lenient = strictness == crate::Strictness::Lenient;
        let mut skipped_units = Vec::new();
        let mut catalog = Self::empty();
        catalog.strictness = strictness;
        for (path, bytes) in documents {
            let mut actions = crate::navigation::deferred::extract(&bytes, path.as_str(), limits)?;
            let extracted =
                crate::document::extract_rich_objects(&actions.sanitized, path, lenient)?;
            for unit in extracted.skipped {
                skipped_units.push((path.as_str().to_owned(), unit));
            }
            let mut deserializer =
                serde_xml_rs::Deserializer::new_from_reader(extracted.sanitized.as_slice())
                    .non_contiguous_seq_elements(true);
            let mut root =
                ResourceRoot::deserialize(&mut deserializer).map_err(|error| Error::Xml {
                    path: path.as_str().to_owned(),
                    message: error.to_string(),
                })?;
            crate::document::inject_rich_objects_into_resource(&mut root, extracted.rich, path)?;
            actions.resources(&mut root);
            if strictness == crate::Strictness::Strict && root.fonts.len() > 1 {
                return Err(Error::InvalidStructure {
                    path: path.as_str().to_owned(),
                    message: "resource catalog declares duplicate Fonts elements".to_owned(),
                });
            }
            if strictness == crate::Strictness::Strict && root.multi_medias.len() > 1 {
                return Err(Error::InvalidStructure {
                    path: path.as_str().to_owned(),
                    message: "resource catalog declares duplicate MultiMedias elements".to_owned(),
                });
            }
            for font in root.fonts.into_iter().flat_map(|fonts| fonts.entries) {
                catalog.insert_font(font, &root.base_loc, path)?;
            }
            for image in root
                .multi_medias
                .into_iter()
                .flat_map(|images| images.entries)
            {
                catalog.insert_image(image, &root.base_loc, path)?;
            }
            for draw_param in root
                .draw_params
                .into_iter()
                .flat_map(|params| params.entries)
            {
                catalog.insert_draw_param(draw_param, path)?;
            }
            for color_space in root
                .color_spaces
                .into_iter()
                .flat_map(|spaces| spaces.entries)
            {
                catalog.insert_color_space(color_space, path)?;
            }
            for unit in root
                .composite_graphic_units
                .into_iter()
                .flat_map(|units| units.entries)
            {
                catalog.insert_vector_graphic(unit, path, strictness)?;
            }
        }
        Ok((catalog, skipped_units))
    }

    pub(crate) fn font(&self, id: u64, container: &Container, limit: u64) -> Result<FontResource> {
        match self.entries.get(&id) {
            Some(ResourceEntry::Font(font)) => Ok(FontResource {
                identity: font.identity.clone(),
                id: font.id,
                font_name: font.font_name.clone(),
                family_name: font.family_name.clone(),
                charset: font.charset.clone(),
                bytes: font
                    .file
                    .as_ref()
                    .map(|file| file.load(container, limit, "font resource"))
                    .transpose()?,
                asset_path: font.file.as_ref().map(|file| file.path.as_str().to_owned()),
            }),
            Some(ResourceEntry::Image(_)) => {
                Err(kind_mismatch(id, ResourceKind::Font, ResourceKind::Image))
            }
            Some(ResourceEntry::DrawParam(_)) => Err(kind_mismatch(
                id,
                ResourceKind::Font,
                ResourceKind::DrawParam,
            )),
            Some(ResourceEntry::ColorSpace(_)) => Err(kind_mismatch(
                id,
                ResourceKind::Font,
                ResourceKind::ColorSpace,
            )),
            Some(ResourceEntry::VectorGraphic(_)) => Err(kind_mismatch(
                id,
                ResourceKind::Font,
                ResourceKind::VectorGraphic,
            )),
            None => Err(Error::UnknownResource { object_id: id }),
        }
    }

    pub(crate) fn image(
        &self,
        id: u64,
        container: &Container,
        limit: u64,
    ) -> Result<ImageResource> {
        match self.entries.get(&id) {
            Some(ResourceEntry::Image(image)) => Ok(ImageResource {
                identity: image.identity.clone(),
                id: image.id,
                format: image.format,
                bytes: image.file.load(container, limit, "image resource")?,
                asset_path: image.file.path.as_str().to_owned(),
            }),
            Some(ResourceEntry::Font(_)) => {
                Err(kind_mismatch(id, ResourceKind::Image, ResourceKind::Font))
            }
            Some(ResourceEntry::DrawParam(_)) => Err(kind_mismatch(
                id,
                ResourceKind::Image,
                ResourceKind::DrawParam,
            )),
            Some(ResourceEntry::ColorSpace(_)) => Err(kind_mismatch(
                id,
                ResourceKind::Image,
                ResourceKind::ColorSpace,
            )),
            Some(ResourceEntry::VectorGraphic(_)) => Err(kind_mismatch(
                id,
                ResourceKind::Image,
                ResourceKind::VectorGraphic,
            )),
            None => Err(Error::UnknownResource { object_id: id }),
        }
    }

    fn insert_font(
        &mut self,
        entry: FontEntry,
        base_loc: &Option<String>,
        catalog_path: &PackagePath,
    ) -> Result<()> {
        let id = parse_id(&entry.id, catalog_path)?;
        let font_name = required(entry.font_name, "FontName", id, catalog_path)?;
        let file = entry
            .font_file
            .map(|value| {
                required(Some(value), "FontFile", id, catalog_path).and_then(|value| {
                    asset_path(catalog_path, base_loc.as_deref(), &value).map(Asset::new)
                })
            })
            .transpose()?;
        self.insert(
            id,
            ResourceEntry::Font(FontRecord {
                identity: ResourceIdentity::new(),
                id,
                font_name,
                family_name: entry.family_name,
                charset: entry.charset,
                file,
                declaration_path: catalog_path.as_str().to_owned(),
            }),
            catalog_path,
        )
    }

    fn insert_image(
        &mut self,
        entry: MultiMediaEntry,
        base_loc: &Option<String>,
        catalog_path: &PackagePath,
    ) -> Result<()> {
        let id = parse_id(&entry.id, catalog_path)?;
        let kind = required(entry.kind, "Type", id, catalog_path)?;
        if !kind.eq_ignore_ascii_case("image") {
            return Err(invalid_resource(
                catalog_path,
                Some(id),
                "Type",
                format!("expected Image, found {kind}"),
            ));
        }
        let media_file = required(entry.media_file, "MediaFile", id, catalog_path)?;
        let raw_format = match entry.format.filter(|format| !format.trim().is_empty()) {
            Some(format) => format,
            None => media_file
                .rsplit_once('.')
                .map(|(_, extension)| extension.to_owned())
                .filter(|extension| !extension.is_empty())
                .ok_or_else(|| {
                    invalid_resource(
                        catalog_path,
                        Some(id),
                        "Format",
                        "required value is missing".to_owned(),
                    )
                })?,
        };
        let format = match raw_format.to_ascii_lowercase().as_str() {
            "png" => ImageFormat::Png,
            "jpg" | "jpeg" => ImageFormat::Jpeg,
            "bmp" => ImageFormat::Bmp,
            "gif" => ImageFormat::Gif,
            "tif" | "tiff" => ImageFormat::Tiff,
            // JBIG2 is a valid OFD image format (ofdrw decodes it through a
            // JBIG2 ImageIO plugin); rofd-core indexes it so pages load, and
            // renderers report it as undecodable instead.
            "jb2" | "gbig2" | "jbig2" => ImageFormat::Jbig2,
            _ => {
                return Err(invalid_resource(
                    catalog_path,
                    Some(id),
                    "Format",
                    format!("unsupported encoded image format {raw_format}"),
                ))
            }
        };
        let path = asset_path(catalog_path, base_loc.as_deref(), &media_file)?;
        self.insert(
            id,
            ResourceEntry::Image(ImageRecord {
                identity: ResourceIdentity::new(),
                id,
                format,
                file: Asset::new(path),
                declaration_path: catalog_path.as_str().to_owned(),
            }),
            catalog_path,
        )
    }

    fn insert_draw_param(&mut self, entry: DrawParamEntry, path: &PackagePath) -> Result<()> {
        let id = parse_id(&entry.id, path)?;
        let relative = entry
            .relative
            .as_deref()
            .map(|value| parse_id(value, path))
            .transpose()?;
        let values = PaintParameters {
            fill_color: None,
            stroke_color: None,
            line_width: parse_positive(entry.line_width.as_deref(), "LineWidth", id, path)?,
            line_join: parse_join(entry.line_join.as_deref(), id, path)?,
            line_cap: parse_cap(entry.line_cap.as_deref(), id, path)?,
            dash_offset: parse_nonnegative(entry.dash_offset.as_deref(), "DashOffset", id, path)?,
            dash_pattern: parse_dash_pattern(entry.dash_pattern.as_deref(), id, path)?,
            miter_limit: parse_positive(entry.miter_limit.as_deref(), "MiterLimit", id, path)?,
        };
        self.insert(
            id,
            ResourceEntry::DrawParam(DrawParamRecord {
                id,
                relative,
                values,
                fill_color: entry.fill_color.map(Box::new),
                stroke_color: entry.stroke_color.map(Box::new),
                declaration_path: path.as_str().to_owned(),
                resolved: OnceLock::new(),
            }),
            path,
        )
    }

    fn insert_vector_graphic(
        &mut self,
        entry: CompositeGraphicUnit,
        path: &PackagePath,
        strictness: crate::Strictness,
    ) -> Result<()> {
        let id = parse_id(&entry.id, path)?;
        let dimension = |value: Option<&str>, field: &'static str| -> Result<Option<f64>> {
            match value {
                None => Ok(None),
                Some(value) => {
                    let parsed = value.parse::<f64>().map_err(|_| {
                        invalid_resource(path, Some(id), field, format!("invalid number {value}"))
                    })?;
                    if !parsed.is_finite() || parsed <= 0.0 {
                        return Err(invalid_resource(
                            path,
                            Some(id),
                            field,
                            format!("expected a positive finite number, found {value}"),
                        ));
                    }
                    Ok(Some(parsed))
                }
            }
        };
        let width = dimension(entry.width.as_deref(), "Width")?;
        let height = dimension(entry.height.as_deref(), "Height")?;
        if strictness == crate::Strictness::Strict {
            for (value, field) in [(width, "Width"), (height, "Height")] {
                if value.is_none() {
                    return Err(invalid_resource(
                        path,
                        Some(id),
                        field,
                        "required value is missing".to_owned(),
                    ));
                }
            }
        }
        let _ = (width, height);
        let actions = entry
            .content
            .as_ref()
            .and_then(|content| content.actions.clone());
        let content = entry
            .content
            .map(|content| content.objects)
            .unwrap_or_default();
        if strictness == crate::Strictness::Strict && content.is_empty() {
            return Err(invalid_resource(
                path,
                Some(id),
                "Content",
                "required child is missing".to_owned(),
            ));
        }
        self.insert(
            id,
            ResourceEntry::VectorGraphic(VectorGraphicRecord {
                content,
                actions,
                declaration_path: path.as_str().to_owned(),
            }),
            path,
        )
    }

    /// Returns the graphic units of one reusable vector graphic.
    pub(crate) fn vector_graphic_units(&self, id: u64) -> Result<&[crate::raw::GraphicUnit]> {
        match self.entries.get(&id) {
            Some(ResourceEntry::VectorGraphic(record)) => Ok(&record.content),
            Some(other) => Err(kind_mismatch(id, ResourceKind::VectorGraphic, other.kind())),
            None => Err(Error::UnknownResource { object_id: id }),
        }
    }

    pub(crate) fn vector_graphic_actions(
        &self,
        id: u64,
    ) -> Result<crate::navigation::deferred::Actions> {
        self.vector_graphic_units(id)?;
        Ok(match self.entries.get(&id) {
            Some(ResourceEntry::VectorGraphic(record)) => record.actions.clone(),
            _ => None,
        })
    }

    fn insert_color_space(&mut self, entry: ColorSpaceEntry, path: &PackagePath) -> Result<()> {
        let id = parse_id(&entry.id, path)?;
        let kind = match entry
            .kind
            .as_deref()
            .map(str::to_ascii_uppercase)
            .as_deref()
        {
            Some("GRAY") => ColorSpaceKind::Gray,
            Some("RGB") => ColorSpaceKind::Rgb,
            Some("CMYK") => ColorSpaceKind::Cmyk,
            Some(other) => {
                return Err(invalid_resource(
                    path,
                    Some(id),
                    "Type",
                    format!("unknown colour space type {other}"),
                ))
            }
            None => {
                return Err(invalid_resource(
                    path,
                    Some(id),
                    "Type",
                    "required value is missing".to_owned(),
                ))
            }
        };
        if let Some(bits) = entry.bits_per_component.as_deref() {
            if !matches!(bits, "1" | "2" | "4" | "8" | "16") {
                return Err(invalid_resource(
                    path,
                    Some(id),
                    "BitsPerComponent",
                    format!("invalid value {bits}"),
                ));
            }
        }
        self.insert(
            id,
            ResourceEntry::ColorSpace(ColorSpaceRecord {
                kind,
                palette: entry.palette.map(|palette| palette.colors),
                declaration_path: path.as_str().to_owned(),
            }),
            path,
        )
    }

    pub(crate) fn kind(&self, id: u64) -> Result<ResourceKind> {
        self.entries
            .get(&id)
            .map(ResourceEntry::kind)
            .ok_or(Error::UnknownResource { object_id: id })
    }

    pub(crate) fn image_format(&self, id: u64) -> Result<ImageFormat> {
        match self.entries.get(&id) {
            Some(ResourceEntry::Image(image)) => Ok(image.format),
            Some(other) => Err(kind_mismatch(id, ResourceKind::Image, other.kind())),
            None => Err(Error::UnknownResource { object_id: id }),
        }
    }

    pub(crate) fn draw_param(&self, id: u64) -> Result<PaintParameters> {
        let requested = match self.entries.get(&id) {
            Some(ResourceEntry::DrawParam(record)) => record,
            Some(other) => return Err(kind_mismatch(id, ResourceKind::DrawParam, other.kind())),
            None => return Err(Error::UnknownResource { object_id: id }),
        };
        if let Some(resolved) = requested.resolved.get() {
            return Ok(resolved.clone());
        }
        let _initialization = self
            .draw_param_initialization
            .lock()
            .map_err(|_| Error::Internal("DrawParam initialization lock is poisoned".to_owned()))?;
        if let Some(resolved) = requested.resolved.get() {
            return Ok(resolved.clone());
        }

        let mut positions = HashMap::new();
        let mut chain = Vec::new();
        let mut current = id;
        let mut resolved = None;
        loop {
            if let Some(position) = positions.insert(current, chain.len()) {
                let mut cycle = chain[position..]
                    .iter()
                    .map(|record: &&DrawParamRecord| record.id.to_string())
                    .collect::<Vec<_>>();
                cycle.push(current.to_string());
                return Err(Error::InvalidStructure {
                    path: self
                        .entries
                        .get(&current)
                        .map(ResourceEntry::declaration_path)
                        .unwrap_or("resource catalog")
                        .to_owned(),
                    message: format!("DrawParam Relative cycle: {}", cycle.join(" -> ")),
                });
            }
            let entry = self
                .entries
                .get(&current)
                .ok_or(Error::UnknownResource { object_id: current })?;
            let ResourceEntry::DrawParam(record) = entry else {
                return Err(kind_mismatch(
                    current,
                    ResourceKind::DrawParam,
                    entry.kind(),
                ));
            };
            #[cfg(test)]
            self.draw_param_visits.fetch_add(1, Ordering::Relaxed);
            debug_assert_eq!(record.id, current);
            if let Some(cached) = record.resolved.get() {
                resolved = Some(cached.clone());
                break;
            }
            chain.push(record);
            match record.relative {
                Some(relative) => current = relative,
                None => break,
            }
        }

        let strict = self.strictness == crate::Strictness::Strict;
        let mut resolved = resolved.unwrap_or_default();
        for record in chain.into_iter().rev() {
            resolved.inherit(&record.values);
            if let Some(color) = &record.fill_color {
                resolved.fill_color = self
                    .resolve_paint_color(color, strict)
                    .map_err(|error| Error::InvalidResource {
                        path: record.declaration_path.clone(),
                        object_id: Some(record.id),
                        field: "FillColor",
                        message: error.to_string(),
                    })?
                    .or(resolved.fill_color);
            }
            if let Some(color) = &record.stroke_color {
                resolved.stroke_color = self
                    .resolve_paint_color(color, strict)
                    .map_err(|error| Error::InvalidResource {
                        path: record.declaration_path.clone(),
                        object_id: Some(record.id),
                        field: "StrokeColor",
                        message: error.to_string(),
                    })?
                    .or(resolved.stroke_color);
            }
            record.resolved.get_or_init(|| resolved.clone());
        }
        Ok(resolved)
    }

    /// Resolves one raw colour element to an RGB colour.
    ///
    /// `Value` channel counts select the colour space (one channel is GRAY,
    /// three RGB, four CMYK) unless a `ColorSpace` reference names another
    /// space. An `Index` selects a palette entry from the referenced space.
    /// Both strictness modes follow ofdrw's defaults for missing references
    /// (RGB space, default colour) except that strict mode rejects them.
    pub(crate) fn resolve_paint_color(
        &self,
        color: &crate::raw::PaintColor,
        strict: bool,
    ) -> Result<Option<Color>> {
        let declared = match &color.color_space {
            Some(reference) => self
                .color_space(reference, strict)?
                .map(|record| (record.kind, record.palette.as_deref())),
            None => None,
        };
        if let Some(value) = color.value.as_deref() {
            let declared_space = declared.map(|(kind, _)| kind);
            return Color::parse_in_space(value, color.alpha.as_deref(), declared_space, strict)
                .map(Some);
        }
        let Some(index) = color.index.as_deref() else {
            return missing_value_or_index(strict);
        };
        let index = index.parse::<usize>().map_err(|_| Error::InvalidValue {
            field: "color index",
            value: index.to_owned(),
            path: None,
        })?;
        let invalid_index = |message: String| Error::InvalidValue {
            field: "color index",
            value: message,
            path: None,
        };
        let Some((_, Some(palette))) = declared else {
            return if strict {
                Err(invalid_index(
                    "Index requires a ColorSpace with a palette".to_owned(),
                ))
            } else {
                Ok(Some(default_color(color.alpha.as_deref())))
            };
        };
        match palette.get(index) {
            Some(entry) => {
                let space = declared.map(|(kind, _)| kind);
                Color::parse_in_space(entry, color.alpha.as_deref(), space, strict).map(Some)
            }
            None if strict => Err(invalid_index(format!(
                "palette has {} colours, Index {index} is out of range",
                palette.len()
            ))),
            None => Ok(Some(default_color(color.alpha.as_deref()))),
        }
    }

    fn color_space(&self, reference: &str, strict: bool) -> Result<Option<&ColorSpaceRecord>> {
        let lookup = |id: u64| match self.entries.get(&id) {
            Some(ResourceEntry::ColorSpace(record)) => Ok(Some(record)),
            Some(other) => {
                if strict {
                    Err(kind_mismatch(id, ResourceKind::ColorSpace, other.kind()))
                } else {
                    Ok(None)
                }
            }
            None => {
                if strict {
                    Err(Error::UnknownResource { object_id: id })
                } else {
                    Ok(None)
                }
            }
        };
        reference
            .parse::<u64>()
            .map_err(|_| Error::InvalidValue {
                field: "color space reference",
                value: reference.to_owned(),
                path: None,
            })
            .and_then(lookup)
    }

    fn insert(&mut self, id: u64, entry: ResourceEntry, path: &PackagePath) -> Result<()> {
        use std::collections::hash_map::Entry;

        match self.entries.entry(id) {
            Entry::Vacant(slot) => {
                slot.insert(entry);
                Ok(())
            }
            Entry::Occupied(slot) => Err(Error::DuplicateResourceId {
                object_id: id,
                first_path: slot.get().declaration_path().to_owned(),
                first_kind: slot.get().kind(),
                duplicate_path: path.as_str().to_owned(),
                duplicate_kind: entry.kind(),
            }),
        }
    }
}

impl ResourceEntry {
    fn kind(&self) -> ResourceKind {
        match self {
            Self::Font(_) => ResourceKind::Font,
            Self::Image(_) => ResourceKind::Image,
            Self::DrawParam(_) => ResourceKind::DrawParam,
            Self::ColorSpace(_) => ResourceKind::ColorSpace,
            Self::VectorGraphic(_) => ResourceKind::VectorGraphic,
        }
    }

    fn declaration_path(&self) -> &str {
        match self {
            Self::Font(font) => &font.declaration_path,
            Self::Image(image) => &image.declaration_path,
            Self::DrawParam(draw_param) => &draw_param.declaration_path,
            Self::ColorSpace(color_space) => &color_space.declaration_path,
            Self::VectorGraphic(vector_graphic) => &vector_graphic.declaration_path,
        }
    }
}

fn preflight(
    bytes: &[u8],
    path: &PackagePath,
    limits: &ResourceLimits,
    total: &mut usize,
) -> Result<()> {
    use xml::reader::{EventReader, XmlEvent};

    #[derive(Clone, Copy)]
    enum Marker {
        Res,
        Fonts,
        MultiMedias,
        DrawParams,
        ColorSpaces,
        CompositeGraphicUnits,
        Other,
    }

    let mut elements = Vec::new();
    for event in EventReader::new(bytes) {
        match event.map_err(|error| Error::Xml {
            path: path.as_str().to_owned(),
            message: error.to_string(),
        })? {
            XmlEvent::StartElement { name, .. } => {
                let depth = elements.len().checked_add(1).ok_or_else(|| {
                    Error::LimitExceeded("resource XML depth overflow".to_owned())
                })?;
                if depth > limits.max_xml_depth {
                    return Err(Error::LimitExceeded(format!(
                        "XML depth {depth} exceeds limit {}",
                        limits.max_xml_depth
                    )));
                }
                if elements.is_empty() && name.local_name != "Res" {
                    return Err(Error::InvalidStructure {
                        path: path.as_str().to_owned(),
                        message: format!(
                            "resource catalog root must be Res, found {}",
                            name.local_name
                        ),
                    });
                }
                let parent = elements.last().copied();
                let is_res = elements.is_empty() && name.local_name == "Res";
                let is_fonts = matches!(parent, Some(Marker::Res)) && name.local_name == "Fonts";
                let is_multi_medias =
                    matches!(parent, Some(Marker::Res)) && name.local_name == "MultiMedias";
                let is_draw_params =
                    matches!(parent, Some(Marker::Res)) && name.local_name == "DrawParams";
                let is_color_spaces =
                    matches!(parent, Some(Marker::Res)) && name.local_name == "ColorSpaces";
                let is_composite_graphic_units = matches!(parent, Some(Marker::Res))
                    && name.local_name == "CompositeGraphicUnits";
                let is_resource = (matches!(parent, Some(Marker::Fonts))
                    && name.local_name == "Font")
                    || (matches!(parent, Some(Marker::MultiMedias))
                        && name.local_name == "MultiMedia")
                    || (matches!(parent, Some(Marker::DrawParams))
                        && name.local_name == "DrawParam")
                    || (matches!(parent, Some(Marker::ColorSpaces))
                        && name.local_name == "ColorSpace")
                    || (matches!(parent, Some(Marker::CompositeGraphicUnits))
                        && name.local_name == "CompositeGraphicUnit");
                if is_resource {
                    *total = total.checked_add(1).ok_or_else(|| {
                        Error::LimitExceeded("resource count overflow".to_owned())
                    })?;
                    if *total > limits.max_resources {
                        return Err(Error::LimitExceeded(format!(
                            "resource count {} exceeds limit {}",
                            *total, limits.max_resources
                        )));
                    }
                }
                elements.push(if is_res {
                    Marker::Res
                } else if is_fonts {
                    Marker::Fonts
                } else if is_multi_medias {
                    Marker::MultiMedias
                } else if is_draw_params {
                    Marker::DrawParams
                } else if is_color_spaces {
                    Marker::ColorSpaces
                } else if is_composite_graphic_units {
                    Marker::CompositeGraphicUnits
                } else {
                    Marker::Other
                });
            }
            XmlEvent::EndElement { .. } => {
                elements.pop();
            }
            _ => {}
        }
    }
    Ok(())
}

fn parse_positive(
    value: Option<&str>,
    field: &'static str,
    id: u64,
    path: &PackagePath,
) -> Result<Option<f64>> {
    value
        .map(|value| {
            let parsed = value.parse::<f64>().map_err(|_| {
                invalid_resource(path, Some(id), field, format!("invalid number {value}"))
            })?;
            if !parsed.is_finite() || parsed <= 0.0 {
                return Err(invalid_resource(
                    path,
                    Some(id),
                    field,
                    format!("expected a positive finite number, found {value}"),
                ));
            }
            Ok(parsed)
        })
        .transpose()
}

fn parse_nonnegative(
    value: Option<&str>,
    field: &'static str,
    id: u64,
    path: &PackagePath,
) -> Result<Option<f64>> {
    value
        .map(|value| {
            let parsed = value.parse::<f64>().map_err(|_| {
                invalid_resource(path, Some(id), field, format!("invalid number {value}"))
            })?;
            if !parsed.is_finite() || parsed < 0.0 {
                return Err(invalid_resource(
                    path,
                    Some(id),
                    field,
                    format!("expected a non-negative finite number, found {value}"),
                ));
            }
            Ok(parsed)
        })
        .transpose()
}

fn parse_join(value: Option<&str>, id: u64, path: &PackagePath) -> Result<Option<LineJoin>> {
    value
        .map(|value| match value {
            "Miter" => Ok(LineJoin::Miter),
            "Round" => Ok(LineJoin::Round),
            "Bevel" => Ok(LineJoin::Bevel),
            _ => Err(invalid_resource(
                path,
                Some(id),
                "Join",
                format!("invalid value {value}"),
            )),
        })
        .transpose()
}

fn parse_cap(value: Option<&str>, id: u64, path: &PackagePath) -> Result<Option<LineCap>> {
    value
        .map(|value| match value {
            "Butt" => Ok(LineCap::Butt),
            "Round" => Ok(LineCap::Round),
            "Square" => Ok(LineCap::Square),
            _ => Err(invalid_resource(
                path,
                Some(id),
                "Cap",
                format!("invalid value {value}"),
            )),
        })
        .transpose()
}

fn parse_dash_pattern(
    value: Option<&str>,
    id: u64,
    path: &PackagePath,
) -> Result<Option<Vec<f64>>> {
    value
        .map(|value| {
            let values = value
                .split_whitespace()
                .map(|item| {
                    let parsed = item.parse::<f64>().map_err(|_| {
                        invalid_resource(
                            path,
                            Some(id),
                            "DashPattern",
                            format!("invalid number {item}"),
                        )
                    })?;
                    if !parsed.is_finite() || parsed <= 0.0 {
                        return Err(invalid_resource(
                            path,
                            Some(id),
                            "DashPattern",
                            format!("expected positive finite values, found {item}"),
                        ));
                    }
                    Ok(parsed)
                })
                .collect::<Result<Vec<_>>>()?;
            if values.is_empty() {
                return Err(invalid_resource(
                    path,
                    Some(id),
                    "DashPattern",
                    "pattern must not be empty".to_owned(),
                ));
            }
            Ok(values)
        })
        .transpose()
}

/// Resolves a colour that references no colour-space resource.
///
/// Kept separate from [`ResourceCatalog::resolve_paint_color`] so pages whose
/// declared resource files are missing still resolve plain channel values,
/// like they did before colour spaces existed (ofdrw's V4RideRight.ofd and
/// h.ofd declare a PublicRes.xml their packages omit).
pub(crate) fn resolve_plain_color(
    color: &crate::raw::PaintColor,
    strict: bool,
) -> Result<Option<Color>> {
    let Some(value) = color.value.as_deref() else {
        return missing_value_or_index(strict);
    };
    Color::parse_in_space(value, color.alpha.as_deref(), None, strict).map(Some)
}

fn missing_value_or_index(strict: bool) -> Result<Option<Color>> {
    // Neither Value nor Index: standard-defined default is all channels
    // zero, but ofdrw's converters paint nothing for such elements, so
    // lenient mode reports "no colour".
    if strict {
        Err(Error::InvalidValue {
            field: "color",
            value: "Value or Index is required".to_owned(),
            path: None,
        })
    } else {
        Ok(None)
    }
}

fn default_color(alpha: Option<&str>) -> Color {
    let alpha = alpha
        .and_then(|alpha| alpha.parse::<u8>().ok())
        .unwrap_or(255);
    Color {
        alpha,
        ..Color::BLACK
    }
}

fn parse_id(value: &str, path: &PackagePath) -> Result<u64> {
    let id = value.parse::<u64>().map_err(|_| Error::InvalidValue {
        field: "resource ID",
        value: value.to_owned(),
        path: Some(path.as_str().to_owned()),
    })?;
    if id == 0 {
        return Err(Error::InvalidValue {
            field: "resource ID",
            value: value.to_owned(),
            path: Some(path.as_str().to_owned()),
        });
    }
    Ok(id)
}

fn required(
    value: Option<String>,
    field: &'static str,
    id: u64,
    path: &PackagePath,
) -> Result<String> {
    value
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            invalid_resource(
                path,
                Some(id),
                field,
                "required value is missing".to_owned(),
            )
        })
}

fn asset_path(path: &PackagePath, base_loc: Option<&str>, value: &str) -> Result<PackagePath> {
    let resolved = match base_loc {
        Some(base) if base.starts_with('/') => {
            let root = base.trim_start_matches('/');
            let combined = if root.is_empty() {
                value.to_owned()
            } else {
                format!("{root}/{value}")
            };
            PackagePath::new(&combined)
        }
        Some(base) if !base.is_empty() => path.resolve(&format!("{base}/{value}")),
        _ => path.resolve(value),
    };
    resolved.map_err(|error| match error {
        Error::InvalidValue {
            field,
            value,
            path: None,
        } => Error::InvalidValue {
            field,
            value,
            path: Some(path.as_str().to_owned()),
        },
        error => error,
    })
}

fn invalid_resource(
    path: &PackagePath,
    object_id: Option<u64>,
    field: &'static str,
    message: String,
) -> Error {
    Error::InvalidResource {
        path: path.as_str().to_owned(),
        object_id,
        field,
        message,
    }
}

fn kind_mismatch(id: u64, expected: ResourceKind, actual: ResourceKind) -> Error {
    Error::ResourceKindMismatch {
        object_id: id,
        expected,
        actual,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Barrier, OnceLock};

    use super::{asset_path, Asset, DrawParamRecord, ResourceCatalog, ResourceEntry};
    use crate::paint::PaintParameters;
    use crate::path::PackagePath;
    use crate::raw::MultiMediaEntry;
    use crate::{Error, ImageFormat, Result};

    fn draw_param_catalog(length: u64) -> ResourceCatalog {
        let mut catalog = ResourceCatalog::empty();
        for id in 1..=length {
            catalog.entries.insert(
                id,
                ResourceEntry::DrawParam(DrawParamRecord {
                    id,
                    relative: (id > 1).then_some(id - 1),
                    values: PaintParameters {
                        line_width: Some(id as f64),
                        ..PaintParameters::default()
                    },
                    fill_color: None,
                    stroke_color: None,
                    declaration_path: "Doc_0/Res.xml".to_owned(),
                    resolved: OnceLock::new(),
                }),
            );
        }
        catalog
    }

    #[test]
    fn concurrent_cold_asset_loads_are_single_flight() {
        let asset = Arc::new(Asset::new(PackagePath::new("asset").unwrap()));
        let barrier = Arc::new(Barrier::new(8));
        let calls = Arc::new(AtomicUsize::new(0));
        let handles = (0..8)
            .map(|_| {
                let asset = Arc::clone(&asset);
                let barrier = Arc::clone(&barrier);
                let calls = Arc::clone(&calls);
                std::thread::spawn(move || {
                    barrier.wait();
                    asset
                        .load_with(|| {
                            calls.fetch_add(1, Ordering::SeqCst);
                            Ok(b"font".to_vec())
                        })
                        .unwrap()
                })
            })
            .collect::<Vec<_>>();

        let results = handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(results
            .iter()
            .skip(1)
            .all(|bytes| Arc::ptr_eq(&results[0], bytes)));
    }

    fn image_entry(id: &str, format: Option<&str>, media_file: &str) -> MultiMediaEntry {
        MultiMediaEntry {
            id: id.to_owned(),
            kind: Some("Image".to_owned()),
            format: format.map(str::to_owned),
            media_file: Some(media_file.to_owned()),
        }
    }

    #[test]
    fn image_format_is_inferred_from_media_file_extension_when_format_missing() {
        let mut catalog = ResourceCatalog::empty();
        let path = PackagePath::new("Doc_0/DocumentRes.xml").unwrap();
        catalog
            .insert_image(image_entry("1", None, "image_1.PNG"), &None, &path)
            .unwrap();
        catalog
            .insert_image(image_entry("2", None, "image_2.jpeg"), &None, &path)
            .unwrap();
        assert_eq!(catalog.image_format(1).unwrap(), ImageFormat::Png);
        assert_eq!(catalog.image_format(2).unwrap(), ImageFormat::Jpeg);
    }

    #[test]
    fn explicit_image_format_takes_precedence_over_media_file_extension() {
        let mut catalog = ResourceCatalog::empty();
        let path = PackagePath::new("Doc_0/DocumentRes.xml").unwrap();
        catalog
            .insert_image(image_entry("1", Some("png"), "image_1.dat"), &None, &path)
            .unwrap();
        assert_eq!(catalog.image_format(1).unwrap(), ImageFormat::Png);
    }

    #[test]
    fn missing_format_with_unrecognized_extension_is_rejected() {
        let mut catalog = ResourceCatalog::empty();
        let path = PackagePath::new("Doc_0/DocumentRes.xml").unwrap();
        let error = catalog
            .insert_image(image_entry("1", None, "image_1.xyz"), &None, &path)
            .unwrap_err();
        assert!(matches!(
            error,
            Error::InvalidResource {
                field: "Format",
                ..
            }
        ));
    }

    #[test]
    fn bmp_gif_and_tiff_formats_are_accepted() {
        let mut catalog = ResourceCatalog::empty();
        let path = PackagePath::new("Doc_0/DocumentRes.xml").unwrap();
        catalog
            .insert_image(image_entry("1", Some("bmp"), "image_1.dat"), &None, &path)
            .unwrap();
        catalog
            .insert_image(image_entry("2", Some("GIF"), "image_2.dat"), &None, &path)
            .unwrap();
        catalog
            .insert_image(image_entry("3", Some("tiff"), "image_3.dat"), &None, &path)
            .unwrap();
        assert_eq!(catalog.image_format(1).unwrap(), ImageFormat::Bmp);
        assert_eq!(catalog.image_format(2).unwrap(), ImageFormat::Gif);
        assert_eq!(catalog.image_format(3).unwrap(), ImageFormat::Tiff);
    }

    #[test]
    fn bmp_gif_and_tiff_formats_are_inferred_from_media_file_extension() {
        let mut catalog = ResourceCatalog::empty();
        let path = PackagePath::new("Doc_0/DocumentRes.xml").unwrap();
        catalog
            .insert_image(image_entry("1", None, "image_1.bmp"), &None, &path)
            .unwrap();
        catalog
            .insert_image(image_entry("2", None, "image_2.GIF"), &None, &path)
            .unwrap();
        catalog
            .insert_image(image_entry("3", None, "image_3.tif"), &None, &path)
            .unwrap();
        catalog
            .insert_image(image_entry("4", None, "image_4.tiff"), &None, &path)
            .unwrap();
        assert_eq!(catalog.image_format(1).unwrap(), ImageFormat::Bmp);
        assert_eq!(catalog.image_format(2).unwrap(), ImageFormat::Gif);
        assert_eq!(catalog.image_format(3).unwrap(), ImageFormat::Tiff);
        assert_eq!(catalog.image_format(4).unwrap(), ImageFormat::Tiff);
    }

    #[test]
    fn root_base_loc_resolves_assets_from_package_root() {
        let catalog_path = PackagePath::new("PublicRes.xml").unwrap();
        let path = asset_path(&catalog_path, Some("/"), "font_4.ttf").unwrap();
        assert_eq!(path.as_str(), "font_4.ttf");
    }

    #[test]
    fn absolute_base_loc_resolves_assets_from_package_root_directory() {
        let catalog_path = PackagePath::new("PublicRes.xml").unwrap();
        let path = asset_path(&catalog_path, Some("/Res"), "font_4.ttf").unwrap();
        assert_eq!(path.as_str(), "Res/font_4.ttf");
    }

    #[test]
    fn root_base_loc_rejects_paths_escaping_package_root() {
        let catalog_path = PackagePath::new("PublicRes.xml").unwrap();
        assert!(asset_path(&catalog_path, Some("/"), "../evil.ttf").is_err());
    }

    #[test]
    fn relative_base_loc_resolves_against_declaring_file() {
        let catalog_path = PackagePath::new("Doc_0/DocumentRes.xml").unwrap();
        let path = asset_path(&catalog_path, Some("Res"), "image_1.png").unwrap();
        assert_eq!(path.as_str(), "Doc_0/Res/image_1.png");
    }

    #[test]
    fn failed_asset_load_is_not_cached_or_poisoned() {
        let asset = Asset::new(PackagePath::new("missing").unwrap());
        let calls = AtomicUsize::new(0);
        let first = asset.load_with(|| -> Result<Vec<u8>> {
            calls.fetch_add(1, Ordering::SeqCst);
            Err(Error::MissingEntry("missing".to_owned()))
        });
        assert!(matches!(first, Err(Error::MissingEntry(_))));

        let second = asset
            .load_with(|| {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok(b"available".to_vec())
            })
            .unwrap();
        assert_eq!(&*second, b"available");
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn repeated_draw_param_resolution_visits_each_chain_node_once() {
        let catalog = draw_param_catalog(1_024);

        for _ in 0..16 {
            assert_eq!(catalog.draw_param(1_024).unwrap().line_width, Some(1_024.0));
        }
        assert_eq!(catalog.draw_param_visits.load(Ordering::Relaxed), 1_024);
    }

    #[test]
    fn concurrent_draw_param_resolution_is_single_flight() {
        let catalog = Arc::new(draw_param_catalog(1_024));
        let barrier = Arc::new(Barrier::new(8));
        let handles = (0..8)
            .map(|_| {
                let catalog = Arc::clone(&catalog);
                let barrier = Arc::clone(&barrier);
                std::thread::spawn(move || {
                    barrier.wait();
                    catalog.draw_param(1_024).unwrap()
                })
            })
            .collect::<Vec<_>>();
        for handle in handles {
            assert_eq!(handle.join().unwrap().line_width, Some(1_024.0));
        }
        assert_eq!(catalog.draw_param_visits.load(Ordering::Relaxed), 1_024);
    }

    #[test]
    fn failed_draw_param_resolution_is_deterministic_and_retryable() {
        let mut catalog = draw_param_catalog(2);
        let ResourceEntry::DrawParam(record) = catalog.entries.get_mut(&1).unwrap() else {
            panic!("expected DrawParam");
        };
        record.relative = Some(99);

        for expected_visits in [2, 4] {
            let error = catalog.draw_param(2).unwrap_err();
            assert!(matches!(error, Error::UnknownResource { object_id: 99 }));
            assert_eq!(
                catalog.draw_param_visits.load(Ordering::Relaxed),
                expected_visits
            );
            assert!(catalog
                .entries
                .values()
                .filter_map(|entry| match entry {
                    ResourceEntry::DrawParam(record) => Some(record),
                    _ => None,
                })
                .all(|record| record.resolved.get().is_none()));
        }
    }

    #[test]
    fn concurrent_failed_draw_param_resolution_is_consistent_and_unpublished() {
        let mut catalog = draw_param_catalog(2);
        let ResourceEntry::DrawParam(record) = catalog.entries.get_mut(&1).unwrap() else {
            panic!("expected DrawParam");
        };
        record.relative = Some(99);
        let catalog = Arc::new(catalog);
        let barrier = Arc::new(Barrier::new(8));
        let handles = (0..8)
            .map(|_| {
                let catalog = Arc::clone(&catalog);
                let barrier = Arc::clone(&barrier);
                std::thread::spawn(move || {
                    barrier.wait();
                    catalog.draw_param(2).unwrap_err()
                })
            })
            .collect::<Vec<_>>();

        for handle in handles {
            assert!(matches!(
                handle.join().unwrap(),
                Error::UnknownResource { object_id: 99 }
            ));
        }
        assert_eq!(catalog.draw_param_visits.load(Ordering::Relaxed), 16);
        assert!(catalog
            .entries
            .values()
            .filter_map(|entry| match entry {
                ResourceEntry::DrawParam(record) => Some(record),
                _ => None,
            })
            .all(|record| record.resolved.get().is_none()));
    }
}
