//! The viewer's text: Fluent messages from `i18n/`, read with [`fl!`].

use std::sync::LazyLock;

use i18n_embed::LanguageLoader as _;
use i18n_embed::fluent::{FluentLanguageLoader, fluent_language_loader};
use i18n_embed::unic_langid::LanguageIdentifier;
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "i18n/"]
struct Localizations;

/// The loader behind [`fl!`]. Starts with the built-in English messages.
pub(crate) static LOADER: LazyLock<FluentLanguageLoader> = LazyLock::new(|| {
    let loader = fluent_language_loader!();
    if let Err(error) = loader.load_fallback_language(&Localizations) {
        tracing::error!(%error, "cannot load the built-in messages of the viewer");
    }
    plain_arguments(&loader);
    loader
});

/// Fluent wraps arguments in Unicode bidi isolation marks, which a terminal shows as garbage.
/// The setting applies to loaded bundles only, so it is renewed after every load.
fn plain_arguments(loader: &FluentLanguageLoader) {
    loader.set_use_isolating(false);
}

/// Selects the language of the viewer's text from `requested`, the most wanted first, as the
/// app selects its own; languages without translations fall back to English. Call it before
/// the viewer first shows.
pub fn select_language(requested: &[LanguageIdentifier]) {
    select_into(&LOADER, requested);
}

fn select_into(loader: &FluentLanguageLoader, requested: &[LanguageIdentifier]) {
    match i18n_embed::select(loader, &Localizations, requested) {
        Ok(selected) => tracing::debug!(?requested, ?selected, "viewer language"),
        Err(error) => tracing::warn!(%error, "cannot load the viewer language"),
    }
    plain_arguments(loader);
}

/// A localized message, checked against the English messages at compile time.
macro_rules! fl {
    ($id:literal) => {
        i18n_embed_fl::fl!($crate::i18n::LOADER, $id)
    };
    ($id:literal, $($args:tt)*) => {
        i18n_embed_fl::fl!($crate::i18n::LOADER, $id, $($args)*)
    };
}
pub(crate) use fl;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arguments_have_no_isolation_marks() {
        assert_eq!(
            fl!("viewer-position", line = "1", lines = "20", percent = "25"),
            "1/20 25%"
        );
    }

    #[test]
    fn unknown_languages_fall_back_to_english() {
        for language in ["fr-FR", "en-US"] {
            let loader: FluentLanguageLoader = fluent_language_loader!();
            select_into(&loader, &[language.parse().unwrap()]);
            assert_eq!(
                i18n_embed_fl::fl!(loader, "viewer-loading"),
                "Loading…",
                "{language}"
            );
        }
    }
}
