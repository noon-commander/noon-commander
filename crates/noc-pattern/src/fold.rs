//! Text in the form it is compared in: NFC, as names typed are, whatever form the file system
//! keeps (macOS keeps NFD), and case folded, as Unicode compares text ignoring case: `ß` as
//! `ss`, every sigma as one.

use std::borrow::Cow;
use std::ops::Range;

use icu_casemap::{CaseMapper, CaseMapperBorrowed};
use icu_normalizer::{ComposingNormalizer, ComposingNormalizerBorrowed};
use unicode_segmentation::UnicodeSegmentation as _;

const NFC: ComposingNormalizerBorrowed<'static> = ComposingNormalizer::new_nfc();
const CASE: CaseMapperBorrowed<'static> = CaseMapper::new();

/// `text` in NFC.
pub fn nfc(text: &str) -> Cow<'_, str> {
    if text.is_ascii() {
        return Cow::Borrowed(text);
    }
    NFC.normalize(text)
}

/// `text` in NFC and case folded, to compare ignoring case and form.
pub fn fold(text: &str) -> Cow<'_, str> {
    if text.is_ascii() {
        return if text.bytes().any(|byte| byte.is_ascii_uppercase()) {
            Cow::Owned(text.to_ascii_lowercase())
        } else {
            Cow::Borrowed(text)
        };
    }
    // Folding may leave text that is no longer in NFC.
    Cow::Owned(
        NFC.normalize(&CASE.fold_string(&NFC.normalize(text)))
            .into_owned(),
    )
}

/// Text in the form it is compared in, with the characters of the text it came from that each
/// of its characters stands for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Normalized {
    /// The text, in NFC, folded or not.
    pub text: String,
    /// For each character of `text`, the characters of the original text it stands for: those
    /// of the grapheme it is part of.
    from: Vec<Range<usize>>,
}

impl Normalized {
    /// `text` in NFC.
    pub fn nfc(text: &str) -> Self {
        Self::new(text, nfc)
    }

    /// `text` in NFC and case folded.
    pub fn folded(text: &str) -> Self {
        Self::new(text, fold)
    }

    /// Each grapheme of `text` as `form` writes it: normalization never joins characters of two
    /// graphemes.
    fn new(text: &str, form: fn(&str) -> Cow<'_, str>) -> Self {
        let mut normalized = String::with_capacity(text.len());
        let mut from = Vec::with_capacity(text.len());
        let mut start = 0;
        for grapheme in text.graphemes(true) {
            let end = start + grapheme.chars().count();
            for c in form(grapheme).chars() {
                normalized.push(c);
                from.push(start..end);
            }
            start = end;
        }
        Self {
            text: normalized,
            from,
        }
    }

    /// The characters of the original text that the characters `chars` of [`Self::text`] stand
    /// for, in order, each once.
    pub fn original(&self, chars: impl IntoIterator<Item = usize>) -> Vec<usize> {
        let mut original: Vec<usize> = chars
            .into_iter()
            .filter_map(|index| self.from.get(index).cloned())
            .flatten()
            .collect();
        original.sort_unstable();
        original.dedup();
        original
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn either_form_and_case_compare_the_same() {
        assert_eq!(fold("Cafe\u{301}"), fold("CAF\u{c9}"));
        assert_eq!(fold("Stra\u{df}e"), fold("STRASSE"));
        assert_eq!(fold("\u{3a3}\u{391}\u{3a3}"), fold("\u{3c3}\u{3b1}\u{3c2}"));
        assert_eq!(fold("ReadMe"), "readme");
        assert!(matches!(fold("plain"), Cow::Borrowed("plain")));
        assert_eq!(nfc("e\u{301}"), "\u{e9}");
        assert!(matches!(nfc("ascii"), Cow::Borrowed(_)));
    }

    #[test]
    fn normalized_characters_point_back_at_their_graphemes() {
        // Two characters in NFD that NFC joins: both stand behind the one.
        let text = "xe\u{301}y";
        let normalized = Normalized::nfc(text);
        assert_eq!(normalized.text, "x\u{e9}y");
        assert_eq!(normalized.original([1]), [1, 2]);
        assert_eq!(normalized.original([0, 2]), [0, 3]);
        // One character that folding makes two: either stands for it.
        let folded = Normalized::folded("A\u{df}");
        assert_eq!(folded.text, "ass");
        assert_eq!(folded.original([1]), [1]);
        assert_eq!(folded.original([2, 0]), [0, 1]);
        assert_eq!(folded.original([9]), Vec::<usize>::new());
    }
}
