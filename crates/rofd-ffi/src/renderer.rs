use crate::abi::{ROFD_RENDERER_OPTIONS_V1_SIZE, ROFD_RENDER_OPTIONS_V1_SIZE};
use crate::document::page_ref;
use crate::error::{
    boundary, boundary_with_inputs, FfiError, HandleOutput, InputRanges, ScalarOutput,
};
use crate::handles::{drop_raw_handle, handle_ref, PageHandle, RenderReportHandle, RendererHandle};
use crate::{
    rofd_error_t, rofd_page_t, rofd_render_options_t, rofd_render_report_t,
    rofd_renderer_options_t, rofd_renderer_t, rofd_status_t, ROFD_IMAGE_INTERPOLATION_BILINEAR,
    ROFD_IMAGE_INTERPOLATION_NEAREST,
};
use rofd_core::{Color, Rect};
use rofd_render::{
    CairoRenderer, ImageDecoder, ImageInterpolation, RenderOptions, SystemFontResolver,
};
use std::ffi::CStr;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::slice;
use std::sync::{Arc, LazyLock};

const MAX_FALLBACK_FAMILIES: usize = 1024;
const DEFAULT_FALLBACK_FAMILIES: [&str; 3] = ["Noto Sans CJK SC", "Noto Sans", "DejaVu Sans"];

struct RendererOptionsInput {
    fallback_families: Vec<String>,
    max_font_bytes: u64,
    image_cache_bytes: u64,
}

impl RendererOptionsInput {
    unsafe fn from_ffi(options: *const rofd_renderer_options_t) -> Result<Self, FfiError> {
        if options.is_null() {
            return Ok(Self::defaults());
        }

        // SAFETY: The caller guarantees that a non-null options pointer is aligned for its
        // record type and designates a valid object with an initialized, readable struct_size.
        let struct_size = unsafe { (*options).struct_size as usize };
        if struct_size < ROFD_RENDERER_OPTIONS_V1_SIZE {
            return Err(FfiError::invalid_argument(format!(
                "renderer options struct_size {struct_size} is smaller than v1 size {ROFD_RENDERER_OPTIONS_V1_SIZE}"
            )));
        }

        // SAFETY: The accepted size covers the complete permanent v1 prefix. Unknown tail bytes
        // are deliberately ignored.
        let (families, family_count, max_font_bytes, image_cache_bytes) = unsafe {
            (
                (*options).fallback_families,
                (*options).fallback_family_count,
                (*options).max_font_bytes,
                (*options).image_cache_bytes,
            )
        };
        if max_font_bytes == 0 {
            return Err(FfiError::invalid_argument(
                "max_font_bytes must be positive",
            ));
        }
        if image_cache_bytes == 0 {
            return Err(FfiError::invalid_argument(
                "image_cache_bytes must be positive",
            ));
        }
        if family_count > MAX_FALLBACK_FAMILIES {
            return Err(FfiError::invalid_argument(format!(
                "fallback_family_count {family_count} exceeds {MAX_FALLBACK_FAMILIES}"
            )));
        }

        let fallback_families = if family_count == 0 {
            if families.is_null() {
                default_fallback_families()
            } else {
                Vec::new()
            }
        } else {
            if families.is_null() {
                return Err(FfiError::invalid_argument(
                    "fallback_families is NULL with a nonzero count",
                ));
            }
            // SAFETY: The caller guarantees that a non-null array pointer is correctly aligned,
            // designates the first element of one initialized allocation readable for
            // family_count pointers, and has a byte extent representable by isize. The count
            // bound is checked before forming this slice.
            let families = unsafe { slice::from_raw_parts(families, family_count) };
            let mut owned = Vec::with_capacity(family_count);
            for (index, &family) in families.iter().enumerate() {
                if family.is_null() {
                    return Err(FfiError::invalid_argument(format!(
                        "fallback_families[{index}] is NULL"
                    )));
                }
                // SAFETY: Each non-null element points to the first byte of one readable
                // allocation containing its terminating NUL within an isize-representable byte
                // range. It remains valid for the call; `to_str` validates UTF-8 before copying.
                let family = unsafe { CStr::from_ptr(family) }
                    .to_str()
                    .map_err(|_| {
                        FfiError::invalid_argument(format!(
                            "fallback_families[{index}] is not valid UTF-8"
                        ))
                    })?
                    .to_owned();
                owned.push(family);
            }
            owned
        };

        Ok(Self {
            fallback_families,
            max_font_bytes,
            image_cache_bytes,
        })
    }

    fn defaults() -> Self {
        Self {
            fallback_families: default_fallback_families(),
            max_font_bytes: 64 * 1024 * 1024,
            image_cache_bytes: 64 * 1024 * 1024,
        }
    }
}

fn default_fallback_families() -> Vec<String> {
    DEFAULT_FALLBACK_FAMILIES
        .into_iter()
        .map(str::to_owned)
        .collect()
}

/// Font metrics for the semantic page text index.
///
/// Page text geometry is resolved with the same font stack a default renderer uses
/// (`rofd_renderer_new` with NULL options), so the page text layout, search
/// rectangles and selection regions land on the glyphs that get drawn - including the
/// vertical extent of each glyph, so a highlight covers the text it belongs to. The
/// resolver is created once per process and shared with every document that resolves
/// metrics, because the system font database scan is the expensive part. A renderer
/// created with other fallback families still chooses its own faces for drawing.
pub(crate) fn text_metrics_provider() -> rofd_render::FontTextMetrics {
    static RESOLVER: LazyLock<Arc<SystemFontResolver>> = LazyLock::new(|| {
        Arc::new(SystemFontResolver::with_system_fonts(
            default_fallback_families(),
            64 * 1024 * 1024,
        ))
    });
    let resolver: Arc<SystemFontResolver> = Arc::clone(&RESOLVER);
    rofd_render::FontTextMetrics::new(resolver)
}

pub(crate) struct RenderOptionsInput;

impl RenderOptionsInput {
    pub(crate) unsafe fn from_ffi(
        options: *const rofd_render_options_t,
    ) -> Result<RenderOptions, FfiError> {
        if options.is_null() {
            return Ok(RenderOptions::default());
        }

        // SAFETY: The caller guarantees that a non-null options pointer is aligned for its
        // record type and designates a valid object with an initialized, readable struct_size.
        let struct_size = unsafe { (*options).struct_size as usize };
        if struct_size < ROFD_RENDER_OPTIONS_V1_SIZE {
            return Err(FfiError::invalid_argument(format!(
                "render options struct_size {struct_size} is smaller than v1 size {ROFD_RENDER_OPTIONS_V1_SIZE}"
            )));
        }

        // SAFETY: The accepted size covers every v1 field. Unknown tail bytes are ignored.
        let options = unsafe { &*options };
        let image_interpolation = match options.image_interpolation {
            ROFD_IMAGE_INTERPOLATION_NEAREST => ImageInterpolation::Nearest,
            ROFD_IMAGE_INTERPOLATION_BILINEAR => ImageInterpolation::Bilinear,
            value => {
                return Err(FfiError::invalid_argument(format!(
                    "unknown image interpolation {value}"
                )))
            }
        };
        let clip = match options.has_clip {
            0 => None,
            1 => Some(Rect {
                x: options.clip_x_mm,
                y: options.clip_y_mm,
                width: options.clip_width_mm,
                height: options.clip_height_mm,
            }),
            value => {
                return Err(FfiError::invalid_argument(format!(
                    "has_clip must be 0 or 1, got {value}"
                )))
            }
        };
        let rotation_degrees = u16::try_from(options.rotation_degrees).map_err(|_| {
            FfiError::invalid_argument(format!(
                "rotation_degrees {} exceeds the supported integer domain",
                options.rotation_degrees
            ))
        })?;

        Ok(RenderOptions {
            dpi: options.dpi,
            scale: options.scale,
            rotation_degrees,
            background: Color {
                red: (options.background_rgba >> 24) as u8,
                green: (options.background_rgba >> 16) as u8,
                blue: (options.background_rgba >> 8) as u8,
                alpha: options.background_rgba as u8,
            },
            clip,
            image_interpolation,
            max_raster_bytes: options.max_raster_bytes,
        })
    }
}

pub(crate) unsafe fn renderer_ref<'a>(
    renderer: *const rofd_renderer_t,
) -> Result<&'a RendererHandle, FfiError> {
    if renderer.is_null() {
        return Err(FfiError::invalid_argument("renderer handle is NULL"));
    }
    // SAFETY: The caller guarantees this is a live renderer token, held immutably for the call.
    Ok(unsafe { handle_ref::<rofd_renderer_t>(renderer) })
}

/// Constructs a reusable renderer with one font snapshot and bounded image cache.
///
/// Null `options` selects built-in fallback families and 64 MiB service limits.
/// On success, `renderer` receives one owned handle that must be released with
/// [`rofd_renderer_free`].
///
/// # Safety
///
/// Non-null `options` must be aligned for its record type and designate a valid
/// object with an initialized, readable `struct_size`; a supported size must
/// cover a fully initialized, readable v1 prefix. When the fallback count is
/// nonzero, its array pointer must be aligned for C string pointers and designate
/// the first element of one initialized allocation readable for `count` elements
/// with an `isize`-representable extent. Every element must be non-null and point
/// to the first byte of one readable allocation containing valid UTF-8 and its
/// terminating NUL within an `isize`-representable range. All inputs stay valid
/// for the call and must be disjoint from the writable, aligned, mutually
/// disjoint `renderer` and `error` slots. None may be accessed concurrently.
#[no_mangle]
pub unsafe extern "C" fn rofd_renderer_new(
    options: *const rofd_renderer_options_t,
    renderer: *mut *mut rofd_renderer_t,
    error: *mut *mut rofd_error_t,
) -> rofd_status_t {
    // SAFETY: The boundary validates detectable output layout properties and transactionally
    // initializes and commits the renderer output.
    unsafe {
        boundary(error, HandleOutput::required(renderer), || {
            let options = RendererOptionsInput::from_ffi(options)?;
            let font_resolver = SystemFontResolver::with_system_fonts(
                options.fallback_families,
                options.max_font_bytes,
            );
            let image_decoder = ImageDecoder::with_cache_byte_budget(options.image_cache_bytes)
                .map_err(FfiError::from)?;
            Ok(Box::new(RendererHandle {
                font_resolver,
                image_decoder,
            }))
        })
    }
}

/// Returns the pixel dimensions required to render one page.
///
/// Null `options` selects the standard render defaults. On every failure, all
/// non-null scalar outputs are reset to zero.
///
/// # Safety
///
/// `renderer` and `page` must be live matching handles returned by this library,
/// kept immutable and not freed for the duration of the call. Non-null `options`
/// must be aligned for its record type and designate a valid object with an
/// initialized, readable `struct_size` and fully initialized, readable declared
/// v1 prefix. These input regions and handle storage must be disjoint from the
/// writable, aligned, mutually disjoint `pixel_width`, `pixel_height`, and
/// `error` output slots. None may be accessed concurrently during this call.
#[no_mangle]
pub unsafe extern "C" fn rofd_renderer_get_pixel_size(
    renderer: *const rofd_renderer_t,
    page: *const rofd_page_t,
    options: *const rofd_render_options_t,
    pixel_width: *mut i32,
    pixel_height: *mut i32,
    error: *mut *mut rofd_error_t,
) -> rofd_status_t {
    // SAFETY: The boundary validates and transactionally writes both required scalar outputs.
    unsafe {
        boundary(
            error,
            (
                ScalarOutput::required(pixel_width),
                ScalarOutput::required(pixel_height),
            ),
            || {
                let renderer = renderer_ref(renderer)?;
                let page = page_ref(page)?;
                let options = RenderOptionsInput::from_ffi(options)?;
                let size =
                    CairoRenderer::pixel_size(&page.inner, &options).map_err(FfiError::from)?;
                // Keep both reusable services borrowed by this operation. Pixel sizing itself is
                // service-independent, while the handle owns them for later Task 7 rendering.
                let _ = (&renderer.font_resolver, &renderer.image_decoder);
                Ok(size)
            },
        )
    }
}

pub(crate) fn render_input_ranges(
    renderer: *const rofd_renderer_t,
    page: *const rofd_page_t,
    context: *mut cairo::ffi::cairo_t,
    options: *const rofd_render_options_t,
) -> Result<InputRanges, ()> {
    let mut inputs = InputRanges::new();
    inputs.push(renderer.cast::<RendererHandle>())?;
    inputs.push(page.cast::<PageHandle>())?;
    inputs.push(options)?;
    // Cairo deliberately keeps cairo_t opaque. One byte is enough to reject an output slot that
    // starts at the same detectable address; all other overlap remains forbidden by contract.
    inputs.push_region(context.cast(), 1, 1)?;
    Ok(inputs)
}

unsafe fn render_page(
    renderer: *const rofd_renderer_t,
    page: *const rofd_page_t,
    context: *mut cairo::ffi::cairo_t,
    options: *const rofd_render_options_t,
) -> Result<rofd_render::RenderReport, FfiError> {
    let renderer = unsafe { renderer_ref(renderer)? };
    let page = unsafe { page_ref(page)? };
    if context.is_null() {
        return Err(FfiError::invalid_argument("Cairo context is NULL"));
    }
    let options = unsafe { RenderOptionsInput::from_ffi(options)? };
    // SAFETY: The caller guarantees a live cairo_t on a thread permitted to use it. from_raw_none
    // acquires one temporary Cairo reference; dropping this wrapper cannot consume the caller's
    // reference. Input/output preflight has already completed before this operation begins.
    let context = unsafe { cairo::Context::from_raw_none(context) };
    CairoRenderer
        .render_page_with_services(
            &page.inner,
            &context,
            &options,
            &renderer.font_resolver,
            &renderer.image_decoder,
        )
        .map_err(FfiError::from)
}

/// Renders a page into a borrowed Cairo context and optionally returns diagnostics.
///
/// Successful calls with a non-null `report` publish one owned report even when
/// it contains no diagnostics. A null `report` discards diagnostics. Every
/// failure leaves a non-null report output set to null.
///
/// # Safety
///
/// `renderer` and `page` must be live matching handles returned by this library,
/// kept immutable and not freed for the call. `context` must be a non-null live
/// `cairo_t` whose target and referenced objects remain valid; the calling thread
/// must be allowed to use it under Cairo's synchronization rules, and no other
/// thread may mutate it during this call. The FFI borrows it by taking and later
/// dropping one temporary Cairo reference without consuming caller ownership.
/// Non-null `options` must be aligned and readable through its initialized,
/// supported `struct_size`. All handle storage, the options prefix, and the
/// detectable Cairo address must be disjoint from the writable, aligned `report`
/// and `error` slots. The inputs and outputs remain live, non-overlapping, and
/// inaccessible to concurrent mutation for the complete call.
#[no_mangle]
pub unsafe extern "C" fn rofd_renderer_render_page_cairo(
    renderer: *const rofd_renderer_t,
    page: *const rofd_page_t,
    context: *mut cairo::ffi::cairo_t,
    options: *const rofd_render_options_t,
    report: *mut *mut rofd_render_report_t,
    error: *mut *mut rofd_error_t,
) -> rofd_status_t {
    let inputs = match render_input_ranges(renderer, page, context, options) {
        Ok(inputs) => inputs,
        Err(()) => {
            // SAFETY: Empty preflight inputs allow the output transaction to publish the error.
            return if report.is_null() {
                unsafe {
                    boundary(error, (), || {
                        Err(FfiError::invalid_argument(
                            "input region is misaligned or overflows",
                        ))
                    })
                }
            } else {
                unsafe {
                    boundary(error, HandleOutput::required(report), || {
                        Err(FfiError::invalid_argument(
                            "input region is misaligned or overflows",
                        ))
                    })
                }
            };
        }
    };

    if report.is_null() {
        // SAFETY: The boundary validates the optional error output against all declared inputs.
        unsafe {
            boundary_with_inputs(error, (), inputs, || {
                let _ = render_page(renderer, page, context, options)?;
                Ok(())
            })
        }
    } else {
        // SAFETY: The boundary nulls the requested report on entry, preflights all aliases, and
        // publishes ownership only after rendering and diagnostic copying both succeed.
        unsafe {
            boundary_with_inputs(error, HandleOutput::required(report), inputs, || {
                let report = render_page(renderer, page, context, options)?;
                Ok(Box::new(RenderReportHandle::from_render_report(report)?))
            })
        }
    }
}

/// Frees an owned renderer handle. A null handle is a no-op.
///
/// # Safety
///
/// A non-null handle must be live, uniquely owned by the caller, not previously
/// freed, and not used concurrently.
#[no_mangle]
pub unsafe extern "C" fn rofd_renderer_free(renderer: *mut rofd_renderer_t) {
    let _ = catch_unwind(AssertUnwindSafe(|| {
        if renderer.is_null() {
            return;
        }
        // SAFETY: The caller transfers the unique allocation back to its sealed token mapping;
        // drop_raw_handle reconstructs its Box exactly once.
        unsafe { drop_raw_handle::<rofd_renderer_t>(renderer) };
    }));
}

#[cfg(test)]
mod tests {
    use super::RenderOptionsInput;
    use crate::{
        rofd_render_options_t, ROFD_IMAGE_INTERPOLATION_NEAREST, ROFD_RENDER_OPTIONS_V1_SIZE,
    };

    #[test]
    fn render_conversion_preserves_rgba_clip_interpolation_and_budget() {
        let ffi = rofd_render_options_t {
            struct_size: ROFD_RENDER_OPTIONS_V1_SIZE as u32,
            dpi: 144.0,
            scale: 1.5,
            rotation_degrees: 270,
            background_rgba: 0x1234_5678,
            has_clip: 1,
            clip_x_mm: 1.0,
            clip_y_mm: 2.0,
            clip_width_mm: 3.0,
            clip_height_mm: 4.0,
            image_interpolation: ROFD_IMAGE_INTERPOLATION_NEAREST,
            max_raster_bytes: 987_654,
        };

        // SAFETY: `ffi` is one aligned, fully initialized v1 record.
        let Ok(converted) = (unsafe { RenderOptionsInput::from_ffi(&ffi) }) else {
            panic!("valid v1 render options must convert");
        };
        assert_eq!((converted.dpi, converted.scale), (144.0, 1.5));
        assert_eq!(converted.rotation_degrees, 270);
        assert_eq!(
            (
                converted.background.red,
                converted.background.green,
                converted.background.blue,
                converted.background.alpha,
            ),
            (0x12, 0x34, 0x56, 0x78)
        );
        assert_eq!(
            converted
                .clip
                .map(|clip| (clip.x, clip.y, clip.width, clip.height)),
            Some((1.0, 2.0, 3.0, 4.0))
        );
        assert_eq!(
            converted.image_interpolation,
            rofd_render::ImageInterpolation::Nearest
        );
        assert_eq!(converted.max_raster_bytes, 987_654);
    }
}
