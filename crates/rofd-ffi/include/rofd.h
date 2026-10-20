#ifndef ROFD_H
#define ROFD_H

#include <cairo.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/**
 * @file rofd.h
 * @brief Stable C ABI for the rofd OFD reader and renderer.
 *
 * Unless documented otherwise, every non-NULL handle returned through an
 * output parameter is owned by the caller and must be released with its
 * matching rofd_*_free function. All free functions are safe to call with
 * NULL. A page retains the document data it needs, so its document may be
 * freed before the page.
 *
 * Every non-NULL input C string must point to the first byte of a valid readable
 * character array object containing UTF-8 bytes. The terminating NUL byte must
 * occur within that same object; its readable extent and byte length must be
 * representable by ptrdiff_t. The complete sequence must remain valid for the
 * entire call. Individual functions specify whether NULL or an empty string is
 * allowed. The string returned by
 * rofd_library_version() has static storage and must not be freed. A
 * diagnostic message is borrowed from its report and remains valid only
 * until that report is freed. A cairo_t passed for rendering is borrowed;
 * ownership remains with the caller.
 *
 * Semantic page queries return caller-owned opaque result handles. Borrowed
 * string data remains valid until its string or text-selection owner is freed.
 * A semantic result remains valid after the source page or document is freed.
 * Every input string and input record must remain readable for the complete
 * call and must be disjoint from every ordinary output and error output slot.
 *
 * Every options record begins with struct_size. Call the matching initializer
 * with the writable capacity of the record before changing fields. A NULL
 * pointer, or a capacity smaller than the oldest supported version boundary,
 * is a no-op. Otherwise the initializer clears and initializes the highest
 * complete supported version that fits, leaves later bytes unchanged, and sets
 * struct_size to that selected version boundary rather than caller capacity.
 *
 * Options records evolve only by appending fields. Every published version
 * boundary, including its tail padding, is permanent. New fields must start at
 * or after the previous boundary and must never reuse an older version's tail
 * padding. Thus a new library initializes the complete older prefix that an old
 * caller can hold, while an old library preserves a new caller's unknown tail.
 * A non-NULL options pointer must be correctly aligned for its record type and
 * designate a valid object whose struct_size field is initialized and readable.
 * When struct_size declares the v1 boundary or a larger supported boundary, the
 * complete v1 prefix must be initialized and readable; every additional declared
 * supported prefix must likewise be initialized and readable for the call.
 *
 * Read-only calls on live handles may run concurrently. Callers must ensure
 * that no handle is freed while another call is using it. Cairo context
 * access and synchronization remain the caller's responsibility under
 * Cairo's threading rules.
 *
 * All non-NULL output parameter locations supplied to one call must occupy
 * distinct, non-overlapping storage. Before writing any output, the library
 * checks address ranges for representable arithmetic, alignment, and overlap.
 * Detectable violations fail with ROFD_STATUS_INVALID_ARGUMENT. If ordinary
 * outputs overlap while error is a separate valid slot, ordinary outputs stay
 * untouched and error receives the failure. If error overlaps an ordinary
 * output, every output stays untouched and no error handle is published.
 * Readable input regions and live handle storage must not overlap any output
 * slot, including error. Input strings, options records, and handle storage
 * must remain valid for the entire call. They must not be concurrently
 * modified or freed.
 */

#define ROFD_ABI_VERSION 1u

typedef uint32_t rofd_status_t;

#define ROFD_STATUS_OK 0u
#define ROFD_STATUS_INVALID_ARGUMENT 1u
#define ROFD_STATUS_IO 2u
#define ROFD_STATUS_INVALID_DOCUMENT 3u
#define ROFD_STATUS_UNSUPPORTED 4u
#define ROFD_STATUS_LIMIT_EXCEEDED 5u
#define ROFD_STATUS_PAGE_OUT_OF_RANGE 6u
#define ROFD_STATUS_RENDER_ERROR 7u
#define ROFD_STATUS_OUT_OF_MEMORY 8u
#define ROFD_STATUS_INTERNAL 255u

#define ROFD_STRICTNESS_LENIENT 0u
#define ROFD_STRICTNESS_STRICT 1u

#define ROFD_WARNING_UNKNOWN 0u
#define ROFD_WARNING_PAGE_AREA_FALLBACK 1u
#define ROFD_WARNING_DOCUMENT_PAGE_AREA_MISSING 2u
#define ROFD_WARNING_SIGNATURE_SKIPPED 3u
#define ROFD_WARNING_UNKNOWN_GRAPHIC_UNIT_SKIPPED 4u
#define ROFD_WARNING_ANNOTATION_SKIPPED 5u
#define ROFD_WARNING_HISTORICAL_DOC_BODY_SKIPPED 6u
#define ROFD_WARNING_NAVIGATION_INVALID 7u
#define ROFD_WARNING_NAVIGATION_UNSUPPORTED 8u
#define ROFD_WARNING_NAVIGATION_COMPATIBILITY 9u

#define ROFD_NO_INDEX SIZE_MAX
#define ROFD_ACTION_UNKNOWN 0u
#define ROFD_ACTION_GOTO 1u
#define ROFD_ACTION_URI 2u
#define ROFD_ACTION_ATTACHMENT 3u
#define ROFD_ACTION_EVENT_UNKNOWN 0u
#define ROFD_ACTION_EVENT_DOCUMENT_OPEN 1u
#define ROFD_ACTION_EVENT_PAGE_OPEN 2u
#define ROFD_ACTION_EVENT_CLICK 3u
#define ROFD_ACTION_NEW_WINDOW (1u << 0)
#define ROFD_DESTINATION_UNKNOWN 0u
#define ROFD_DESTINATION_XYZ 1u
#define ROFD_DESTINATION_FIT 2u
#define ROFD_DESTINATION_FIT_H 3u
#define ROFD_DESTINATION_FIT_V 4u
#define ROFD_DESTINATION_FIT_R 5u
#define ROFD_DESTINATION_HAS_PAGE_INDEX (1u << 0)
#define ROFD_DESTINATION_HAS_PAGE_ID (1u << 1)
#define ROFD_DESTINATION_HAS_LEFT (1u << 2)
#define ROFD_DESTINATION_HAS_TOP (1u << 3)
#define ROFD_DESTINATION_HAS_RIGHT (1u << 4)
#define ROFD_DESTINATION_HAS_BOTTOM (1u << 5)
#define ROFD_DESTINATION_HAS_ZOOM (1u << 6)

#define ROFD_IMAGE_INTERPOLATION_NEAREST 0u
#define ROFD_IMAGE_INTERPOLATION_BILINEAR 1u

#define ROFD_FIND_CASE_SENSITIVE (1u << 0)
#define ROFD_FIND_WHOLE_WORDS (1u << 1)

#define ROFD_TEXT_CHAR_SYNTHESIZED_SEPARATOR (1u << 0)
#define ROFD_TEXT_CHAR_CONSERVATIVE_GEOMETRY (1u << 1)

#define ROFD_SELECTION_GLYPH 0u
#define ROFD_SELECTION_WORD 1u
#define ROFD_SELECTION_LINE 2u

#define ROFD_DIAGNOSTIC_UNSUPPORTED_OBJECT 1u
#define ROFD_DIAGNOSTIC_FONT_FALLBACK 2u
#define ROFD_DIAGNOSTIC_MISSING_GLYPH 3u
#define ROFD_DIAGNOSTIC_IMAGE_SUBSTITUTION_UNSUPPORTED 4u
#define ROFD_DIAGNOSTIC_IMAGE_MASK_UNSUPPORTED 5u
#define ROFD_DIAGNOSTIC_IMAGE_BORDER_UNSUPPORTED 6u
#define ROFD_DIAGNOSTIC_SINGULAR_TRANSFORM 7u

typedef struct rofd_document rofd_document_t;
typedef struct rofd_metadata rofd_metadata_t;
typedef struct rofd_warning_list rofd_warning_list_t;
typedef struct rofd_outline rofd_outline_t;
typedef struct rofd_link_list rofd_link_list_t;
typedef struct rofd_page rofd_page_t;
typedef struct rofd_renderer rofd_renderer_t;
typedef struct rofd_render_report rofd_render_report_t;
typedef struct rofd_error rofd_error_t;
typedef struct rofd_string rofd_string_t;
typedef struct rofd_text_layout rofd_text_layout_t;
typedef struct rofd_text_search rofd_text_search_t;
typedef struct rofd_text_selection rofd_text_selection_t;

typedef struct rofd_load_options {
    uint32_t struct_size;
    uint32_t strictness;
} rofd_load_options_t;

typedef struct rofd_find_options {
    uint32_t struct_size;
    uint32_t flags;
    size_t max_results;
} rofd_find_options_t;

/**
 * Renderer construction options.
 *
 * NULL options, or fallback_families == NULL and fallback_family_count == 0,
 * select built-in default families. Non-NULL fallback_families with a zero
 * count explicitly disables fallback. A count greater than zero requires a
 * pointer correctly aligned for const char * that designates the first element
 * of a single valid, initialized, readable array object containing at least
 * fallback_family_count elements. Its total extent must be representable by
 * ptrdiff_t. Every element must be non-NULL and satisfy the input C string
 * contract above.
 */
typedef struct rofd_renderer_options {
    uint32_t struct_size;
    const char *const *fallback_families;
    size_t fallback_family_count;
    uint64_t max_font_bytes;
    uint64_t image_cache_bytes;
} rofd_renderer_options_t;

typedef struct rofd_render_options {
    uint32_t struct_size;
    double dpi;
    double scale;
    uint32_t rotation_degrees;
    uint32_t background_rgba;
    uint32_t has_clip;
    double clip_x_mm;
    double clip_y_mm;
    double clip_width_mm;
    double clip_height_mm;
    uint32_t image_interpolation;
    uint64_t max_raster_bytes;
} rofd_render_options_t;

/** Rectangle in the final rotated/scaled full-page pixel canvas. Initialize
 * with rofd_pixel_rect_init, then set nonnegative x/y and positive width/height.
 * The full rectangle must lie inside the canvas; it is never silently clamped. */
typedef struct rofd_pixel_rect {
    uint32_t struct_size;
    int32_t x;
    int32_t y;
    int32_t width;
    int32_t height;
} rofd_pixel_rect_t;

/** Initialize the supported prefix to an empty rectangle; set dimensions before
 * rendering. NULL and undersized capacities are no-ops. Unknown tails are
 * preserved; struct_size receives the selected supported version size. Non-NULL
 * storage with sufficient capacity must be aligned and writable for that prefix. */
void rofd_pixel_rect_init(rofd_pixel_rect_t *rect, size_t capacity);

typedef struct rofd_rect {
    double x_mm;
    double y_mm;
    double width_mm;
    double height_mm;
} rofd_rect_t;

typedef struct rofd_text_char {
    uint32_t struct_size;
    size_t utf8_offset;
    size_t utf8_length;
    rofd_rect_t rect_mm;
    uint32_t flags;
    uint64_t object_id;
} rofd_text_char_t;

typedef struct rofd_text_match {
    uint32_t struct_size;
    size_t utf8_offset;
    size_t utf8_length;
    rofd_rect_t rect_mm;
} rofd_text_match_t;

typedef struct rofd_render_diagnostic {
    uint32_t struct_size;
    uint32_t kind;
    uint64_t object_id;
    const char *message;
} rofd_render_diagnostic_t;

/** One parse warning. Set struct_size before passing this output record.
 * The path and message are borrowed NUL-terminated UTF-8 strings owned by the
 * warning list; they remain valid until rofd_warning_list_free. Embedded NULs
 * are represented by the two visible characters '\\' and '0'. */
typedef struct rofd_warning {
    uint32_t struct_size;
    uint32_t code; /* ROFD_WARNING_*; future unknown categories map to UNKNOWN. */
    const char *path;
    const char *message;
} rofd_warning_t;

/** Preorder outline node. All indices are zero-based; absent relations use
 * ROFD_NO_INDEX. title is borrowed until the outline snapshot is freed. */
typedef struct rofd_outline_node {
    uint32_t struct_size;
    uint32_t expanded; /**< Exactly 0 or 1; omitted source values default to 1. */
    const char *title;
    size_t parent;
    size_t first_child;
    size_t next_sibling;
    size_t action_count;
} rofd_outline_node_t;

/** Inert action. All strings are borrowed from the queried snapshot. An
 * irrelevant or missing optional payload is NULL, distinct from an empty
 * string. URI and URI base are preserved, not resolved or executed. GotoA
 * uses attachment_id, an identifier string (not necessarily a number). */
typedef struct rofd_action {
    uint32_t struct_size;
    uint32_t kind; /**< ROFD_ACTION_UNKNOWN/GOTO/URI/ATTACHMENT. */
    uint32_t event; /**< ROFD_ACTION_EVENT_*; inspect before choosing to act. */
    uint32_t flags; /**< ROFD_ACTION_NEW_WINDOW, default true for GotoA. */
    const char *type_name; /**< Source name, retained for unknown actions. */
    const char *event_name; /**< Source event, retained for unknown events. */
    const char *uri;
    const char *uri_base;
    const char *attachment_id;
    const char *bookmark;
} rofd_action_t;

/** Destination in absolute physical-page millimetres, independent of render
 * DPI, scale or rotation. Only HAS_* fields are meaningful. On successful
 * unresolved queries page_index is ROFD_NO_INDEX, never an invented page zero.
 * Absent numeric coordinates are zero, with the corresponding HAS_* bit clear.
 * The caller applies destination-mode defaults; explicit zero zoom means keep
 * current zoom. mode_name is borrowed from the queried snapshot. */
typedef struct rofd_destination {
    uint32_t struct_size;
    uint32_t kind; /**< ROFD_DESTINATION_UNKNOWN/XYZ/FIT/FIT_H/FIT_V/FIT_R. */
    uint32_t flags; /**< ROFD_DESTINATION_HAS_* presence bits. */
    size_t page_index;
    uint64_t page_id;
    const char *mode_name;
    double left_mm;
    double top_mm;
    double right_mm;
    double bottom_mm;
    double zoom;
} rofd_destination_t;

uint32_t rofd_abi_version(void);
const char *rofd_library_version(void);

void rofd_load_options_init(rofd_load_options_t *options, size_t options_size);
void rofd_find_options_init(rofd_find_options_t *options, size_t options_size);
void rofd_renderer_options_init(rofd_renderer_options_t *options,
                                size_t options_size);
void rofd_render_options_init(rofd_render_options_t *options,
                              size_t options_size);

/**
 * Open an OFD document. A NULL path is a defined input that returns
 * ROFD_STATUS_INVALID_ARGUMENT under the normal output transaction. An empty
 * path also returns ROFD_STATUS_INVALID_ARGUMENT. A non-NULL path must satisfy
 * the input-string contract above; options may be NULL or must satisfy the
 * options-record contract. Both readable inputs must be disjoint from document
 * and error output storage.
 */
rofd_status_t rofd_document_open(const char *path,
                                 const rofd_load_options_t *options,
                                 rofd_document_t **document,
                                 rofd_error_t **error);
/** Borrow a live document without consuming it; its storage must be disjoint
 * from page_count and error and must not be freed during the call. */
rofd_status_t rofd_document_get_page_count(const rofd_document_t *document,
                                           size_t *page_count,
                                           rofd_error_t **error);
/** Borrow a live document and copy one page's physical box in millimetres. This
 * metadata-only query reads the page area without parsing or caching page content,
 * so it performs no content validation, collects no content warnings, and also
 * succeeds when that page's content cannot be loaded. The value matches
 * rofd_page_get_size_mm for the same index. document, page_size_mm and error follow
 * the rules above; an index outside [0, page_count) returns
 * ROFD_STATUS_PAGE_OUT_OF_RANGE. */
rofd_status_t rofd_document_get_page_size_mm(const rofd_document_t *document,
                                             size_t page_index,
                                             rofd_rect_t *page_size_mm,
                                             rofd_error_t **error);
/** Borrow a live document and return an independently owned page. The document
 * storage must be disjoint from page and error and remain live for the call. */
rofd_status_t rofd_document_get_page(const rofd_document_t *document,
                                     size_t page_index,
                                     rofd_page_t **page,
                                     rofd_error_t **error);
void rofd_document_free(rofd_document_t *document);

/** Copies document metadata into an independently owned immutable snapshot.
 * document and metadata are required; error is optional. The snapshot survives
 * document free and must be released with rofd_metadata_free.
 *
 * All non-NULL output slots must be aligned, writable, mutually disjoint and
 * disjoint from source handle storage. Invalid input/output address layouts
 * leave every output untouched. Overlaps leave ordinary outputs untouched but
 * may publish a failure to an independent valid error slot. Other failures
 * null the metadata output. These transaction rules also apply to metadata
 * keyword and warning-list queries below, zeroing counts or nulling string/
 * handle outputs on ordinary failures. NULL required inputs are invalid.
 * Inputs must remain live and immutable for each call, and outputs must not be
 * accessed concurrently. Snapshots support concurrent read-only queries. */
rofd_status_t rofd_document_get_metadata(const rofd_document_t *document,
                                        rofd_metadata_t **metadata,
                                        rofd_error_t **error);

/** Borrow metadata strings. NULL handles or missing fields return NULL;
 * explicitly empty fields return a non-NULL empty string. Dates retain their
 * producer representation. Every returned UTF-8 string remains valid until
 * rofd_metadata_free; do not modify or free it. Embedded NULs are represented
 * by the two visible characters '\\' and '0'. A non-NULL handle must be a live
 * metadata snapshot and must not be freed concurrently. */
const char *rofd_metadata_get_document_id(const rofd_metadata_t *metadata);
const char *rofd_metadata_get_title(const rofd_metadata_t *metadata);
const char *rofd_metadata_get_author(const rofd_metadata_t *metadata);
const char *rofd_metadata_get_subject(const rofd_metadata_t *metadata);
const char *rofd_metadata_get_abstract(const rofd_metadata_t *metadata);
const char *rofd_metadata_get_creator(const rofd_metadata_t *metadata);
const char *rofd_metadata_get_creator_version(const rofd_metadata_t *metadata);
const char *rofd_metadata_get_creation_date(const rofd_metadata_t *metadata);
const char *rofd_metadata_get_modification_date(const rofd_metadata_t *metadata);

/** Counts keywords in declaration order, including repeated and empty entries.
 * metadata and count are required; error is optional. */
rofd_status_t rofd_metadata_get_keyword_count(const rofd_metadata_t *metadata,
                                             size_t *count,
                                             rofd_error_t **error);
/** Borrows a keyword with the same string lifetime as the metadata readers.
 * metadata and keyword are required; error is optional. A zero-based index
 * outside the keyword list returns ROFD_STATUS_PAGE_OUT_OF_RANGE. */
rofd_status_t rofd_metadata_get_keyword(const rofd_metadata_t *metadata,
                                       size_t index,
                                       const char **keyword,
                                       rofd_error_t **error);
/** Frees metadata and invalidates all strings borrowed from it. NULL is a no-op.
 * A non-NULL handle must be uniquely owned, live and freed exactly once, without
 * concurrent readers or subsequent uses of its borrowed strings. */
void rofd_metadata_free(rofd_metadata_t *metadata);

/** Captures only parse warnings already collected by the document. This call
 * does not load pages or force any lazy parsing. Later queries may collect new
 * warnings; existing snapshots remain unchanged. The independently owned
 * snapshot survives document/page free. document and warnings are required;
 * error is optional. The metadata transaction and pointer rules above apply. */
rofd_status_t rofd_document_get_warnings(const rofd_document_t *document,
                                        rofd_warning_list_t **warnings,
                                        rofd_error_t **error);
/** warnings and count are required; error is optional. */
rofd_status_t rofd_warning_list_get_count(const rofd_warning_list_t *warnings,
                                         size_t *count,
                                         rofd_error_t **error);
/** Borrows a zero-based warning. Invalid indices return
 * ROFD_STATUS_PAGE_OUT_OF_RANGE. warnings and warning are required; error is
 * optional. The caller must initialize warning->struct_size to at least
 * sizeof(rofd_warning_t). The non-NULL record must be aligned and readable for
 * struct_size, and a supported prefix must be writable. The transaction clears
 * the complete v1 prefix including padding, preserves the caller's struct_size,
 * and leaves unknown tail bytes unchanged. Ordinary failures zero the valid
 * prefix except struct_size. Undersized records stay untouched. Invalid address
 * layouts and overlaps follow the metadata transaction rules above. Borrowed
 * path/message pointers remain valid only until rofd_warning_list_free. */
rofd_status_t rofd_warning_list_get_warning(const rofd_warning_list_t *warnings,
                                           size_t index,
                                           rofd_warning_t *warning,
                                           rofd_error_t **error);
/** Frees a snapshot and invalidates borrowed strings. NULL is a no-op.
 * A non-NULL handle must be uniquely owned, live and freed exactly once, without
 * concurrent readers or subsequent uses of its borrowed strings. */
void rofd_warning_list_free(rofd_warning_list_t *warnings);

/** Returns an independently owned immutable preorder outline snapshot, even
 * when empty. Navigation is parsed lazily and may add parse warnings; it never
 * executes actions. Nodes, actions and strings remain valid after document
 * free. document and outline are required; error is optional.
 *
 * The same pointer/transaction rules apply to all outline queries below:
 * non-NULL inputs must remain live and immutable; output slots must be aligned,
 * writable, mutually disjoint and disjoint from input handle storage, without
 * conflicting concurrent access. Malformed address layouts leave all outputs
 * untouched; overlaps preserve ordinary outputs but may publish an independent
 * error. Other failures null handle outputs or zero counts. NULL required
 * arguments return INVALID_ARGUMENT; invalid node/action indices return
 * PAGE_OUT_OF_RANGE. Snapshots permit concurrent read-only access. */
rofd_status_t rofd_document_get_outline(const rofd_document_t *document,
                                       rofd_outline_t **outline,
                                       rofd_error_t **error);
/** outline and count are required; error is optional. */
rofd_status_t rofd_outline_get_count(const rofd_outline_t *outline,
                                    size_t *count, rofd_error_t **error);
/** Borrows a node. All navigation output records require initialized
 * struct_size >= sizeof(the v1 record). The first uint32_t must be readable;
 * a supported prefix must be writable. Valid prefixes are cleared including
 * padding on ordinary failure, preserving captured struct_size and unknown
 * tails. Undersized records remain untouched. These record rules apply to node,
 * action and destination queries. Title/other borrowed UTF-8 strings must not
 * be modified or freed and expire on outline free; embedded NUL is represented
 * by a visible backslash followed by zero. outline and node are required. */
rofd_status_t rofd_outline_get_node(const rofd_outline_t *outline,
                                   size_t node_index, rofd_outline_node_t *node,
                                   rofd_error_t **error);
/** Borrows an action by its zero-based index in the node's declaration order.
 * Unknown action/event names remain inspectable. No action is executed.
 * outline and action are required; error is optional. */
rofd_status_t rofd_outline_get_action(const rofd_outline_t *outline,
                                     size_t node_index, size_t action_index,
                                     rofd_action_t *action, rofd_error_t **error);
/** Non-Goto actions return UNSUPPORTED. Unresolved Goto actions succeed without
 * HAS_PAGE_INDEX; a missing bookmark may have no destination fields at all.
 * Check presence bits before navigation; never interpret a missing target as
 * page zero. outline and destination are required; error is optional. */
rofd_status_t rofd_outline_get_action_destination(
    const rofd_outline_t *outline, size_t node_index, size_t action_index,
    rofd_destination_t *destination, rofd_error_t **error);
/** Frees a uniquely owned snapshot and invalidates all borrowed strings. NULL
 * is a no-op. A non-NULL handle must be freed exactly once, without concurrent
 * readers or later uses of borrowed data. */
void rofd_outline_free(rofd_outline_t *outline);

/** Copies page links into an independently owned immutable snapshot. Empty
 * pages return a non-NULL empty list. Each entry represents one source Action
 * with one or more conservative axis-aligned physical-page mm rectangles.
 * Page-root actions precede effective content, object actions precede children,
 * and visible annotation actions follow page content. Inspect event metadata;
 * mouse hit testing normally uses CLICK actions. No action is executed, even
 * file URIs or attachments. First query can add parse warnings. Invalid explicit
 * Region fails in strict mode or skips that link with a warning in lenient mode,
 * never falling back to an invented whole-page region.
 *
 * Page and links are required. A non-NULL page must be live and immutable;
 * links/optional error slots must be aligned, writable, mutually disjoint and
 * disjoint from page storage, without conflicting concurrent accesses.
 * Malformed address layouts leave all outputs untouched. Overlaps leave ordinary
 * outputs untouched but may publish an independent error. Other failures null
 * a valid links output. These preflight rules apply to every link-list query.
 * The snapshot survives page/document free; release it with link_list_free. */
rofd_status_t rofd_page_get_links(const rofd_page_t *page,
                                  rofd_link_list_t **links, rofd_error_t **error);

/** Link-list queries borrow a live immutable list without consuming it. Every
 * list and output is required except error; all non-NULL outputs must be aligned,
 * writable, mutually disjoint and disjoint from list storage. Storage must stay
 * live without conflicting concurrent access. All indices are zero-based;
 * invalid link/region/action indices return PAGE_OUT_OF_RANGE. Ordinary failures
 * zero count/rectangle outputs. No borrowed string may outlive the list. */
rofd_status_t rofd_link_list_get_count(const rofd_link_list_t *links,
                                       size_t *count, rofd_error_t **error);
rofd_status_t rofd_link_list_get_region_count(const rofd_link_list_t *links,
    size_t link_index, size_t *count, rofd_error_t **error);
/** Region coordinates already include applicable owner/ancestor transforms,
 * independent of render DPI/rotation. Fallback Boundary is in parent coordinates
 * and does not receive the object's CTM twice. */
rofd_status_t rofd_link_list_get_region(const rofd_link_list_t *links,
    size_t link_index, size_t region_index, rofd_rect_t *region_mm, rofd_error_t **error);
rofd_status_t rofd_link_list_get_action_count(const rofd_link_list_t *links,
    size_t link_index, size_t *count, rofd_error_t **error);
/** Initialize struct_size for action/destination outputs; a layout-valid pointer
 * must be readable for its initialized size and writable for a supported prefix.
 * Ordinary failures clear the known prefix including padding, preserve the
 * declared struct_size and leave unknown tails untouched. Strings are borrowed
 * read-only until link_list_free, including unknown type and event names. */
rofd_status_t rofd_link_list_get_action(const rofd_link_list_t *links,
    size_t link_index, size_t action_index, rofd_action_t *action, rofd_error_t **error);
/** Non-Goto actions return UNSUPPORTED. Unresolved Goto succeeds with NO_INDEX
 * and without HAS_PAGE_INDEX. Inspect HAS_* flags before using coordinates or
 * page_index; zero zoom means retain current zoom. Other semantics match
 * rofd_outline_get_action_destination. */
rofd_status_t rofd_link_list_get_action_destination(const rofd_link_list_t *links,
    size_t link_index, size_t action_index, rofd_destination_t *destination,
    rofd_error_t **error);
/** NULL is a no-op; a non-NULL list must be uniquely owned, live, and freed
 * exactly once without concurrent readers or subsequent borrowed-string uses. */
void rofd_link_list_free(rofd_link_list_t *links);

/** Borrow a live page without consuming it; its storage must be disjoint from
 * page_index and error and must not be freed during the call. */
rofd_status_t rofd_page_get_index(const rofd_page_t *page,
                                  size_t *page_index,
                                  rofd_error_t **error);
/** Borrow a live page without consuming it; its storage must be disjoint from
 * page_size and error and must not be freed during the call. */
rofd_status_t rofd_page_get_size_mm(const rofd_page_t *page,
                                    rofd_rect_t *page_size,
                                    rofd_error_t **error);
void rofd_page_free(rofd_page_t *page);

/** Query canonical UTF-8 page text, including synthesized object separators.
 * The returned string is independently owned and survives freeing page and
 * document. Free it with rofd_string_free. Layout byte offsets index these
 * exact bytes. page and text are required; error is optional. Every non-NULL
 * output must be aligned, writable, mutually disjoint, and disjoint from live
 * page storage. No conflicting concurrent access is permitted. */
rofd_status_t rofd_page_get_text(const rofd_page_t *page,
                                rofd_string_t **text, rofd_error_t **error);
/** Return owned source glyphs with positive-area intersection, in canonical
 * order, omitting synthesized separators. area_mm is required and must point
 * to a complete initialized, readable, aligned rofd_rect_t in physical-page
 * millimetres. Nonfinite coordinates or edges and negative dimensions return
 * ROFD_STATUS_INVALID_ARGUMENT. Negative positions are valid; empty areas
 * produce empty strings. The entire rectangle and live page storage must be
 * disjoint from text and error. Ownership and output rules match get_text. */
rofd_status_t rofd_page_get_text_for_area(const rofd_page_t *page,
                                         const rofd_rect_t *area_mm,
                                         rofd_string_t **text, rofd_error_t **error);
/** Borrow immutable NUL-terminated UTF-8 bytes until rofd_string_free; NULL
 * returns NULL. A non-NULL string handle must remain live during every read. */
const char *rofd_string_get_data(const rofd_string_t *text);
/** UTF-8 byte length excluding the terminating NUL; NULL returns zero. */
size_t rofd_string_get_length(const rofd_string_t *text);
/** Free once after all reads finish; NULL is a no-op. */
void rofd_string_free(rofd_string_t *text);

/** Return an independently owned immutable snapshot, one entry per Unicode
 * scalar in canonical page text, including synthesized separators. Free with
 * rofd_text_layout_free. The snapshot survives page and document destruction.
 * A scalar whose run declares no explicit delta is placed by its font's advance,
 * and its box spans that font's painted extents around the baseline, both resolved
 * through the default font stack (the one a renderer created with NULL options
 * uses); a renderer configured with other fallback families may draw slightly
 * different glyphs. Scalars that cannot be resolved fall back to a conservative
 * font-size box.
 * page and layout are required; error is optional. All outputs must be aligned,
 * writable, mutually disjoint and disjoint from live page storage. */
rofd_status_t rofd_page_get_text_layout(const rofd_page_t *page,
                                       rofd_text_layout_t **layout, rofd_error_t **error);
/** Return the scalar count. layout and count are required. Live layout storage
 * must be disjoint from aligned writable count and optional error outputs;
 * outputs must be mutually disjoint. Failure initializes valid count to zero. */
rofd_status_t rofd_text_layout_get_count(const rofd_text_layout_t *layout,
                                        size_t *count, rofd_error_t **error);
/** Copy one character by index; out-of-range returns ROFD_STATUS_PAGE_OUT_OF_RANGE.
 * Before calling, set character->struct_size to sizeof(rofd_text_char_t) or a
 * larger caller record size. A non-NULL record must have an initialized readable
 * size field and, for supported sizes, an aligned writable complete v1 prefix.
 * The transaction clears the permanent v1 prefix including padding, preserves
 * the caller-declared struct_size and unknown tail, and publishes fields only
 * on success. NULL and undersized records remain untouched; a separate error
 * receives ROFD_STATUS_INVALID_ARGUMENT. Offsets and lengths are UTF-8 bytes in
 * rofd_page_get_text's canonical string, not character indices or area-text
 * offsets. Rectangles use physical-page millimetres; missing geometry is a zero
 * rectangle. Synthesized separators have object_id zero and SYNTHESIZED_SEPARATOR;
 * approximate geometry sets CONSERVATIVE_GEOMETRY (also set on current separators).
 * Live layout storage and every output, including optional error, must be
 * mutually disjoint and have no conflicting concurrent access during the call. */
rofd_status_t rofd_text_layout_get_char(const rofd_text_layout_t *layout,
                                       size_t index, rofd_text_char_t *character,
                                       rofd_error_t **error);
/** Free once after all reads finish; NULL is a no-op. Copied records remain valid. */
void rofd_text_layout_free(rofd_text_layout_t *layout);

/** Find canonical page text with case-insensitive substring defaults. query
 * must be non-NULL, valid nonempty UTF-8. The independently owned result
 * survives page/document destruction and is freed with rofd_text_search_free.
 * Input and output storage follow the global disjointness contract. */
rofd_status_t rofd_page_find_text(const rofd_page_t *page, const char *query,
                                  rofd_text_search_t **search,
                                  rofd_error_t **error);
/** Find canonical page text with versioned options. NULL options select
 * defaults. Unknown flags and zero max_results are invalid; larger record tails
 * are ignored and result count is bounded by the page resource policy. */
rofd_status_t rofd_page_find_text_with_options(
    const rofd_page_t *page, const char *query,
    const rofd_find_options_t *options, rofd_text_search_t **search,
    rofd_error_t **error);
/** Return the number of owned matches. search and count are required; failure
 * initializes a valid count output to zero. */
rofd_status_t rofd_text_search_get_count(const rofd_text_search_t *search,
                                         size_t *count,
                                         rofd_error_t **error);
/** Copy one match by index. Set match->struct_size to sizeof(rofd_text_match_t)
 * or a larger caller size. The permanent v1 prefix is cleared transactionally,
 * struct_size and unknown tail are preserved, and an absent match rectangle is
 * returned as zeros. Out-of-range returns ROFD_STATUS_PAGE_OUT_OF_RANGE. */
rofd_status_t rofd_text_search_get_match(const rofd_text_search_t *search,
                                         size_t index,
                                         rofd_text_match_t *match,
                                         rofd_error_t **error);
/** Free an owned search result once after all reads; NULL is a no-op. */
void rofd_text_search_free(rofd_text_search_t *search);

/** Select text intersecting selection_mm using a ROFD_SELECTION_* style. The
 * rectangle is in physical-page millimetres and follows the same finite,
 * non-negative-dimension rules as area extraction. The independently owned
 * result survives page/document destruction and must be freed with
 * rofd_text_selection_free. page, selection_mm and selection are required. */
rofd_status_t rofd_page_get_selected_text(
    const rofd_page_t *page, uint32_t style,
    const rofd_rect_t *selection_mm, rofd_text_selection_t **selection,
    rofd_error_t **error);
/** Borrow immutable NUL-terminated selected UTF-8 text until selection free;
 * NULL returns NULL. */
const char *rofd_text_selection_get_text(
    const rofd_text_selection_t *selection);
/** Selected UTF-8 byte length excluding the terminating NUL; NULL is zero. */
size_t rofd_text_selection_get_text_length(
    const rofd_text_selection_t *selection);
/** Return the number of ordered selected page regions. selection and count are
 * required; failure initializes a valid count output to zero. */
rofd_status_t rofd_text_selection_get_region_count(
    const rofd_text_selection_t *selection, size_t *count,
    rofd_error_t **error);
/** Copy one region in physical-page millimetres. Out-of-range returns
 * ROFD_STATUS_PAGE_OUT_OF_RANGE and initializes a valid output to zeros. */
rofd_status_t rofd_text_selection_get_region(
    const rofd_text_selection_t *selection, size_t index,
    rofd_rect_t *region_mm, rofd_error_t **error);
/** Free an owned text selection after all borrowed text reads; NULL is a no-op. */
void rofd_text_selection_free(rofd_text_selection_t *selection);

rofd_status_t rofd_renderer_new(const rofd_renderer_options_t *options,
                                rofd_renderer_t **renderer,
                                rofd_error_t **error);
rofd_status_t rofd_renderer_get_pixel_size(
    const rofd_renderer_t *renderer,
    const rofd_page_t *page,
    const rofd_render_options_t *options,
    int32_t *pixel_width,
    int32_t *pixel_height,
    rofd_error_t **error);
/** Compute the final full-page pixel canvas (ceil after DPI/scale/rotation).
 * Unlike get_pixel_size, this accepts positive dimensions up to INT32_MAX
 * without requiring a full-page Cairo surface or full-page raster budget.
 * Options are validated and NULL uses defaults. Renderer/page and both scalar
 * outputs are required. Input handles and options must be live, immutable and
 * disjoint from writable aligned outputs; outputs must be mutually disjoint.
 * Invalid input or output address layouts leave all outputs untouched. Overlaps preserve
 * ordinary outputs but may populate an independent valid error slot. Other
 * failures initialize valid scalar outputs to zero. */
rofd_status_t rofd_renderer_get_pixel_canvas_size(
    const rofd_renderer_t *renderer, const rofd_page_t *page,
    const rofd_render_options_t *options, int32_t *pixel_width,
    int32_t *pixel_height, rofd_error_t **error);
/**
 * Render a page into a borrowed Cairo context.
 *
 * The report output may be NULL when the caller wants to discard diagnostics.
 * Otherwise it is set to NULL on entry and receives one owned report on
 * success, even when the report is empty. cairo must be a non-NULL live
 * context whose target and referenced objects remain valid for the call. The
 * caller must obey Cairo's context/thread synchronization rules and prevent
 * concurrent context mutation. rofd takes and drops one temporary reference;
 * it does not consume the caller's reference. Renderer/page handle storage,
 * the readable options prefix, and the detectable Cairo address must not
 * overlap report or error output storage.
 */
rofd_status_t rofd_renderer_render_page_cairo(
    const rofd_renderer_t *renderer,
    const rofd_page_t *page,
    cairo_t *cairo,
    const rofd_render_options_t *options,
    rofd_render_report_t **report,
    rofd_error_t **error);
/** Render directly into a viewport-sized borrowed Cairo target. viewport is
 * required and its initialized struct_size must cover the supported prefix;
 * larger record tails are ignored. Pixel (viewport.x, viewport.y) in the final
 * full-page canvas maps to target (0, 0). Target dimensions must be at least
 * viewport.width/height; drawing stays within that rectangle. clip_mm remains
 * an absolute physical-page millimetre clip, independent of viewport origin.
 * Working masks/intermediates and their raster budget use viewport dimensions;
 * decoded source images still use the existing bounded decoder/cache contract.
 * Invalid rectangles return INVALID_ARGUMENT, without silent clipping.
 * Cairo borrowing, synchronization and optional report ownership follow
 * render_page_cairo. The viewport prefix must also remain live, aligned,
 * readable and disjoint from report/error outputs. Invalid input or output address layouts
 * leave outputs untouched. Overlaps preserve ordinary outputs but may populate
 * an independent valid error slot. Other failures null the report.
 * No input or output may be concurrently mutated during this call. */
rofd_status_t rofd_renderer_render_page_region_cairo(
    const rofd_renderer_t *renderer, const rofd_page_t *page, cairo_t *cairo,
    const rofd_render_options_t *options, const rofd_pixel_rect_t *viewport,
    rofd_render_report_t **report, rofd_error_t **error);
void rofd_renderer_free(rofd_renderer_t *renderer);

/** Borrow a live report and return its diagnostic count. report and
 * diagnostic_count are required; diagnostic_count must be writable and
 * aligned. error is optional. When error is non-NULL, it and diagnostic_count
 * must satisfy the global writable, aligned, and mutually disjoint output
 * contract. Report storage must be disjoint from every non-NULL output. */
rofd_status_t rofd_render_report_get_count(const rofd_render_report_t *report,
                                           size_t *diagnostic_count,
                                           rofd_error_t **error);
/**
 * Get one diagnostic from a render report.
 *
 * Before calling, set diagnostic->struct_size to
 * sizeof(rofd_render_diagnostic_t) or any larger caller record size.
 * A valid output has its complete v1 prefix, including padding, cleared before
 * lookup while preserving the caller's input struct_size and unknown tail.
 * Thus failure after record validation leaves kind/object_id zero and message
 * NULL. The returned message is borrowed until the report is freed. The report
 * handle storage must not overlap diagnostic or error output storage.
 */
rofd_status_t rofd_render_report_get_diagnostic(
    const rofd_render_report_t *report,
    size_t diagnostic_index,
    rofd_render_diagnostic_t *diagnostic,
    rofd_error_t **error);
void rofd_render_report_free(rofd_render_report_t *report);

rofd_status_t rofd_error_get_status(const rofd_error_t *error);
/**
 * Return a borrowed UTF-8 error message.
 *
 * The returned pointer must not be freed and remains valid only until error is
 * freed with rofd_error_free().
 */
const char *rofd_error_get_message(const rofd_error_t *error);
void rofd_error_free(rofd_error_t *error);

#ifdef __cplusplus
}
#endif

#endif
