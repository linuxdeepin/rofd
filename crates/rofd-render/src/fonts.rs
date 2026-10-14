use std::collections::HashMap;
use std::ops::Range;
use std::sync::{Arc, Condvar, Mutex};

use fontdb::{Database, Family, Query, Source, ID};
use freetype::face::LoadFlag;
use rofd_core::{
    FontResource, Page, ResourceIdentity, ResourceLimits, TextAdvanceProvider, TextObject,
    Transform,
};

use crate::{Error, Result};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) struct FontAllocationKey {
    bytes: usize,
    face_index: u32,
}

/// Validated stable font identity that cannot contain a host path.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct FontIdentity(Arc<str>);

impl FontIdentity {
    /// Creates a nonempty identity without path separators, drive separators, or controls.
    pub fn new(value: impl Into<String>) -> Result<Self> {
        let value = value.into();
        if value.trim().is_empty()
            || value
                .chars()
                .any(|character| character.is_control() || matches!(character, '/' | '\\' | ':'))
        {
            return Err(Error::InvalidFont {
                identity: "invalid-identity".to_owned(),
                message:
                    "identity must be nonempty and must not contain path separators or controls"
                        .to_owned(),
            });
        }
        Ok(Self(Arc::from(value)))
    }

    /// Returns the validated non-path identity text.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn system(id: ID, face_index: u32, post_script_name: &str) -> Self {
        let display = post_script_name
            .chars()
            .map(|character| {
                if character.is_alphanumeric() || matches!(character, ' ' | '-' | '_' | '.') {
                    character
                } else {
                    '_'
                }
            })
            .collect::<String>();
        Self(Arc::from(format!(
            "fontdb#{id}#face#{face_index}#{}",
            if display.is_empty() {
                "unnamed"
            } else {
                &display
            }
        )))
    }
}

/// Describes where a selected face came from without exposing host paths.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum FontSource {
    /// Bytes embedded by the OFD document.
    Embedded {
        /// Document-wide font resource identifier.
        resource_id: u64,
    },
    /// A face in the resolver's configured system-font snapshot.
    System {
        /// Stable resolver-local, non-path identity.
        identity: FontIdentity,
    },
    /// A face selected from the ordered configured fallback families.
    ConfiguredFallback {
        /// Stable resolver-local, non-path identity.
        identity: FontIdentity,
    },
    /// No usable face or visible replacement glyph was available.
    Missing,
}

/// Immutable validated font bytes and selected collection face.
#[derive(Clone, Debug)]
pub struct ResolvedFont {
    bytes: Arc<[u8]>,
    face_index: u32,
    identity: Arc<str>,
    source: FontSource,
    family_name: Option<Arc<str>>,
    glyph_count: u32,
    units_per_em: u16,
    metrics: Arc<Mutex<FontMetricsCache>>,
}

impl PartialEq for ResolvedFont {
    fn eq(&self, other: &Self) -> bool {
        self.face_index == other.face_index
            && self.identity == other.identity
            && self.source == other.source
            && self.family_name == other.family_name
            && self.glyph_count == other.glyph_count
            && self.units_per_em == other.units_per_em
            && Arc::ptr_eq(&self.bytes, &other.bytes)
    }
}

#[derive(Debug, Default)]
struct FontMetricsCache {
    characters: HashMap<char, Option<u32>>,
    advances: HashMap<u32, (i64, i64)>,
}

impl ResolvedFont {
    pub(crate) fn allocation_key(&self) -> FontAllocationKey {
        FontAllocationKey {
            bytes: self.bytes.as_ptr() as usize,
            face_index: self.face_index,
        }
    }

    pub(crate) fn create_cairo_font_face(&self) -> Result<cairo::FontFace> {
        // cairo-rs owns its FreeType face through the safe default `Rc<Vec<u8>>`
        // storage. Copy once per render-local face; callers cache the result.
        let library = freetype::Library::init().map_err(|error| Error::InvalidFont {
            identity: self.identity().to_owned(),
            message: error.to_string(),
        })?;
        let face_index = isize::try_from(self.face_index).map_err(|_| Error::InvalidFont {
            identity: self.identity().to_owned(),
            message: "face index is not representable".to_owned(),
        })?;
        let face = library
            .new_memory_face(self.bytes.to_vec(), face_index)
            .map_err(|error| Error::InvalidFont {
                identity: self.identity().to_owned(),
                message: error.to_string(),
            })?;
        cairo::FontFace::create_from_ft(&face).map_err(|source| Error::FontBackend {
            identity: self.identity().to_owned(),
            operation: "create Cairo FreeType font face",
            source,
        })
    }

    /// Validates and constructs a face from one embedded document resource.
    pub fn from_embedded_resource(resource: &FontResource, face_index: u32) -> Result<Self> {
        let resource_id = resource.id();
        let bytes = resource
            .encoded_bytes_arc()
            .ok_or_else(|| Error::InvalidFont {
                identity: format!("embedded#{resource_id}"),
                message: "font resource has no embedded bytes".to_owned(),
            })?;
        if resource_id == 0 {
            return Err(Error::InvalidFont {
                identity: "embedded".to_owned(),
                message: "embedded resource ID must be nonzero".to_owned(),
            });
        }
        let identity = FontIdentity(Arc::from(format!(
            "embedded#{resource_id}#face#{face_index}"
        )));
        Self::from_validated_bytes(
            bytes,
            face_index,
            identity,
            FontSource::Embedded { resource_id },
        )
    }

    /// Validates owned bytes for a primary face from a configured system snapshot.
    pub fn from_system_bytes(
        bytes: Arc<[u8]>,
        face_index: u32,
        identity: FontIdentity,
    ) -> Result<Self> {
        Self::from_validated_bytes(
            bytes,
            face_index,
            identity.clone(),
            FontSource::System { identity },
        )
    }

    /// Validates owned bytes for a face from the ordered fallback configuration.
    pub fn from_configured_fallback_bytes(
        bytes: Arc<[u8]>,
        face_index: u32,
        identity: FontIdentity,
    ) -> Result<Self> {
        Self::from_validated_bytes(
            bytes,
            face_index,
            identity.clone(),
            FontSource::ConfiguredFallback { identity },
        )
    }

    fn from_validated_bytes(
        bytes: Arc<[u8]>,
        face_index: u32,
        identity: FontIdentity,
        source: FontSource,
    ) -> Result<Self> {
        let identity_text = identity.as_str();
        let face_index_value = isize::try_from(face_index).map_err(|_| Error::InvalidFont {
            identity: identity_text.to_owned(),
            message: "face index is not representable".to_owned(),
        })?;
        let library = freetype::Library::init().map_err(|error| Error::InvalidFont {
            identity: identity_text.to_owned(),
            message: error.to_string(),
        })?;
        let face = library
            .new_memory_face2(Arc::clone(&bytes), face_index_value)
            .map_err(|error| Error::InvalidFont {
                identity: identity_text.to_owned(),
                message: error.to_string(),
            })?;
        let glyph_count = u32::try_from(face.num_glyphs()).map_err(|_| Error::InvalidFont {
            identity: identity_text.to_owned(),
            message: "face reports an invalid glyph count".to_owned(),
        })?;
        let units_per_em = u16::try_from(face.em_size()).map_err(|_| Error::InvalidFont {
            identity: identity_text.to_owned(),
            message: "face reports an invalid units-per-em value".to_owned(),
        })?;
        if units_per_em == 0 {
            return Err(Error::InvalidFont {
                identity: identity_text.to_owned(),
                message: "face reports zero units per em".to_owned(),
            });
        }
        let family_name = face.family_name().map(Arc::<str>::from);
        Ok(Self {
            bytes,
            face_index,
            identity: identity.0,
            source,
            family_name,
            glyph_count,
            units_per_em,
            metrics: Arc::new(Mutex::new(FontMetricsCache::default())),
        })
    }

    /// Returns the immutable encoded font bytes.
    pub fn encoded_bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Returns a shared owner for the immutable encoded font bytes.
    pub fn encoded_bytes_arc(&self) -> Arc<[u8]> {
        Arc::clone(&self.bytes)
    }

    /// Returns the face index for a font collection, or zero for a single-face font.
    pub fn face_index(&self) -> u32 {
        self.face_index
    }

    /// Returns the stable non-path identity.
    pub fn identity(&self) -> &str {
        &self.identity
    }

    /// Returns the source category and its safe identity.
    pub fn source(&self) -> &FontSource {
        &self.source
    }

    /// Returns the family reported by FreeType, when present.
    pub fn family_name(&self) -> Option<&str> {
        self.family_name.as_deref()
    }

    /// Returns the number of glyphs in the selected face.
    pub fn glyph_count(&self) -> u32 {
        self.glyph_count
    }

    /// Returns the face glyph identifier for one Unicode scalar, when mapped.
    pub fn glyph_index(&self, character: char) -> Result<Option<u32>> {
        self.glyph_for(character)
    }

    /// Returns the unshaped horizontal-layout advance at `size_mm`.
    pub fn glyph_advance_mm(&self, glyph_id: u32, size_mm: f64) -> Result<(f64, f64)> {
        validate_glyph(self, glyph_id)?;
        if !size_mm.is_finite() || size_mm <= 0.0 {
            return Err(Error::InvalidFont {
                identity: self.identity().to_owned(),
                message: "glyph advance size must be finite and positive".to_owned(),
            });
        }
        self.advance_mm(glyph_id, size_mm)
    }

    fn glyph_for(&self, character: char) -> Result<Option<u32>> {
        let mut metrics = self.metrics.lock().map_err(|_| Error::FontCache {
            message: "face metadata lock is poisoned".to_owned(),
        })?;
        if let Some(glyph_id) = metrics.characters.get(&character) {
            return Ok(*glyph_id);
        }
        let glyph_id = self.with_face(|face| Ok(face.get_char_index(character as usize)))?;
        metrics.characters.insert(character, glyph_id);
        Ok(glyph_id)
    }

    fn advance_mm(&self, glyph_id: u32, size_mm: f64) -> Result<(f64, f64)> {
        let mut metrics = self.metrics.lock().map_err(|_| Error::FontCache {
            message: "face metadata lock is poisoned".to_owned(),
        })?;
        let advance = if let Some(advance) = metrics.advances.get(&glyph_id) {
            *advance
        } else {
            let advance = self.with_face(|face| {
                face.load_glyph(glyph_id, LoadFlag::NO_SCALE | LoadFlag::NO_HINTING)
                    .map_err(|error| Error::InvalidFont {
                        identity: self.identity().to_owned(),
                        message: error.to_string(),
                    })?;
                let advance = face.glyph().advance();
                Ok((advance.x, advance.y))
            })?;
            metrics.advances.insert(glyph_id, advance);
            advance
        };
        let scale = size_mm / f64::from(self.units_per_em);
        let x = advance.0 as f64 * scale;
        let y = advance.1 as f64 * scale;
        if !x.is_finite() || !y.is_finite() {
            return Err(Error::InvalidFont {
                identity: self.identity().to_owned(),
                message: "glyph advance is non-finite".to_owned(),
            });
        }
        Ok((x, y))
    }

    fn with_face<T>(
        &self,
        operation: impl FnOnce(&freetype::Face<Arc<[u8]>>) -> Result<T>,
    ) -> Result<T> {
        let face = self.open_face()?;
        operation(&face)
    }

    fn open_face(&self) -> Result<freetype::Face<Arc<[u8]>>> {
        let library = freetype::Library::init().map_err(|error| Error::InvalidFont {
            identity: self.identity().to_owned(),
            message: error.to_string(),
        })?;
        let face_index = isize::try_from(self.face_index).map_err(|_| Error::InvalidFont {
            identity: self.identity().to_owned(),
            message: "face index is not representable".to_owned(),
        })?;
        library
            .new_memory_face2(Arc::clone(&self.bytes), face_index)
            .map_err(|error| Error::InvalidFont {
                identity: self.identity().to_owned(),
                message: error.to_string(),
            })
    }

    fn into_configured_fallback(mut self) -> Self {
        let identity = FontIdentity(Arc::clone(&self.identity));
        self.source = FontSource::ConfiguredFallback { identity };
        self
    }
}

/// A stable diagnostic emitted while selecting glyph faces.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum FontDiagnostic {
    /// The declared font lacked the character and an ordered fallback was selected.
    FamilyFallback {
        /// Original Unicode scalar.
        character: char,
        /// Zero-based scalar index in the text object.
        scalar_index: usize,
        /// Declared family or font name.
        requested: String,
        /// Stable identity of the selected fallback face.
        selected: String,
    },
    /// No selected face contained the requested scalar.
    MissingGlyph {
        /// Original Unicode scalar.
        character: char,
        /// Zero-based scalar index in the text object.
        scalar_index: usize,
        /// OFD text object identifier.
        object_id: u64,
        /// Whether a visible replacement glyph was found.
        used_visible_replacement: bool,
    },
}

/// One exact glyph placement in OFD object-space millimetres.
#[derive(Clone, Debug, PartialEq)]
pub struct PositionedGlyph {
    glyph_id: u32,
    x: f64,
    y: f64,
    advance: (f64, f64),
    character: Option<char>,
    source_range: Range<usize>,
    scalar_index: usize,
    source_scalar_range: Range<usize>,
    font: Option<ResolvedFont>,
    font_source: FontSource,
    transform: Option<Transform>,
}

impl PositionedGlyph {
    /// Returns the selected face glyph identifier.
    pub fn glyph_id(&self) -> u32 {
        self.glyph_id
    }
    /// Returns the x coordinate in OFD object-space millimetres.
    pub fn x(&self) -> f64 {
        self.x
    }
    /// Returns the y coordinate in OFD object-space millimetres.
    pub fn y(&self) -> f64 {
        self.y
    }
    /// Returns the advance this glyph contributes on each axis, in millimetres.
    ///
    /// This is the selected face advance, or the font size when no face was
    /// available. It is what positions the next glyph, so a caller that needs the
    /// per-scalar geometry of a run without explicit deltas can sum it instead of
    /// resolving fonts again.
    pub fn advance(&self) -> (f64, f64) {
        self.advance
    }
    /// Returns the associated source scalar, or `None` for an extra mapped glyph.
    pub fn character(&self) -> Option<char> {
        self.character
    }
    /// Returns the byte range in the containing [`GlyphRun`]'s original text.
    pub fn source_range(&self) -> Range<usize> {
        self.source_range.clone()
    }
    /// Returns the zero-based scalar index in the complete text object.
    pub fn scalar_index(&self) -> usize {
        self.scalar_index
    }
    /// Returns the scalar range in the complete text object represented by this glyph.
    pub fn source_scalar_range(&self) -> Range<usize> {
        self.source_scalar_range.clone()
    }
    /// Returns the stable selected-font identity, or `missing`.
    pub fn font_identity(&self) -> &str {
        self.font.as_ref().map_or("missing", ResolvedFont::identity)
    }
    /// Returns the selected font source.
    pub fn font_source(&self) -> &FontSource {
        &self.font_source
    }
    /// Returns the immutable selected font when one exists.
    pub fn font(&self) -> Option<&ResolvedFont> {
        self.font.as_ref()
    }
    /// Returns whether the backend should draw the deterministic synthetic missing box.
    pub fn is_synthetic_box(&self) -> bool {
        matches!(self.font_source, FontSource::Missing)
    }
    /// Returns an optional per-glyph transform from a structured `CGTransform`.
    ///
    /// When the `Glyphs` element used structured `Glyph` children with `M00`/`M01`/`M10`/`M11`
    /// attributes, this carries the per-glyph affine matrix.  The legacy whitespace-separated
    /// ID form produces `None`.
    pub fn transform(&self) -> Option<Transform> {
        self.transform
    }
}

/// One source `TextCode` and its ordered, unshaped positioned glyphs.
///
/// Clones share immutable text, glyph, and diagnostic storage. Equality is
/// structural and linear in glyph/diagnostic count for independently allocated
/// runs, but it never rescans font bytes.
#[derive(Clone, Debug, PartialEq)]
pub struct GlyphRun {
    text: Arc<str>,
    source_range: Range<usize>,
    size_mm: f64,
    glyphs: Arc<[PositionedGlyph]>,
    diagnostics: Arc<[FontDiagnostic]>,
}

impl GlyphRun {
    /// Returns the original `TextCode` Unicode text without normalization.
    pub fn text(&self) -> &str {
        &self.text
    }
    /// Returns the scalar range in the complete text object.
    pub fn source_range(&self) -> Range<usize> {
        self.source_range.clone()
    }
    /// Returns the positive text size in millimetres.
    pub fn size_mm(&self) -> f64 {
        self.size_mm
    }
    /// Returns source-ordered glyph placements.
    pub fn glyphs(&self) -> &[PositionedGlyph] {
        &self.glyphs
    }
    /// Returns stable fallback and missing-glyph diagnostics.
    pub fn diagnostics(&self) -> &[FontDiagnostic] {
        &self.diagnostics
    }
}

/// Thread-safe font selection used by the backend-neutral glyph positioner.
pub trait FontResolver: Send + Sync {
    /// Resolves embedded bytes first, then the declared system family or font name.
    fn resolve_primary(&self, resource: &FontResource) -> Result<Option<ResolvedFont>>;
    /// Resolves the first configured fallback family containing `character`.
    fn resolve_fallback(&self, character: char) -> Result<Option<ResolvedFont>>;
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
enum CacheKey {
    Embedded {
        identity: ResourceIdentity,
        resource_id: u64,
        face_index: u32,
    },
    System {
        id: ID,
        face_index: u32,
    },
}

#[derive(Clone, Debug)]
enum FontLoadFailure {
    InvalidFont {
        identity: String,
        message: String,
    },
    FontBytesExceeded {
        identity: String,
        actual_bytes: u64,
        max_bytes: u64,
    },
    Cache(String),
}

impl FontLoadFailure {
    fn from_error(error: Error) -> Self {
        match error {
            Error::InvalidFont { identity, message } => Self::InvalidFont { identity, message },
            Error::FontBytesExceeded {
                identity,
                actual_bytes,
                max_bytes,
            } => Self::FontBytesExceeded {
                identity,
                actual_bytes,
                max_bytes,
            },
            error => Self::Cache(error.to_string()),
        }
    }

    fn into_error(self) -> Error {
        match self {
            Self::InvalidFont { identity, message } => Error::InvalidFont { identity, message },
            Self::FontBytesExceeded {
                identity,
                actual_bytes,
                max_bytes,
            } => Error::FontBytesExceeded {
                identity,
                actual_bytes,
                max_bytes,
            },
            Self::Cache(message) => Error::FontCache { message },
        }
    }
}

#[derive(Debug, Default)]
struct LoadSlot {
    outcome: Mutex<Option<std::result::Result<ResolvedFont, FontLoadFailure>>>,
    completed: Condvar,
}

impl LoadSlot {
    fn complete(&self, outcome: std::result::Result<ResolvedFont, FontLoadFailure>) -> Result<()> {
        let mut stored = self.outcome.lock().map_err(|_| Error::FontCache {
            message: "font load slot is poisoned".to_owned(),
        })?;
        *stored = Some(outcome);
        self.completed.notify_all();
        Ok(())
    }

    fn wait(&self) -> Result<ResolvedFont> {
        let mut stored = self.outcome.lock().map_err(|_| Error::FontCache {
            message: "font load slot is poisoned".to_owned(),
        })?;
        while stored.is_none() {
            stored = self.completed.wait(stored).map_err(|_| Error::FontCache {
                message: "font load slot is poisoned".to_owned(),
            })?;
        }
        stored
            .as_ref()
            .expect("completed slot checked")
            .clone()
            .map_err(FontLoadFailure::into_error)
    }
}

#[derive(Debug)]
struct CacheEntry {
    font: ResolvedFont,
    last_used: u64,
}

#[derive(Debug, Default)]
struct CacheState {
    ready: HashMap<CacheKey, CacheEntry>,
    in_flight: HashMap<CacheKey, Arc<LoadSlot>>,
    clock: u64,
}

impl CacheState {
    fn tick(&mut self) -> u64 {
        if self.clock == u64::MAX {
            self.ready.clear();
            self.clock = 0;
        }
        self.clock += 1;
        self.clock
    }
}

/// A deterministic resolver over one immutable `fontdb` snapshot.
///
/// Misses are never cached globally. Constructing a new resolver creates a new
/// lookup scope and is how callers observe fonts installed after this immutable
/// database snapshot was built.
pub struct SystemFontResolver {
    database: Database,
    fallback_faces: Vec<Vec<ID>>,
    max_font_bytes: u64,
    cache_capacity: usize,
    cache: Mutex<CacheState>,
}

impl SystemFontResolver {
    /// Default maximum number of fully resolved faces retained by one resolver.
    pub const DEFAULT_CACHE_CAPACITY: usize = 64;

    /// Uses an empty database, useful for hermetic callers and tests.
    pub fn empty(fallback_families: Vec<String>, max_font_bytes: u64) -> Self {
        Self::from_database(Database::new(), fallback_families, max_font_bytes)
    }

    /// Takes ownership of an already configured database without scanning the host.
    pub fn from_database(
        database: Database,
        fallback_families: Vec<String>,
        max_font_bytes: u64,
    ) -> Self {
        Self::build(
            database,
            fallback_families,
            max_font_bytes,
            Self::DEFAULT_CACHE_CAPACITY,
        )
    }

    /// Takes a configured database with an explicit finite resolved-face cache capacity.
    pub fn from_database_with_cache_capacity(
        database: Database,
        fallback_families: Vec<String>,
        max_font_bytes: u64,
        cache_capacity: usize,
    ) -> Result<Self> {
        if cache_capacity == 0 {
            return Err(Error::InvalidOption {
                field: "font_cache_capacity",
                value: "0".to_owned(),
            });
        }
        Ok(Self::build(
            database,
            fallback_families,
            max_font_bytes,
            cache_capacity,
        ))
    }

    fn build(
        database: Database,
        fallback_families: Vec<String>,
        max_font_bytes: u64,
        cache_capacity: usize,
    ) -> Self {
        let fallback_faces = fallback_families
            .iter()
            .map(|family| face_ids_for_name(&database, family))
            .collect();
        Self {
            database,
            fallback_faces,
            max_font_bytes,
            cache_capacity,
            cache: Mutex::new(CacheState::default()),
        }
    }

    /// Scans currently installed system fonts once and retains that immutable snapshot.
    pub fn with_system_fonts(fallback_families: Vec<String>, max_font_bytes: u64) -> Self {
        let mut database = Database::new();
        database.load_system_fonts();
        Self::from_database(database, fallback_families, max_font_bytes)
    }

    /// Returns the maximum number of successfully resolved faces retained.
    pub fn cache_capacity(&self) -> usize {
        self.cache_capacity
    }

    fn cached(
        &self,
        key: CacheKey,
        loader: impl FnOnce() -> Result<ResolvedFont>,
    ) -> Result<ResolvedFont> {
        let (slot, leader) = {
            let mut cache = self.cache.lock().map_err(|_| Error::FontCache {
                message: "font cache index is poisoned".to_owned(),
            })?;
            let tick = cache.tick();
            if let Some(entry) = cache.ready.get_mut(&key) {
                entry.last_used = tick;
                return Ok(entry.font.clone());
            }
            if let Some(slot) = cache.in_flight.get(&key) {
                (Arc::clone(slot), false)
            } else {
                let slot = Arc::new(LoadSlot::default());
                cache.in_flight.insert(key.clone(), Arc::clone(&slot));
                (slot, true)
            }
        };
        if !leader {
            return slot.wait();
        }

        let outcome = loader().map_err(FontLoadFailure::from_error);
        if let Ok(font) = &outcome {
            {
                let mut cache = self.cache.lock().map_err(|_| Error::FontCache {
                    message: "font cache index is poisoned".to_owned(),
                })?;
                cache.in_flight.remove(&key);
                let tick = cache.tick();
                cache.ready.insert(
                    key,
                    CacheEntry {
                        font: font.clone(),
                        last_used: tick,
                    },
                );
                while cache.ready.len() > self.cache_capacity {
                    let evicted = cache
                        .ready
                        .iter()
                        .min_by_key(|(_, entry)| entry.last_used)
                        .map(|(key, _)| key.clone())
                        .expect("over-capacity cache is nonempty");
                    cache.ready.remove(&evicted);
                }
            }
            slot.complete(outcome.clone())?;
        } else {
            slot.complete(outcome.clone())?;
            let mut cache = self.cache.lock().map_err(|_| Error::FontCache {
                message: "font cache index is poisoned".to_owned(),
            })?;
            cache.in_flight.remove(&key);
        }
        outcome.map_err(FontLoadFailure::into_error)
    }

    fn face_for_name(&self, name: &str) -> Option<ID> {
        face_ids_for_name(&self.database, name).into_iter().next()
    }

    fn load_system_face(&self, id: ID) -> Result<ResolvedFont> {
        let info = self.database.face(id).ok_or_else(|| Error::InvalidFont {
            identity: "configured-system-face".to_owned(),
            message: "fontdb face disappeared".to_owned(),
        })?;
        let face_index = info.index;
        let identity = FontIdentity::system(id, face_index, &info.post_script_name);
        self.cached(CacheKey::System { id, face_index }, || {
            let declared_bytes = match &info.source {
                Source::Binary(data) => {
                    u64::try_from(data.as_ref().as_ref().len()).unwrap_or(u64::MAX)
                }
                Source::File(path) => std::fs::metadata(path)
                    .map(|metadata| metadata.len())
                    .map_err(|error| Error::InvalidFont {
                        identity: identity.as_str().to_owned(),
                        message: format!("font bytes could not be inspected: {error}"),
                    })?,
                Source::SharedFile(_, data) => {
                    u64::try_from(data.as_ref().as_ref().len()).unwrap_or(u64::MAX)
                }
            };
            if declared_bytes > self.max_font_bytes {
                return Err(Error::FontBytesExceeded {
                    identity: identity.as_str().to_owned(),
                    actual_bytes: declared_bytes,
                    max_bytes: self.max_font_bytes,
                });
            }
            let (actual_bytes, bytes, face_index) = self
                .database
                .with_face_data(id, |bytes, index| {
                    let length = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
                    let owned = (length <= self.max_font_bytes).then(|| bytes.to_vec());
                    (length, owned, index)
                })
                .ok_or_else(|| Error::InvalidFont {
                    identity: identity.as_str().to_owned(),
                    message: "font bytes could not be read".to_owned(),
                })?;
            if actual_bytes > self.max_font_bytes {
                return Err(Error::FontBytesExceeded {
                    identity: identity.as_str().to_owned(),
                    actual_bytes,
                    max_bytes: self.max_font_bytes,
                });
            }
            let bytes = bytes.ok_or_else(|| Error::FontBytesExceeded {
                identity: identity.as_str().to_owned(),
                actual_bytes,
                max_bytes: self.max_font_bytes,
            })?;
            ResolvedFont::from_system_bytes(Arc::from(bytes), face_index, identity)
        })
    }
}

impl FontResolver for SystemFontResolver {
    fn resolve_primary(&self, resource: &FontResource) -> Result<Option<ResolvedFont>> {
        if resource.encoded_bytes().is_some() {
            return self
                .cached(
                    CacheKey::Embedded {
                        identity: resource.identity(),
                        resource_id: resource.id(),
                        face_index: 0,
                    },
                    || ResolvedFont::from_embedded_resource(resource, 0),
                )
                .map(Some);
        }
        for name in resource
            .family_name()
            .into_iter()
            .chain(std::iter::once(resource.font_name()))
        {
            if let Some(id) = self.face_for_name(name) {
                return self.load_system_face(id).map(Some);
            }
        }
        Ok(None)
    }

    fn resolve_fallback(&self, character: char) -> Result<Option<ResolvedFont>> {
        for faces in &self.fallback_faces {
            for id in faces {
                let font = self.load_system_face(*id)?;
                if font.glyph_for(character)?.is_some() {
                    return Ok(Some(font.into_configured_fallback()));
                }
            }
        }
        Ok(None)
    }
}

fn face_ids_for_name(database: &Database, name: &str) -> Vec<ID> {
    let requested = match name.to_ascii_lowercase().as_str() {
        "serif" => Family::Serif,
        "sans-serif" => Family::SansSerif,
        "monospace" => Family::Monospace,
        "cursive" => Family::Cursive,
        "fantasy" => Family::Fantasy,
        _ => Family::Name(name),
    };
    let families = [requested];
    let mut ids = database
        .query(&Query {
            families: &families,
            ..Query::default()
        })
        .into_iter()
        .collect::<Vec<_>>();
    for face in database.faces().filter(|face| {
        face.post_script_name.eq_ignore_ascii_case(name)
            || face
                .families
                .iter()
                .any(|(family, _)| family.eq_ignore_ascii_case(name))
    }) {
        if !ids.contains(&face.id) {
            ids.push(face.id);
        }
    }
    ids
}

struct Selection {
    glyph_id: u32,
    font: Option<ResolvedFont>,
    source: FontSource,
}

/// Resolves and positions the exact source-ordered glyph stream for one text object.
///
/// This does not shape, reorder, or render text. Explicit CG glyph identifiers
/// replace covered scalar positions. Explicit delta attributes, including zero,
/// are authoritative; only a genuinely absent axis uses the selected glyph advance.
/// When a CG mapping emits multiple glyphs, they retain declared glyph order and
/// advance consecutively without shaping; every emitted glyph records the whole
/// mapped scalar range. A mapping may span `TextCode` boundaries and is emitted
/// by the run containing its first scalar. The supplied resource ID must equal
/// [`TextObject::font_id`]; identity and semantic budgets are checked before the
/// resolver is invoked.
pub fn position_glyph_runs(
    resolver: &dyn FontResolver,
    resource: &FontResource,
    text: &TextObject,
    limits: &ResourceLimits,
) -> Result<Vec<GlyphRun>> {
    if resource.id() != text.font_id() {
        return Err(Error::FontResourceMismatch {
            text_object_id: text.object_id(),
            expected_font_id: text.font_id(),
            actual_resource_id: resource.id(),
        });
    }
    if !text.font_size().is_finite() || text.font_size() <= 0.0 {
        return Err(layout_error(text, "Size", "must be finite and positive"));
    }
    let total_scalars = text.runs().iter().try_fold(0usize, |total, run| {
        total
            .checked_add(run.text().chars().count())
            .ok_or_else(|| layout_error(text, "text characters", "character count overflow"))
    })?;
    if total_scalars > limits.max_text_characters_per_page {
        return Err(layout_error(
            text,
            "max_text_characters_per_page",
            &format!(
                "{total_scalars} exceeds {}",
                limits.max_text_characters_per_page
            ),
        ));
    }
    let mapped_codes = text
        .glyph_maps()
        .iter()
        .try_fold(0usize, |sum, map| sum.checked_add(map.code_count()))
        .ok_or_else(|| layout_error(text, "CGTransform", "mapped character count overflow"))?;
    let mapped_glyphs = text
        .glyph_maps()
        .iter()
        .try_fold(0usize, |sum, map| sum.checked_add(map.glyphs().len()))
        .ok_or_else(|| layout_error(text, "Glyphs", "mapped glyph count overflow"))?;
    let glyph_count = total_scalars
        .checked_sub(mapped_codes)
        .and_then(|count| count.checked_add(mapped_glyphs))
        .ok_or_else(|| layout_error(text, "Glyphs", "positioned glyph count overflow"))?;
    if glyph_count > limits.max_glyphs_per_page {
        return Err(layout_error(
            text,
            "max_glyphs_per_page",
            &format!("{glyph_count} exceeds {}", limits.max_glyphs_per_page),
        ));
    }
    let expansion = text
        .runs()
        .len()
        .checked_add(text.glyph_maps().len())
        .and_then(|count| count.checked_add(glyph_count))
        .ok_or_else(|| layout_error(text, "text expansion", "entry count overflow"))?;
    if expansion > limits.max_text_expansion_entries {
        return Err(layout_error(
            text,
            "max_text_expansion_entries",
            &format!("{expansion} exceeds {}", limits.max_text_expansion_entries),
        ));
    }
    let primary = resolver.resolve_primary(resource)?;
    if let Some(font) = &primary {
        let valid_source = if resource.encoded_bytes().is_some() {
            matches!(
                font.source(),
                FontSource::Embedded { resource_id } if *resource_id == resource.id()
            )
        } else {
            matches!(font.source(), FontSource::System { .. })
        };
        if !valid_source {
            return Err(Error::InvalidFont {
                identity: font.identity().to_owned(),
                message: "primary resolver returned a font with an inconsistent source".to_owned(),
            });
        }
    }

    let mut map_starts = HashMap::with_capacity(text.glyph_maps().len());
    for map in text.glyph_maps() {
        map_starts.insert(map.code_position(), map);
    }
    let requested = resource
        .family_name()
        .unwrap_or_else(|| resource.font_name())
        .to_owned();
    let mut runs = Vec::with_capacity(text.runs().len());
    let mut global_scalar = 0usize;
    let mut covered_until = 0usize;
    for run in text.runs() {
        let characters = run.text().char_indices().collect::<Vec<_>>();
        let run_start = global_scalar;
        let mut glyphs = Vec::new();
        let mut diagnostics = Vec::new();
        let mut local = 0usize;
        let mut x = run.x();
        let mut y = run.y();
        while local < characters.len() {
            let scalar_index = global_scalar + local;
            if scalar_index < covered_until {
                local += 1;
                continue;
            }
            if let Some(map) = map_starts.get(&scalar_index) {
                // ofdrw applies explicit glyph IDs to whatever face it
                // loaded, substituting a default font when the declared one
                // is unavailable; mirror that with the configured fallback
                // chain instead of failing the whole text object.
                let substitute = match primary.as_ref() {
                    Some(_) => None,
                    None => {
                        let font = resolver.resolve_fallback(characters[local].1)?;
                        if let Some(font) = &font {
                            if !matches!(font.source(), FontSource::ConfiguredFallback { .. }) {
                                return Err(Error::InvalidFont {
                                    identity: font.identity().to_owned(),
                                    message: "fallback resolver returned a font with an inconsistent source".to_owned(),
                                });
                            }
                        }
                        font
                    }
                };
                let selected = primary.as_ref().or(substitute.as_ref());
                let consumed = map.code_count().min(characters.len() - local);
                covered_until = scalar_index.checked_add(map.code_count()).ok_or_else(|| {
                    layout_error(text, "CGTransform", "mapped scalar range overflow")
                })?;
                let start_byte = characters[local].0;
                let end_local = local + consumed;
                let end_byte = characters
                    .get(end_local)
                    .map_or(run.text().len(), |(offset, _)| *offset);
                if let Some(selected) = selected {
                    if substitute.is_some() {
                        diagnostics.push(FontDiagnostic::FamilyFallback {
                            character: characters[local].1,
                            scalar_index,
                            requested: requested.clone(),
                            selected: selected.identity().to_owned(),
                        });
                    }
                    let mut inferred_x = 0.0;
                    let mut inferred_y = 0.0;
                    let transforms = map.transforms();
                    for (glyph_offset, glyph_id) in map.glyphs().iter().copied().enumerate() {
                        if substitute.is_some() {
                            // The substitute face has an unrelated glyph
                            // table, so an out-of-range glyph ID is skipped
                            // (ofdrw draws a null glyph) instead of failing.
                            if glyph_id >= selected.glyph_count() {
                                diagnostics.push(FontDiagnostic::MissingGlyph {
                                    character: characters[local].1,
                                    scalar_index,
                                    object_id: text.object_id(),
                                    used_visible_replacement: false,
                                });
                                continue;
                            }
                        } else {
                            validate_glyph(selected, glyph_id)?;
                        }
                        let character =
                            (glyph_offset < consumed).then(|| characters[local + glyph_offset].1);
                        let glyph_transform = transforms.get(glyph_offset).copied().flatten();
                        let (offset_x, offset_y) = glyph_transform
                            .map(|t| (t.x(), t.y()))
                            .unwrap_or((0.0, 0.0));
                        let advance = selected.advance_mm(glyph_id, text.font_size())?;
                        glyphs.push(PositionedGlyph {
                            glyph_id,
                            x: finite_coordinate(text, "glyph x", x + inferred_x + offset_x)?,
                            y: finite_coordinate(text, "glyph y", y + inferred_y + offset_y)?,
                            advance,
                            character,
                            source_range: start_byte..end_byte,
                            scalar_index,
                            source_scalar_range: scalar_index..covered_until,
                            font: Some(selected.clone()),
                            font_source: selected.source().clone(),
                            transform: glyph_transform.and_then(|t| t.matrix()),
                        });
                        inferred_x = finite_coordinate(text, "advance x", inferred_x + advance.0)?;
                        inferred_y = finite_coordinate(text, "advance y", inferred_y + advance.1)?;
                    }
                    let delta_x = if run.has_explicit_delta_x() {
                        run.delta_x()[local..end_local].iter().sum()
                    } else {
                        inferred_x
                    };
                    let delta_y = if run.has_explicit_delta_y() {
                        run.delta_y()[local..end_local].iter().sum()
                    } else {
                        inferred_y
                    };
                    x = finite_coordinate(text, "glyph x", x + delta_x)?;
                    y = finite_coordinate(text, "glyph y", y + delta_y)?;
                } else {
                    // No face at all: report every consumed scalar as missing
                    // and advance past the mapped range like ofdrw does when
                    // the font fails to load.
                    for offset in 0..consumed {
                        diagnostics.push(FontDiagnostic::MissingGlyph {
                            character: characters[local + offset].1,
                            scalar_index,
                            object_id: text.object_id(),
                            used_visible_replacement: false,
                        });
                    }
                    if run.has_explicit_delta_x() {
                        x = finite_coordinate(
                            text,
                            "glyph x",
                            x + run.delta_x()[local..end_local].iter().sum::<f64>(),
                        )?;
                    }
                    if run.has_explicit_delta_y() {
                        y = finite_coordinate(
                            text,
                            "glyph y",
                            y + run.delta_y()[local..end_local].iter().sum::<f64>(),
                        )?;
                    }
                }
                local = end_local;
                continue;
            }

            let (byte_start, character) = characters[local];
            let byte_end = characters
                .get(local + 1)
                .map_or(run.text().len(), |(offset, _)| *offset);
            let selection = select_character(
                resolver,
                primary.as_ref(),
                character,
                scalar_index,
                text.object_id(),
                &requested,
                &mut diagnostics,
            )?;
            let advance = selection
                .font
                .as_ref()
                .map(|font| font.advance_mm(selection.glyph_id, text.font_size()))
                .transpose()?
                .unwrap_or((text.font_size(), 0.0));
            glyphs.push(PositionedGlyph {
                glyph_id: selection.glyph_id,
                x: finite_coordinate(text, "glyph x", x)?,
                y: finite_coordinate(text, "glyph y", y)?,
                advance,
                character: Some(character),
                source_range: byte_start..byte_end,
                scalar_index,
                source_scalar_range: scalar_index..scalar_index + 1,
                font: selection.font.clone(),
                font_source: selection.source,
                transform: None,
            });
            let delta_x = if run.has_explicit_delta_x() {
                run.delta_x()[local]
            } else {
                advance.0
            };
            let delta_y = if run.has_explicit_delta_y() {
                run.delta_y()[local]
            } else {
                advance.1
            };
            x = finite_coordinate(text, "glyph x", x + delta_x)?;
            y = finite_coordinate(text, "glyph y", y + delta_y)?;
            local += 1;
        }
        global_scalar = global_scalar
            .checked_add(characters.len())
            .ok_or_else(|| layout_error(text, "text characters", "character index overflow"))?;
        runs.push(GlyphRun {
            text: Arc::from(run.text()),
            source_range: run_start..global_scalar,
            size_mm: text.font_size(),
            glyphs: glyphs.into(),
            diagnostics: diagnostics.into(),
        });
    }
    Ok(runs)
}

/// Per-scalar advances from the same font resolution the renderer positions text with.
///
/// This is the [`TextAdvanceProvider`] rofd-core asks for when a text run carries no
/// explicit `DeltaX`: the answer is the advance of the glyph the positioner selected,
/// summed per scalar for structured `Glyphs` mappings, so page geometry matches the
/// rendered glyphs. Resolution failures answer `None`, which keeps rofd-core's
/// font-size footprint.
#[derive(Clone, Copy)]
pub struct FontTextAdvances<'a> {
    resolver: &'a dyn FontResolver,
}

impl<'a> FontTextAdvances<'a> {
    /// Wraps the resolver a renderer would use for the same page.
    pub fn new(resolver: &'a dyn FontResolver) -> Self {
        Self { resolver }
    }
}

impl TextAdvanceProvider for FontTextAdvances<'_> {
    fn advances_mm(&self, page: &Page, text: &TextObject, size_mm: f64) -> Option<Vec<f64>> {
        let resource = page.font_resource(text.font_id()).ok()?;
        let runs =
            position_glyph_runs(self.resolver, &resource, text, page.resource_limits()).ok()?;
        let total = text.runs().iter().try_fold(0usize, |total, run| {
            total.checked_add(run.text().chars().count())
        })?;
        // Glyphs of one structured mapping share the scalar index of the mapped
        // range, so their advances sum there, exactly as the positioner advances the
        // mapped run. A scalar without a glyph keeps the size footprint the builder
        // uses on its own.
        let mut advances: Vec<Option<f64>> = vec![None; total];
        for run in &runs {
            for glyph in run.glyphs() {
                let slot = advances.get_mut(glyph.scalar_index())?;
                let (x, _) = glyph.advance();
                *slot = Some(slot.unwrap_or(0.0) + x);
            }
        }
        Some(
            advances
                .into_iter()
                .map(|advance| advance.unwrap_or(size_mm))
                .collect(),
        )
    }
}

fn select_character(
    resolver: &dyn FontResolver,
    primary: Option<&ResolvedFont>,
    character: char,
    scalar_index: usize,
    object_id: u64,
    requested: &str,
    diagnostics: &mut Vec<FontDiagnostic>,
) -> Result<Selection> {
    if let Some(font) = primary {
        if let Some(glyph_id) = font.glyph_for(character)? {
            return Ok(Selection {
                glyph_id,
                font: Some(font.clone()),
                source: font.source().clone(),
            });
        }
    }
    if let Some(font) = resolver.resolve_fallback(character)? {
        if !matches!(font.source(), FontSource::ConfiguredFallback { .. }) {
            return Err(Error::InvalidFont {
                identity: font.identity().to_owned(),
                message: "fallback resolver returned a font with an inconsistent source".to_owned(),
            });
        }
        if let Some(glyph_id) = font.glyph_for(character)? {
            diagnostics.push(FontDiagnostic::FamilyFallback {
                character,
                scalar_index,
                requested: requested.to_owned(),
                selected: font.identity().to_owned(),
            });
            return Ok(Selection {
                glyph_id,
                source: font.source().clone(),
                font: Some(font),
            });
        }
    }
    for replacement in ['\u{25a1}', '\u{fffd}'] {
        if let Some(font) = primary {
            if let Some(glyph_id) = font.glyph_for(replacement)? {
                diagnostics.push(FontDiagnostic::MissingGlyph {
                    character,
                    scalar_index,
                    object_id,
                    used_visible_replacement: true,
                });
                return Ok(Selection {
                    glyph_id,
                    source: font.source().clone(),
                    font: Some(font.clone()),
                });
            }
        }
        if let Some(font) = resolver.resolve_fallback(replacement)? {
            if !matches!(font.source(), FontSource::ConfiguredFallback { .. }) {
                return Err(Error::InvalidFont {
                    identity: font.identity().to_owned(),
                    message: "fallback resolver returned a font with an inconsistent source"
                        .to_owned(),
                });
            }
            if let Some(glyph_id) = font.glyph_for(replacement)? {
                diagnostics.push(FontDiagnostic::MissingGlyph {
                    character,
                    scalar_index,
                    object_id,
                    used_visible_replacement: true,
                });
                return Ok(Selection {
                    glyph_id,
                    source: font.source().clone(),
                    font: Some(font),
                });
            }
        }
    }
    diagnostics.push(FontDiagnostic::MissingGlyph {
        character,
        scalar_index,
        object_id,
        used_visible_replacement: false,
    });
    Ok(Selection {
        glyph_id: 0,
        font: None,
        source: FontSource::Missing,
    })
}

fn validate_glyph(font: &ResolvedFont, glyph_id: u32) -> Result<()> {
    if glyph_id >= font.glyph_count() {
        return Err(Error::InvalidGlyph {
            identity: font.identity().to_owned(),
            glyph_id,
            glyph_count: font.glyph_count(),
        });
    }
    Ok(())
}

fn finite_coordinate(text: &TextObject, field: &'static str, value: f64) -> Result<f64> {
    if value.is_finite() {
        Ok(value)
    } else {
        Err(layout_error(
            text,
            field,
            "arithmetic produced a non-finite value",
        ))
    }
}

fn layout_error(text: &TextObject, field: &'static str, message: &str) -> Error {
    Error::InvalidTextLayout {
        object_id: text.object_id(),
        field,
        message: message.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use std::mem::size_of;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    use super::*;

    const FONT: &[u8] = include_bytes!("../tests/fixtures/fonts/phase3-subset.ttf");

    fn fixture_font(bytes: Arc<[u8]>) -> Result<ResolvedFont> {
        ResolvedFont::from_system_bytes(bytes, 0, FontIdentity::new("unit-fixture")?)
    }

    fn key(face_index: u32) -> CacheKey {
        CacheKey::System {
            id: ID::dummy(),
            face_index,
        }
    }

    #[test]
    fn cache_is_single_flight_and_does_not_publish_failures() {
        let resolver = Arc::new(SystemFontResolver::empty(Vec::new(), 1 << 20));
        let bytes: Arc<[u8]> = Arc::from(FONT);
        let initializations = Arc::new(AtomicUsize::new(0));
        let threads = (0..12)
            .map(|_| {
                let resolver = Arc::clone(&resolver);
                let bytes = Arc::clone(&bytes);
                let initializations = Arc::clone(&initializations);
                thread::spawn(move || {
                    resolver.cached(key(1), || {
                        initializations.fetch_add(1, Ordering::SeqCst);
                        fixture_font(bytes)
                    })
                })
            })
            .collect::<Vec<_>>();
        for thread in threads {
            thread.join().unwrap().unwrap();
        }
        assert_eq!(initializations.load(Ordering::SeqCst), 1);

        let failed_key = key(2);
        let attempts = AtomicUsize::new(0);
        assert!(resolver
            .cached(failed_key.clone(), || {
                attempts.fetch_add(1, Ordering::SeqCst);
                Err(Error::InvalidFont {
                    identity: "retry".to_owned(),
                    message: "injected failure".to_owned(),
                })
            })
            .is_err());
        resolver
            .cached(failed_key, || {
                attempts.fetch_add(1, Ordering::SeqCst);
                fixture_font(Arc::from(FONT))
            })
            .unwrap();
        assert_eq!(attempts.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn cache_keys_are_constant_size_and_warm_hits_do_not_reload_bytes() {
        assert!(size_of::<CacheKey>() <= 48);
        let resolver = SystemFontResolver::empty(Vec::new(), 1 << 20);
        let loads = AtomicUsize::new(0);
        let cache_key = key(1);
        let first = resolver
            .cached(cache_key.clone(), || {
                loads.fetch_add(1, Ordering::SeqCst);
                fixture_font(Arc::from(FONT))
            })
            .unwrap();
        let second = resolver
            .cached(cache_key, || {
                loads.fetch_add(1, Ordering::SeqCst);
                fixture_font(Arc::from(FONT))
            })
            .unwrap();
        assert_eq!(loads.load(Ordering::SeqCst), 1);
        assert!(Arc::ptr_eq(
            &first.encoded_bytes_arc(),
            &second.encoded_bytes_arc()
        ));
    }

    #[test]
    fn cache_capacity_uses_deterministic_lru_and_eviction_keeps_live_fonts_valid() {
        let resolver = SystemFontResolver::from_database_with_cache_capacity(
            Database::new(),
            Vec::new(),
            1 << 20,
            2,
        )
        .unwrap();
        let loads = AtomicUsize::new(0);
        let load = || {
            loads.fetch_add(1, Ordering::SeqCst);
            fixture_font(Arc::from(FONT))
        };

        let first = resolver.cached(key(1), load).unwrap();
        let evicted_but_live = resolver.cached(key(2), load).unwrap();
        resolver.cached(key(1), load).unwrap();
        resolver.cached(key(3), load).unwrap();
        assert_eq!(loads.load(Ordering::SeqCst), 3);
        assert_eq!(resolver.cache.lock().unwrap().ready.len(), 2);

        assert!(evicted_but_live.glyph_index('A').unwrap().is_some());
        resolver.cached(key(2), load).unwrap();
        assert_eq!(loads.load(Ordering::SeqCst), 4);
        assert!(first.glyph_index('A').unwrap().is_some());
    }

    #[test]
    fn unrelated_cache_loads_are_not_serialized_by_the_index_lock() {
        let resolver = Arc::new(SystemFontResolver::empty(Vec::new(), 1 << 20));
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_one_tx, release_one_rx) = mpsc::channel();
        let (release_two_tx, release_two_rx) = mpsc::channel();

        let first_resolver = Arc::clone(&resolver);
        let first_entered = entered_tx.clone();
        let first = thread::spawn(move || {
            first_resolver.cached(key(1), || {
                first_entered.send(1).unwrap();
                release_one_rx.recv().unwrap();
                fixture_font(Arc::from(FONT))
            })
        });
        assert_eq!(entered_rx.recv_timeout(Duration::from_secs(2)).unwrap(), 1);

        let second_resolver = Arc::clone(&resolver);
        let second = thread::spawn(move || {
            second_resolver.cached(key(2), || {
                entered_tx.send(2).unwrap();
                release_two_rx.recv().unwrap();
                fixture_font(Arc::from(FONT))
            })
        });
        let second_entered = entered_rx.recv_timeout(Duration::from_secs(2));
        release_one_tx.send(()).unwrap();
        release_two_tx.send(()).unwrap();
        assert_eq!(second_entered.unwrap(), 2);
        first.join().unwrap().unwrap();
        second.join().unwrap().unwrap();
    }
}
