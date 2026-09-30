use std::collections::HashMap;
use std::io::Cursor;
use std::sync::{Arc, Condvar, Mutex};

use image::{GenericImageView, ImageFormat as DecoderFormat, ImageReader, Limits};
use rofd_core::{ImageFormat, ImageResource, ResourceIdentity, ResourceLimits};

use crate::{Error, Result};

/// Immutable, validated, row-major RGBA8 pixels decoded from one OFD image resource.
///
/// Clones share their pixel allocation. Equality compares collision-free source
/// resource identity and constant-size metadata rather than rescanning pixels.
#[derive(Clone, Debug)]
pub struct DecodedImage {
    identity: Option<ResourceIdentity>,
    resource_id: u64,
    width: u32,
    height: u32,
    stride: usize,
    rgba: Arc<[u8]>,
}

impl PartialEq for DecodedImage {
    fn eq(&self, other: &Self) -> bool {
        self.resource_id == other.resource_id
            && self.width == other.width
            && self.height == other.height
            && self.stride == other.stride
            && match (&self.identity, &other.identity) {
                (Some(left), Some(right)) => left == right,
                _ => Arc::ptr_eq(&self.rgba, &other.rgba),
            }
    }
}

impl DecodedImage {
    /// Returns the OFD image resource identifier.
    pub fn resource_id(&self) -> u64 {
        self.resource_id
    }

    /// Returns the decoded width in pixels.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Returns the decoded height in pixels.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Returns `(width, height)` in pixels.
    pub fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// Returns the number of bytes between adjacent RGBA8 rows.
    pub fn stride(&self) -> usize {
        self.stride
    }

    /// Returns the native RGBA8 bytes in top-to-bottom row-major order.
    pub fn rgba(&self) -> &[u8] {
        &self.rgba
    }

    /// Returns a shared owner for the immutable RGBA8 bytes.
    pub fn rgba_arc(&self) -> Arc<[u8]> {
        Arc::clone(&self.rgba)
    }

    /// Returns one RGBA8 pixel, or `None` when `(x, y)` is outside the image.
    pub fn pixel(&self, x: u32, y: u32) -> Option<[u8; 4]> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let offset = usize::try_from(y)
            .ok()?
            .checked_mul(self.stride)?
            .checked_add(usize::try_from(x).ok()?.checked_mul(4)?)?;
        let pixel = self.rgba.get(offset..offset.checked_add(4)?)?;
        Some([pixel[0], pixel[1], pixel[2], pixel[3]])
    }

    /// Returns the decoded RGBA byte length.
    pub fn byte_len(&self) -> u64 {
        u64::try_from(self.rgba.len()).unwrap_or(u64::MAX)
    }

    /// Builds an image from already-composed RGBA pixels, keeping the
    /// identity and resource attribution of the source it derives from.
    pub(crate) fn from_parts(source: &DecodedImage, rgba: Vec<u8>) -> Result<Self> {
        let stride = usize::try_from(u64::from(source.width) * 4)
            .ok()
            .filter(|stride| {
                stride
                    .checked_mul(source.height as usize)
                    .is_some_and(|total| total == rgba.len())
            })
            .ok_or_else(|| Error::ImageDecode {
                resource_id: source.resource_id(),
                path: String::new(),
                message: "composed image size overflow".to_owned(),
            })?;
        Ok(Self {
            identity: source.identity.clone(),
            resource_id: source.resource_id,
            width: source.width,
            height: source.height,
            stride,
            rgba: Arc::from(rgba),
        })
    }

    pub(crate) fn allocation_id(&self) -> usize {
        self.rgba.as_ptr() as usize
    }
}

#[derive(Clone, Debug)]
enum DecodeFailure {
    Unsupported {
        resource_id: u64,
        path: String,
    },
    FormatMismatch {
        resource_id: u64,
        path: String,
        declared: ImageFormat,
        detected: ImageFormat,
    },
    Decode {
        resource_id: u64,
        path: String,
        message: String,
    },
    InvalidDimensions {
        resource_id: u64,
        path: String,
        width: u32,
        height: u32,
    },
    DimensionsOverflow {
        resource_id: u64,
        path: String,
        width: u32,
        height: u32,
    },
    Limit {
        resource_id: u64,
        path: String,
        field: &'static str,
        actual: u64,
        max: u64,
    },
    Cache(String),
}

impl DecodeFailure {
    fn from_error(error: Error) -> Self {
        match error {
            Error::UnsupportedImageFormat { resource_id, path } => {
                Self::Unsupported { resource_id, path }
            }
            Error::ImageFormatMismatch {
                resource_id,
                path,
                declared,
                detected,
            } => Self::FormatMismatch {
                resource_id,
                path,
                declared,
                detected,
            },
            Error::ImageDecode {
                resource_id,
                path,
                message,
            } => Self::Decode {
                resource_id,
                path,
                message,
            },
            Error::InvalidImageDimensions {
                resource_id,
                path,
                width,
                height,
            } => Self::InvalidDimensions {
                resource_id,
                path,
                width,
                height,
            },
            Error::ImageDimensionsOverflow {
                resource_id,
                path,
                width,
                height,
            } => Self::DimensionsOverflow {
                resource_id,
                path,
                width,
                height,
            },
            Error::ImageLimitExceeded {
                resource_id,
                path,
                field,
                actual,
                max,
            } => Self::Limit {
                resource_id,
                path,
                field,
                actual,
                max,
            },
            error => Self::Cache(error.to_string()),
        }
    }

    fn into_error(self) -> Error {
        match self {
            Self::Unsupported { resource_id, path } => {
                Error::UnsupportedImageFormat { resource_id, path }
            }
            Self::FormatMismatch {
                resource_id,
                path,
                declared,
                detected,
            } => Error::ImageFormatMismatch {
                resource_id,
                path,
                declared,
                detected,
            },
            Self::Decode {
                resource_id,
                path,
                message,
            } => Error::ImageDecode {
                resource_id,
                path,
                message,
            },
            Self::InvalidDimensions {
                resource_id,
                path,
                width,
                height,
            } => Error::InvalidImageDimensions {
                resource_id,
                path,
                width,
                height,
            },
            Self::DimensionsOverflow {
                resource_id,
                path,
                width,
                height,
            } => Error::ImageDimensionsOverflow {
                resource_id,
                path,
                width,
                height,
            },
            Self::Limit {
                resource_id,
                path,
                field,
                actual,
                max,
            } => Error::ImageLimitExceeded {
                resource_id,
                path,
                field,
                actual,
                max,
            },
            Self::Cache(message) => Error::ImageCache { message },
        }
    }
}

#[derive(Debug, Default)]
struct DecodeSlot {
    outcome: Mutex<Option<std::result::Result<DecodedImage, DecodeFailure>>>,
    completed: Condvar,
}

impl DecodeSlot {
    fn complete(&self, outcome: std::result::Result<DecodedImage, DecodeFailure>) -> Result<()> {
        let mut stored = self.outcome.lock().map_err(|_| Error::ImageCache {
            message: "image decode slot is poisoned".to_owned(),
        })?;
        *stored = Some(outcome);
        self.completed.notify_all();
        Ok(())
    }

    fn wait(&self) -> Result<DecodedImage> {
        let mut stored = self.outcome.lock().map_err(|_| Error::ImageCache {
            message: "image decode slot is poisoned".to_owned(),
        })?;
        while stored.is_none() {
            stored = self.completed.wait(stored).map_err(|_| Error::ImageCache {
                message: "image decode slot is poisoned".to_owned(),
            })?;
        }
        stored
            .as_ref()
            .expect("completed image slot checked")
            .clone()
            .map_err(DecodeFailure::into_error)
    }
}

#[derive(Debug)]
struct CacheEntry {
    image: DecodedImage,
    last_used: u64,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
enum CacheKey {
    Resource(ResourceIdentity),
    #[cfg(test)]
    Test(u64),
}

#[derive(Debug, Default)]
struct CacheState {
    ready: HashMap<CacheKey, CacheEntry>,
    in_flight: HashMap<CacheKey, Arc<DecodeSlot>>,
    ready_bytes: u64,
    clock: u64,
}

impl CacheState {
    fn tick(&mut self) -> u64 {
        if self.clock == u64::MAX {
            self.ready.clear();
            self.ready_bytes = 0;
            self.clock = 0;
        }
        self.clock += 1;
        self.clock
    }
}

/// Thread-safe bounded decoder and deterministic decoded-image LRU cache.
///
/// The default retains at most 64 MiB of decoded RGBA bytes. A valid image larger
/// than the configured cache budget is returned without caching. Cache eviction
/// never invalidates existing [`DecodedImage`] clones.
pub struct ImageDecoder {
    cache_byte_budget: u64,
    cache: Mutex<CacheState>,
}

impl Default for ImageDecoder {
    fn default() -> Self {
        Self::with_cache_byte_budget(64 * 1024 * 1024)
            .expect("the built-in image cache budget is positive")
    }
}

impl ImageDecoder {
    /// Constructs a decoder with a positive decoded-RGBA cache byte budget.
    pub fn with_cache_byte_budget(cache_byte_budget: u64) -> Result<Self> {
        if cache_byte_budget == 0 {
            return Err(Error::InvalidOption {
                field: "image_cache_byte_budget",
                value: "0".to_owned(),
            });
        }
        Ok(Self {
            cache_byte_budget,
            cache: Mutex::new(CacheState::default()),
        })
    }

    /// Returns the maximum decoded RGBA bytes retained by this decoder.
    pub fn cache_byte_budget(&self) -> u64 {
        self.cache_byte_budget
    }

    /// Validates and decodes one supported image resource into native RGBA8
    /// pixels.
    ///
    /// PNG, JPEG, BMP, GIF, and TIFF resources decode through the `image`
    /// crate. With the default `jbig2` feature, JBIG2 resources decode
    /// through the system jbig2dec library; without it they are recognized
    /// but fail with [`Error::UnsupportedImageFormat`].
    ///
    /// Magic, dimensions, and the caller's per-image limits are validated before
    /// pixel allocation. Work for one resource is single-flight; unrelated resources
    /// decode without holding the cache-index lock.
    pub fn decode(
        &self,
        resource: &ImageResource,
        limits: &ResourceLimits,
    ) -> Result<DecodedImage> {
        let format = preflight(resource, limits)?;
        self.cached(CacheKey::Resource(resource.identity()), || {
            decode_pixels(resource, limits, format)
        })
    }

    fn cached(
        &self,
        key: CacheKey,
        loader: impl FnOnce() -> Result<DecodedImage>,
    ) -> Result<DecodedImage> {
        self.cached_with_join_observer(key, loader, || {})
    }

    fn cached_with_join_observer(
        &self,
        key: CacheKey,
        loader: impl FnOnce() -> Result<DecodedImage>,
        follower_joined: impl FnOnce(),
    ) -> Result<DecodedImage> {
        let (slot, leader) = {
            let mut cache = self.cache.lock().map_err(|_| Error::ImageCache {
                message: "image cache index is poisoned".to_owned(),
            })?;
            let tick = cache.tick();
            if let Some(entry) = cache.ready.get_mut(&key) {
                entry.last_used = tick;
                return Ok(entry.image.clone());
            }
            if let Some(slot) = cache.in_flight.get(&key) {
                (Arc::clone(slot), false)
            } else {
                let slot = Arc::new(DecodeSlot::default());
                cache.in_flight.insert(key.clone(), Arc::clone(&slot));
                (slot, true)
            }
        };
        if !leader {
            follower_joined();
            return slot.wait();
        }

        let outcome = loader().map_err(DecodeFailure::from_error);
        if let Ok(image) = &outcome {
            let byte_len = image.byte_len();
            if byte_len <= self.cache_byte_budget {
                let mut cache = self.cache.lock().map_err(|_| Error::ImageCache {
                    message: "image cache index is poisoned".to_owned(),
                })?;
                cache.in_flight.remove(&key);
                while cache.ready_bytes.saturating_add(byte_len) > self.cache_byte_budget {
                    let evicted = cache
                        .ready
                        .iter()
                        .min_by_key(|(_, entry)| entry.last_used)
                        .map(|(identity, _)| identity.clone())
                        .expect("over-budget image cache is nonempty");
                    if let Some(entry) = cache.ready.remove(&evicted) {
                        cache.ready_bytes =
                            cache.ready_bytes.saturating_sub(entry.image.byte_len());
                    }
                }
                let tick = cache.tick();
                cache.ready_bytes =
                    cache
                        .ready_bytes
                        .checked_add(byte_len)
                        .ok_or_else(|| Error::ImageCache {
                            message: "cached image byte count overflow".to_owned(),
                        })?;
                cache.ready.insert(
                    key,
                    CacheEntry {
                        image: image.clone(),
                        last_used: tick,
                    },
                );
                drop(cache);
                slot.complete(outcome.clone())?;
            } else {
                slot.complete(outcome.clone())?;
                let mut cache = self.cache.lock().map_err(|_| Error::ImageCache {
                    message: "image cache index is poisoned".to_owned(),
                })?;
                cache.in_flight.remove(&key);
            }
        } else {
            slot.complete(outcome.clone())?;
            let mut cache = self.cache.lock().map_err(|_| Error::ImageCache {
                message: "image cache index is poisoned".to_owned(),
            })?;
            cache.in_flight.remove(&key);
        }
        outcome.map_err(DecodeFailure::into_error)
    }
}

/// The decoding backend one resource resolves to.
enum SourceFormat {
    /// An `image`-crate decoder format.
    Image(DecoderFormat),
    /// A standalone JBIG2 stream; decodable only with the `jbig2` feature.
    #[allow(dead_code)]
    Jbig2,
}

fn preflight(resource: &ImageResource, limits: &ResourceLimits) -> Result<SourceFormat> {
    let bytes = resource.encoded_bytes();
    let encoded_len = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    check_limit(
        resource,
        "encoded bytes",
        encoded_len,
        limits.max_encoded_image_bytes,
    )?;
    let detected = detect_format(resource)?;
    // The catalog's declared format is unreliable metadata from real-world
    // producers (invoice generators declaring PNG for JPEG data); the byte
    // signature is authoritative, so decoding always follows `detected`.
    if detected == ImageFormat::Jbig2 {
        if cfg!(feature = "jbig2") {
            // Page dimensions only exist after decoding, so validation
            // happens in the JBIG2 decode path.
            return Ok(SourceFormat::Jbig2);
        }
        return Err(Error::UnsupportedImageFormat {
            resource_id: resource.id(),
            path: resource.asset_path().to_owned(),
        });
    }
    let decoder_format = match detected {
        ImageFormat::Png => DecoderFormat::Png,
        ImageFormat::Jpeg => DecoderFormat::Jpeg,
        ImageFormat::Bmp => DecoderFormat::Bmp,
        ImageFormat::Gif => DecoderFormat::Gif,
        ImageFormat::Tiff => DecoderFormat::Tiff,
        // Jbig2 is handled above; other future formats stay unsupported.
        _ => {
            return Err(Error::UnsupportedImageFormat {
                resource_id: resource.id(),
                path: resource.asset_path().to_owned(),
            })
        }
    };
    let (width, height) = dimensions(resource, decoder_format)?;
    validate_dimensions(resource, width, height, limits)?;
    Ok(SourceFormat::Image(decoder_format))
}

fn detect_format(resource: &ImageResource) -> Result<ImageFormat> {
    let bytes = resource.encoded_bytes();
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Ok(ImageFormat::Png);
    }
    if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        return Ok(ImageFormat::Jpeg);
    }
    if bytes.starts_with(b"BM") {
        return Ok(ImageFormat::Bmp);
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return Ok(ImageFormat::Gif);
    }
    if bytes.starts_with(b"II\x2a\x00") || bytes.starts_with(b"MM\x00\x2a") {
        return Ok(ImageFormat::Tiff);
    }
    if bytes.starts_with(b"\x97JB2\r\n\x1a\n") {
        return Ok(ImageFormat::Jbig2);
    }
    Err(Error::UnsupportedImageFormat {
        resource_id: resource.id(),
        path: resource.asset_path().to_owned(),
    })
}

fn dimensions(resource: &ImageResource, format: DecoderFormat) -> Result<(u32, u32)> {
    let bytes = resource.encoded_bytes();
    if format == DecoderFormat::Png {
        if bytes.len() < 24 || &bytes[12..16] != b"IHDR" {
            return Err(decode_error(resource, "truncated or malformed PNG header"));
        }
        let width = u32::from_be_bytes(bytes[16..20].try_into().expect("fixed PNG width slice"));
        let height = u32::from_be_bytes(bytes[20..24].try_into().expect("fixed PNG height slice"));
        return Ok((width, height));
    }
    ImageReader::with_format(Cursor::new(bytes), format)
        .into_dimensions()
        .map_err(|error| decode_error(resource, error.to_string()))
}

fn validate_dimensions(
    resource: &ImageResource,
    width: u32,
    height: u32,
    limits: &ResourceLimits,
) -> Result<(u64, u64)> {
    if width == 0 || height == 0 {
        return Err(Error::InvalidImageDimensions {
            resource_id: resource.id(),
            path: resource.asset_path().to_owned(),
            width,
            height,
        });
    }
    let pixels = u64::from(width)
        .checked_mul(u64::from(height))
        .ok_or_else(|| overflow_error(resource, width, height))?;
    let decoded_bytes = pixels
        .checked_mul(4)
        .ok_or_else(|| overflow_error(resource, width, height))?;
    let stride = u64::from(width)
        .checked_mul(4)
        .and_then(|value| usize::try_from(value).ok())
        .ok_or_else(|| overflow_error(resource, width, height))?;
    let total =
        usize::try_from(decoded_bytes).map_err(|_| overflow_error(resource, width, height))?;
    if stride.checked_mul(usize::try_from(height).unwrap_or(usize::MAX)) != Some(total) {
        return Err(overflow_error(resource, width, height));
    }
    check_limit(
        resource,
        "decoded pixels",
        pixels,
        limits.max_decoded_image_pixels,
    )?;
    check_limit(
        resource,
        "decoded bytes",
        decoded_bytes,
        limits.max_decoded_image_bytes,
    )?;
    Ok((pixels, decoded_bytes))
}

fn decode_pixels(
    resource: &ImageResource,
    limits: &ResourceLimits,
    format: SourceFormat,
) -> Result<DecodedImage> {
    #[cfg(feature = "jbig2")]
    if let SourceFormat::Jbig2 = format {
        return decode_jbig2_pixels(resource, limits);
    }
    let SourceFormat::Image(format) = format else {
        return Err(Error::UnsupportedImageFormat {
            resource_id: resource.id(),
            path: resource.asset_path().to_owned(),
        });
    };
    let (width, height) = dimensions(resource, format)?;
    let (_, expected_bytes) = validate_dimensions(resource, width, height, limits)?;
    let mut reader = ImageReader::with_format(Cursor::new(resource.encoded_bytes()), format);
    let mut decoder_limits = Limits::default();
    decoder_limits.max_image_width = Some(width);
    decoder_limits.max_image_height = Some(height);
    decoder_limits.max_alloc = Some(limits.max_decoded_image_bytes);
    reader.limits(decoder_limits);
    let decoded = reader
        .decode()
        .map_err(|error| decode_error(resource, error.to_string()))?;
    if decoded.dimensions() != (width, height) {
        return Err(decode_error(
            resource,
            "decoded dimensions changed after header validation",
        ));
    }
    let rgba = decoded.into_rgba8().into_raw();
    if u64::try_from(rgba.len()).unwrap_or(u64::MAX) != expected_bytes {
        return Err(decode_error(
            resource,
            "decoded RGBA length does not match dimensions",
        ));
    }
    let stride = usize::try_from(u64::from(width) * 4)
        .map_err(|_| overflow_error(resource, width, height))?;
    Ok(DecodedImage {
        identity: Some(resource.identity()),
        resource_id: resource.id(),
        width,
        height,
        stride,
        rgba: Arc::from(rgba),
    })
}

/// Decodes one standalone JBIG2 stream and applies the same dimension and
/// budget validation as the `image`-crate path, after decoding because page
/// dimensions only become known then.
#[cfg(feature = "jbig2")]
fn decode_jbig2_pixels(resource: &ImageResource, limits: &ResourceLimits) -> Result<DecodedImage> {
    let decoded = crate::jbig2::decode_standalone(resource.encoded_bytes())
        .map_err(|message| decode_error(resource, message))?;
    let (width, height) = (decoded.width, decoded.height);
    let (_, expected_bytes) = validate_dimensions(resource, width, height, limits)?;
    if u64::try_from(decoded.rgba.len()).unwrap_or(u64::MAX) != expected_bytes {
        return Err(decode_error(
            resource,
            "decoded RGBA length does not match dimensions",
        ));
    }
    let stride = usize::try_from(u64::from(width) * 4)
        .map_err(|_| overflow_error(resource, width, height))?;
    Ok(DecodedImage {
        identity: Some(resource.identity()),
        resource_id: resource.id(),
        width,
        height,
        stride,
        rgba: Arc::from(decoded.rgba),
    })
}

fn check_limit(resource: &ImageResource, field: &'static str, actual: u64, max: u64) -> Result<()> {
    if actual > max {
        return Err(Error::ImageLimitExceeded {
            resource_id: resource.id(),
            path: resource.asset_path().to_owned(),
            field,
            actual,
            max,
        });
    }
    Ok(())
}

fn decode_error(resource: &ImageResource, message: impl Into<String>) -> Error {
    Error::ImageDecode {
        resource_id: resource.id(),
        path: resource.asset_path().to_owned(),
        message: message.into(),
    }
}

fn overflow_error(resource: &ImageResource, width: u32, height: u32) -> Error {
    Error::ImageDimensionsOverflow {
        resource_id: resource.id(),
        path: resource.asset_path().to_owned(),
        width,
        height,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::mpsc;
    use std::sync::Barrier;
    use std::thread;
    use std::time::Duration;

    use super::*;

    fn image(value: u8, byte_len: usize) -> DecodedImage {
        let pixels = vec![value; byte_len];
        DecodedImage {
            identity: None,
            resource_id: u64::from(value),
            width: u32::try_from(byte_len / 4).unwrap(),
            height: 1,
            stride: byte_len,
            rgba: Arc::from(pixels),
        }
    }

    #[test]
    fn overlapping_success_cohort_uses_one_in_flight_attempt() {
        const WORKERS: usize = 12;
        let decoder = Arc::new(ImageDecoder::with_cache_byte_budget(64).unwrap());
        let initializations = Arc::new(AtomicUsize::new(0));
        let start = Arc::new(Barrier::new(WORKERS + 1));
        let gate = Arc::new((Mutex::new(false), Condvar::new()));
        let (leader_entered_tx, leader_entered_rx) = mpsc::channel();
        let (follower_joined_tx, follower_joined_rx) = mpsc::channel();
        let threads = (0..WORKERS)
            .map(|_| {
                let decoder = Arc::clone(&decoder);
                let initializations = Arc::clone(&initializations);
                let start = Arc::clone(&start);
                let gate = Arc::clone(&gate);
                let leader_entered_tx = leader_entered_tx.clone();
                let follower_joined_tx = follower_joined_tx.clone();
                thread::spawn(move || {
                    start.wait();
                    decoder.cached_with_join_observer(
                        CacheKey::Test(1),
                        || {
                            initializations.fetch_add(1, Ordering::SeqCst);
                            leader_entered_tx.send(()).unwrap();
                            let (open, released) = &*gate;
                            let mut open = open.lock().unwrap();
                            while !*open {
                                open = released.wait(open).unwrap();
                            }
                            Ok(image(1, 4))
                        },
                        || follower_joined_tx.send(()).unwrap(),
                    )
                })
            })
            .collect::<Vec<_>>();
        drop(leader_entered_tx);
        drop(follower_joined_tx);
        start.wait();
        leader_entered_rx
            .recv_timeout(Duration::from_secs(2))
            .unwrap();
        for _ in 1..WORKERS {
            follower_joined_rx
                .recv_timeout(Duration::from_secs(2))
                .unwrap();
        }
        assert_eq!(initializations.load(Ordering::SeqCst), 1);
        let (open, released) = &*gate;
        *open.lock().unwrap() = true;
        released.notify_all();
        for worker in threads {
            worker.join().unwrap().unwrap();
        }
        assert_eq!(initializations.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn overlapping_failure_cohort_shares_one_attempt_then_later_retry_succeeds() {
        const WORKERS: usize = 12;
        let decoder = Arc::new(ImageDecoder::with_cache_byte_budget(64).unwrap());
        let attempts = Arc::new(AtomicUsize::new(0));
        let start = Arc::new(Barrier::new(WORKERS + 1));
        let gate = Arc::new((Mutex::new(false), Condvar::new()));
        let (leader_entered_tx, leader_entered_rx) = mpsc::channel();
        let (follower_joined_tx, follower_joined_rx) = mpsc::channel();
        let threads = (0..WORKERS)
            .map(|_| {
                let decoder = Arc::clone(&decoder);
                let attempts = Arc::clone(&attempts);
                let start = Arc::clone(&start);
                let gate = Arc::clone(&gate);
                let leader_entered_tx = leader_entered_tx.clone();
                let follower_joined_tx = follower_joined_tx.clone();
                thread::spawn(move || {
                    start.wait();
                    decoder.cached_with_join_observer(
                        CacheKey::Test(2),
                        || {
                            attempts.fetch_add(1, Ordering::SeqCst);
                            leader_entered_tx.send(()).unwrap();
                            let (open, released) = &*gate;
                            let mut open = open.lock().unwrap();
                            while !*open {
                                open = released.wait(open).unwrap();
                            }
                            Err(Error::ImageDecode {
                                resource_id: 2,
                                path: "Doc/image.png".to_owned(),
                                message: "injected".to_owned(),
                            })
                        },
                        || follower_joined_tx.send(()).unwrap(),
                    )
                })
            })
            .collect::<Vec<_>>();
        drop(leader_entered_tx);
        drop(follower_joined_tx);
        start.wait();
        leader_entered_rx
            .recv_timeout(Duration::from_secs(2))
            .unwrap();
        for _ in 1..WORKERS {
            follower_joined_rx
                .recv_timeout(Duration::from_secs(2))
                .unwrap();
        }
        assert_eq!(attempts.load(Ordering::SeqCst), 1);
        let (open, released) = &*gate;
        *open.lock().unwrap() = true;
        released.notify_all();
        for worker in threads {
            assert!(matches!(
                worker.join().unwrap(),
                Err(Error::ImageDecode {
                    resource_id: 2,
                    ref message,
                    ..
                }) if message == "injected"
            ));
        }
        assert_eq!(attempts.load(Ordering::SeqCst), 1);

        decoder
            .cached(CacheKey::Test(2), || {
                attempts.fetch_add(1, Ordering::SeqCst);
                Ok(image(2, 4))
            })
            .unwrap();
        assert_eq!(attempts.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn unrelated_cache_loads_run_concurrently() {
        let decoder = Arc::new(ImageDecoder::with_cache_byte_budget(64).unwrap());
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_one_tx, release_one_rx) = mpsc::channel();
        let (release_two_tx, release_two_rx) = mpsc::channel();

        let first_decoder = Arc::clone(&decoder);
        let first_entered = entered_tx.clone();
        let first = thread::spawn(move || {
            first_decoder.cached(CacheKey::Test(1), || {
                first_entered.send(1).unwrap();
                release_one_rx.recv().unwrap();
                Ok(image(1, 4))
            })
        });
        assert_eq!(entered_rx.recv_timeout(Duration::from_secs(2)).unwrap(), 1);

        let second_decoder = Arc::clone(&decoder);
        let second = thread::spawn(move || {
            second_decoder.cached(CacheKey::Test(2), || {
                entered_tx.send(2).unwrap();
                release_two_rx.recv().unwrap();
                Ok(image(2, 4))
            })
        });
        let second_entered = entered_rx.recv_timeout(Duration::from_secs(2));
        release_one_tx.send(()).unwrap();
        release_two_tx.send(()).unwrap();
        assert_eq!(second_entered.unwrap(), 2);
        first.join().unwrap().unwrap();
        second.join().unwrap().unwrap();
    }

    #[test]
    fn byte_lru_is_exact_and_oversize_success_is_not_retained() {
        let decoder = ImageDecoder::with_cache_byte_budget(8).unwrap();
        let loads = AtomicUsize::new(0);
        let load = |value, length| {
            loads.fetch_add(1, Ordering::SeqCst);
            Ok(image(value, length))
        };
        let first = decoder.cached(CacheKey::Test(1), || load(1, 4)).unwrap();
        decoder.cached(CacheKey::Test(2), || load(2, 4)).unwrap();
        decoder.cached(CacheKey::Test(1), || load(1, 4)).unwrap();
        decoder.cached(CacheKey::Test(3), || load(3, 4)).unwrap();
        assert_eq!(loads.load(Ordering::SeqCst), 3);
        assert!(first.pixel(0, 0).is_some());
        decoder.cached(CacheKey::Test(2), || load(2, 4)).unwrap();
        assert_eq!(loads.load(Ordering::SeqCst), 4);

        let large = decoder.cached(CacheKey::Test(9), || load(9, 12)).unwrap();
        decoder.cached(CacheKey::Test(9), || load(9, 12)).unwrap();
        assert_eq!(loads.load(Ordering::SeqCst), 6);
        assert!(large.pixel(0, 0).is_some());
    }
}
