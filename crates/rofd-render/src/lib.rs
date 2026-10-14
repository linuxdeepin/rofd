// `deny` rather than `forbid` because the jbig2dec FFI module below is the
// one audited place that needs `unsafe`; every other module stays clean.
#![deny(unsafe_code)]
#![deny(missing_docs)]
#![doc = include_str!("../README.md")]

mod cairo_renderer;
mod display_list;
mod fonts;
mod images;
#[cfg(feature = "jbig2")]
#[allow(unsafe_code)]
mod jbig2;

pub use cairo_renderer::{
    CairoRenderer, ImageInterpolation, PixelRect, RenderOptions, RenderReport,
};
pub use display_list::{
    ClipPath, Command, DisplayList, DisplayListBuilder, RenderDiagnostic, RenderDiagnosticKind,
};
pub use fonts::{
    position_glyph_runs, FontDiagnostic, FontIdentity, FontResolver, FontSource, FontTextAdvances,
    GlyphRun, PositionedGlyph, ResolvedFont, SystemFontResolver,
};
pub use images::{DecodedImage, ImageDecoder};

/// Backend-neutral draw command categories whose Cairo implementations may differ.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum DisplayCommandKind {
    /// Positioned glyph drawing.
    GlyphRun,
    /// Decoded raster-image drawing.
    Image,
}

impl std::fmt::Display for DisplayCommandKind {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::GlyphRun => "DrawGlyphRun",
            Self::Image => "DrawImage",
        })
    }
}

/// An error encountered while lowering or rendering a validated page.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// A render option is outside its supported domain.
    #[error("invalid render option {field}: {value}")]
    InvalidOption {
        /// The invalid option name.
        field: &'static str,
        /// A description of the invalid value.
        value: String,
    },
    /// The requested page surface dimensions cannot be represented safely.
    #[error("invalid render surface size {width} by {height} pixels")]
    InvalidSurfaceSize {
        /// The requested width before integer conversion.
        width: f64,
        /// The requested height before integer conversion.
        height: f64,
    },
    /// A caller-provided image surface cannot contain the requested page.
    #[error(
        "render target is {actual_width} by {actual_height} pixels but requires at least {required_width} by {required_height}"
    )]
    SurfaceTooSmall {
        /// Required output width.
        required_width: i32,
        /// Required output height.
        required_height: i32,
        /// Actual image-surface width.
        actual_width: i32,
        /// Actual image-surface height.
        actual_height: i32,
    },
    /// A page object contains a value that cannot form a valid display command.
    #[error("path object {object_id} has invalid {field}: {value}")]
    InvalidModel {
        /// The OFD object identifier containing the invalid value.
        object_id: u64,
        /// The invalid model field.
        field: &'static str,
        /// A description of the invalid value.
        value: String,
    },
    /// The display list contains an invalid graphics-state sequence.
    #[error("invalid display list: {message}")]
    InvalidDisplayList {
        /// A description of the invalid sequence.
        message: String,
    },
    /// A geometric primitive cannot be represented numerically by the backend.
    #[error("invalid {primitive} geometry in {field}: {value}")]
    InvalidGeometry {
        /// The primitive whose geometry failed, such as `arc`.
        primitive: &'static str,
        /// The input or derived field that was invalid.
        field: &'static str,
        /// A description of the invalid value.
        value: String,
    },
    /// The configured memory budget cannot contain the renderer's worst-case surfaces.
    #[error("raster surfaces require {required_bytes} bytes but the limit is {max_bytes}")]
    RasterBudgetExceeded {
        /// Worst-case simultaneously live bytes required for this page.
        required_bytes: u64,
        /// Configured maximum simultaneously live raster bytes.
        max_bytes: u64,
    },
    /// The checked native image buffer could not be allocated.
    #[error("could not allocate {required_bytes} bytes for image compositing")]
    RasterAllocation {
        /// Number of bytes requested for the premultiplied Cairo image.
        required_bytes: u64,
    },
    /// A decoded source image exceeds Cairo's supported surface dimensions.
    #[error(
        "image resource {resource_id} is {width} by {height} pixels, exceeding Cairo's surface limit"
    )]
    InvalidImageSurfaceSize {
        /// OFD image resource identifier.
        resource_id: u64,
        /// Decoded source width.
        width: u32,
        /// Decoded source height.
        height: u32,
    },
    /// Cairo rejected a rendering operation.
    #[error("Cairo {operation} failed: {source}")]
    Backend {
        /// The operation being performed.
        operation: &'static str,
        /// The Cairo error.
        #[source]
        source: cairo::Error,
    },
    /// A primary failure was followed by another failure while restoring state.
    #[error("{operation} failed after {primary}: {cleanup}")]
    Cleanup {
        /// The cleanup operation that failed.
        operation: &'static str,
        /// The error that initiated cleanup.
        primary: Box<Error>,
        /// The cleanup error.
        cleanup: Box<Error>,
    },
    /// Font bytes or a selected face cannot be used safely.
    #[error("invalid font {identity}: {message}")]
    InvalidFont {
        /// Stable non-path identity of the font.
        identity: String,
        /// Backend validation detail.
        message: String,
    },
    /// An explicit OFD glyph identifier is outside the selected face.
    #[error("glyph ID {glyph_id} is invalid for font {identity} with {glyph_count} glyphs")]
    InvalidGlyph {
        /// Stable non-path identity of the font.
        identity: String,
        /// Invalid glyph identifier.
        glyph_id: u32,
        /// Number of glyphs advertised by the selected face.
        glyph_count: u32,
    },
    /// Text positioning produced an invalid value or exceeded a semantic bound.
    #[error("invalid text layout for object {object_id} in {field}: {message}")]
    InvalidTextLayout {
        /// OFD text object identifier.
        object_id: u64,
        /// Stable field or budget name.
        field: &'static str,
        /// Validation detail.
        message: String,
    },
    /// A system font exceeds the configured encoded-byte limit.
    #[error("font {identity} has {actual_bytes} bytes but the limit is {max_bytes}")]
    FontBytesExceeded {
        /// Stable non-path font identity.
        identity: String,
        /// Encoded byte length.
        actual_bytes: u64,
        /// Configured maximum encoded byte length.
        max_bytes: u64,
    },
    /// Internal font cache synchronization failed.
    #[error("font cache unavailable: {message}")]
    FontCache {
        /// Synchronization detail.
        message: String,
    },
    /// Cairo could not create or use a resolved FreeType face.
    #[error("font {identity} failed to {operation}: {source}")]
    FontBackend {
        /// Stable non-path resolved-font identity.
        identity: String,
        /// Font backend operation being attempted.
        operation: &'static str,
        /// Cairo backend failure.
        #[source]
        source: cairo::Error,
    },
    /// A font resource was paired with a text object referencing another identifier.
    #[error(
        "text object {text_object_id} references font {expected_font_id}, not resource {actual_resource_id}"
    )]
    FontResourceMismatch {
        /// OFD text object identifier.
        text_object_id: u64,
        /// Font identifier referenced by the text object.
        expected_font_id: u64,
        /// Font resource identifier supplied by the caller.
        actual_resource_id: u64,
    },
    /// A page object's document resource could not be loaded.
    #[error("object {object_id} could not load {kind:?} resource {resource_id}: {source}")]
    ObjectResource {
        /// OFD page object identifier.
        object_id: u64,
        /// Referenced document-wide resource identifier.
        resource_id: u64,
        /// Expected resource category.
        kind: rofd_core::ResourceKind,
        /// Structured core resource failure with package-local provenance.
        #[source]
        source: rofd_core::Error,
    },
    /// Processing a loaded page-object resource failed.
    #[error(
        "object {object_id} could not process {kind:?} resource {resource_id} at {asset_path}: {source}"
    )]
    ObjectResourceProcessing {
        /// OFD page object identifier.
        object_id: u64,
        /// Referenced document-wide resource identifier.
        resource_id: u64,
        /// Resource category being processed.
        kind: rofd_core::ResourceKind,
        /// Safe package-local asset path, or a stable marker for an external font.
        asset_path: String,
        /// Structured font-positioning or image-decoding failure.
        #[source]
        source: Box<Error>,
    },
    /// The Cairo backend has not implemented a backend-neutral draw command yet.
    #[error("Cairo rendering for {command} is not implemented")]
    UnsupportedDisplayCommand {
        /// Stable display-command category.
        command: DisplayCommandKind,
    },
    /// Decoded images retained by one display list exceed its aggregate byte budget.
    #[error(
        "display-list images require {required_bytes} decoded bytes but the limit is {max_bytes}"
    )]
    DisplayListImageBudgetExceeded {
        /// Aggregate decoded RGBA bytes required through the failing object.
        required_bytes: u64,
        /// Maximum decoded RGBA bytes retained by one display list.
        max_bytes: u64,
    },
    /// Encoded bytes do not have a supported PNG or JPEG signature.
    #[error("image resource {resource_id} at {path} has an unsupported byte signature")]
    UnsupportedImageFormat {
        /// OFD image resource identifier.
        resource_id: u64,
        /// Safe package-local encoded asset path.
        path: String,
    },
    /// The byte signature does not agree with the catalog's declared image format.
    #[error(
        "image resource {resource_id} at {path} declares {declared:?} but contains {detected:?}"
    )]
    ImageFormatMismatch {
        /// OFD image resource identifier.
        resource_id: u64,
        /// Safe package-local encoded asset path.
        path: String,
        /// Catalog-declared format.
        declared: rofd_core::ImageFormat,
        /// Signature-detected format.
        detected: rofd_core::ImageFormat,
    },
    /// Encoded image bytes are corrupt or cannot be decoded safely.
    #[error("image resource {resource_id} at {path} could not be decoded: {message}")]
    ImageDecode {
        /// OFD image resource identifier.
        resource_id: u64,
        /// Safe package-local encoded asset path.
        path: String,
        /// Decoder detail without host paths.
        message: String,
    },
    /// An encoded image advertises a zero dimension.
    #[error("image resource {resource_id} at {path} has invalid dimensions {width} by {height}")]
    InvalidImageDimensions {
        /// OFD image resource identifier.
        resource_id: u64,
        /// Safe package-local encoded asset path.
        path: String,
        /// Header width.
        width: u32,
        /// Header height.
        height: u32,
    },
    /// Image stride or decoded-size arithmetic cannot be represented safely.
    #[error("image resource {resource_id} at {path} dimensions {width} by {height} overflow RGBA sizing")]
    ImageDimensionsOverflow {
        /// OFD image resource identifier.
        resource_id: u64,
        /// Safe package-local encoded asset path.
        path: String,
        /// Header width.
        width: u32,
        /// Header height.
        height: u32,
    },
    /// An encoded or decoded image quantity exceeds its configured per-image limit.
    #[error("image resource {resource_id} at {path} has {actual} {field}, exceeding {max}")]
    ImageLimitExceeded {
        /// OFD image resource identifier.
        resource_id: u64,
        /// Safe package-local encoded asset path.
        path: String,
        /// Stable limit category.
        field: &'static str,
        /// Actual checked quantity.
        actual: u64,
        /// Configured maximum.
        max: u64,
    },
    /// Internal decoded-image cache synchronization failed.
    #[error("image cache unavailable: {message}")]
    ImageCache {
        /// Synchronization detail.
        message: String,
    },
}

/// A result produced by `rofd-render`.
pub type Result<T> = std::result::Result<T, Error>;
