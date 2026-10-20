use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};

use serde::{de::DeserializeOwned, Deserialize};

use crate::container::Container;
use crate::path::PackagePath;
use crate::raw::{self, CommonData, DocumentRoot, OfdRoot};
use crate::{Error, LoadOptions, Result};

/// Descriptive metadata stored in an OFD document.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Metadata {
    /// Document identifier.
    pub document_id: Option<String>,
    /// Document title.
    pub title: Option<String>,
    /// Document author.
    pub author: Option<String>,
    /// Document subject.
    pub subject: Option<String>,
    /// Document abstract.
    pub abstract_text: Option<String>,
    /// Producing application.
    pub creator: Option<String>,
    /// Producing application version.
    pub creator_version: Option<String>,
    /// Original creation date as stored by the producer.
    pub creation_date: Option<String>,
    /// Modification date as stored by the producer.
    pub modification_date: Option<String>,
    /// Keywords in declaration order, preserving repeated and empty entries.
    pub keywords: Vec<String>,
}

/// Stable categories for recoverable OFD problems.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum WarningCode {
    /// The page omitted Area and inherited the document PageArea.
    PageAreaFallback,
    /// The document CommonData omitted PageArea; only pages declaring their
    /// own Area remain loadable.
    DocumentPageAreaMissing,
    /// A signature or one of its stamp annotations could not be parsed and
    /// was skipped.
    SignatureSkipped,
    /// An element that is not a known page graphic unit was skipped in
    /// lenient mode so the rest of the page could load.
    UnknownGraphicUnitSkipped,
    /// A page annotation or one of its files could not be parsed and was
    /// skipped.
    AnnotationSkipped,
    /// The OFD.xml contains multiple DocBody elements; only the first
    /// (current version) was loaded and historical versions were skipped.
    HistoricalDocBodySkipped,
    /// Malformed or unresolved optional navigation data was retained safely.
    NavigationInvalid,
    /// An unsupported navigation action, event, or destination mode was retained.
    NavigationUnsupported,
    /// A historical navigation namespace or numeric destination child used
    /// compatibility parsing.
    NavigationCompatibility,
}

/// A recoverable OFD conformance diagnostic.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Warning {
    /// Machine-readable category.
    pub code: WarningCode,
    /// Path inside the OFD package.
    pub path: String,
    /// Human-readable explanation.
    pub message: String,
}

#[derive(Debug)]
struct PageReference {
    id: u64,
    path: PackagePath,
    cache: OnceLock<Arc<PageData>>,
    initialization: Mutex<()>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TemplateZOrder {
    Background,
    Foreground,
}

#[derive(Debug)]
struct TemplateReference {
    id: u64,
    path: PackagePath,
    default_z_order: TemplateZOrder,
    cache: OnceLock<Arc<TemplateData>>,
}

#[derive(Debug)]
struct DocumentInner {
    container: Container,
    document_path: PackagePath,
    limits: crate::ResourceLimits,
    metadata: Metadata,
    default_page_area: Option<crate::raw::PageArea>,
    pages: Vec<PageReference>,
    templates: HashMap<u64, TemplateReference>,
    strictness: crate::Strictness,
    warnings: Mutex<Vec<Warning>>,
    resource_paths: Vec<PackagePath>,
    resource_catalog: OnceLock<Arc<crate::resources::ResourceCatalog>>,
    resource_initialization: Mutex<()>,
    signatures_path: Option<PackagePath>,
    stamp_annotations: OnceLock<Vec<crate::StampAnnotation>>,
    stamp_initialization: Mutex<()>,
    annotations_path: Option<PackagePath>,
    page_annotations: OnceLock<PageAnnotations>,
    page_annotation_initialization: Mutex<()>,
    navigation: OnceLock<crate::navigation::NavigationData>,
    navigation_initialization: Mutex<()>,
    bookmarks: OnceLock<crate::navigation::BookmarkData>,
    bookmark_initialization: Mutex<()>,
    navigation_warning_state: Mutex<NavigationWarningState>,
    /// Font-resolved per-scalar geometry for the page text index, when the loader
    /// supplied a provider (see [`crate::TextMetricsProvider`]).
    text_metrics: Option<TextMetricsSource>,
}

/// Debug-friendly owner of the optional text metrics provider.
#[derive(Clone)]
struct TextMetricsSource(Arc<dyn crate::TextMetricsProvider>);

impl std::fmt::Debug for TextMetricsSource {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("TextMetricsSource(<provider>)")
    }
}

#[derive(Debug, Default)]
struct NavigationWarningState {
    bookmarks: bool,
    historical_paths: std::collections::HashSet<String>,
}

#[derive(Debug, Default)]
struct PageAnnotations {
    annotations: Vec<crate::PageAnnotation>,
    // Preserve only the first original limit message. The public annotation
    // API remains tolerant, but navigation must not mistake truncated content
    // for a complete successful result, regardless of cache initialization order.
    first_limit: Option<String>,
}

impl PageAnnotations {
    fn record_limit(&mut self, error: &Error) {
        if let Error::LimitExceeded(message) = error {
            if self.first_limit.is_none() {
                self.first_limit = Some(message.clone());
            }
        }
    }
}

/// A read-only OFD document.
#[derive(Clone, Debug)]
pub struct Document(Arc<DocumentInner>);

#[derive(Debug)]
struct PageData {
    size: crate::Rect,
    application_box: Option<crate::Rect>,
    content_box: Option<crate::Rect>,
    bleed_box: Option<crate::Rect>,
    layers: Vec<crate::Layer>,
    semantic_text: OnceLock<crate::PageText>,
    semantic_text_initialization: Mutex<()>,
    navigation_anchors: Vec<(usize, crate::navigation::deferred::Actions)>,
    links: OnceLock<Vec<crate::PageLink>>,
    links_initialization: Mutex<()>,
}

#[derive(Debug)]
struct TemplateData {
    layers: Vec<crate::Layer>,
    usage: crate::content::ContentUsage,
    referenced_templates: Vec<u64>,
    navigation_anchors: Vec<(usize, crate::navigation::deferred::Actions)>,
}

/// One parsed page in an OFD document.
#[derive(Clone, Debug)]
pub struct Page {
    _document: Arc<DocumentInner>,
    index: usize,
    object_id: u64,
    data: Arc<PageData>,
}

impl Page {
    /// Returns the zero-based page index.
    pub fn index(&self) -> usize {
        self.index
    }

    /// Returns the OFD object identifier of the page.
    pub fn object_id(&self) -> u64 {
        self.object_id
    }

    /// Returns the effective physical page box in millimetres.
    pub fn size(&self) -> crate::Rect {
        self.data.size
    }

    /// Returns the application box, if declared.
    ///
    /// The application box defines the visible area in the reader application.
    /// When absent, the physical box should be used.
    pub fn application_box(&self) -> Option<crate::Rect> {
        self.data.application_box
    }

    /// Returns the content box, if declared.
    ///
    /// The content box defines the area that contains page content.
    pub fn content_box(&self) -> Option<crate::Rect> {
        self.data.content_box
    }

    /// Returns the bleed box, if declared.
    ///
    /// The bleed box defines the area that extends beyond the physical box
    /// for printing purposes.
    pub fn bleed_box(&self) -> Option<crate::Rect> {
        self.data.bleed_box
    }

    /// Returns immutable layers in effective template/page paint order.
    pub fn layers(&self) -> &[crate::Layer] {
        &self.data.layers
    }

    /// Returns flattened source text and per-character geometry in physical-page
    /// millimetres, in effective paint order.
    ///
    /// Visible annotation appearances follow the page layers. Distinct nonempty
    /// text objects are separated by synthesized newlines. Geometry uses validated OFD
    /// positioning; a scalar whose run declares no `DeltaX` advances by the advance of
    /// a provider's font when the document was loaded with one, and its box spans that
    /// font's ascent and descent around the baseline, so it covers the painted glyph.
    /// Without a provider the font-size footprint stays in place. The complete result
    /// is cached and shared by cloned page handles. Failed builds are not cached and
    /// may be retried on a later call.
    pub fn text(&self) -> crate::Result<&crate::PageText> {
        if let Some(text) = self.data.semantic_text.get() {
            return Ok(text);
        }
        let _initialization = self.data.semantic_text_initialization.lock().map_err(|_| {
            Error::Internal("semantic text initialization lock is poisoned".to_owned())
        })?;
        if let Some(text) = self.data.semantic_text.get() {
            return Ok(text);
        }
        let metrics = self
            ._document
            .text_metrics
            .as_ref()
            .map(|metrics| metrics.0.as_ref() as &dyn crate::TextMetricsProvider);
        let text = crate::semantic::build_page_text(self, metrics)?;
        Ok(self.data.semantic_text.get_or_init(|| text))
    }

    /// Returns inert actions and conservative clickable regions in page millimetres.
    ///
    /// Each source action has a separate entry. Results and warnings are shared
    /// by cloned page handles; malformed explicit regions never become fallback
    /// page-sized targets. Page actions precede effective template/layer content;
    /// visible annotation actions and appearances follow it. Graphic owner
    /// actions precede their children. No action is executed by this library.
    ///
    /// Explicit region areas use the full content transform and retain separate
    /// conservative bounds (Bezier control hulls or full corrected ellipses),
    /// without clipping or occlusion hit-testing. Without an explicit region,
    /// the owner boundary uses only its parent transform; absent boundaries use
    /// the physical page. Strict mode rejects invalid region geometry; lenient
    /// mode skips the affected link or transformed subtree with a warning.
    ///
    /// Object/action XML and region-command budgets apply cumulatively to the
    /// effective page, including repeated references and annotations. Expanded
    /// strings and diagnostics share an independent `max_entry_size` budget.
    /// Resource limits are fatal in both modes. Failed queries publish neither
    /// a link cache nor navigation warnings and can be retried. Named bookmarks
    /// are resolved independently of unrelated outline errors.
    pub fn links(&self) -> Result<&[crate::PageLink]> {
        if let Some(links) = self.data.links.get() {
            return Ok(links);
        }
        let _initialization = self
            .data
            .links_initialization
            .lock()
            .map_err(|_| Error::Internal("page links initialization lock poisoned".into()))?;
        if let Some(links) = self.data.links.get() {
            return Ok(links);
        }
        let document = Document(Arc::clone(&self._document));
        let (links, warnings, bookmarks_used) =
            crate::links::build(self, &document, &self.data.navigation_anchors)?;
        document.commit_navigation_warnings(bookmarks_used, warnings)?;
        Ok(self.data.links.get_or_init(|| links))
    }

    /// Returns the immutable resource limits used to validate this page and its document.
    pub fn resource_limits(&self) -> &crate::ResourceLimits {
        &self._document.limits
    }

    /// Looks up a font resource in the document that owns this page.
    pub fn font_resource(&self, resource_id: u64) -> Result<crate::FontResource> {
        Document(Arc::clone(&self._document)).font_resource(resource_id)
    }

    /// Looks up an image resource in the document that owns this page.
    pub fn image_resource(&self, resource_id: u64) -> Result<crate::ImageResource> {
        Document(Arc::clone(&self._document)).image_resource(resource_id)
    }

    /// Returns the signature stamp annotations targeting this page.
    ///
    /// Signature parsing problems are recorded as warnings on the owning
    /// document and yield no annotations, so an empty result means either no
    /// stamps or skipped ones; inspect [`Document::warnings`] to tell apart.
    pub fn stamp_annotations(&self) -> Vec<crate::StampAnnotation> {
        Document(Arc::clone(&self._document))
            .stamp_annotations()
            .unwrap_or_default()
            .into_iter()
            .filter(|annotation| annotation.page_ref == self.object_id)
            .collect()
    }

    /// Returns the page annotations targeting this page, in declaration
    /// order.
    ///
    /// Like signature stamps, an annotation that fails to parse is skipped
    /// and reported through [`Document::warnings`] with
    /// [`WarningCode::AnnotationSkipped`].
    pub fn annotations(&self) -> Vec<crate::PageAnnotation> {
        Document(Arc::clone(&self._document))
            .page_annotations()
            .unwrap_or_default()
            .into_iter()
            .filter(|annotation| annotation.page_ref == self.object_id)
            .collect()
    }
}

impl Document {
    /// Opens an OFD document from a host file path.
    pub fn open(path: impl AsRef<Path>, options: LoadOptions) -> Result<Self> {
        Self::open_with_metrics(path, options, None)
    }

    /// Opens an OFD document from a host file path, resolving per-scalar text
    /// geometry through `metrics`.
    pub fn open_with_metrics(
        path: impl AsRef<Path>,
        options: LoadOptions,
        metrics: Option<Arc<dyn crate::TextMetricsProvider>>,
    ) -> Result<Self> {
        let path = path.as_ref();
        let bytes = fs::read(path).map_err(|source| Error::Io {
            path: path.to_owned(),
            source,
        })?;
        Self::from_bytes_with_metrics(bytes, options, metrics)
    }

    /// Opens an OFD document from owned bytes.
    pub fn from_bytes(bytes: Vec<u8>, options: LoadOptions) -> Result<Self> {
        Self::from_bytes_with_metrics(bytes, options, None)
    }

    /// Opens an OFD document from owned bytes, resolving per-scalar text geometry
    /// through `metrics`.
    ///
    /// Without a provider, text geometry keeps the font-size footprint. With one, a
    /// scalar whose run declares no `DeltaX` advances by the font's advance and its
    /// box spans the font's extent around the baseline, which is what a renderer
    /// paints.
    pub fn from_bytes_with_metrics(
        bytes: Vec<u8>,
        options: LoadOptions,
        metrics: Option<Arc<dyn crate::TextMetricsProvider>>,
    ) -> Result<Self> {
        let strictness = options.strictness;
        let limits = options.limits;
        let container = Container::from_bytes(bytes, limits.clone())?;
        let entry_path = PackagePath::new("OFD.xml")?;
        let ofd: OfdRoot = parse_xml(&container, &entry_path, limits.max_xml_depth)?;
        let mut initial_warnings = Vec::new();
        if ofd.doc_bodies.is_empty() {
            return Err(Error::InvalidStructure {
                path: entry_path.as_str().to_owned(),
                message: "OFD.xml must contain at least one DocBody".to_owned(),
            });
        }
        // The first DocBody is the current document version; subsequent
        // DocBody elements are historical versions (GB/T 33190-2016 7.3).
        // Only the first body is loaded; the rest are acknowledged but not
        // rendered, matching ofdrw behaviour.
        if ofd.doc_bodies.len() > 1 {
            initial_warnings.push(Warning {
                code: WarningCode::HistoricalDocBodySkipped,
                path: entry_path.as_str().to_owned(),
                message: format!(
                    "{} DocBody elements found; only the first (current version) is loaded",
                    ofd.doc_bodies.len()
                ),
            });
        }
        let body = ofd
            .doc_bodies
            .into_iter()
            .next()
            .expect("non-empty checked above");
        let document_path = entry_path.resolve(&body.doc_root)?;
        let root: DocumentRoot = parse_xml(&container, &document_path, limits.max_xml_depth)?;
        let CommonData {
            page_area,
            public_res,
            document_res,
            template_pages,
        } = root.common_data;
        if page_area.is_none() {
            if strictness == crate::Strictness::Strict {
                return Err(Error::InvalidStructure {
                    path: document_path.as_str().to_owned(),
                    message: "CommonData.PageArea is missing".to_owned(),
                });
            }
            initial_warnings.push(Warning {
                code: WarningCode::DocumentPageAreaMissing,
                path: document_path.as_str().to_owned(),
                message: "CommonData.PageArea is missing; pages must declare their own Area"
                    .to_owned(),
            });
        }
        let mut resource_paths = Vec::new();
        for declaration in [public_res, document_res].into_iter().flatten() {
            if resource_paths.len() >= limits.max_resource_files {
                return Err(Error::LimitExceeded(format!(
                    "resource file count {} exceeds limit {}",
                    resource_paths.len().saturating_add(1),
                    limits.max_resource_files
                )));
            }
            resource_paths.push(
                document_path
                    .resolve(&declaration)
                    .map_err(|error| with_error_path(error, &document_path))?,
            );
        }
        let pages = root
            .pages
            .pages
            .into_iter()
            .map(|page| {
                Ok(PageReference {
                    id: page.id,
                    path: document_path.resolve(&page.base_loc)?,
                    cache: OnceLock::new(),
                    initialization: Mutex::new(()),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let mut templates = HashMap::new();
        for template in template_pages {
            let id = parse_template_id(&template.id, &document_path)?;
            if templates.contains_key(&id) {
                return Err(Error::InvalidStructure {
                    path: document_path.as_str().to_owned(),
                    message: format!("duplicate template ID {id}"),
                });
            }
            let path = document_path
                .resolve(&template.base_loc)
                .map_err(|error| with_error_path(error, &document_path))?;
            let default_z_order = parse_template_z_order(
                template.z_order.as_deref(),
                TemplateZOrder::Background,
                &document_path,
            )?;
            templates.insert(
                id,
                TemplateReference {
                    id,
                    path,
                    default_z_order,
                    cache: OnceLock::new(),
                },
            );
        }
        let info = body.doc_info;
        let annotations_path = root
            .annotations
            .as_deref()
            .map(str::trim)
            .and_then(|declaration| match document_path.resolve(declaration) {
                Ok(path) => Some(path),
                Err(error) => {
                    initial_warnings.push(Warning {
                        code: WarningCode::AnnotationSkipped,
                        path: declaration.to_owned(),
                        message: format!("Annotations location is invalid: {error}"),
                    });
                    None
                }
            });
        let signatures_path = match body.signatures.as_deref().map(str::trim) {
            None => None,
            Some(declaration) => match entry_path.resolve(declaration) {
                Ok(path) => Some(path),
                Err(error) => {
                    initial_warnings.push(Warning {
                        code: WarningCode::SignatureSkipped,
                        path: declaration.to_owned(),
                        message: format!("Signatures location is invalid: {error}"),
                    });
                    None
                }
            },
        };
        Ok(Self(Arc::new(DocumentInner {
            container,
            document_path,
            limits,
            text_metrics: metrics.map(TextMetricsSource),
            metadata: Metadata {
                document_id: info.document_id,
                title: info.title,
                author: info.author,
                subject: info.subject,
                abstract_text: info.abstract_,
                creator: info.creator,
                creator_version: info.creator_version,
                creation_date: info.creation_date,
                modification_date: info.mod_date,
                keywords: info.keywords.entries,
            },
            default_page_area: page_area,
            pages,
            templates,
            strictness,
            warnings: Mutex::new(initial_warnings),
            resource_paths,
            resource_catalog: OnceLock::new(),
            resource_initialization: Mutex::new(()),
            signatures_path,
            stamp_annotations: OnceLock::new(),
            stamp_initialization: Mutex::new(()),
            annotations_path,
            page_annotations: OnceLock::new(),
            page_annotation_initialization: Mutex::new(()),
            navigation: OnceLock::new(),
            navigation_initialization: Mutex::new(()),
            bookmarks: OnceLock::new(),
            bookmark_initialization: Mutex::new(()),
            navigation_warning_state: Mutex::new(NavigationWarningState::default()),
        })))
    }

    /// Returns document metadata.
    pub fn metadata(&self) -> &Metadata {
        &self.0.metadata
    }

    /// Returns the number of indexed pages.
    pub fn page_count(&self) -> usize {
        self.0.pages.len()
    }

    /// Returns the outline tree as an immutable preorder array.
    ///
    /// Navigation is parsed only on first query and shared by document clones.
    /// Its semantic errors do not prevent opening the document or reading pages.
    /// Strict mode rejects malformed or unresolved destinations; lenient mode
    /// retains usable entries and reports navigation warnings once. Unsupported
    /// action types and events remain inspectable in both modes and are never
    /// executed. The advisory OFD `Count` does not determine tree topology.
    /// Native namespaces are the standard `http://www.ofdspec.org/2016`,
    /// unqualified XML, and the warned historical `http://www.ofdspec.org` alias.
    /// Numeric `Dest` child compatibility never overrides normative attributes.
    pub fn outline(&self) -> Result<&[crate::OutlineNode]> {
        if let Some(data) = self.0.navigation.get() {
            return Ok(&data.outlines);
        }
        let _initialization =
            self.0.navigation_initialization.lock().map_err(|_| {
                Error::Internal("navigation initialization lock poisoned".to_owned())
            })?;
        if let Some(data) = self.0.navigation.get() {
            return Ok(&data.outlines);
        }
        let bytes = self.0.container.read(&self.0.document_path)?;
        let pages: Vec<_> = self
            .0
            .pages
            .iter()
            .enumerate()
            .map(|(index, page)| (page.id, index))
            .collect();
        let (data, warnings) = crate::navigation::parse_navigation(
            &bytes,
            self.0.document_path.as_str(),
            &self.0.limits,
            self.0.strictness,
            &pages,
            self.bookmarks()?,
        )?;
        self.commit_navigation_warnings(true, warnings)?;
        Ok(&self.0.navigation.get_or_init(|| data).outlines)
    }

    pub(crate) fn page_indices(&self) -> Vec<(u64, usize)> {
        self.0
            .pages
            .iter()
            .enumerate()
            .map(|(index, page)| (page.id, index))
            .collect()
    }

    pub(crate) fn bookmarks(&self) -> Result<&crate::navigation::BookmarkData> {
        if let Some(data) = self.0.bookmarks.get() {
            return Ok(data);
        }
        let _initialization = self
            .0
            .bookmark_initialization
            .lock()
            .map_err(|_| Error::Internal("bookmark initialization lock poisoned".into()))?;
        if let Some(data) = self.0.bookmarks.get() {
            return Ok(data);
        }
        let bytes = self.0.container.read(&self.0.document_path)?;
        let data = crate::navigation::parse_bookmarks(
            &bytes,
            self.0.document_path.as_str(),
            &self.0.limits,
            self.0.strictness,
            &self.page_indices(),
        )?;
        Ok(self.0.bookmarks.get_or_init(|| data))
    }

    pub(crate) fn commit_navigation_warnings(
        &self,
        bookmarks_used: bool,
        mut pending: Vec<Warning>,
    ) -> Result<()> {
        let mut state = self
            .0
            .navigation_warning_state
            .lock()
            .map_err(|_| Error::Internal("navigation warning lock poisoned".into()))?;
        if bookmarks_used && !state.bookmarks {
            let mut bookmark_warnings = self.bookmarks()?.warnings.clone();
            bookmark_warnings.append(&mut pending);
            pending = bookmark_warnings;
        }
        let mut warnings = self
            .0
            .warnings
            .lock()
            .map_err(|_| Error::Internal("document warning lock poisoned".into()))?;
        for warning in pending {
            if warning.code == WarningCode::NavigationCompatibility
                && warning.message
                    == "accepted historical OFD navigation namespace http://www.ofdspec.org"
                && !state.historical_paths.insert(warning.path.clone())
            {
                continue;
            }
            warnings.push(warning);
        }
        state.bookmarks |= bookmarks_used;
        Ok(())
    }

    /// Looks up a font resource after atomically validating all declared catalogs.
    ///
    /// An optional embedded font file is read only when its font is requested
    /// and is bounded by [`crate::ResourceLimits::max_font_bytes`].
    pub fn font_resource(&self, object_id: u64) -> Result<crate::FontResource> {
        self.resource_catalog()?
            .font(object_id, &self.0.container, self.0.limits.max_font_bytes)
    }

    /// Looks up an image resource after atomically validating all declared catalogs.
    ///
    /// Encoded bytes are read only when the image is requested and are bounded
    /// by [`crate::ResourceLimits::max_encoded_image_bytes`].
    pub fn image_resource(&self, object_id: u64) -> Result<crate::ImageResource> {
        self.resource_catalog()?.image(
            object_id,
            &self.0.container,
            self.0.limits.max_encoded_image_bytes,
        )
    }

    /// Reads the physical page box without loading or caching graphical objects.
    ///
    /// This metadata-only operation respects XML/package limits and strictness,
    /// but does not validate page content. Call [`Self::page`] to load content.
    pub fn page_size(&self, index: usize) -> Result<crate::Rect> {
        #[derive(Deserialize)]
        struct Geometry {
            #[serde(rename = "Area")]
            area: Option<raw::PageArea>,
        }
        let reference = self.0.pages.get(index).ok_or(Error::PageOutOfRange {
            index,
            page_count: self.page_count(),
        })?;
        let validate = |size: crate::Rect| {
            if size.width <= 0. || size.height <= 0. {
                Err(Error::InvalidStructure {
                    path: reference.path.as_str().to_owned(),
                    message: "page dimensions must be positive".into(),
                })
            } else {
                Ok(size)
            }
        };
        if let Some(data) = reference.cache.get() {
            return validate(data.size);
        }
        let geometry: Geometry = parse_xml(
            &self.0.container,
            &reference.path,
            self.0.limits.max_xml_depth,
        )?;
        let area = match geometry.area {
            Some(area) if area.physical_box.is_some() => area,
            area => {
                let missing = if area.is_some() {
                    "Page.Area PhysicalBox is missing"
                } else {
                    "Page.Area is missing"
                };
                if self.0.strictness == crate::Strictness::Strict {
                    return Err(Error::InvalidStructure {
                        path: reference.path.as_str().to_owned(),
                        message: missing.to_owned(),
                    });
                }
                self.0
                    .default_page_area
                    .clone()
                    .ok_or_else(|| Error::InvalidStructure {
                        path: reference.path.as_str().to_owned(),
                        message: format!("{missing} and the document declares no PageArea"),
                    })?
            }
        };
        let physical_box = area.physical_box.as_deref().ok_or_else(|| {
            Error::InvalidStructure {
                path: reference.path.as_str().to_owned(),
                message: "Page.Area PhysicalBox is missing and the document PageArea declares no PhysicalBox"
                    .to_owned(),
            }
        })?;
        let size = crate::Rect::parse(physical_box)
            .map_err(|error| with_error_path(error, &reference.path))?;
        validate(size)
    }

    /// Loads and returns a page by zero-based index.
    pub fn page(&self, index: usize) -> Result<Page> {
        let reference = self.0.pages.get(index).ok_or(Error::PageOutOfRange {
            index,
            page_count: self.page_count(),
        })?;
        if let Some(data) = reference.cache.get() {
            return Ok(Page {
                _document: Arc::clone(&self.0),
                index,
                object_id: reference.id,
                data: Arc::clone(data),
            });
        }

        let _initialization = reference
            .initialization
            .lock()
            .map_err(|_| Error::Internal("page initialization lock is poisoned".to_owned()))?;
        if let Some(data) = reference.cache.get() {
            return Ok(Page {
                _document: Arc::clone(&self.0),
                index,
                object_id: reference.id,
                data: Arc::clone(data),
            });
        }

        let (page, skipped_units) = parse_page_xml(
            &self.0.container,
            &reference.path,
            &self.0.limits,
            self.0.strictness,
        )?;
        let (area, pending_warning) = match page.area {
            Some(area) if area.physical_box.is_some() => (area, None),
            area => {
                let missing = if area.is_some() {
                    "Page.Area PhysicalBox is missing"
                } else {
                    "Page.Area is missing"
                };
                if self.0.strictness == crate::Strictness::Strict {
                    return Err(Error::InvalidStructure {
                        path: reference.path.as_str().to_owned(),
                        message: missing.to_owned(),
                    });
                }
                let Some(default) = self.0.default_page_area.clone() else {
                    return Err(Error::InvalidStructure {
                        path: reference.path.as_str().to_owned(),
                        message: format!("{missing} and the document declares no PageArea"),
                    });
                };
                (
                    default,
                    Some(Warning {
                        code: WarningCode::PageAreaFallback,
                        path: reference.path.as_str().to_owned(),
                        message: format!("{missing}; inherited Document PageArea"),
                    }),
                )
            }
        };
        let Some(physical_box) = area.physical_box.as_deref() else {
            return Err(Error::InvalidStructure {
                path: reference.path.as_str().to_owned(),
                message: "Page.Area PhysicalBox is missing and the document PageArea declares no PhysicalBox"
                    .to_owned(),
            });
        };
        let size = crate::Rect::parse(physical_box)
            .map_err(|error| with_error_path(error, &reference.path))?;
        let application_box = area
            .application_box
            .as_deref()
            .map(crate::Rect::parse)
            .transpose()
            .map_err(|error| with_error_path(error, &reference.path))?;
        let content_box = area
            .content_box
            .as_deref()
            .map(crate::Rect::parse)
            .transpose()
            .map_err(|error| with_error_path(error, &reference.path))?;
        let bleed_box = area
            .bleed_box
            .as_deref()
            .map(crate::Rect::parse)
            .transpose()
            .map_err(|error| with_error_path(error, &reference.path))?;
        let (direct_layers, direct_usage) = crate::content::convert_layers(
            page.content,
            self,
            &self.0.limits,
            reference.path.as_str(),
            crate::LayerSource::Page,
        )
        .map_err(|error| with_error_path(error, &reference.path))?;
        let effective = self.merge_effective_layers(
            page.templates,
            direct_layers,
            direct_usage,
            page.actions,
            &reference.path,
            &mut Vec::new(),
        )?;
        let parsed = Arc::new(PageData {
            size,
            application_box,
            content_box,
            bleed_box,
            layers: effective.layers,
            semantic_text: OnceLock::new(),
            semantic_text_initialization: Mutex::new(()),
            navigation_anchors: effective.navigation_anchors,
            links: OnceLock::new(),
            links_initialization: Mutex::new(()),
        });
        if let Some(warning) = pending_warning {
            self.push_warning(warning)?;
        }
        for unit in skipped_units {
            self.push_warning(Warning {
                code: WarningCode::UnknownGraphicUnitSkipped,
                path: reference.path.as_str().to_owned(),
                message: format!("skipped unknown graphic unit {}", unit.describe()),
            })?;
        }
        let data = reference.cache.get_or_init(|| Arc::clone(&parsed));
        Ok(Page {
            _document: Arc::clone(&self.0),
            index,
            object_id: reference.id,
            data: Arc::clone(data),
        })
    }

    /// Returns a snapshot of recoverable diagnostics collected so far.
    pub fn warnings(&self) -> Vec<Warning> {
        self.0
            .warnings
            .lock()
            .map(|warnings| warnings.clone())
            .unwrap_or_else(|poisoned| poisoned.into_inner().clone())
    }

    /// Returns every signature stamp annotation declared by this document.
    ///
    /// Annotations are parsed lazily on first access from the signatures
    /// listed in `OFD.xml`. Following ofdrw, any signature or annotation that
    /// fails to parse is skipped and reported through [`Document::warnings`]
    /// with [`WarningCode::SignatureSkipped`]; only lock-poisoning internal
    /// errors surface as `Err`.
    pub fn stamp_annotations(&self) -> Result<Vec<crate::StampAnnotation>> {
        if let Some(annotations) = self.0.stamp_annotations.get() {
            return Ok(annotations.clone());
        }
        let _initialization = self.0.stamp_initialization.lock().map_err(|_| {
            Error::Internal("stamp annotation initialization lock is poisoned".to_owned())
        })?;
        if let Some(annotations) = self.0.stamp_annotations.get() {
            return Ok(annotations.clone());
        }
        let parsed = self.load_stamp_annotations();
        Ok(self.0.stamp_annotations.get_or_init(|| parsed).clone())
    }

    /// Returns every page annotation declared by this document.
    ///
    /// Annotations are parsed lazily on first access from the location in
    /// `Document.xml`. Like signature stamps, files or annotations that fail
    /// to parse are skipped and reported through [`Document::warnings`] with
    /// [`WarningCode::AnnotationSkipped`]; only lock-poisoning internal
    /// errors surface as `Err`.
    pub fn page_annotations(&self) -> Result<Vec<crate::PageAnnotation>> {
        Ok(self.page_annotations_cache()?.annotations.to_vec())
    }

    pub(crate) fn page_annotations_ref(&self) -> Result<&[crate::PageAnnotation]> {
        let cached = self.page_annotations_cache()?;
        if let Some(message) = &cached.first_limit {
            return Err(Error::LimitExceeded(message.clone()));
        }
        Ok(&cached.annotations)
    }

    fn page_annotations_cache(&self) -> Result<&PageAnnotations> {
        if let Some(annotations) = self.0.page_annotations.get() {
            return Ok(annotations);
        }
        let _initialization = self.0.page_annotation_initialization.lock().map_err(|_| {
            Error::Internal("page annotation initialization lock is poisoned".to_owned())
        })?;
        if let Some(annotations) = self.0.page_annotations.get() {
            return Ok(annotations);
        }
        let parsed = self.load_page_annotations();
        Ok(self.0.page_annotations.get_or_init(|| parsed))
    }

    fn load_page_annotations(&self) -> PageAnnotations {
        let mut annotations = PageAnnotations::default();
        let Some(entry_path) = self.0.annotations_path.clone() else {
            return annotations;
        };
        let entry: raw::AnnotationsRoot =
            match parse_annotations_xml(&self.0.container, &entry_path, &self.0.limits) {
                Ok(entry) => entry,
                Err(error) => {
                    annotations.record_limit(&error);
                    self.push_annotation_warning(
                        entry_path.as_str(),
                        format!("annotations entry file could not be parsed: {error}"),
                    );
                    return annotations;
                }
            };
        for page in entry.pages {
            self.load_annotation_page(&entry_path, page, &mut annotations);
        }
        annotations
    }

    fn load_annotation_page(
        &self,
        entry_path: &PackagePath,
        page: raw::AnnotationPageEntry,
        annotations: &mut PageAnnotations,
    ) {
        let page_ref = match page.page_id.parse::<u64>() {
            Ok(page_ref) => page_ref,
            Err(_) => {
                self.push_annotation_warning(
                    entry_path.as_str(),
                    format!("annotation page reference {} is invalid", page.page_id),
                );
                return;
            }
        };
        // Inline annotations keep the annotation id space of the entry file.
        for annot in page.inline_annots {
            self.push_annotation(entry_path, page_ref, annot, annotations);
        }
        let Some(file_loc) = page.file_loc.as_deref().map(str::trim) else {
            return;
        };
        let annot_path = match entry_path.resolve(file_loc) {
            Ok(path) => path,
            Err(error) => {
                self.push_annotation_warning(
                    entry_path.as_str(),
                    format!("annotation file location {file_loc} is invalid: {error}"),
                );
                return;
            }
        };
        let root = match parse_page_annot_xml(&self.0.container, &annot_path, &self.0.limits) {
            Ok(root) => root,
            Err(error) => {
                annotations.record_limit(&error);
                self.push_annotation_warning(
                    annot_path.as_str(),
                    format!("page annotation file could not be parsed: {error}"),
                );
                return;
            }
        };
        for annot in root.annots {
            self.push_annotation(&annot_path, page_ref, annot, annotations);
        }
    }

    fn push_annotation(
        &self,
        path: &PackagePath,
        page_ref: u64,
        annot: raw::AnnotEntry,
        annotations: &mut PageAnnotations,
    ) {
        match crate::content::convert_annotation(annot, self, &self.0.limits, path.as_str()) {
            Ok(mut annotation) => {
                annotation.page_ref = page_ref;
                annotations.annotations.push(annotation);
            }
            Err(error) => {
                annotations.record_limit(&error);
                self.push_annotation_warning(path.as_str(), format!("annotation skipped: {error}"))
            }
        }
    }

    fn push_annotation_warning(&self, path: &str, message: String) {
        if let Ok(mut warnings) = self.0.warnings.lock() {
            warnings.push(Warning {
                code: WarningCode::AnnotationSkipped,
                path: path.to_owned(),
                message,
            });
        }
    }

    fn load_stamp_annotations(&self) -> Vec<crate::StampAnnotation> {
        let Some(signatures_path) = self.0.signatures_path.clone() else {
            return Vec::new();
        };
        let signatures: raw::SignaturesRoot = match parse_xml(
            &self.0.container,
            &signatures_path,
            self.0.limits.max_xml_depth,
        ) {
            Ok(signatures) => signatures,
            Err(error) => {
                self.push_signature_warning(
                    signatures_path.as_str(),
                    format!("signatures file could not be parsed: {error}"),
                );
                return Vec::new();
            }
        };
        let mut annotations = Vec::new();
        for entry in signatures
            .signatures
            .iter()
            .take(self.0.limits.max_signatures)
        {
            self.load_signature_stamps(&signatures_path, entry, &mut annotations);
        }
        annotations
    }

    fn load_signature_stamps(
        &self,
        signatures_path: &PackagePath,
        entry: &raw::SignatureEntry,
        annotations: &mut Vec<crate::StampAnnotation>,
    ) {
        let label = entry.id.as_deref().unwrap_or("<unknown>");
        let loaded = (|| -> Result<Vec<crate::StampAnnotation>> {
            let signature_path = signatures_path.resolve(entry.base_loc.trim())?;
            let signature: raw::SignatureRoot = parse_xml(
                &self.0.container,
                &signature_path,
                self.0.limits.max_xml_depth,
            )?;
            if signature.signed_info.stamp_annots.is_empty() {
                return Ok(Vec::new());
            }
            let signed_value = signature
                .signed_value
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| Error::InvalidStructure {
                    path: signature_path.as_str().to_owned(),
                    message: "SignedValue is missing".to_owned(),
                })?;
            let value_path = signature_path.resolve(signed_value)?;
            let value_bytes = self.0.container.read_with_limit(
                &value_path,
                self.0.limits.max_signature_bytes,
                "signed value",
            )?;
            let picture = crate::ses::parse_seal_picture(&value_bytes)?;
            let mut parsed = Vec::new();
            for annot in &signature.signed_info.stamp_annots {
                match convert_stamp_annot(annot, &picture, &signature_path) {
                    Ok(annotation) => parsed.push(annotation),
                    Err(error) => self.push_signature_warning(
                        signature_path.as_str(),
                        format!("stamp annotation skipped: {error}"),
                    ),
                }
            }
            Ok(parsed)
        })();
        match loaded {
            Ok(mut parsed) => annotations.append(&mut parsed),
            Err(error) => self.push_signature_warning(
                signatures_path.as_str(),
                format!("signature {label} skipped: {error}"),
            ),
        }
    }

    fn push_warning(&self, warning: Warning) -> Result<()> {
        self.0
            .warnings
            .lock()
            .map_err(|_| Error::Internal("warning store lock is poisoned".to_owned()))?
            .push(warning);
        Ok(())
    }

    fn push_signature_warning(&self, path: &str, message: String) {
        if let Ok(mut warnings) = self.0.warnings.lock() {
            warnings.push(Warning {
                code: WarningCode::SignatureSkipped,
                path: path.to_owned(),
                message,
            });
        }
    }

    fn resource_catalog(&self) -> Result<&Arc<crate::resources::ResourceCatalog>> {
        if let Some(catalog) = self.0.resource_catalog.get() {
            return Ok(catalog);
        }
        let _initialization = self.0.resource_initialization.lock().map_err(|_| {
            Error::Internal("resource catalog initialization lock is poisoned".to_owned())
        })?;
        if let Some(catalog) = self.0.resource_catalog.get() {
            return Ok(catalog);
        }
        let (parsed, skipped_units) = crate::resources::ResourceCatalog::load(
            &self.0.container,
            &self.0.resource_paths,
            &self.0.limits,
            self.0.strictness,
        )?;
        for (skipped_path, unit) in skipped_units {
            self.push_warning(Warning {
                code: WarningCode::UnknownGraphicUnitSkipped,
                path: skipped_path,
                message: format!("skipped unknown graphic unit {}", unit.describe()),
            })?;
        }
        let parsed = Arc::new(parsed);
        Ok(self.0.resource_catalog.get_or_init(|| Arc::clone(&parsed)))
    }

    pub(crate) fn strictness(&self) -> crate::Strictness {
        self.0.strictness
    }

    pub(crate) fn resource_kind(&self, id: u64) -> Result<crate::ResourceKind> {
        self.resource_catalog()?.kind(id)
    }

    pub(crate) fn image_resource_format(&self, id: u64) -> Result<crate::ImageFormat> {
        self.resource_catalog()?.image_format(id)
    }

    pub(crate) fn draw_param(&self, id: u64) -> Result<crate::paint::PaintParameters> {
        self.resource_catalog()?.draw_param(id)
    }

    /// Returns the graphic units of one reusable vector graphic resource.
    pub(crate) fn vector_graphic_units(&self, id: u64) -> Result<&[raw::GraphicUnit]> {
        self.resource_catalog()?.vector_graphic_units(id)
    }

    pub(crate) fn vector_graphic_actions(
        &self,
        id: u64,
    ) -> Result<crate::navigation::deferred::Actions> {
        self.resource_catalog()?.vector_graphic_actions(id)
    }

    pub(crate) fn resolve_paint_color(
        &self,
        color: &raw::PaintColor,
        strict: bool,
    ) -> Result<Option<crate::Color>> {
        // Colours that reference no colour-space resource never touch the
        // catalog; loading it could fail for pages whose declared resource
        // files are absent from the package.
        if color.color_space.is_none() && color.index.is_none() {
            return crate::resources::resolve_plain_color(color, strict);
        }
        self.resource_catalog()?.resolve_paint_color(color, strict)
    }

    fn resolve_template(
        &self,
        id: u64,
        referring_path: &PackagePath,
        active: &mut Vec<u64>,
    ) -> Result<Arc<TemplateData>> {
        if let Some(position) = active.iter().position(|active_id| *active_id == id) {
            let mut cycle = active[position..]
                .iter()
                .map(u64::to_string)
                .collect::<Vec<_>>();
            cycle.push(id.to_string());
            return Err(Error::InvalidStructure {
                path: referring_path.as_str().to_owned(),
                message: format!("template reference cycle: {}", cycle.join(" -> ")),
            });
        }
        let reference = self
            .0
            .templates
            .get(&id)
            .ok_or_else(|| Error::InvalidStructure {
                path: referring_path.as_str().to_owned(),
                message: format!("unknown template ID {id}"),
            })?;
        debug_assert_eq!(reference.id, id);

        active.push(id);
        let loaded = (|| {
            if active.len() > self.0.limits.max_page_block_depth {
                return Err(Error::LimitExceeded(format!(
                    "template reference depth {} exceeds limit {}",
                    active.len(),
                    self.0.limits.max_page_block_depth
                )));
            }
            if let Some(data) = reference.cache.get() {
                for referenced_id in &data.referenced_templates {
                    self.resolve_template(*referenced_id, &reference.path, active)?;
                }
                return Ok(Arc::clone(data));
            }
            let (root, skipped_units) = parse_page_xml(
                &self.0.container,
                &reference.path,
                &self.0.limits,
                self.0.strictness,
            )?;
            for unit in skipped_units {
                self.push_warning(Warning {
                    code: WarningCode::UnknownGraphicUnitSkipped,
                    path: reference.path.as_str().to_owned(),
                    message: format!("skipped unknown graphic unit {}", unit.describe()),
                })?;
            }
            let (direct_layers, direct_usage) = crate::content::convert_layers(
                root.content,
                self,
                &self.0.limits,
                reference.path.as_str(),
                crate::LayerSource::Template(id),
            )
            .map_err(|error| with_error_path(error, &reference.path))?;
            self.merge_effective_layers(
                root.templates,
                direct_layers,
                direct_usage,
                root.actions,
                &reference.path,
                active,
            )
            .map(Arc::new)
        })();
        active.pop();
        let loaded = loaded?;
        Ok(Arc::clone(
            reference.cache.get_or_init(|| Arc::clone(&loaded)),
        ))
    }

    fn merge_effective_layers(
        &self,
        template_references: Vec<crate::raw::TemplateReference>,
        direct_layers: Vec<crate::Layer>,
        mut usage: crate::content::ContentUsage,
        direct_actions: crate::navigation::deferred::Actions,
        path: &PackagePath,
        active: &mut Vec<u64>,
    ) -> Result<TemplateData> {
        let mut background_templates = Vec::new();
        let mut foreground_templates = Vec::new();
        let mut background_anchors = Vec::new();
        let mut foreground_anchors = Vec::new();
        let mut referenced_templates = Vec::with_capacity(template_references.len());
        for template in template_references {
            usage = add_effective_usage(
                usage,
                crate::content::ContentUsage::template_reference(),
                &self.0.limits,
            )?;
            let id = parse_template_id(&template.template_id, path)?;
            referenced_templates.push(id);
            let declaration = self
                .0
                .templates
                .get(&id)
                .ok_or_else(|| Error::InvalidStructure {
                    path: path.as_str().to_owned(),
                    message: format!("unknown template ID {id}"),
                })?;
            let z_order = parse_template_z_order(
                template.z_order.as_deref(),
                declaration.default_z_order,
                path,
            )?;
            let resolved = self.resolve_template(id, path, active)?;
            usage = add_effective_usage(usage, resolved.usage, &self.0.limits)?;
            match z_order {
                TemplateZOrder::Background => {
                    background_anchors.extend(resolved.navigation_anchors.iter().map(
                        |(index, actions)| (index + background_templates.len(), actions.clone()),
                    ));
                    background_templates.extend(resolved.layers.iter().cloned())
                }
                TemplateZOrder::Foreground => {
                    foreground_anchors.extend(resolved.navigation_anchors.iter().map(
                        |(index, actions)| (index + foreground_templates.len(), actions.clone()),
                    ));
                    foreground_templates.extend(resolved.layers.iter().cloned())
                }
            }
        }

        validate_effective_usage(usage, &self.0.limits)?;
        let mut background = Vec::new();
        let mut body = Vec::new();
        let mut foreground = Vec::new();
        for layer in direct_layers {
            match layer.kind() {
                crate::LayerType::Background => background.push(layer),
                crate::LayerType::Body => body.push(layer),
                crate::LayerType::Foreground => foreground.push(layer),
            }
        }
        if direct_actions.is_some() {
            background_anchors.insert(0, (0, direct_actions));
        }
        background_templates.extend(background);
        background_templates.extend(body);
        background_templates.extend(foreground);
        background_anchors.extend(
            foreground_anchors
                .into_iter()
                .map(|(index, actions)| (index + background_templates.len(), actions)),
        );
        background_templates.extend(foreground_templates);
        Ok(TemplateData {
            layers: background_templates,
            usage,
            referenced_templates,
            navigation_anchors: background_anchors,
        })
    }
}

fn convert_stamp_annot(
    annot: &raw::StampAnnotRaw,
    picture: &crate::ses::SealPictureData,
    path: &PackagePath,
) -> Result<crate::StampAnnotation> {
    let page_ref = annot
        .page_ref
        .parse::<u64>()
        .map_err(|_| Error::InvalidValue {
            field: "stamp annotation PageRef",
            value: annot.page_ref.clone(),
            path: Some(path.as_str().to_owned()),
        })?;
    let boundary =
        crate::Rect::parse(&annot.boundary).map_err(|error| with_error_path(error, path))?;
    let clip = annot
        .clip
        .as_deref()
        .map(|value| crate::Rect::parse(value).map_err(|error| with_error_path(error, path)))
        .transpose()?;
    Ok(crate::StampAnnotation {
        page_ref,
        id: annot.id.clone(),
        boundary,
        clip,
        picture: crate::SealPicture {
            kind: crate::SealPictureKind::from_type_name(&picture.kind),
            data: picture.data.clone(),
            width_mm: picture.width.map(f64::from),
            height_mm: picture.height.map(f64::from),
        },
    })
}

fn parse_template_id(value: &str, path: &PackagePath) -> Result<u64> {
    match value.parse::<u64>() {
        Ok(id) if id != 0 => Ok(id),
        _ => Err(Error::InvalidValue {
            field: "template ID",
            value: value.to_owned(),
            path: Some(path.as_str().to_owned()),
        }),
    }
}

fn parse_template_z_order(
    value: Option<&str>,
    default: TemplateZOrder,
    path: &PackagePath,
) -> Result<TemplateZOrder> {
    match value {
        None => Ok(default),
        Some("Background") => Ok(TemplateZOrder::Background),
        Some("Foreground") => Ok(TemplateZOrder::Foreground),
        Some(value) => Err(Error::InvalidValue {
            field: "template ZOrder",
            value: value.to_owned(),
            path: Some(path.as_str().to_owned()),
        }),
    }
}

fn add_effective_usage(
    current: crate::content::ContentUsage,
    added: crate::content::ContentUsage,
    limits: &crate::ResourceLimits,
) -> Result<crate::content::ContentUsage> {
    let total = current.checked_add(added).ok_or_else(|| {
        Error::LimitExceeded("effective page resource accounting overflowed".to_owned())
    })?;
    validate_effective_usage(total, limits)?;
    Ok(total)
}

fn validate_effective_usage(
    usage: crate::content::ContentUsage,
    limits: &crate::ResourceLimits,
) -> Result<()> {
    if usage.page_objects > limits.max_page_objects {
        return Err(Error::LimitExceeded(format!(
            "effective page object count {} exceeds limit {}",
            usage.page_objects, limits.max_page_objects
        )));
    }
    if usage.path_commands > limits.max_path_commands {
        return Err(Error::LimitExceeded(format!(
            "effective page path command count {} exceeds limit {}",
            usage.path_commands, limits.max_path_commands
        )));
    }
    if usage.text_characters > limits.max_text_characters_per_page {
        return Err(Error::LimitExceeded(format!(
            "effective page text character count {} exceeds limit {}",
            usage.text_characters, limits.max_text_characters_per_page
        )));
    }
    if usage.glyphs > limits.max_glyphs_per_page {
        return Err(Error::LimitExceeded(format!(
            "effective page glyph count {} exceeds limit {}",
            usage.glyphs, limits.max_glyphs_per_page
        )));
    }
    if usage.text_expansion_entries > limits.max_text_expansion_entries {
        return Err(Error::LimitExceeded(format!(
            "effective page text expansion count {} exceeds limit {}",
            usage.text_expansion_entries, limits.max_text_expansion_entries
        )));
    }
    Ok(())
}

fn with_error_path(error: Error, path: &PackagePath) -> Error {
    match error {
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
    }
}

fn parse_xml<T: DeserializeOwned>(
    container: &Container,
    path: &PackagePath,
    max_xml_depth: usize,
) -> Result<T> {
    let bytes = container.read(path)?;
    preflight_xml_depth(&bytes, path, max_xml_depth)?;
    // Real-world producers scatter repeated elements (e.g. ofdrw's
    // path_unstd.ofd splits TemplatePage entries around other CommonData
    // children); accept sequence members in any order like the page parser.
    let mut deserializer = serde_xml_rs::Deserializer::new_from_reader(bytes.as_slice())
        .non_contiguous_seq_elements(true);
    T::deserialize(&mut deserializer).map_err(|error| Error::Xml {
        path: path.as_str().to_owned(),
        message: error.to_string(),
    })
}

fn parse_page_xml(
    container: &Container,
    path: &PackagePath,
    limits: &crate::ResourceLimits,
    strictness: crate::Strictness,
) -> Result<(raw::PageRoot, Vec<SkippedGraphicUnit>)> {
    let bytes = container.read(path)?;
    preflight_xml_depth(&bytes, path, limits.max_xml_depth)?;
    let mut actions = crate::navigation::deferred::extract(&bytes, path.as_str(), limits)?;
    preflight_page_xml(&actions.sanitized, path, limits, strictness)?;
    let lenient = strictness == crate::Strictness::Lenient;
    let extracted = extract_rich_objects(&actions.sanitized, path, lenient)?;
    let mut deserializer =
        serde_xml_rs::Deserializer::new_from_reader(extracted.sanitized.as_slice())
            .non_contiguous_seq_elements(true);
    let mut page = raw::PageRoot::deserialize(&mut deserializer).map_err(|error| Error::Xml {
        path: path.as_str().to_owned(),
        message: error.to_string(),
    })?;
    inject_rich_objects(&mut page, extracted.rich, path)?;
    actions.page(&mut page);
    Ok((page, extracted.skipped))
}

/// Parses one `PageAnnot` file, whose annotation appearances carry the same
/// standalone-extracted graphic units as page XML.
fn parse_page_annot_xml(
    container: &Container,
    path: &PackagePath,
    limits: &crate::ResourceLimits,
) -> Result<raw::PageAnnotRoot> {
    let bytes = container.read(path)?;
    preflight_xml_depth(&bytes, path, limits.max_xml_depth)?;
    let mut actions = crate::navigation::deferred::extract(&bytes, path.as_str(), limits)?;
    let extracted = extract_rich_objects(&actions.sanitized, path, true)?;
    let mut deserializer =
        serde_xml_rs::Deserializer::new_from_reader(extracted.sanitized.as_slice())
            .non_contiguous_seq_elements(true);
    let mut root =
        raw::PageAnnotRoot::deserialize(&mut deserializer).map_err(|error| Error::Xml {
            path: path.as_str().to_owned(),
            message: error.to_string(),
        })?;
    let RichObjects {
        paths,
        texts,
        images,
    } = extracted.rich;
    let mut paths = paths.into_iter();
    let mut texts = texts.into_iter();
    let mut images = images.into_iter();
    for annot in &mut root.annots {
        if let Some(appearance) = &mut annot.appearance {
            inject_rich_units(
                &mut appearance.objects,
                &mut paths,
                &mut texts,
                &mut images,
                path,
            )?;
        }
    }
    finish_rich_injection(paths, texts, images, path)?;
    actions.annotations(&mut root.annots);
    Ok(root)
}

fn parse_annotations_xml(
    container: &Container,
    path: &PackagePath,
    limits: &crate::ResourceLimits,
) -> Result<raw::AnnotationsRoot> {
    let bytes = container.read(path)?;
    preflight_xml_depth(&bytes, path, limits.max_xml_depth)?;
    let mut actions = crate::navigation::deferred::extract(&bytes, path.as_str(), limits)?;
    let extracted = extract_rich_objects(&actions.sanitized, path, true)?;
    let mut deserializer =
        serde_xml_rs::Deserializer::new_from_reader(extracted.sanitized.as_slice())
            .non_contiguous_seq_elements(true);
    let mut root =
        raw::AnnotationsRoot::deserialize(&mut deserializer).map_err(|error| Error::Xml {
            path: path.as_str().into(),
            message: error.to_string(),
        })?;
    let RichObjects {
        paths,
        texts,
        images,
    } = extracted.rich;
    let (mut paths, mut texts, mut images) =
        (paths.into_iter(), texts.into_iter(), images.into_iter());
    for page in &mut root.pages {
        for annot in &mut page.inline_annots {
            if let Some(appearance) = &mut annot.appearance {
                inject_rich_units(
                    &mut appearance.objects,
                    &mut paths,
                    &mut texts,
                    &mut images,
                    path,
                )?;
            }
        }
        actions.annotations(&mut page.inline_annots);
    }
    finish_rich_injection(paths, texts, images, path)?;
    Ok(root)
}

/// An element below `Layer` or `PageBlock` that this version does not know.
///
/// Lenient parsing drops such elements (like ofdrw, which ignores every
/// unrecognized page-block child) and reports each one as a warning instead
/// of failing the whole page.
pub(crate) struct SkippedGraphicUnit {
    name: String,
    id: Option<String>,
}

impl SkippedGraphicUnit {
    fn describe(&self) -> String {
        match &self.id {
            Some(id) => format!("{} (ID {id})", self.name),
            None => self.name.clone(),
        }
    }
}

/// Graphic-unit payloads parsed standalone from a sanitized XML copy.
///
/// serde-xml-rs 0.6 corrupts its reader state when a graphic unit containing
/// a nested `Vec` field (the Clips chain of PathObject) is followed by
/// another sibling unit, so these payloads never go through the main pass.
/// The sanitized copy additionally omits skipped unknown graphic units so
/// the main pass only ever sees deserializable elements.
pub(crate) struct RichObjects {
    paths: Vec<raw::PathObject>,
    texts: Vec<raw::TextObject>,
    images: Vec<raw::ImageObject>,
}

pub(crate) struct ExtractedPage {
    pub(crate) rich: RichObjects,
    pub(crate) sanitized: Vec<u8>,
    pub(crate) skipped: Vec<SkippedGraphicUnit>,
}

pub(crate) fn extract_rich_objects(
    bytes: &[u8],
    path: &PackagePath,
    lenient: bool,
) -> Result<ExtractedPage> {
    use xml::reader::{EventReader, XmlEvent};

    #[derive(Clone, Copy)]
    enum Kind {
        Path,
        Text,
        Image,
    }

    struct Capture {
        kind: Kind,
        depth: usize,
        writer: xml::EventWriter<Vec<u8>>,
        text_codes: Vec<String>,
        current_text_code: Option<String>,
    }

    let mut capture = None::<Capture>;
    let mut paths = Vec::new();
    let mut texts = Vec::new();
    let mut images = Vec::new();
    let mut elements = Vec::<String>::new();
    let mut sanitized = xml::EventWriter::new(Vec::new());
    let mut skipped = Vec::new();
    // Depth of open elements belonging to a skipped unknown graphic unit;
    // `None` while events pass through to the sanitized writer.
    let mut skip_depth: Option<usize> = None;
    for event in EventReader::new(bytes) {
        let event = event.map_err(|error| xml_error(path, error))?;
        if skip_depth.is_some() {
            let mut exited = false;
            match &event {
                XmlEvent::StartElement { name, .. } => {
                    elements.push(name.local_name.clone());
                    if let Some(depth) = skip_depth.as_mut() {
                        *depth += 1;
                    }
                }
                XmlEvent::EndElement { .. } => {
                    elements.pop();
                    if let Some(depth) = skip_depth.as_mut() {
                        *depth -= 1;
                        exited = *depth == 0;
                    }
                }
                _ => {}
            }
            if exited {
                skip_depth = None;
            }
            continue;
        }
        let mut started_skip = false;
        if capture.is_none() {
            if let XmlEvent::StartElement {
                name, attributes, ..
            } = &event
            {
                // `Content` is a graphic-unit container only inside resource
                // catalogs (a `CompositeGraphicUnit` payload); a page's
                // `Content` element holds `Layer` children instead.
                // `Appearance` holds the inline page block of an annotation.
                let is_graphic_unit_container = match elements.last().map(String::as_str) {
                    Some("Layer" | "PageBlock" | "Appearance") => true,
                    Some("Content") => elements
                        .get(elements.len().saturating_sub(2))
                        .is_some_and(|grandparent| grandparent == "CompositeGraphicUnit"),
                    _ => false,
                };
                let kind = is_graphic_unit_container
                    .then_some(match name.local_name.as_str() {
                        "PathObject" => Some(Kind::Path),
                        "TextObject" => Some(Kind::Text),
                        "ImageObject" => Some(Kind::Image),
                        _ => None,
                    })
                    .flatten();
                let main_pass_unit =
                    matches!(name.local_name.as_str(), "PageBlock" | "CompositeObject");
                if let Some(kind) = kind {
                    capture = Some(Capture {
                        kind,
                        depth: 0,
                        writer: xml::EventWriter::new(Vec::new()),
                        text_codes: Vec::new(),
                        current_text_code: None,
                    });
                } else if lenient && is_graphic_unit_container && !main_pass_unit {
                    skipped.push(SkippedGraphicUnit {
                        name: name.local_name.clone(),
                        id: attributes
                            .iter()
                            .find(|attribute| attribute.name.local_name == "ID")
                            .map(|attribute| attribute.value.clone()),
                    });
                    elements.push(name.local_name.clone());
                    skip_depth = Some(1);
                    started_skip = true;
                }
            }
        }
        if started_skip {
            continue;
        }
        if let Some(writer_event) = event.as_writer_event() {
            sanitized.write(writer_event).map_err(|error| Error::Xml {
                path: path.as_str().to_owned(),
                message: error.to_string(),
            })?;
        }
        if let XmlEvent::StartElement { name, .. } = &event {
            elements.push(name.local_name.clone());
        }
        if let Some(active) = &mut capture {
            if let XmlEvent::StartElement { name, .. } = &event {
                if matches!(active.kind, Kind::Text)
                    && active.depth == 1
                    && name.local_name == "TextCode"
                {
                    active.current_text_code = Some(String::new());
                }
                active.depth += 1;
            }
            if let Some(text) = &mut active.current_text_code {
                match &event {
                    XmlEvent::Characters(value)
                    | XmlEvent::Whitespace(value)
                    | XmlEvent::CData(value) => text.push_str(value),
                    _ => {}
                }
            }
            if let Some(writer_event) = event.as_writer_event() {
                active
                    .writer
                    .write(writer_event)
                    .map_err(|error| Error::Xml {
                        path: path.as_str().to_owned(),
                        message: error.to_string(),
                    })?;
            }
            if let XmlEvent::EndElement { name } = &event {
                if matches!(active.kind, Kind::Text)
                    && active.depth == 2
                    && name.local_name == "TextCode"
                {
                    active
                        .text_codes
                        .push(active.current_text_code.take().unwrap_or_default());
                }
                active.depth -= 1;
                if active.depth == 0 {
                    let completed = capture.take().expect("capture exists");
                    let xml = completed.writer.into_inner();
                    let mut deserializer =
                        serde_xml_rs::Deserializer::new_from_reader(xml.as_slice())
                            .non_contiguous_seq_elements(true);
                    let map_error = |error: serde_xml_rs::Error| Error::Xml {
                        path: path.as_str().to_owned(),
                        message: error.to_string(),
                    };
                    match completed.kind {
                        Kind::Path => paths.push(
                            raw::PathObject::deserialize(&mut deserializer).map_err(map_error)?,
                        ),
                        Kind::Text => {
                            let mut text = raw::TextObject::deserialize(&mut deserializer)
                                .map_err(map_error)?;
                            if text.text_codes.len() != completed.text_codes.len() {
                                return Err(Error::InvalidStructure {
                                    path: path.as_str().to_owned(),
                                    message: "TextCode extraction count mismatch".to_owned(),
                                });
                            }
                            for (run, value) in text.text_codes.iter_mut().zip(completed.text_codes)
                            {
                                run.text = value;
                            }
                            texts.push(text);
                        }
                        Kind::Image => images.push(
                            raw::ImageObject::deserialize(&mut deserializer).map_err(map_error)?,
                        ),
                    }
                }
            }
        }
        if matches!(event, XmlEvent::EndElement { .. }) {
            elements.pop();
        }
    }
    Ok(ExtractedPage {
        rich: RichObjects {
            paths,
            texts,
            images,
        },
        sanitized: sanitized.into_inner(),
        skipped,
    })
}

pub(crate) fn inject_rich_objects(
    page: &mut raw::PageRoot,
    rich: RichObjects,
    path: &PackagePath,
) -> Result<()> {
    let RichObjects {
        paths,
        texts,
        images,
    } = rich;
    let mut paths = paths.into_iter();
    let mut texts = texts.into_iter();
    let mut images = images.into_iter();
    if let Some(content) = &mut page.content {
        for layer in &mut content.layers {
            inject_rich_units(
                &mut layer.objects,
                &mut paths,
                &mut texts,
                &mut images,
                path,
            )?;
        }
    }
    finish_rich_injection(paths, texts, images, path)
}

/// Injects standalone-parsed payloads into a resource catalog's vector
/// graphics, which hold graphic units through the same serde enum.
pub(crate) fn inject_rich_objects_into_resource(
    root: &mut raw::ResourceRoot,
    rich: RichObjects,
    path: &PackagePath,
) -> Result<()> {
    let RichObjects {
        paths,
        texts,
        images,
    } = rich;
    let mut paths = paths.into_iter();
    let mut texts = texts.into_iter();
    let mut images = images.into_iter();
    for unit in root
        .composite_graphic_units
        .iter_mut()
        .flat_map(|units| units.entries.iter_mut())
    {
        if let Some(content) = &mut unit.content {
            inject_rich_units(
                &mut content.objects,
                &mut paths,
                &mut texts,
                &mut images,
                path,
            )?;
        }
    }
    finish_rich_injection(paths, texts, images, path)
}

fn finish_rich_injection(
    mut paths: impl Iterator<Item = raw::PathObject>,
    mut texts: impl Iterator<Item = raw::TextObject>,
    mut images: impl Iterator<Item = raw::ImageObject>,
    path: &PackagePath,
) -> Result<()> {
    if paths.next().is_some() || texts.next().is_some() || images.next().is_some() {
        return Err(Error::InvalidStructure {
            path: path.as_str().to_owned(),
            message: "rich object extraction did not match parsed page structure".to_owned(),
        });
    }
    Ok(())
}

fn inject_rich_units(
    objects: &mut [raw::GraphicUnit],
    paths: &mut impl Iterator<Item = raw::PathObject>,
    texts: &mut impl Iterator<Item = raw::TextObject>,
    images: &mut impl Iterator<Item = raw::ImageObject>,
    path: &PackagePath,
) -> Result<()> {
    for object in objects {
        match object {
            raw::GraphicUnit::Path(path_object) => {
                let parsed = paths.next().ok_or_else(|| Error::InvalidStructure {
                    path: path.as_str().to_owned(),
                    message: "missing parsed PathObject payload".to_owned(),
                })?;
                if parsed.id != path_object.id {
                    return Err(Error::InvalidStructure {
                        path: path.as_str().to_owned(),
                        message: "PathObject extraction order mismatch".to_owned(),
                    });
                }
                path_object.object = Some(Box::new(parsed));
            }
            raw::GraphicUnit::Text(text) => {
                let parsed = texts.next().ok_or_else(|| Error::InvalidStructure {
                    path: path.as_str().to_owned(),
                    message: "missing parsed TextObject payload".to_owned(),
                })?;
                if parsed.id != text.id {
                    return Err(Error::InvalidStructure {
                        path: path.as_str().to_owned(),
                        message: "TextObject extraction order mismatch".to_owned(),
                    });
                }
                text.object = Some(Box::new(parsed));
            }
            raw::GraphicUnit::Image(image) => {
                let parsed = images.next().ok_or_else(|| Error::InvalidStructure {
                    path: path.as_str().to_owned(),
                    message: "missing parsed ImageObject payload".to_owned(),
                })?;
                if parsed.id != image.id {
                    return Err(Error::InvalidStructure {
                        path: path.as_str().to_owned(),
                        message: "ImageObject extraction order mismatch".to_owned(),
                    });
                }
                image.object = Some(Box::new(parsed));
            }
            raw::GraphicUnit::Group(group) => {
                inject_rich_units(&mut group.objects, paths, texts, images, path)?
            }
            _ => {}
        }
    }
    Ok(())
}

fn preflight_xml_depth(bytes: &[u8], path: &PackagePath, max_xml_depth: usize) -> Result<()> {
    use xml::reader::{EventReader, XmlEvent};

    let mut depth = 0usize;
    for event in EventReader::new(bytes) {
        match event.map_err(|error| xml_error(path, error))? {
            XmlEvent::StartElement { .. } => {
                depth = depth.saturating_add(1);
                if depth > max_xml_depth {
                    return Err(xml_depth_error(depth, max_xml_depth));
                }
            }
            XmlEvent::EndElement { .. } => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    Ok(())
}

fn preflight_page_xml(
    bytes: &[u8],
    path: &PackagePath,
    limits: &crate::ResourceLimits,
    strictness: crate::Strictness,
) -> Result<()> {
    use xml::reader::{EventReader, XmlEvent};

    #[derive(Clone, Copy)]
    enum ElementMarker {
        Page,
        Content,
        Layer,
        PageBlock,
        PathObject,
        TextObject,
        ImageObject,
        ImageBorder,
        TextCode,
        Clips,
        Clip,
        ClipArea,
        Other,
    }

    let mut elements: Vec<ElementMarker> = Vec::new();
    let mut page_block_depth = 0usize;
    let mut page_object_count = 0usize;
    let mut text_expansion_node_count = 0usize;
    for event in EventReader::new(bytes) {
        match event.map_err(|error| xml_error(path, error))? {
            XmlEvent::StartElement { name, .. } => {
                let depth = elements.len().saturating_add(1);
                if depth > limits.max_xml_depth {
                    return Err(xml_depth_error(depth, limits.max_xml_depth));
                }
                let parent = elements.last().copied();
                let is_page = elements.is_empty() && name.local_name == "Page";
                let is_content =
                    matches!(parent, Some(ElementMarker::Page)) && name.local_name == "Content";
                let is_template_reference =
                    matches!(parent, Some(ElementMarker::Page)) && name.local_name == "Template";
                let is_layer =
                    matches!(parent, Some(ElementMarker::Content)) && name.local_name == "Layer";
                let parent_is_object_container = matches!(
                    parent,
                    Some(ElementMarker::Layer | ElementMarker::PageBlock)
                );
                let known_graphic_unit = matches!(
                    name.local_name.as_str(),
                    "PathObject" | "PageBlock" | "TextObject" | "ImageObject" | "CompositeObject"
                );
                // Lenient mode accepts any page-block child like ofdrw, which
                // ignores elements it does not recognize; they are dropped by
                // the extraction pass and reported as warnings.
                let is_graphic_unit = parent_is_object_container
                    && (known_graphic_unit || strictness == crate::Strictness::Lenient);
                let is_clips = matches!(
                    parent,
                    Some(
                        ElementMarker::PathObject
                            | ElementMarker::TextObject
                            | ElementMarker::ImageObject
                    )
                ) && name.local_name == "Clips";
                let is_text_child = matches!(parent, Some(ElementMarker::TextObject))
                    && matches!(
                        name.local_name.as_str(),
                        "FillColor" | "StrokeColor" | "Clips" | "TextCode" | "CGTransform"
                    );
                let is_text_expansion_node = matches!(parent, Some(ElementMarker::TextObject))
                    && matches!(name.local_name.as_str(), "TextCode" | "CGTransform");
                let is_image_child = matches!(parent, Some(ElementMarker::ImageObject))
                    && matches!(name.local_name.as_str(), "Clips" | "Border");
                let is_path_child = matches!(parent, Some(ElementMarker::PathObject))
                    && matches!(
                        name.local_name.as_str(),
                        "AbbreviatedData" | "StrokeColor" | "FillColor" | "Clips"
                    );
                if matches!(parent, Some(ElementMarker::PathObject)) && !is_path_child {
                    return Err(Error::InvalidStructure {
                        path: path.as_str().to_owned(),
                        message: format!("unknown PathObject child {}", name.local_name),
                    });
                }
                if matches!(parent, Some(ElementMarker::TextObject)) && !is_text_child {
                    return Err(Error::InvalidStructure {
                        path: path.as_str().to_owned(),
                        message: format!("unknown TextObject child {}", name.local_name),
                    });
                }
                if matches!(parent, Some(ElementMarker::ImageObject)) && !is_image_child {
                    return Err(Error::InvalidStructure {
                        path: path.as_str().to_owned(),
                        message: format!("unknown ImageObject child {}", name.local_name),
                    });
                }
                if matches!(parent, Some(ElementMarker::ImageBorder))
                    && name.local_name != "BorderColor"
                {
                    return Err(Error::InvalidStructure {
                        path: path.as_str().to_owned(),
                        message: format!("unknown Border child {}", name.local_name),
                    });
                }
                if matches!(parent, Some(ElementMarker::TextCode)) {
                    return Err(Error::InvalidStructure {
                        path: path.as_str().to_owned(),
                        message: format!("TextCode contains child {}", name.local_name),
                    });
                }
                let is_clip =
                    matches!(parent, Some(ElementMarker::Clips)) && name.local_name == "Clip";
                let is_clip_area =
                    matches!(parent, Some(ElementMarker::Clip)) && name.local_name == "Area";
                let is_clip_child = matches!(parent, Some(ElementMarker::ClipArea))
                    && matches!(name.local_name.as_str(), "Path" | "Text");
                if parent_is_object_container && !is_graphic_unit {
                    return Err(Error::InvalidStructure {
                        path: path.as_str().to_owned(),
                        message: format!("unknown graphic unit {}", name.local_name),
                    });
                }
                if is_template_reference
                    || is_layer
                    || is_graphic_unit
                    || is_clip
                    || is_clip_area
                    || is_clip_child
                {
                    if page_object_count >= limits.max_page_objects {
                        return Err(Error::LimitExceeded(format!(
                            "page object count {} exceeds limit {}",
                            page_object_count.saturating_add(1),
                            limits.max_page_objects
                        )));
                    }
                    page_object_count += 1;
                }
                if is_text_expansion_node {
                    if text_expansion_node_count >= limits.max_text_expansion_entries {
                        return Err(Error::LimitExceeded(format!(
                            "XML text expansion node count {} exceeds limit {}",
                            text_expansion_node_count.saturating_add(1),
                            limits.max_text_expansion_entries
                        )));
                    }
                    text_expansion_node_count += 1;
                }
                let is_page_block = is_graphic_unit && name.local_name == "PageBlock";
                if is_page_block {
                    page_block_depth = page_block_depth.saturating_add(1);
                    if page_block_depth > limits.max_page_block_depth {
                        return Err(Error::LimitExceeded(format!(
                            "page block depth {page_block_depth} exceeds limit {}",
                            limits.max_page_block_depth
                        )));
                    }
                }
                elements.push(match () {
                    _ if is_page => ElementMarker::Page,
                    _ if is_content => ElementMarker::Content,
                    _ if is_layer => ElementMarker::Layer,
                    _ if is_page_block => ElementMarker::PageBlock,
                    _ if is_graphic_unit && name.local_name == "PathObject" => {
                        ElementMarker::PathObject
                    }
                    _ if is_graphic_unit && name.local_name == "TextObject" => {
                        ElementMarker::TextObject
                    }
                    _ if is_graphic_unit && name.local_name == "ImageObject" => {
                        ElementMarker::ImageObject
                    }
                    _ if is_image_child && name.local_name == "Border" => {
                        ElementMarker::ImageBorder
                    }
                    _ if is_text_child && name.local_name == "TextCode" => ElementMarker::TextCode,
                    _ if is_clips => ElementMarker::Clips,
                    _ if is_clip => ElementMarker::Clip,
                    _ if is_clip_area => ElementMarker::ClipArea,
                    _ => ElementMarker::Other,
                });
            }
            XmlEvent::EndElement { .. } => {
                let ended = elements.pop();
                if matches!(ended, Some(ElementMarker::PageBlock)) {
                    page_block_depth = page_block_depth.saturating_sub(1);
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn xml_error(path: &PackagePath, error: xml::reader::Error) -> Error {
    Error::Xml {
        path: path.as_str().to_owned(),
        message: error.to_string(),
    }
}

fn xml_depth_error(depth: usize, limit: usize) -> Error {
    Error::LimitExceeded(format!("XML depth {depth} exceeds limit {limit}"))
}

#[cfg(test)]
mod page_size_tests {
    use super::*;
    mod support {
        include!(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/support/mod.rs"));
    }
    #[test]
    fn metadata_rejects_nonpositive_sizes_before_and_after_content_cache() {
        for physical_box in ["0 0 0 100", "0 0 100 -1"] {
            let xml = format!(
                r#"<ofd:Page xmlns:ofd="http://www.ofdspec.org/2016"><ofd:Area><ofd:PhysicalBox>{physical_box}</ofd:PhysicalBox></ofd:Area></ofd:Page>"#
            );
            let document =
                Document::from_bytes(support::minimal_ofd(&xml), LoadOptions::default()).unwrap();
            assert!(document.page_size(0).is_err());
            document.page(0).unwrap();
            assert!(document.page_size(0).is_err());
        }
    }
    #[test]
    fn metadata_does_not_initialize_page_content_cache() {
        let bytes = support::minimal_ofd(
            r#"<ofd:Page xmlns:ofd="http://www.ofdspec.org/2016"><ofd:Area><ofd:PhysicalBox>10 20 120 180</ofd:PhysicalBox></ofd:Area><ofd:Content><ofd:Layer ID="1"><ofd:TextObject ID="2" Boundary="0 0 20 10" Font="999" Size="4"><ofd:TextCode X="0" Y="4">text</ofd:TextCode></ofd:TextObject></ofd:Layer></ofd:Content></ofd:Page>"#,
        );
        let document = Document::from_bytes(bytes, LoadOptions::default()).unwrap();
        assert_eq!(
            document.page_size(0).unwrap(),
            crate::Rect {
                x: 10.,
                y: 20.,
                width: 120.,
                height: 180.
            }
        );
        assert!(document.0.pages[0].cache.get().is_none());
        assert!(document.page(0).is_err());
        assert!(document.page_size(1).is_err());
    }
    #[test]
    fn metadata_obeys_area_fallback_and_strictness() {
        let bytes = support::minimal_ofd(r#"<ofd:Page xmlns:ofd="http://www.ofdspec.org/2016"/>"#);
        let document = Document::from_bytes(bytes.clone(), LoadOptions::default()).unwrap();
        assert_eq!(document.page_size(0).unwrap().width, 210.);
        let options = LoadOptions {
            strictness: crate::Strictness::Strict,
            ..Default::default()
        };
        assert!(Document::from_bytes(bytes, options)
            .unwrap()
            .page_size(0)
            .is_err());
    }

    #[test]
    fn metadata_rejects_a_missing_page_area_without_a_document_fallback() {
        let document_xml = r#"<ofd:Document xmlns:ofd="http://www.ofdspec.org/2016">
  <ofd:CommonData><ofd:MaxUnitID>2</ofd:MaxUnitID></ofd:CommonData>
  <ofd:Pages><ofd:Page ID="2" BaseLoc="Pages/Page_0/Content.xml"/></ofd:Pages>
</ofd:Document>"#;
        let page_xml = r#"<ofd:Page xmlns:ofd="http://www.ofdspec.org/2016"/>"#;
        let bytes = support::ofd_with_document_page_and_entries(document_xml, page_xml, &[]);
        let document = Document::from_bytes(bytes, LoadOptions::default()).unwrap();

        let error = document.page_size(0).unwrap_err().to_string();

        assert!(error.contains("document declares no PageArea"), "{error}");
        assert!(document.0.pages[0].cache.get().is_none());
    }
}
