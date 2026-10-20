use crate::abi::ROFD_LOAD_OPTIONS_V1_SIZE;
use crate::error::{boundary, FfiError, HandleOutput, ScalarOutput};
use crate::handles::{drop_raw_handle, handle_ref, DocumentHandle, PageHandle};
use crate::{
    rofd_document_t, rofd_error_t, rofd_load_options_t, rofd_page_t, rofd_rect_t, rofd_status_t,
    ROFD_STRICTNESS_LENIENT, ROFD_STRICTNESS_STRICT,
};
use std::ffi::{c_char, CStr};
use std::panic::{catch_unwind, AssertUnwindSafe};

struct LoadOptionsInput;

impl LoadOptionsInput {
    unsafe fn from_ffi(
        options: *const rofd_load_options_t,
    ) -> Result<rofd_core::LoadOptions, FfiError> {
        if options.is_null() {
            return Ok(rofd_core::LoadOptions::default());
        }

        // SAFETY: The caller guarantees a non-null options pointer addresses a readable,
        // aligned `struct_size` field for the duration of this call.
        let struct_size = unsafe { (*options).struct_size as usize };
        if struct_size < ROFD_LOAD_OPTIONS_V1_SIZE {
            return Err(FfiError::invalid_argument(format!(
                "load options struct_size {struct_size} is smaller than v1 size {ROFD_LOAD_OPTIONS_V1_SIZE}"
            )));
        }

        // SAFETY: The accepted size covers the complete permanent v1 prefix, including
        // `strictness`. Later bytes, if any, are deliberately ignored.
        let strictness = unsafe { (*options).strictness };
        let strictness = match strictness {
            ROFD_STRICTNESS_LENIENT => rofd_core::Strictness::Lenient,
            ROFD_STRICTNESS_STRICT => rofd_core::Strictness::Strict,
            value => {
                return Err(FfiError::invalid_argument(format!(
                    "unknown load strictness {value}"
                )))
            }
        };

        Ok(rofd_core::LoadOptions {
            strictness,
            ..rofd_core::LoadOptions::default()
        })
    }
}

pub(crate) unsafe fn document_ref<'a>(
    document: *const rofd_document_t,
) -> Result<&'a DocumentHandle, FfiError> {
    if document.is_null() {
        return Err(FfiError::invalid_argument("document handle is NULL"));
    }
    // SAFETY: The caller guarantees this is a live document token and keeps it alive and
    // immutably borrowed for the duration of the FFI call.
    Ok(unsafe { handle_ref::<rofd_document_t>(document) })
}

pub(crate) unsafe fn page_ref<'a>(page: *const rofd_page_t) -> Result<&'a PageHandle, FfiError> {
    if page.is_null() {
        return Err(FfiError::invalid_argument("page handle is NULL"));
    }
    // SAFETY: The caller guarantees this is a live page token and keeps it alive and immutably
    // borrowed for the duration of the FFI call.
    Ok(unsafe { handle_ref::<rofd_page_t>(page) })
}

/// Opens an OFD document from a NUL-terminated UTF-8 host path.
///
/// Null `options` selects defaults. On success, `document` receives one owned
/// handle that must be released with [`rofd_document_free`]. A null or empty
/// path is a defined invalid argument and leaves the handle output null.
///
/// # Safety
///
/// A null `path` is accepted as a defined failure. A non-null `path` must point
/// to readable NUL-terminated bytes that stay valid for the call; invalid UTF-8
/// and an empty string are defined failures. Non-null `options` must be correctly
/// aligned and readable through `struct_size`; when that field covers v1, the
/// complete v1 prefix must remain readable. The path and options regions must not
/// overlap `document` or `error`, whose non-null slots must be valid, writable,
/// aligned, and mutually disjoint. None may be concurrently modified.
#[no_mangle]
pub unsafe extern "C" fn rofd_document_open(
    path: *const c_char,
    options: *const rofd_load_options_t,
    document: *mut *mut rofd_document_t,
    error: *mut *mut rofd_error_t,
) -> rofd_status_t {
    // SAFETY: The shared boundary validates detectable output layout properties and owns all
    // initialization and publication writes.
    unsafe {
        boundary(error, HandleOutput::required(document), || {
            if path.is_null() {
                return Err(FfiError::invalid_argument("path is NULL"));
            }
            // SAFETY: The caller guarantees a non-null path points to a NUL-terminated byte string.
            let path = CStr::from_ptr(path)
                .to_str()
                .map_err(|_| FfiError::invalid_argument("path is not valid UTF-8"))?;
            if path.is_empty() {
                return Err(FfiError::invalid_argument("path is empty"));
            }
            let options = LoadOptionsInput::from_ffi(options)?;
            // Semantic text geometry resolves font metrics through the default font
            // stack, so search/selection rectangles match the rendered glyphs (see
            // `text_metrics_provider`).
            let metrics: std::sync::Arc<dyn rofd_core::TextMetricsProvider> =
                std::sync::Arc::new(crate::renderer::text_metrics_provider());
            let inner = rofd_core::Document::open_with_metrics(path, options, Some(metrics))
                .map_err(FfiError::from)?;
            Ok(Box::new(DocumentHandle { inner }))
        })
    }
}

/// Returns the number of pages indexed by a document.
///
/// # Safety
///
/// A non-null `document` must be a live document handle returned by this library.
/// Its storage must remain valid, must not be concurrently freed or modified, and
/// must not overlap the writable, aligned `page_count` or `error` output slots.
/// Non-null output slots must also be mutually disjoint and valid for the call.
#[no_mangle]
pub unsafe extern "C" fn rofd_document_get_page_count(
    document: *const rofd_document_t,
    page_count: *mut usize,
    error: *mut *mut rofd_error_t,
) -> rofd_status_t {
    // SAFETY: The shared boundary validates and transactionally writes the caller output slots.
    unsafe {
        boundary(error, ScalarOutput::required(page_count), || {
            Ok(document_ref(document)?.inner.page_count())
        })
    }
}

/// Copies the physical page box of a zero-based index as millimetres.
///
/// This metadata-only query reads the page area without parsing or caching page
/// content: it performs no content validation, collects no content warnings, and
/// also succeeds for a page whose content cannot be loaded. The value matches
/// [`rofd_page_get_size_mm`] for the same index.
///
/// # Safety
///
/// A non-null `document` must be a live document handle returned by this library.
/// Its storage must remain valid, must not be concurrently freed or modified, and
/// must not overlap the writable, aligned `page_size_mm` or `error` output slots.
/// Non-null output slots must also be mutually disjoint and valid for the call.
#[no_mangle]
pub unsafe extern "C" fn rofd_document_get_page_size_mm(
    document: *const rofd_document_t,
    page_index: usize,
    page_size_mm: *mut rofd_rect_t,
    error: *mut *mut rofd_error_t,
) -> rofd_status_t {
    // SAFETY: The shared boundary validates and transactionally writes the caller output slots.
    unsafe {
        boundary(error, ScalarOutput::required(page_size_mm), || {
            let size = document_ref(document)?
                .inner
                .page_size(page_index)
                .map_err(FfiError::from)?;
            Ok(rofd_rect_t {
                x_mm: size.x,
                y_mm: size.y,
                width_mm: size.width,
                height_mm: size.height,
            })
        })
    }
}

/// Returns an independently owned page handle for a zero-based index.
///
/// The returned page retains its document data and remains valid after the
/// document handle is freed.
///
/// # Safety
///
/// A non-null `document` must be a live document handle returned by this library.
/// Its storage must remain valid, must not be concurrently freed or modified, and
/// must not overlap the writable, aligned `page` or `error` output slots. Non-null
/// output slots must also be mutually disjoint and valid for the call.
#[no_mangle]
pub unsafe extern "C" fn rofd_document_get_page(
    document: *const rofd_document_t,
    page_index: usize,
    page: *mut *mut rofd_page_t,
    error: *mut *mut rofd_error_t,
) -> rofd_status_t {
    // SAFETY: The shared boundary validates and transactionally writes the caller output slots.
    unsafe {
        boundary(error, HandleOutput::required(page), || {
            let inner = document_ref(document)?
                .inner
                .page(page_index)
                .map_err(FfiError::from)?;
            Ok(Box::new(PageHandle { inner }))
        })
    }
}

/// Frees an owned document handle. A null handle is a no-op.
///
/// # Safety
///
/// A non-null handle must be live, owned by the caller, not previously freed,
/// and not used concurrently.
#[no_mangle]
pub unsafe extern "C" fn rofd_document_free(document: *mut rofd_document_t) {
    let _ = catch_unwind(AssertUnwindSafe(|| {
        if document.is_null() {
            return;
        }
        // SAFETY: The caller transfers the unique allocation to its sealed token mapping;
        // drop_raw_handle reconstructs its Box exactly once.
        unsafe { drop_raw_handle::<rofd_document_t>(document) };
    }));
}

/// Returns a page's zero-based document index.
///
/// # Safety
///
/// A non-null `page` must be a live page handle returned by this library. Its
/// storage must remain valid, must not be concurrently freed or modified, and
/// must not overlap the writable, aligned `page_index` or `error` output slots.
/// Non-null output slots must also be mutually disjoint and valid for the call.
#[no_mangle]
pub unsafe extern "C" fn rofd_page_get_index(
    page: *const rofd_page_t,
    page_index: *mut usize,
    error: *mut *mut rofd_error_t,
) -> rofd_status_t {
    // SAFETY: The shared boundary validates and transactionally writes the caller output slots.
    unsafe {
        boundary(error, ScalarOutput::required(page_index), || {
            Ok(page_ref(page)?.inner.index())
        })
    }
}

/// Returns a page's effective physical rectangle in millimetres.
///
/// # Safety
///
/// A non-null `page` must be a live page handle returned by this library. Its
/// storage must remain valid, must not be concurrently freed or modified, and
/// must not overlap the writable, aligned `page_size` or `error` output slots.
/// Non-null output slots must also be mutually disjoint and valid for the call.
#[no_mangle]
pub unsafe extern "C" fn rofd_page_get_size_mm(
    page: *const rofd_page_t,
    page_size: *mut rofd_rect_t,
    error: *mut *mut rofd_error_t,
) -> rofd_status_t {
    // SAFETY: The shared boundary validates and transactionally writes the caller output slots.
    unsafe {
        boundary(error, ScalarOutput::required(page_size), || {
            let size = page_ref(page)?.inner.size();
            Ok(rofd_rect_t {
                x_mm: size.x,
                y_mm: size.y,
                width_mm: size.width,
                height_mm: size.height,
            })
        })
    }
}

/// Frees an owned page handle. A null handle is a no-op.
///
/// # Safety
///
/// A non-null handle must be live, owned by the caller, not previously freed,
/// and not used concurrently.
#[no_mangle]
pub unsafe extern "C" fn rofd_page_free(page: *mut rofd_page_t) {
    let _ = catch_unwind(AssertUnwindSafe(|| {
        if page.is_null() {
            return;
        }
        // SAFETY: The caller transfers the unique allocation to its sealed token mapping;
        // drop_raw_handle reconstructs its Box exactly once.
        unsafe { drop_raw_handle::<rofd_page_t>(page) };
    }));
}
