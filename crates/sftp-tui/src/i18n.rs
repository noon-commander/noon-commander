//! User-facing TUI text: Fluent messages from `i18n/`, read with [`fl!`].

use std::sync::LazyLock;

use i18n_embed::fluent::{FluentLanguageLoader, fluent_language_loader};
use i18n_embed::unic_langid::LanguageIdentifier;
use i18n_embed::{DesktopLanguageRequester, LanguageLoader as _};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "i18n/"]
struct Localizations;

/// The loader behind [`fl!`]. Starts with the built-in English messages.
pub(crate) static LOADER: LazyLock<FluentLanguageLoader> = LazyLock::new(|| {
    let loader = fluent_language_loader!();
    if let Err(error) = loader.load_fallback_language(&Localizations) {
        tracing::error!(%error, "cannot load the built-in messages");
    }
    plain_arguments(&loader);
    loader
});

/// Fluent wraps arguments in Unicode bidi isolation marks, which a terminal shows as garbage.
/// The setting applies to loaded bundles only, so it is renewed after every load.
fn plain_arguments(loader: &FluentLanguageLoader) {
    loader.set_use_isolating(false);
}

/// Whether `language` is valid for `ui.language`: `auto`, or a language tag such as `en-US`.
pub(crate) fn is_valid_language(language: &str) -> bool {
    language == "auto" || language.parse::<LanguageIdentifier>().is_ok()
}

/// Selects the UI language; `auto` follows the system locale. Languages without translations
/// fall back to English. Call it before the first message is shown.
pub(crate) fn select(language: &str) {
    select_into(&LOADER, language);
}

fn select_into(loader: &FluentLanguageLoader, language: &str) {
    let requested = if language == "auto" {
        DesktopLanguageRequester::requested_languages()
    } else {
        language.parse().into_iter().collect()
    };
    match i18n_embed::select(loader, &Localizations, &requested) {
        Ok(selected) => tracing::debug!(?requested, ?selected, "UI language"),
        Err(error) => tracing::warn!(%error, "cannot load the UI language"),
    }
    plain_arguments(loader);
}

/// A localized message, checked against the English messages at compile time:
/// `fl!("tui-not-implemented", program = "sftp-tui")`.
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

    const NOT_IMPLEMENTED: &str = "st: the TUI is not implemented yet; try `st hosts` or \
                                   `st ls <host>:<path>`";

    #[test]
    fn arguments_have_no_isolation_marks() {
        assert_eq!(fl!("tui-not-implemented", program = "st"), NOT_IMPLEMENTED);
    }

    #[test]
    fn validates_languages() {
        assert!(is_valid_language("auto"));
        assert!(is_valid_language("en-US"));
        assert!(is_valid_language("de"));
        assert!(!is_valid_language("not a tag"));
        assert!(!is_valid_language(""));
    }

    #[test]
    fn unknown_languages_fall_back_to_english() {
        for language in ["fr-FR", "en-US", "auto"] {
            let loader: FluentLanguageLoader = fluent_language_loader!();
            select_into(&loader, language);
            assert_eq!(
                i18n_embed_fl::fl!(loader, "tui-not-implemented", program = "st"),
                NOT_IMPLEMENTED,
                "{language}"
            );
        }
    }
}
