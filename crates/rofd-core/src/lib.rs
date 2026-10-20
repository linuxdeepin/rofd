#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![doc = include_str!("../README.md")]

mod container;
mod content;
mod document;
mod error;
mod geometry;
mod image;
mod links;
mod navigation;
mod options;
mod paint;
mod path;
mod path_data;
mod raw;
mod resources;
mod semantic;
mod ses;
mod signature;
mod text;

pub use content::{
    AnnotationType, Clip, ClipPath, CompositeObject, FillRule, Layer, LayerSource, LayerType,
    PageAnnotation, PageGroup, PageObject, PathObject, UnsupportedObject, UnsupportedObjectKind,
};
pub use document::{Document, Metadata, Page, Warning, WarningCode};
pub use error::{Error, Result};
pub use geometry::{Point, Rect, Transform};
pub use image::{ImageBorder, ImageObject};
pub use links::PageLink;
pub use navigation::{Action, ActionEvent, ActionKind, Destination, DestinationMode, OutlineNode};
pub use options::{LoadOptions, ResourceLimits, Strictness};
pub use paint::{Color, LineCap, LineJoin, StrokeStyle};
pub use path_data::{PathCommand, PathData};
pub use resources::{FontResource, ImageFormat, ImageResource, ResourceIdentity, ResourceKind};
pub use semantic::{
    FindOptions, PageText, SelectionStyle, TextChar, TextCharFlags, TextGeometryPrecision,
    TextMatch, TextMetricsProvider, TextScalarMetrics, TextSelection,
};
pub use signature::{SealPicture, SealPictureKind, StampAnnotation};
pub use text::{CharacterGlyphMap, GlyphTransform, TextCode, TextObject};

/// The crate API version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
