//! The text around the caret that a focused field tells input methods, through
//! `zwp_text_input_v3.set_surrounding_text`.
use wayland_protocols::wp::text_input::zv3::client::zwp_text_input_v3::ChangeCause;

use crate::ImePurpose;

/// The text around the caret of the focused field, for
/// [`WindowState::set_ime_surrounding_text`](crate::WindowState::set_ime_surrounding_text).
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct ImeSurroundingText {
    /// The field's text, or the part of it around the caret and selection.
    pub text: String,
    /// The byte offset of the caret in `text`.
    pub cursor: usize,
    /// The byte offset of the selection's other end in `text`, or `cursor` when
    /// nothing is selected.
    pub anchor: usize,
}

/// The most bytes of text `set_surrounding_text` may carry.
const SURROUNDING_TEXT_MAX: usize = 4000;

/// A field's surrounding text in the form `set_surrounding_text` takes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurroundingText {
    pub(crate) text: String,
    pub(crate) cursor: usize,
    pub(crate) anchor: usize,
}

impl SurroundingText {
    /// What a field of `purpose` may tell input methods: never a password field's text.
    pub(crate) fn for_purpose(
        purpose: ImePurpose,
        surrounding: Option<&ImeSurroundingText>,
    ) -> Option<Self> {
        surrounding
            .filter(|_| purpose != ImePurpose::Password)
            .map(Self::new)
    }

    /// `surrounding` with its offsets on character boundaries, cut to at most
    /// [`SURROUNDING_TEXT_MAX`] bytes around the caret and as much of the selection as fits.
    fn new(surrounding: &ImeSurroundingText) -> Self {
        let text = surrounding.text.as_str();
        let floor = |mut i: usize| {
            i = i.min(text.len());
            while !text.is_char_boundary(i) {
                i -= 1;
            }
            i
        };
        let ceil = |mut i: usize| {
            while !text.is_char_boundary(i) {
                i += 1;
            }
            i
        };
        let cursor = floor(surrounding.cursor);
        let anchor = floor(surrounding.anchor);
        if text.len() <= SURROUNDING_TEXT_MAX {
            return Self {
                text: text.to_owned(),
                cursor,
                anchor,
            };
        }

        let (mut lo, mut hi) = (cursor.min(anchor), cursor.max(anchor));
        if hi - lo > SURROUNDING_TEXT_MAX {
            // Too long a selection: keep the end at the caret.
            if anchor < cursor {
                lo = cursor - SURROUNDING_TEXT_MAX;
            } else {
                hi = cursor + SURROUNDING_TEXT_MAX;
            }
        }
        let spare = SURROUNDING_TEXT_MAX - (hi - lo);
        let after = (spare - (spare / 2).min(lo)).min(text.len() - hi);
        let before = (spare - after).min(lo);
        let start = ceil(lo - before);
        let end = floor(hi + after);

        Self {
            text: text[start..end].to_owned(),
            cursor: cursor - start,
            anchor: anchor.clamp(start, end) - start,
        }
    }
}

/// What a text input needs when its field's surrounding text changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SurroundingChange {
    /// Nothing changed.
    Unchanged,
    /// Send the new text, changed for this reason.
    Send(ChangeCause),
    /// The field stopped sharing its text. Only a new `enable` makes the input
    /// method forget what it was told.
    Reenable,
}

impl SurroundingChange {
    /// The change from `sent` to `next`; `committed` when the input method's own
    /// text is what last changed the field.
    pub(crate) fn between(
        sent: Option<&SurroundingText>,
        next: Option<&SurroundingText>,
        committed: bool,
    ) -> Self {
        match (sent, next) {
            (sent, next) if sent == next => Self::Unchanged,
            (Some(_), None) => Self::Reenable,
            (None, None) => Self::Unchanged,
            (_, Some(_)) if committed => Self::Send(ChangeCause::InputMethod),
            (_, Some(_)) => Self::Send(ChangeCause::Other),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fitted(text: &str, cursor: usize, anchor: usize) -> SurroundingText {
        SurroundingText::new(&ImeSurroundingText {
            text: text.into(),
            cursor,
            anchor,
        })
    }

    fn surrounding(text: &str) -> SurroundingText {
        fitted(text, text.len(), text.len())
    }

    #[test]
    fn short_text_is_sent_whole_with_offsets_on_character_boundaries() {
        assert_eq!(
            fitted("Find", 4, 4),
            SurroundingText {
                text: "Find".into(),
                cursor: 4,
                anchor: 4
            }
        );
        // Inside "é" (bytes 3..5): both offsets fall back to its start; past the end, to the end.
        assert_eq!(
            fitted("café", 4, 99),
            SurroundingText {
                text: "café".into(),
                cursor: 3,
                anchor: 5
            }
        );
    }

    #[test]
    fn long_text_is_cut_around_the_caret_and_selection() {
        let text: String = (0..6000u32)
            .map(|i| {
                if i % 3 == 0 {
                    'é'
                } else {
                    char::from(b'a' + (i % 26) as u8)
                }
            })
            .collect();
        let len = text.len();
        let floor = |i: usize| (0..=i).rev().find(|&i| text.is_char_boundary(i)).unwrap();
        for (cursor, anchor) in [
            (0, 0),
            (4000, 4030),
            (len, len),
            (6000, 2500),
            (10, len - 10),
        ] {
            let (cursor, anchor) = (floor(cursor), floor(anchor));
            let fit = fitted(&text, cursor, anchor);
            let kept = fit.text.len();
            assert!(
                (SURROUNDING_TEXT_MAX - 1..=SURROUNDING_TEXT_MAX).contains(&kept),
                "{kept}"
            );
            let start = cursor - fit.cursor;
            assert_eq!(&text[start..start + kept], fit.text, "{cursor}..{anchor}");
            assert!(fit.text.is_char_boundary(fit.anchor));
            if cursor.abs_diff(anchor) <= SURROUNDING_TEXT_MAX {
                assert_eq!(
                    start + fit.anchor,
                    anchor,
                    "{cursor}..{anchor}: the selection is kept"
                );
            }
        }
    }

    #[test]
    fn caret_context_is_shared_both_sides_when_there_is_room() {
        let text = format!("{}Find{}", "a".repeat(5000), "b".repeat(5000));
        let fit = fitted(&text, 5004, 5004);
        assert_eq!(fit.text.len(), SURROUNDING_TEXT_MAX);
        assert_eq!(fit.cursor, 2000);
        assert_eq!(&fit.text[1996..2000], "Find");
    }

    #[test]
    fn a_password_field_tells_nothing() {
        let text = ImeSurroundingText {
            text: "hunter2".into(),
            cursor: 7,
            anchor: 7,
        };
        assert_eq!(
            SurroundingText::for_purpose(ImePurpose::Password, Some(&text)),
            None
        );
        assert_eq!(
            SurroundingText::for_purpose(ImePurpose::Normal, Some(&text)),
            Some(surrounding("hunter2"))
        );
        assert_eq!(
            SurroundingText::for_purpose(ImePurpose::Terminal, None),
            None
        );
    }

    #[test]
    fn changes_say_why_and_a_field_that_stops_sharing_reenables() {
        let find = surrounding("Find");
        let found = surrounding("Find a voice");
        let change = SurroundingChange::between;
        assert_eq!(
            change(Some(&find), Some(&find), true),
            SurroundingChange::Unchanged
        );
        assert_eq!(change(None, None, false), SurroundingChange::Unchanged);
        assert_eq!(
            change(None, Some(&find), false),
            SurroundingChange::Send(ChangeCause::Other)
        );
        assert_eq!(
            change(Some(&find), Some(&found), true),
            SurroundingChange::Send(ChangeCause::InputMethod)
        );
        assert_eq!(
            change(Some(&found), None, false),
            SurroundingChange::Reenable
        );
    }
}
