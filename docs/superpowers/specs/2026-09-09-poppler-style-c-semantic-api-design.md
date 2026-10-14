# Poppler-style C Semantic API Design

## Goal

Extend rofd's stable C ABI with the semantic document and page queries needed by
document viewers. A C consumer familiar with Poppler's GLib API should recognize
the workflow without rofd taking a dependency on GLib or reproducing PDF-only
concepts. Rust implementation details remain free to use the structure that best
fits OFD, safety, and performance.

The first delivery covers text extraction, text layout, rectangular selection,
and text search. Metadata, outline, links, annotations, signatures, and image
mapping follow the same C API conventions and can land as independently tested
increments on this branch.

## Reference and compatibility boundary

The usage reference is the public `poppler-glib` C API, especially:

- `poppler_document_get_n_pages` and `poppler_document_get_page`;
- `poppler_page_get_text` and `poppler_page_get_text_for_area`;
- `poppler_page_get_selected_text` and the selection-style enum;
- `poppler_page_find_text_with_options`;
- `poppler_page_get_text_layout`;
- page mapping queries paired with dedicated free functions;
- document metadata getters and index iteration.

Similarity means the same document/page mental model, page-local query methods,
rectangular results in page coordinates, option flags, and explicit ownership.
It does not mean source or binary compatibility. rofd retains its `rofd_`
prefix, millimetre coordinate system, structured status/error returns, opaque
handles, output transactions, and no-GLib dependency.

The already published ABI remains valid. Existing names such as
`rofd_document_get_page_count` are not renamed merely to match Poppler. New APIs
are additive, and no existing structure boundary or symbol changes meaning.

## C consumer workflow

A normal reader performs semantic queries directly against an owned page:

```c
rofd_document_t *document = NULL;
rofd_page_t *page = NULL;
rofd_string_t *text = NULL;
rofd_text_search_t *matches = NULL;
rofd_error_t *error = NULL;

rofd_document_open(path, NULL, &document, &error);
rofd_document_get_page(document, 0, &page, &error);
rofd_page_get_text(page, &text, &error);
rofd_page_find_text_with_options(page, "invoice", NULL, &matches, &error);

printf("%.*s\n", (int)rofd_string_get_length(text),
       rofd_string_get_data(text));

rofd_text_search_free(matches);
rofd_string_free(text);
rofd_page_free(page);
rofd_document_free(document);
```

Like Poppler, the public query surface stays on the page object. rofd does not
expose a preparatory text-page handle merely to reflect its internal cache.
`PageHandle` may lazily retain an immutable semantic index so full-text,
selection, layout, and repeated search operations do not reparse the page.
Returned result handles own or share the data they expose and remain valid after
their source page and document handles are freed. Read-only calls may run
concurrently under the same lifetime rules as existing handles.

## Text model

The semantic representation consists of source characters, logical reading
order, and page-coordinate geometry. It is built from effective template/page
paint order and recursively visits text inside groups, composites, annotations,
and signature appearances where those objects are part of visible page content.

Each logical character records:

- its Unicode scalar and UTF-8 byte range in the flattened page text;
- a finite rectangle in millimetres in physical page coordinates;
- the source text object and run identity;
- separators inserted between runs, lines, or blocks without inventing visible
  glyph geometry.

The initial layout uses validated OFD text origins, `DeltaX`/`DeltaY`, font size,
object boundary, and accumulated transforms. It must be deterministic and must
not require Cairo, Qt, GLib, system font discovery, or font outline loading.
Where OFD data cannot yield an exact glyph box, the API returns a conservative
finite character box and records its precision in flags instead of pretending
font-derived accuracy.

A run that declares no delta for a scalar is positioned by its font's advance, so
the core builder accepts an optional `TextAdvanceProvider` that answers those
advances without pulling fonts into the core crate: `rofd-render` implements it
over the same font resolution the renderer positions text with, and `rofd-ffi`
loads documents with the default font stack so the ABI's rectangles land on the
glyphs a default renderer draws. Without a provider the builder keeps the
font-size footprint, and scalars it cannot resolve stay flagged as conservative.

Reading order is deterministic. It starts with effective paint/source order,
then groups compatible characters into lines using writing direction and
geometry. Stable source order breaks geometric ties. Whitespace from the OFD
source is preserved; separators synthesized for useful extraction are explicitly
classified so selection and layout lengths remain consistent.

## First C ABI increment

### Owned strings and page text

The flattened UTF-8 string is returned from a Poppler-like page-level function.
An opaque string handle avoids requiring C consumers to free Rust allocations
with a platform allocator:

```c
typedef struct rofd_string rofd_string_t;

rofd_status_t rofd_page_get_text(
    const rofd_page_t *page,
    rofd_string_t **text,
    rofd_error_t **error);

rofd_status_t rofd_page_get_text_for_area(
    const rofd_page_t *page,
    const rofd_rect_t *area_mm,
    rofd_string_t **text,
    rofd_error_t **error);

const char *rofd_string_get_data(const rofd_string_t *string);
size_t rofd_string_get_length(const rofd_string_t *string);
void rofd_string_free(rofd_string_t *string);
```

The data pointer stays valid until the string handle is freed. Embedded NUL is
represented in the byte length even though ordinary OFD text is expected not to
contain it. Like the existing error-message getter, the infallible borrowed
getters require a valid live handle; fallible page queries use status plus error
output.

### Layout

Layout uses a versioned record and indexed access instead of exposing a GLib
list or caller-visible allocation:

```c
typedef struct rofd_text_char {
    uint32_t struct_size;
    size_t utf8_offset;
    size_t utf8_length;
    rofd_rect_t rect_mm;
    uint32_t flags;
    uint64_t object_id;
} rofd_text_char_t;

typedef struct rofd_text_layout rofd_text_layout_t;

rofd_status_t rofd_page_get_text_layout(
    const rofd_page_t *page,
    rofd_text_layout_t **layout,
    rofd_error_t **error);

rofd_status_t rofd_text_layout_get_count(
    const rofd_text_layout_t *layout,
    size_t *count,
    rofd_error_t **error);

rofd_status_t rofd_text_layout_get_char(
    const rofd_text_layout_t *layout,
    size_t index,
    rofd_text_char_t *character,
    rofd_error_t **error);

void rofd_text_layout_free(rofd_text_layout_t *layout);
```

The UTF-8 range always indexes the string returned by
`rofd_page_get_text`. Synthesized separators have explicit flags and an
empty rectangle. Exact and conservative geometry are distinguishable by flags.

### Search

Search follows Poppler's page-local query and flag model but stores results in
an owned opaque result handle:

```c
typedef struct rofd_find_options {
    uint32_t struct_size;
    uint32_t flags;
    size_t max_results;
} rofd_find_options_t;

typedef struct rofd_text_match {
    uint32_t struct_size;
    size_t utf8_offset;
    size_t utf8_length;
    rofd_rect_t rect_mm;
} rofd_text_match_t;

rofd_status_t rofd_page_find_text(
    const rofd_page_t *page,
    const char *query,
    rofd_text_search_t **search,
    rofd_error_t **error);

rofd_status_t rofd_page_find_text_with_options(
    const rofd_page_t *page,
    const char *query,
    const rofd_find_options_t *options,
    rofd_text_search_t **search,
    rofd_error_t **error);

rofd_status_t rofd_text_search_get_count(
    const rofd_text_search_t *search,
    size_t *count,
    rofd_error_t **error);

rofd_status_t rofd_text_search_get_match(
    const rofd_text_search_t *search,
    size_t index,
    rofd_text_match_t *match,
    rofd_error_t **error);

void rofd_text_search_free(rofd_text_search_t *search);
```

Initial flags cover case-sensitive and whole-word matching. Default matching is
Unicode-aware case-insensitive literal search. Search never slices in the middle
of a UTF-8 scalar. `max_results` is bounded by a resource-policy default and
cannot request an unbounded allocation. Match rectangles are unions of visible
character boxes; matches containing only synthesized separators have an empty
rectangle.

### Selection and area extraction

Selection styles mirror Poppler's intent rather than its exact enum values:

- glyph: characters whose boxes intersect the rectangle;
- word: expand intersected characters to word boundaries;
- line: expand them to logical line boundaries.

Area extraction uses `rofd_page_get_text_for_area`. Styled selection produces
an owned result handle so returned text and regions share one lifetime and no
allocator crosses the ABI:

```c
rofd_status_t rofd_page_get_selected_text(
    const rofd_page_t *page,
    uint32_t style,
    const rofd_rect_t *selection_mm,
    rofd_text_selection_t **selection,
    rofd_error_t **error);
```

The selection handle exposes borrowed UTF-8 text and indexed rectangles, with a
matching `rofd_text_selection_free`.

## Later semantic increments

The same Poppler-like placement rules apply to subsequent APIs:

- document metadata remains document-level and returns borrowed values from an
  owned metadata handle or individual getter family;
- outline/index traversal uses an iterator or immutable indexed handle, not raw
  internal vectors;
- links, annotations, signatures, and images are page mapping queries with
  rectangles and owned result handles;
- destinations distinguish internal page/position targets from external URIs;
- signature parsing and cryptographic validation remain separate, with explicit
  `NOT_CHECKED`, `VALID`, `INVALID`, and `UNSUPPORTED` states.

No C API exposes `PageObject`, Rust enums, `Vec`, `String`, references, GLib
containers, or renderer service types.

## Rust architecture freedom

The Rust implementation may use a private semantic module, public query types,
page extension methods, or a separate cached text-index object. The deciding
criteria are correctness, immutable sharing, bounded memory, concurrency, and
testability—not similarity to Poppler naming.

The current Qt reader's ad-hoc search traversal becomes a compatibility test and
eventual consumer of the semantic layer; it is not the semantic implementation.
`rofd-core` remains free of GUI and rendering dependencies. If exact glyph
outlines later improve geometry, that enhancement must be optional and must not
change the baseline semantic result contract silently.

## Error, safety, and compatibility rules

All new fallible C calls use the existing boundary helper and preserve outputs
on failure. Every output slot must be aligned, writable, non-overlapping, and
disjoint from readable inputs and live handles. Invalid UTF-8 queries, unknown
flags, undersized records, non-finite rectangles, invalid selection styles,
out-of-range indices, and resource-limit failures return specific existing
status categories plus owned error details.

New string, layout, search, and selection handles participate in the sealed
handle registry and have matching null-safe free functions. Handles are
independently owned and retain upstream data needed for their lifetime. No panic
crosses the C boundary.

The v1 header remains source and binary compatible. New record layouts append
fields only in future versions and permanently preserve every published size
boundary. The exported-symbol allowlist is updated deliberately with every ABI
increment.

## Verification

Implementation proceeds test-first at four boundaries:

1. Rust semantic tests cover Unicode scalars, whitespace, nested groups,
   templates, transforms, vertical movement, malformed geometry, ordering,
   area selection, whole-word matching, and exact resource limits.
2. Rust FFI tests cover handle independence, borrowed string lifetimes,
   transactional outputs, overlap rejection, invalid records and flags, optional
   errors, concurrent reads, and null-safe frees.
3. C11 and C++17 consumers open the real invoice fixture, extract known Chinese
   text, search it, select an area, inspect character/match rectangles, free
   parent handles early, and link dynamically to the library built by that run.
4. Header and symbol tests compile under C11/C++17 and enforce the expanded
   `rofd_*` allowlist without exporting Rust internals.

Required gates are `cargo fmt --all -- --check`, strict Clippy for core and FFI,
affected package tests, the independent C tests, and the real fixture check.
Rendering-specific tests are not required for semantic-only commits, but the
full workspace gate runs once the local JBIG2 development dependency is
available.

## Non-goals

This work does not add editing, form filling, signature creation, decryption,
OCR, accessibility tree generation, GLib/GObject wrappers, or Poppler ABI
compatibility. It does not redesign rendering or absorb the uncommitted image
mask work from the main checkout.
