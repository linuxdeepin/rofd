use std::ops::Range;

use crate::{Error, Page, PageObject, Point, Rect, Result, TextObject, Transform};

/// Supplies the horizontal advance of one scalar when a text run declares no
/// `DeltaX` for it.
///
/// The OFD text model positions such a scalar by its font's advance, so geometry
/// that resolves fonts matches what a renderer draws; this crate parses and indexes
/// text without font access, which is why a provider is injected from outside
/// (`rofd-render` implements it over the same font stack the renderer uses). A
/// document loaded without a provider keeps the font-size footprint for those runs,
/// which stays a valid conservative approximation.
pub trait TextAdvanceProvider: Send + Sync {
    /// Returns one advance in millimetres per scalar of `text`, concatenating its
    /// runs in order, or `None` when the object's font cannot be resolved (the
    /// builder then keeps the font-size footprint).
    ///
    /// Non-finite or negative values are ignored per scalar.
    fn advances_mm(&self, page: &Page, text: &TextObject, size_mm: f64) -> Option<Vec<f64>>;
}

/// Options for literal search in canonical page text.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FindOptions {
    /// Matches exact original case when true; otherwise uses scalar lowercasing.
    pub case_sensitive: bool,
    /// Requires adjacent original scalars to be neither alphanumeric nor `_`.
    pub whole_words: bool,
    /// Maximum number of returned matches; must be greater than zero.
    pub max_results: usize,
}

impl Default for FindOptions {
    fn default() -> Self {
        Self {
            case_sensitive: false,
            whole_words: false,
            max_results: 10_000,
        }
    }
}

/// Controls expansion from source glyphs intersecting a selection rectangle.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SelectionStyle {
    /// Selects only intersecting source glyphs.
    Glyph,
    /// Expands intersecting word characters through adjacent alphanumeric or `_` scalars.
    Word,
    /// Expands to logical lines delimited by synthesized newline separators.
    Line,
}

/// A literal search match in canonical page text.
#[derive(Clone, Debug, PartialEq)]
pub struct TextMatch {
    utf8_range: Range<usize>,
    rect_mm: Option<Rect>,
}

impl TextMatch {
    /// Returns the matched byte range in [`PageText::as_str`].
    pub fn utf8_range(&self) -> Range<usize> {
        self.utf8_range.clone()
    }

    /// Returns the union of visible matched character boxes in page millimetres.
    /// Separator-only matches have no geometry.
    pub fn rect_mm(&self) -> Option<Rect> {
        self.rect_mm
    }
}

/// Selected canonical text and its ordered per-line geometry.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TextSelection {
    text: String,
    regions: Vec<Rect>,
}

impl TextSelection {
    /// Returns the selected text in canonical order.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Returns one union of selected visible boxes per participating logical line.
    pub fn regions(&self) -> &[Rect] {
        &self.regions
    }
}

/// Describes the fidelity of a character's reported geometry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TextGeometryPrecision {
    /// Geometry precisely represents the character's painted bounds.
    Exact,
    /// Geometry is a nonempty approximation derived from available OFD data.
    Conservative,
}

/// Bit flags that describe special properties of a semantic text character.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TextCharFlags(u32);

impl TextCharFlags {
    /// Marks a separator inserted while flattening distinct source text objects.
    pub const SYNTHESIZED_SEPARATOR: Self = Self(1 << 0);

    /// Marks a whitespace character from the source text.
    pub const WHITESPACE: Self = Self(1 << 1);

    /// Returns whether all flags in `other` are set.
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// Returns the raw bit representation.
    pub const fn bits(self) -> u32 {
        self.0
    }
}

/// One Unicode scalar value in flattened page text and its source metadata.
#[derive(Clone, Debug, PartialEq)]
pub struct TextChar {
    utf8_range: Range<usize>,
    rect_mm: Option<Rect>,
    flags: TextCharFlags,
    precision: TextGeometryPrecision,
    object_id: Option<u64>,
}

impl TextChar {
    /// Returns this character's byte range in [`PageText::as_str`].
    pub fn utf8_range(&self) -> Range<usize> {
        self.utf8_range.clone()
    }

    /// Returns the character's bounding rectangle in physical-page millimetre
    /// coordinates after transforms, when known. Synthesized separators have no
    /// rectangle.
    pub fn rect_mm(&self) -> Option<Rect> {
        self.rect_mm
    }

    /// Returns flags describing this character.
    pub fn flags(&self) -> TextCharFlags {
        self.flags
    }

    /// Returns the fidelity of this character's geometry.
    pub fn geometry_precision(&self) -> TextGeometryPrecision {
        self.precision
    }

    /// Returns the source `TextObject` identifier, if this character was not synthesized.
    pub fn object_id(&self) -> Option<u64> {
        self.object_id
    }
}

/// Immutable flattened text and per-character metadata for one page.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PageText {
    text: String,
    characters: Vec<TextChar>,
}

impl PageText {
    /// Returns the flattened UTF-8 page text.
    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// Returns metadata for each Unicode scalar value in [`Self::as_str`].
    pub fn characters(&self) -> &[TextChar] {
        &self.characters
    }

    /// Extracts source glyphs with positive-area intersection, in canonical order.
    /// Synthesized separators are omitted. Empty rectangles produce empty text;
    /// nonfinite coordinates, nonfinite edges or negative dimensions return
    /// [`Error::InvalidOption`]. Negative positions are valid.
    pub fn text_for_area(&self, area: Rect) -> Result<String> {
        validate_area(area)?;
        let mut text = String::new();
        for character in &self.characters {
            if character.rect_mm.is_some_and(|rect| intersects(rect, area)) {
                text.push_str(&self.text[character.utf8_range.clone()]);
            }
        }
        Ok(text)
    }

    /// Finds nonoverlapping literal matches from left to right, stopping at the
    /// configured result limit. Empty queries and a zero limit return
    /// [`Error::InvalidOption`]. Unrepresentable geometry unions return
    /// [`Error::InvalidValue`].
    ///
    /// Case-insensitive search uses Unicode scalar lowercasing, without Unicode
    /// normalization or full case folding. Matches inside a lowercase expansion
    /// cover the complete original scalar; returned original ranges never overlap.
    /// Whole-word boundaries are evaluated against adjacent original scalars.
    pub fn find(&self, query: &str, options: FindOptions) -> Result<Vec<TextMatch>> {
        if query.is_empty() {
            return Err(Error::InvalidOption {
                field: "query",
                value: String::new(),
            });
        }
        if options.max_results == 0 {
            return Err(Error::InvalidOption {
                field: "max_results",
                value: "0".into(),
            });
        }
        let mut folded = String::new();
        // Every folded byte explicitly maps back to its canonical TextChar.
        let mut original_indices = Vec::new();
        let haystack = if options.case_sensitive {
            self.as_str()
        } else {
            for (index, character) in self.characters.iter().enumerate() {
                for scalar in self.text[character.utf8_range.clone()]
                    .chars()
                    .flat_map(char::to_lowercase)
                {
                    let end = checked_index_add(folded.len(), scalar.len_utf8())?;
                    folded.push(scalar);
                    original_indices.resize(end, index);
                }
            }
            &folded
        };
        // Bound query allocation by the searchable page, even for a huge query.
        let mut folded_query = String::new();
        let needle = if options.case_sensitive {
            query
        } else {
            for scalar in query.chars().flat_map(char::to_lowercase) {
                if checked_index_add(folded_query.len(), scalar.len_utf8())? > haystack.len() {
                    return Ok(Vec::new());
                }
                folded_query.push(scalar);
            }
            &folded_query
        };
        if needle.len() > haystack.len() {
            return Ok(Vec::new());
        }
        let needle = needle.as_bytes();
        let prefix_table = literal_prefix_table(needle);
        let mut matches = Vec::new();
        let mut previous_end = 0;
        let mut matched = 0;
        for (index, byte) in haystack.bytes().enumerate() {
            while matched > 0 && needle[matched] != byte {
                matched = prefix_table[matched - 1];
            }
            if needle[matched] == byte {
                matched += 1;
            }
            if matched != needle.len() {
                continue;
            }

            let end = checked_index_add(index, 1)?;
            let start = end.checked_sub(needle.len()).ok_or_else(index_overflow)?;
            debug_assert!(haystack.is_char_boundary(start));
            debug_assert!(haystack.is_char_boundary(end));
            let (first, after_last) = if options.case_sensitive {
                (
                    self.characters
                        .partition_point(|ch| ch.utf8_range.end <= start),
                    self.characters
                        .partition_point(|ch| ch.utf8_range.start < end),
                )
            } else {
                let last_byte = end.checked_sub(1).ok_or_else(index_overflow)?;
                (
                    original_indices[start],
                    checked_index_add(original_indices[last_byte], 1)?,
                )
            };
            let characters = &self.characters[first..after_last];
            let utf8_range = characters
                .first()
                .expect("nonempty literal match")
                .utf8_range
                .start
                ..characters
                    .last()
                    .expect("nonempty literal match")
                    .utf8_range
                    .end;
            if utf8_range.start < previous_end {
                // Retain the longest literal prefix that is also a suffix. This
                // considers the next candidate at a later folded-scalar boundary
                // instead of skipping every candidate that overlaps a rejection.
                matched = prefix_table[matched - 1];
                continue;
            }
            if options.whole_words
                && (first
                    .checked_sub(1)
                    .is_some_and(|index| self.is_word(index))
                    || self.is_word(after_last))
            {
                matched = prefix_table[matched - 1];
                continue;
            }
            let rect_mm = union_rectangles(characters.iter().filter_map(|ch| ch.rect_mm))?;
            previous_end = utf8_range.end;
            matches.push(TextMatch {
                utf8_range,
                rect_mm,
            });
            if matches.len() == options.max_results {
                break;
            }
            // Accepted matches remain nonoverlapping in the searched text.
            matched = 0;
        }
        Ok(matches)
    }

    /// Selects intersecting source glyphs, optionally expanding words or logical
    /// lines. Geometry intersection and area errors follow [`Self::text_for_area`].
    /// A region union that cannot be represented finitely returns
    /// [`Error::InvalidValue`], without publishing partial selection data.
    ///
    /// Glyph and word selections emit selected source scalars without synthesized
    /// separators. Line selections retain a synthesized newline only when both
    /// adjacent lines are selected. Regions are ordered by canonical text, with
    /// one union per selected logical line, never across synthesized separators.
    pub fn select(&self, area: Rect, style: SelectionStyle) -> Result<TextSelection> {
        validate_area(area)?;
        let mut selected = self
            .characters
            .iter()
            .map(|ch| ch.rect_mm.is_some_and(|rect| intersects(rect, area)))
            .collect::<Vec<_>>();
        if style != SelectionStyle::Glyph {
            // Expand disjoint groups once, so selecting a long word or line is
            // linear in the page size rather than quadratic in the number of hits.
            let mut first = 0;
            for index in 0..self.characters.len() {
                let boundary = match style {
                    SelectionStyle::Word => !self.is_word(index),
                    SelectionStyle::Line => self.is_separator(index),
                    SelectionStyle::Glyph => unreachable!(),
                };
                if boundary {
                    expand_selected_group(&mut selected[first..index]);
                    first = checked_index_add(index, 1)?;
                }
            }
            expand_selected_group(&mut selected[first..]);
        }
        let mut selection = TextSelection::default();
        let mut region = None;
        for (index, character) in self.characters.iter().enumerate() {
            if self.is_separator(index) {
                if let Some(rect) = region.take() {
                    selection.regions.push(rect);
                }
                if style == SelectionStyle::Line
                    && index.checked_sub(1).is_some_and(|before| selected[before])
                    && selected
                        .get(checked_index_add(index, 1)?)
                        .copied()
                        .unwrap_or(false)
                {
                    selection
                        .text
                        .push_str(&self.text[character.utf8_range.clone()]);
                }
            } else if selected[index] {
                selection
                    .text
                    .push_str(&self.text[character.utf8_range.clone()]);
                region = union_rectangles(region.into_iter().chain(character.rect_mm))?;
            }
        }
        if let Some(rect) = region {
            selection.regions.push(rect);
        }
        Ok(selection)
    }

    fn is_word(&self, index: usize) -> bool {
        self.characters.get(index).is_some_and(|ch| {
            self.text[ch.utf8_range.clone()]
                .chars()
                .any(|scalar| scalar.is_alphanumeric() || scalar == '_')
        })
    }

    fn is_separator(&self, index: usize) -> bool {
        self.characters[index]
            .flags
            .contains(TextCharFlags::SYNTHESIZED_SEPARATOR)
    }
}

fn expand_selected_group(selected: &mut [bool]) {
    if selected.iter().any(|hit| *hit) {
        selected.fill(true);
    }
}

fn index_overflow() -> Error {
    Error::LimitExceeded("semantic query index overflow".into())
}

fn checked_index_add(index: usize, amount: usize) -> Result<usize> {
    index.checked_add(amount).ok_or_else(index_overflow)
}

fn literal_prefix_table(needle: &[u8]) -> Vec<usize> {
    debug_assert!(!needle.is_empty());
    let mut table = vec![0; needle.len()];
    let mut matched = 0;
    for index in 1..needle.len() {
        while matched > 0 && needle[matched] != needle[index] {
            matched = table[matched - 1];
        }
        if needle[matched] == needle[index] {
            matched += 1;
        }
        table[index] = matched;
    }
    table
}

fn validate_area(area: Rect) -> Result<()> {
    if [
        area.x,
        area.y,
        area.width,
        area.height,
        area.x + area.width,
        area.y + area.height,
    ]
    .iter()
    .any(|value| !value.is_finite())
        || area.width < 0.0
        || area.height < 0.0
    {
        return Err(Error::InvalidOption {
            field: "area",
            value: format!("{area:?}"),
        });
    }
    Ok(())
}

fn intersects(left: Rect, right: Rect) -> bool {
    left.x.max(right.x) < (left.x + left.width).min(right.x + right.width)
        && left.y.max(right.y) < (left.y + left.height).min(right.y + right.height)
}

fn union_rectangles(mut rectangles: impl Iterator<Item = Rect>) -> Result<Option<Rect>> {
    rectangles.try_fold(None, |union: Option<Rect>, rect| {
        let Some(previous) = union else {
            return Ok(Some(rect));
        };
        let x = previous.x.min(rect.x);
        let y = previous.y.min(rect.y);
        let right = finite_add(previous.x, previous.width)?.max(finite_add(rect.x, rect.width)?);
        let bottom = finite_add(previous.y, previous.height)?.max(finite_add(rect.y, rect.height)?);
        Ok(Some(Rect {
            x,
            y,
            width: finite_add(right, -x)?,
            height: finite_add(bottom, -y)?,
        }))
    })
}

pub(crate) fn build_page_text(
    page: &crate::Page,
    advances: Option<&dyn TextAdvanceProvider>,
) -> Result<PageText> {
    let mut builder = TextBuilder::new(page, advances);
    for layer in page.layers() {
        builder.objects(layer.objects(), Transform::IDENTITY)?;
    }
    for annotation in page.annotations() {
        if annotation.visible() {
            builder.objects(annotation.objects(), translation(annotation.boundary())?)?;
        }
    }
    Ok(builder.text)
}

struct TextBuilder<'a> {
    text: PageText,
    page: &'a Page,
    advances: Option<&'a dyn TextAdvanceProvider>,
    source_characters: usize,
    metadata_entries: usize,
    max_source_characters: usize,
    max_metadata_entries: usize,
}

impl<'a> TextBuilder<'a> {
    fn new(page: &'a Page, advances: Option<&'a dyn TextAdvanceProvider>) -> Self {
        let limits = page.resource_limits();
        Self {
            text: PageText::default(),
            page,
            advances,
            source_characters: 0,
            metadata_entries: 0,
            max_source_characters: limits.max_text_characters_per_page,
            max_metadata_entries: limits.max_text_expansion_entries,
        }
    }

    fn objects(&mut self, objects: &[PageObject], parent: Transform) -> Result<()> {
        for object in objects {
            match object {
                PageObject::Group(group) => self.objects(group.objects(), parent)?,
                PageObject::Composite(composite) if !composite.transform().is_singular() => {
                    let transform =
                        object_to_page(composite.transform(), composite.boundary(), parent)?;
                    if !transform.is_singular() {
                        self.objects(composite.objects(), transform)?;
                    }
                }
                PageObject::Text(text) if !text.transform().is_singular() => {
                    self.text_object(text, parent)?;
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn text_object(&mut self, object: &TextObject, parent: Transform) -> Result<()> {
        if object.runs().iter().all(|run| run.text().is_empty()) {
            return Ok(());
        }
        let transform = object_to_page(object.transform(), object.boundary(), parent)?;
        if transform.is_singular() {
            return Ok(());
        }
        if !self.text.text.is_empty() {
            self.push_separator()?;
        }
        let size = object.font_size();
        // One font-resolved advance per scalar of the whole object, in run order;
        // absent when the provider has no font for it. Explicit deltas stay
        // authoritative and are never replaced, and they are the only thing this
        // geometry takes from the provider, so an object whose runs all declare their
        // own deltas never asks it for a font at all.
        let needs_advances = object.runs().iter().any(|run| !run.has_explicit_delta_x());
        let resolved = if needs_advances {
            self.advances
                .and_then(|advances| advances.advances_mm(self.page, object, size))
        } else {
            None
        };
        let mut scalar = 0usize;
        for run in object.runs() {
            let (mut x, mut y) = (run.x(), run.y());
            for (index, character) in run.text().chars().enumerate() {
                let dx = if run.has_explicit_delta_x() {
                    run.delta_x()[index]
                } else {
                    resolved
                        .as_ref()
                        .and_then(|values| values.get(scalar))
                        .copied()
                        .filter(|value| value.is_finite() && *value >= 0.0)
                        .unwrap_or(size)
                };
                scalar += 1;
                let dy = if run.has_explicit_delta_y() {
                    run.delta_y()[index]
                } else {
                    0.0
                };
                let next_x = finite_add(x, dx)?;
                let next_y = finite_add(y, dy)?;
                // Bound the horizontal baseline sweep with a direction-independent
                // font-size footprint. Explicit zero advances still receive
                // nonempty conservative geometry.
                let left = x.min(next_x);
                let right = finite_add(left, dx.abs().max(size))?;
                let top = finite_add(y.min(next_y), -size)?;
                let bottom = y.max(next_y);
                let rect = transformed_box(left, top, right, bottom, transform)?;
                let flags = if character.is_whitespace() {
                    TextCharFlags::WHITESPACE
                } else {
                    TextCharFlags::default()
                };
                self.push_source(character, rect, flags, object.object_id())?;
                (x, y) = (next_x, next_y);
            }
        }
        Ok(())
    }

    fn push_separator(&mut self) -> Result<()> {
        let metadata_entries = checked_increment(
            self.metadata_entries,
            self.max_metadata_entries,
            "semantic text metadata entry",
        )?;
        self.metadata_entries = metadata_entries;
        self.push_unchecked('\n', None, TextCharFlags::SYNTHESIZED_SEPARATOR, None);
        Ok(())
    }

    fn push_source(
        &mut self,
        character: char,
        rect_mm: Rect,
        flags: TextCharFlags,
        object_id: u64,
    ) -> Result<()> {
        let source_characters = checked_increment(
            self.source_characters,
            self.max_source_characters,
            "semantic page source character",
        )?;
        let metadata_entries = checked_increment(
            self.metadata_entries,
            self.max_metadata_entries,
            "semantic text metadata entry",
        )?;
        self.source_characters = source_characters;
        self.metadata_entries = metadata_entries;
        self.push_unchecked(character, Some(rect_mm), flags, Some(object_id));
        Ok(())
    }

    fn push_unchecked(
        &mut self,
        character: char,
        rect_mm: Option<Rect>,
        flags: TextCharFlags,
        object_id: Option<u64>,
    ) {
        let start = self.text.text.len();
        self.text.text.push(character);
        self.text.characters.push(TextChar {
            utf8_range: start..self.text.text.len(),
            rect_mm,
            flags,
            precision: TextGeometryPrecision::Conservative,
            object_id,
        });
    }
}

fn checked_increment(current: usize, limit: usize, label: &'static str) -> Result<usize> {
    let next = current
        .checked_add(1)
        .ok_or_else(|| Error::LimitExceeded(format!("{label} count overflow")))?;
    if next > limit {
        return Err(Error::LimitExceeded(format!(
            "{label} limit exceeded: {next} > {limit}"
        )));
    }
    Ok(next)
}

fn translation(boundary: Rect) -> Result<Transform> {
    Transform::new(1.0, 0.0, 0.0, 1.0, boundary.x, boundary.y)
}

fn object_to_page(local: Transform, boundary: Rect, parent: Transform) -> Result<Transform> {
    local.then(translation(boundary)?)?.then(parent)
}

fn transformed_box(
    left: f64,
    top: f64,
    right: f64,
    bottom: f64,
    transform: Transform,
) -> Result<Rect> {
    let corners = [(left, top), (right, top), (right, bottom), (left, bottom)];
    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    for (x, y) in corners {
        let point = transform.apply(Point::new(x, y)?)?;
        min_x = min_x.min(point.x());
        min_y = min_y.min(point.y());
        max_x = max_x.max(point.x());
        max_y = max_y.max(point.y());
    }
    let width = finite_add(max_x, -min_x)?;
    let height = finite_add(max_y, -min_y)?;
    if width <= 0.0 || height <= 0.0 {
        return Err(Error::InvalidValue {
            field: "semantic text geometry",
            value: format!("{min_x} {min_y} {max_x} {max_y}"),
            path: None,
        });
    }
    Ok(Rect {
        x: min_x,
        y: min_y,
        width,
        height,
    })
}

fn finite_add(left: f64, right: f64) -> Result<f64> {
    let sum = left + right;
    if sum.is_finite() {
        Ok(sum)
    } else {
        Err(Error::InvalidValue {
            field: "semantic text coordinate",
            value: format!("{left} + {right}"),
            path: None,
        })
    }
}
