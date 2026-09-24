//! Shared host-side text selection — draw-time line rects, the drag
//! selection itself, hit-testing, and the pure highlight/copy math. Used by
//! DM message bubbles (`layout::modals::messenger_overlay`, moved off
//! `dm_overlay` by the messenger UI-2a paint move) and Forum post/reply
//! bodies (`layout::modals::forum_v2_overlay`); the same shape a future
//! pass can point the Agents chat panel's own selection at (today still a
//! parallel implementation — `mlc-app-sidebar-content`'s `ChatSelection`,
//! `mlc-app`'s `chat_selection_text` — not touched by this pass).
//!
//! Harvest: uzor selection
//!
//! No `ChartApp`/`BackendCore`/modal-state access: every function here
//! takes plain values (line rects, a selection, already-measured widths)
//! and returns plain values. The caller (a `layout::modals` file) owns the
//! `RenderContext`/`InputCoordinator` and does the actual painting/click
//! dispatch — this module never touches either.
//!
//! ## R29 (owner report, 0.5.614): DM selection "does not work"
//!
//! A full static trace of the input path — mousedown hit-test
//! (`mlc-app/src/input/mod.rs::on_drag_start`), drag-extend
//! (`on_drag_move`), and Ctrl+C routing (`mlc-app-input-shell/src/
//! dispatch.rs`) — found the mechanism wired correctly and SYMMETRICALLY
//! with Forum's own post-body selection (`54913374`, confirmed working):
//! same mousedown → `selection = Some(..)` + `selection_drag_active = true`
//! → drag-extend updates `end_*` → Ctrl+C reads `frame_result` line rects
//! and clears the selection, in that order, for both. Hit-test bypasses the
//! `InputCoordinator`'s layer/z-order system entirely (a raw scan over this
//! frame's collected [`LineHit`]s) — so a layer mismatch against the modal
//! is not possible. The one CONFIRMED defect: the hit-test had no X bound
//! at all, `y >= y_top && y < y_bot` alone — any click in a line's full row
//! (an empty margin beside a short line, over the peer avatar's column,
//! past a short bubble's right edge) still claimed the drag. Closed here
//! ([`hit_test`]'s own `HIT_X_SLACK`). That looseness makes the hit region
//! WIDER than intended, not narrower — it does not on its own explain "no
//! selection at all".
//!
//! Given the mechanism traces correctly end to end, the best-supported
//! explanation for the owner's report is a STALE BUILD: this crate's own
//! `CLAUDE.md` documents this exact failure mode twice over (the wasm dev
//! stand at `:17499` is unsupervised and nobody auto-rebuilds it; the
//! native shell did not even compile the `cloud` feature into any shipped
//! binary until 2026-09-03, silently, for the same "opt-in only works when
//! someone remembers to opt in" reason) — and the selection code's own
//! comments are dated one day AFTER the 2026-09-23 owner review this
//! report follows. Rebuilding and relaunching the exact shell tested is the
//! next step to confirm, before any further selection-code change.

/// One visual (wrapped) line of a selectable text block — one bubble line,
/// one post-body line — collected during the owning view's own paint pass.
/// `block` is whatever key groups lines into one selectable unit for the
/// caller (a message index for DM, a post number for Forum): a plain
/// `i64` rather than a per-caller type, since both callers already have
/// their own integer key and comparing `(block, line)` pairs is all
/// [`Selection::ordered`] needs.
#[derive(Clone, Debug, PartialEq)]
pub struct LineHit {
    pub block: i64,
    pub line: u16,
    pub y_top: f64,
    pub y_bot: f64,
    /// The line's own text, verbatim — re-measured on demand by the
    /// highlight overlay and by copy (no precomputed char-position table).
    pub text: String,
    /// Left edge the line's text was drawn from.
    pub text_x: f64,
    /// The line's own rendered width (`ctx.measure_text(text)`, captured at
    /// paint time) — lets [`hit_test`] bound a click to the line's actual
    /// visible extent instead of the whole row's width.
    pub width: f64,
    /// Monospace ciphertext line vs. an ordinary body line — the caller's
    /// own paint pass needs this to pick the same font before re-measuring
    /// (DM ciphertext bubbles only; Forum always `false`).
    pub mono: bool,
}

/// A host-side text selection, in `(block, line, char)` coordinates —
/// start and end tracked independently, so `start > end` (dragged upward/
/// backward) is a valid state until [`Self::ordered`] sorts it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Selection {
    pub start_block: i64,
    pub start_line: u16,
    pub start_char: u16,
    pub end_block: i64,
    pub end_line: u16,
    pub end_char: u16,
}

impl Selection {
    /// Anchor a fresh selection at the given position (start == end until a
    /// drag extends it).
    pub fn new_at(block: i64, line: u16, char_idx: u16) -> Self {
        Self {
            start_block: block,
            start_line: line,
            start_char: char_idx,
            end_block: block,
            end_line: line,
            end_char: char_idx,
        }
    }

    /// Returns `((lo_block, lo_line, lo_char), (hi_block, hi_line,
    /// hi_char))` in reading order, regardless of which end the drag
    /// started from.
    pub fn ordered(&self) -> ((i64, u16, u16), (i64, u16, u16)) {
        let s = (self.start_block, self.start_line, self.start_char);
        let e = (self.end_block, self.end_line, self.end_char);
        if s <= e {
            (s, e)
        } else {
            (e, s)
        }
    }

    pub fn is_empty(&self) -> bool {
        self.start_block == self.end_block && self.start_line == self.end_line && self.start_char == self.end_char
    }
}

/// Slack (px) added on either side of a line's own measured width when
/// hit-testing a click against it — a click just past the last character
/// (or just before the first) still starts/extends the selection, the same
/// forgiving margin any text editor gives, without accepting a click
/// anywhere in the row's full, often much wider, viewport width. See this
/// module's own "R29" doc for the looseness this constant closes.
const HIT_X_SLACK: f64 = 6.0;

/// Find the line under `(x, y)` — the Y band AND the X range (the line's
/// own `text_x - slack ..= text_x + width + slack`) must both contain the
/// point. `None` when the point falls on no collected line (e.g. an
/// avatar's column, a nick/clock line — neither is pushed as a `LineHit`).
pub fn hit_test(rects: &[LineHit], x: f64, y: f64) -> Option<&LineHit> {
    rects
        .iter()
        .find(|l| y >= l.y_top && y < l.y_bot && x >= l.text_x - HIT_X_SLACK && x <= l.text_x + l.width + HIT_X_SLACK)
}

/// Extract the plain text `sel` covers, walking `rects` in draw order
/// (already top-to-bottom for both callers) and joining crossed lines with
/// `\n` — timestamps/nicks are never included since they are never pushed
/// as a `LineHit` in the first place.
pub fn selection_text(rects: &[LineHit], sel: &Selection) -> Option<String> {
    if sel.is_empty() {
        return None;
    }
    let ((lo_block, lo_line, lo_char), (hi_block, hi_line, hi_char)) = sel.ordered();
    let mut text = String::new();
    for entry in rects {
        let pos = (entry.block, entry.line);
        if pos < (lo_block, lo_line) || pos > (hi_block, hi_line) {
            continue;
        }
        if !text.is_empty() {
            text.push('\n');
        }
        let is_first = pos == (lo_block, lo_line);
        let is_last = pos == (hi_block, hi_line);
        if is_first && is_last {
            let slice: String = entry
                .text
                .chars()
                .skip(lo_char as usize)
                .take((hi_char as usize).saturating_sub(lo_char as usize))
                .collect();
            text.push_str(&slice);
        } else if is_first {
            let slice: String = entry.text.chars().skip(lo_char as usize).collect();
            text.push_str(&slice);
        } else if is_last {
            let slice: String = entry.text.chars().take(hi_char as usize).collect();
            text.push_str(&slice);
        } else {
            text.push_str(&entry.text);
        }
    }
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

/// Every line whose own `block` equals `block_id`, plain text joined by
/// `\n`, in draw order — a message's own right-click "Copy" when there is
/// no active drag selection (the concept's own "Ctrl+C / правый клик →
/// Copy" row; with a selection, the caller uses [`selection_text`]
/// instead). `None` when `block_id` matches no collected line (a stale
/// widget/target, a closed conversation) — the caller is expected to check
/// first; this returns `None` rather than an empty string, same "absent
/// means nothing" convention this module's own callers already follow.
pub fn block_text(rects: &[LineHit], block_id: i64) -> Option<String> {
    let mut text = String::new();
    for entry in rects.iter().filter(|l| l.block == block_id) {
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(&entry.text);
    }
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

/// A [`Selection`] spanning every line in `rects` — "select the whole
/// loaded chat" (a message's own right-click "Select all", same action
/// Ctrl+A performs: the concept's own spec reads "Ctrl+A — весь загруженный
/// чат"). Built from whichever `LineHit`s the caller's own last render pass
/// collected, so it always matches exactly what is on screen rather than a
/// second, independent reconstruction of the same bounds. `None` when
/// `rects` is empty — nothing to select.
pub fn select_all(rects: &[LineHit]) -> Option<Selection> {
    let first = rects.first()?;
    let last = rects.last()?;
    Some(Selection {
        start_block: first.block,
        start_line: first.line,
        start_char: 0,
        end_block: last.block,
        end_line: last.line,
        end_char: last.text.chars().count() as u16,
    })
}

/// Pure highlight-span geometry for ONE line already known to fall inside
/// `sel`'s ordered range (the caller filters with this — walk `rects`,
/// keep only entries whose `(block, line)` lies between [`Selection::
/// ordered`]'s two ends) — decides the `(x, width)` a translucent overlay
/// rect should paint, given already-measured character-prefix widths
/// (`prefix_width(n)` = the rendered width of the line's first `n`
/// characters, in the SAME font the caller already set before calling
/// this — this module never touches a `RenderContext` itself).
/// `is_first`/`is_last` are whether THIS `(block, line)` pair is the
/// selection's own first/last line (both `true` for a single-line
/// selection).
pub fn highlight_span(
    line_text_x: f64,
    line_width: f64,
    lo_char: u16,
    hi_char: u16,
    is_first: bool,
    is_last: bool,
    prefix_width: impl Fn(u16) -> f64,
) -> (f64, f64) {
    if is_first && is_last {
        let prefix_w = prefix_width(lo_char);
        let end_w = prefix_width(hi_char);
        (line_text_x + prefix_w, (end_w - prefix_w).max(0.0))
    } else if is_first {
        let prefix_w = prefix_width(lo_char);
        (line_text_x + prefix_w, (line_width - prefix_w).max(4.0))
    } else if is_last {
        let end_w = prefix_width(hi_char);
        (line_text_x, end_w.max(4.0))
    } else {
        (line_text_x, line_width.max(4.0))
    }
}

/// Highlight-rect geometry for ONE already-drawn line, anchored to that
/// line's own [`LineHit`] — the exact `y_top`/`y_bot` the line was drawn
/// and hit-tested with, never a second, independent copy of the caller's
/// own vertical-layout formula. Two parallel copies of "where is this
/// line" (one for painting the highlight, one for the `LineHit` push) are
/// exactly the drift risk that produces a highlight band that no longer
/// tracks the text it is meant to cover the moment either copy is edited
/// without the other — this function removes the second copy entirely by
/// reading straight off the [`LineHit`] instead of recomputing. Combines
/// [`highlight_span`]'s X math with the line's own Y band into one
/// `(x, y, width, height)` rect the caller passes straight to
/// `fill_rect`.
pub fn highlight_rect(
    line: &LineHit,
    lo_char: u16,
    hi_char: u16,
    is_first: bool,
    is_last: bool,
    prefix_width: impl Fn(u16) -> f64,
) -> (f64, f64, f64, f64) {
    let (x, w) = highlight_span(line.text_x, line.width, lo_char, hi_char, is_first, is_last, prefix_width);
    (x, line.y_top, w, line.y_bot - line.y_top)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(block: i64, line: u16, y_top: f64, text: &str, text_x: f64, width: f64) -> LineHit {
        LineHit { block, line, y_top, y_bot: y_top + 16.0, text: text.to_string(), text_x, width, mono: false }
    }

    #[test]
    fn hit_test_requires_both_the_y_band_and_the_x_range() {
        let rects = vec![hit(0, 0, 10.0, "hello", 20.0, 40.0)];
        // Inside the row's Y band but far to the right, past the line's own
        // measured width + slack — this is the looseness R29 closes.
        assert!(hit_test(&rects, 200.0, 15.0).is_none());
        // Inside both the Y band and the X range.
        assert!(hit_test(&rects, 30.0, 15.0).is_some());
        // Just past the end, within slack.
        assert!(hit_test(&rects, 20.0 + 40.0 + 3.0, 15.0).is_some());
        // Wrong row entirely.
        assert!(hit_test(&rects, 30.0, 40.0).is_none());
    }

    #[test]
    fn selection_text_joins_a_single_line_slice() {
        let rects = vec![hit(0, 0, 0.0, "hello world", 0.0, 80.0)];
        let sel = Selection { start_block: 0, start_line: 0, start_char: 0, end_block: 0, end_line: 0, end_char: 5 };
        assert_eq!(selection_text(&rects, &sel).as_deref(), Some("hello"));
    }

    #[test]
    fn selection_text_spans_multiple_lines_and_blocks_with_newlines() {
        let rects = vec![
            hit(0, 0, 0.0, "first line", 0.0, 80.0),
            hit(0, 1, 16.0, "second line", 0.0, 80.0),
            hit(1, 0, 32.0, "third line", 0.0, 80.0),
        ];
        let sel = Selection { start_block: 0, start_line: 0, start_char: 6, end_block: 1, end_line: 0, end_char: 5 };
        assert_eq!(selection_text(&rects, &sel).as_deref(), Some("line\nsecond line\nthird"));
    }

    #[test]
    fn an_empty_selection_extracts_nothing() {
        let rects = vec![hit(0, 0, 0.0, "hello", 0.0, 40.0)];
        let sel = Selection::new_at(0, 0, 2);
        assert_eq!(selection_text(&rects, &sel), None);
    }

    #[test]
    fn highlight_span_a_single_line_selection_spans_only_its_own_chars() {
        let (x, w) = highlight_span(10.0, 80.0, 2, 5, true, true, |n| n as f64 * 6.0);
        assert_eq!(x, 10.0 + 12.0);
        assert_eq!(w, (5 * 6 - 2 * 6) as f64);
    }

    #[test]
    fn highlight_span_a_middle_line_spans_the_whole_line() {
        let (x, w) = highlight_span(10.0, 80.0, 0, 0, false, false, |n| n as f64 * 6.0);
        assert_eq!(x, 10.0);
        assert_eq!(w, 80.0);
    }

    #[test]
    fn highlight_rect_takes_its_y_and_height_from_the_line_hit_not_a_recomputation() {
        // A `y_top` that does NOT sit on any "obvious" multiple of a line
        // height constant — if this function ever regressed to
        // recomputing its own `y = n * LINE_H` formula instead of reading
        // the `LineHit` it was given, a value like this would expose it.
        let line = hit(2, 3, 137.5, "hello world", 20.0, 60.0);
        let (x, y, w, h) = highlight_rect(&line, 0, 5, true, true, |n| n as f64 * 6.0);
        assert_eq!(y, line.y_top);
        assert_eq!(h, line.y_bot - line.y_top);
        assert_eq!((x, w), highlight_span(line.text_x, line.width, 0, 5, true, true, |n| n as f64 * 6.0));
    }

    #[test]
    fn block_text_joins_only_the_matching_blocks_own_lines() {
        let rects = vec![
            hit(0, 0, 0.0, "first line", 0.0, 80.0),
            hit(0, 1, 16.0, "second line", 0.0, 80.0),
            hit(1, 0, 32.0, "other message", 0.0, 80.0),
        ];
        assert_eq!(block_text(&rects, 0).as_deref(), Some("first line\nsecond line"));
        assert_eq!(block_text(&rects, 1).as_deref(), Some("other message"));
        assert_eq!(block_text(&rects, 2), None);
    }

    #[test]
    fn select_all_spans_the_first_to_the_last_collected_line() {
        let rects = vec![
            hit(0, 0, 0.0, "first line", 0.0, 80.0),
            hit(1, 0, 16.0, "second message", 0.0, 80.0),
        ];
        let sel = select_all(&rects).expect("non-empty rects yield a selection");
        assert_eq!(sel.start_block, 0);
        assert_eq!(sel.start_char, 0);
        assert_eq!(sel.end_block, 1);
        assert_eq!(sel.end_char, "second message".chars().count() as u16);
        assert_eq!(select_all(&[]), None);
    }

    #[test]
    fn highlight_rect_height_follows_a_non_default_line_hit_band() {
        // `hit()` always uses a 16.0 band — build one with a different
        // band by hand to prove the height is NOT a hardcoded constant.
        let line = LineHit { block: 0, line: 0, y_top: 4.0, y_bot: 30.0, text: "x".to_string(), text_x: 0.0, width: 6.0, mono: false };
        let (_, y, _, h) = highlight_rect(&line, 0, 1, true, true, |n| n as f64 * 6.0);
        assert_eq!(y, 4.0);
        assert_eq!(h, 26.0);
    }
}
