//! Changes to `config.toml` from the TUI, which keep the user's comments and formatting.

use std::fs;
use std::io;
use std::path::Path;

use toml_edit::{Array, DocumentMut, InlineTable, Item, Table, Value};

use crate::write::write_atomic;
use crate::{Config, ConfigError};

/// Writes to the file at `path` the settings that differ between `old`, what the TUI showed,
/// and `new`, what it changed them to; every other key stays as the file has it, so changes
/// made to the file meanwhile are kept. A changed value keeps the comments around it, a new key
/// goes at the end of its table, and a key that is no longer set is removed. A value back at
/// its default is written as such where the file sets the key, and left out where it does not.
/// A missing file is created; one that is not valid TOML, or that the changes would make
/// invalid, is left alone. The file is replaced atomically, through a symbolic link if it is
/// one.
///
/// Blocking: call it from a blocking thread.
pub fn save_config(path: &Path, old: &Config, new: &Config) -> Result<(), ConfigError> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => String::new(),
        Err(source) => {
            return Err(ConfigError::Read {
                path: path.to_path_buf(),
                source,
            });
        }
    };
    let mut document: DocumentMut = text.parse().map_err(|source| ConfigError::Edit {
        path: path.to_path_buf(),
        source: Box::new(source),
    })?;
    let [old, new, default] = [old, new, &Config::default()].map(table_of);
    let mut changed = false;
    for (section, new_section) in &new {
        let Some(new_section) = new_section.as_table() else {
            continue;
        };
        let old_section = old.get(section).and_then(toml::Value::as_table);
        let default_section = default.get(section).and_then(toml::Value::as_table);
        let keys = new_section
            .keys()
            .chain(old_section.into_iter().flat_map(toml::Table::keys));
        let mut done = Vec::new();
        for key in keys {
            if done.contains(&key) {
                continue;
            }
            done.push(key);
            let value = new_section.get(key);
            if value == old_section.and_then(|old| old.get(key)) {
                continue;
            }
            let is_default = value == default_section.and_then(|default| default.get(key));
            changed |= set(&mut document, section, key, value, is_default);
        }
    }
    if !changed {
        return Ok(());
    }
    let text = document.to_string();
    Config::from_toml(&text, path)?;
    write_atomic(path, text.as_bytes())
}

/// The settings as a TOML table of sections.
fn table_of(config: &Config) -> toml::Table {
    toml::Table::try_from(config).unwrap_or_default()
}

/// Sets `section.key` to `value` in `document`, or removes it for `None`; a default value is
/// only written over a key that is there. Returns whether the document changed.
fn set(
    document: &mut DocumentMut,
    section: &str,
    key: &str,
    value: Option<&toml::Value>,
    is_default: bool,
) -> bool {
    let present = document
        .get(section)
        .and_then(Item::as_table_like)
        .and_then(|table| table.get(key))
        .is_some();
    let Some(value) = value else {
        if let Some(table) = document.get_mut(section).and_then(Item::as_table_like_mut) {
            table.remove(key);
        }
        return present;
    };
    if is_default && !present {
        return false;
    }
    if document
        .get(section)
        .is_none_or(|item| !item.is_table_like())
    {
        document.insert(section, Item::Table(Table::new()));
    }
    let Some(table) = document.get_mut(section).and_then(Item::as_table_like_mut) else {
        return false;
    };
    let mut value = to_edit(value);
    // In place, so that the comments above the key, which belong to it, stay.
    match table.get_mut(key) {
        Some(item) => {
            if let Some(old) = item.as_value() {
                *value.decor_mut() = old.decor().clone();
            }
            *item = Item::Value(value);
        }
        None => {
            table.insert(key, Item::Value(value));
        }
    }
    true
}

/// A value as `toml_edit` writes it.
fn to_edit(value: &toml::Value) -> Value {
    match value {
        toml::Value::String(text) => Value::from(text.as_str()),
        toml::Value::Integer(number) => Value::from(*number),
        toml::Value::Float(number) => Value::from(*number),
        toml::Value::Boolean(flag) => Value::from(*flag),
        toml::Value::Datetime(time) => Value::from(time.to_string()),
        toml::Value::Array(values) => Value::Array(values.iter().map(to_edit).collect::<Array>()),
        toml::Value::Table(table) => {
            let mut inline = InlineTable::new();
            for (key, value) in table {
                inline.insert(key, to_edit(value));
            }
            Value::InlineTable(inline)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::{Borders, DEFAULT_CONFIG, MenuBar};

    fn changed(change: impl FnOnce(&mut Config)) -> Config {
        let mut config = Config::default();
        change(&mut config);
        config
    }

    #[test]
    fn writes_changed_keys_and_keeps_comments_and_other_keys() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(
            &path,
            "# Mine\n[ui]\n# Colors\ntheme = \"mc-classic\"  # the default\nicons = false\n\n\
             [ssh]\nmultiplex = false\n",
        )
        .unwrap();
        let old = changed(|config| config.ui.icons = false);
        let new = changed(|config| {
            config.ui.icons = false;
            config.ui.theme = "terminal".to_owned();
            config.ui.menu_bar = MenuBar::Always;
        });
        save_config(&path, &old, &new).unwrap();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "# Mine\n[ui]\n# Colors\ntheme = \"terminal\"  # the default\nicons = false\n\
             menu_bar = \"always\"\n\n[ssh]\nmultiplex = false\n",
            "ssh.multiplex differs from what the TUI had and stays"
        );
    }

    #[test]
    fn a_default_value_is_written_only_over_a_key_that_is_there() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, "[ui]\nborders = \"single\"\n").unwrap();
        let old = changed(|config| {
            config.ui.borders = Borders::Single;
            config.ui.icons = false;
        });
        save_config(&path, &old, &Config::default()).unwrap();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "[ui]\nborders = \"double\"\n"
        );
    }

    #[test]
    fn creates_a_missing_file_and_its_table() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("noc/config.toml");
        let new = changed(|config| config.ui.show_hidden = false);
        save_config(&path, &Config::default(), &new).unwrap();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "[ui]\nshow_hidden = false\n"
        );
        assert_eq!(Config::load(&path, dir.path()).unwrap(), new);
    }

    #[test]
    fn nothing_changed_writes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        save_config(&path, &Config::default(), &Config::default()).unwrap();
        assert!(!path.exists());
    }

    #[test]
    fn the_commented_defaults_keep_their_comments() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, DEFAULT_CONFIG).unwrap();
        let new = changed(|config| config.ui.theme = "terminal".to_owned());
        save_config(&path, &Config::default(), &new).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert_eq!(
            text,
            DEFAULT_CONFIG.replace("theme = \"mc-classic\"", "theme = \"terminal\"")
        );
    }

    #[test]
    fn leaves_a_broken_file_alone() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, "[ui\n").unwrap();
        let new = changed(|config| config.ui.icons = false);
        assert!(matches!(
            save_config(&path, &Config::default(), &new),
            Err(ConfigError::Edit { .. })
        ));
        // Valid TOML that the schema rejects stays too.
        fs::write(&path, "[ui]\ncolour = true\n").unwrap();
        assert!(matches!(
            save_config(&path, &Config::default(), &new),
            Err(ConfigError::Parse { .. })
        ));
        assert_eq!(fs::read_to_string(&path).unwrap(), "[ui]\ncolour = true\n");
    }
}
