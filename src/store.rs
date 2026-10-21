use std::env;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use atomic_write_file::AtomicWriteFile;
use thiserror::Error;
use toml_edit::{Array, DocumentMut, InlineTable, Item, Table, Value};

use crate::model::{Alias, AliasPatch, ValidationError};

const SCHEMA_VERSION: i64 = 1;
const EMPTY_CATALOG: &str = "version = 1\n\n[aliases]\n";

/// A file-backed alias catalog.
#[derive(Clone, Debug)]
pub struct Store {
    config_path: PathBuf,
    backup_path: PathBuf,
    lock_path: PathBuf,
}

impl Store {
    /// Resolves the catalog using `ALS_CONFIG_DIR`, XDG, then `~/.config`.
    pub fn discover() -> Result<Self, StoreError> {
        let directory = if let Some(path) = non_empty_env("ALS_CONFIG_DIR") {
            PathBuf::from(path)
        } else if let Some(path) = non_empty_env("XDG_CONFIG_HOME") {
            PathBuf::from(path).join("als")
        } else if let Some(home) = non_empty_env("HOME") {
            PathBuf::from(home).join(".config").join("als")
        } else {
            return Err(StoreError::ConfigDirectoryUnavailable);
        };

        Ok(Self::in_directory(directory))
    }

    /// Creates a store rooted at an explicit directory.
    pub fn in_directory(directory: impl Into<PathBuf>) -> Self {
        let directory = directory.into();
        Self {
            config_path: directory.join("aliases.toml"),
            backup_path: directory.join("aliases.toml.bak"),
            lock_path: directory.join("aliases.lock"),
        }
    }

    pub fn config_path(&self) -> &Path {
        &self.config_path
    }

    pub fn backup_path(&self) -> &Path {
        &self.backup_path
    }

    pub fn list(&self) -> Result<Vec<Alias>, StoreError> {
        let _lock = self.lock(false)?;
        let loaded = self.load_document()?;
        aliases_from_document(&loaded.document, &self.config_path)
    }

    pub fn add(&self, alias: Alias) -> Result<(), StoreError> {
        alias.validate()?;
        let _lock = self.lock(true)?;
        let mut loaded = self.load_document()?;

        let aliases = aliases_table_mut(&mut loaded.document, &self.config_path)?;
        if aliases.contains_key(&alias.name) {
            return Err(StoreError::AliasExists(alias.name));
        }
        aliases.insert(&alias.name, alias_item(&alias));

        self.save(loaded)
    }

    pub fn edit(&self, name: &str, patch: AliasPatch) -> Result<Alias, StoreError> {
        let patch = patch.normalized()?;
        if patch.is_empty() {
            return Err(StoreError::NoChanges);
        }

        let _lock = self.lock(true)?;
        let mut loaded = self.load_document()?;
        let aliases = aliases_table_mut(&mut loaded.document, &self.config_path)?;
        if !aliases.contains_key(name) {
            return Err(StoreError::AliasNotFound(name.to_owned()));
        }

        let updated_name = patch.name.as_deref().unwrap_or(name).to_owned();
        let renaming = updated_name != name;
        if renaming && aliases.contains_key(&updated_name) {
            return Err(StoreError::AliasExists(updated_name));
        }

        let updated = if renaming {
            let mut item = aliases
                .remove(name)
                .ok_or_else(|| StoreError::AliasNotFound(name.to_owned()))?;
            let updated = {
                let table = item.as_inline_table_mut().ok_or_else(|| {
                    invalid_config(
                        &self.config_path,
                        format!("alias `{name}` must be an inline table"),
                    )
                })?;
                apply_patch(table, &patch);
                alias_from_inline(&updated_name, table, &self.config_path)?
            };
            aliases.insert(&updated_name, item);
            updated
        } else {
            let item = aliases
                .get_mut(name)
                .ok_or_else(|| StoreError::AliasNotFound(name.to_owned()))?;
            let table = item.as_inline_table_mut().ok_or_else(|| {
                invalid_config(
                    &self.config_path,
                    format!("alias `{name}` must be an inline table"),
                )
            })?;
            apply_patch(table, &patch);
            alias_from_inline(name, table, &self.config_path)?
        };

        self.save(loaded)?;
        Ok(updated)
    }

    pub fn delete(&self, name: &str) -> Result<Alias, StoreError> {
        let _lock = self.lock(true)?;
        let mut loaded = self.load_document()?;

        let removed = {
            let aliases = aliases_table_mut(&mut loaded.document, &self.config_path)?;
            let item = aliases
                .get(name)
                .ok_or_else(|| StoreError::AliasNotFound(name.to_owned()))?;
            let table = item.as_inline_table().ok_or_else(|| {
                invalid_config(
                    &self.config_path,
                    format!("alias `{name}` must be an inline table"),
                )
            })?;
            alias_from_inline(name, table, &self.config_path)?
        };

        aliases_table_mut(&mut loaded.document, &self.config_path)?.remove(name);
        self.save(loaded)?;
        Ok(removed)
    }

    fn lock(&self, exclusive: bool) -> Result<CatalogLock, StoreError> {
        let directory = self
            .config_path
            .parent()
            .ok_or_else(|| invalid_config(&self.config_path, "catalog has no parent directory"))?;
        fs::create_dir_all(directory)
            .map_err(|error| io_error("create config directory", directory, error))?;

        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&self.lock_path)
            .map_err(|error| io_error("open lock file", &self.lock_path, error))?;

        if exclusive {
            file.lock()
        } else {
            file.lock_shared()
        }
        .map_err(|error| io_error("lock catalog", &self.lock_path, error))?;

        Ok(CatalogLock { _file: file })
    }

    fn load_document(&self) -> Result<LoadedDocument, StoreError> {
        if !self.config_path.exists() {
            return Ok(LoadedDocument {
                document: EMPTY_CATALOG
                    .parse()
                    .expect("default catalog is valid TOML"),
                original: None,
            });
        }

        let metadata = fs::symlink_metadata(&self.config_path)
            .map_err(|error| io_error("inspect catalog", &self.config_path, error))?;
        if metadata.file_type().is_symlink() {
            return Err(StoreError::SymlinkCatalog(self.config_path.clone()));
        }
        if !metadata.is_file() {
            return Err(invalid_config(&self.config_path, "catalog is not a file"));
        }

        let original = fs::read_to_string(&self.config_path)
            .map_err(|error| io_error("read catalog", &self.config_path, error))?;
        let document = original.parse::<DocumentMut>().map_err(|error| {
            invalid_config(&self.config_path, format!("TOML parse error: {error}"))
        })?;
        aliases_from_document(&document, &self.config_path)?;

        Ok(LoadedDocument {
            document,
            original: Some(original),
        })
    }

    fn save(&self, loaded: LoadedDocument) -> Result<(), StoreError> {
        if let Some(original) = loaded.original {
            atomic_write(&self.backup_path, original.as_bytes())?;
        }

        let mut contents = loaded.document.to_string();
        if !contents.ends_with('\n') {
            contents.push('\n');
        }
        atomic_write(&self.config_path, contents.as_bytes())
    }
}

struct CatalogLock {
    _file: File,
}

struct LoadedDocument {
    document: DocumentMut,
    original: Option<String>,
}

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("could not determine the config directory; set ALS_CONFIG_DIR or HOME")]
    ConfigDirectoryUnavailable,
    #[error("failed to {operation} at {}: {source}", path.display())]
    Io {
        operation: &'static str,
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("invalid catalog at {}: {message}", path.display())]
    InvalidConfig { path: PathBuf, message: String },
    #[error("alias `{0}` already exists")]
    AliasExists(String),
    #[error("alias `{0}` was not found")]
    AliasNotFound(String),
    #[error("no changes were provided")]
    NoChanges,
    #[error("refusing to replace symlinked catalog {}; set ALS_CONFIG_DIR to its real directory instead", .0.display())]
    SymlinkCatalog(PathBuf),
    #[error(transparent)]
    Validation(#[from] ValidationError),
}

fn aliases_from_document(document: &DocumentMut, path: &Path) -> Result<Vec<Alias>, StoreError> {
    let version = document
        .get("version")
        .and_then(Item::as_integer)
        .ok_or_else(|| invalid_config(path, "`version` must be an integer"))?;
    if version != SCHEMA_VERSION {
        return Err(invalid_config(
            path,
            format!("unsupported version {version}; expected {SCHEMA_VERSION}"),
        ));
    }

    let aliases = document
        .get("aliases")
        .and_then(Item::as_table)
        .ok_or_else(|| invalid_config(path, "`aliases` must be a table"))?;

    aliases
        .iter()
        .map(|(name, item)| {
            let table = item.as_inline_table().ok_or_else(|| {
                invalid_config(path, format!("alias `{name}` must be an inline table"))
            })?;
            alias_from_inline(name, table, path)
        })
        .collect()
}

fn alias_from_inline(name: &str, table: &InlineTable, path: &Path) -> Result<Alias, StoreError> {
    let cmd = table
        .get("cmd")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid_config(path, format!("alias `{name}` needs a string `cmd`")))?;

    let desc = match table.get("desc") {
        Some(value) => Some(
            value
                .as_str()
                .ok_or_else(|| {
                    invalid_config(path, format!("alias `{name}` has a non-string `desc`"))
                })?
                .to_owned(),
        ),
        None => None,
    };

    let tags = match table.get("tags") {
        Some(value) => value
            .as_array()
            .ok_or_else(|| invalid_config(path, format!("alias `{name}` has non-array `tags")))?
            .iter()
            .map(|tag| {
                tag.as_str().map(str::to_owned).ok_or_else(|| {
                    invalid_config(path, format!("alias `{name}` has a non-string tag"))
                })
            })
            .collect::<Result<Vec<_>, _>>()?,
        None => Vec::new(),
    };

    Alias::new(name, cmd, desc, tags)
        .map_err(|error| invalid_config(path, format!("alias `{name}` is invalid: {error}")))
}

fn aliases_table_mut<'a>(
    document: &'a mut DocumentMut,
    path: &Path,
) -> Result<&'a mut Table, StoreError> {
    document
        .get_mut("aliases")
        .and_then(Item::as_table_mut)
        .ok_or_else(|| invalid_config(path, "`aliases` must be a table"))
}

fn alias_item(alias: &Alias) -> Item {
    let mut table = InlineTable::new();
    table.insert("cmd", Value::from(alias.cmd.clone()));
    if let Some(desc) = &alias.desc {
        table.insert("desc", Value::from(desc.clone()));
    }
    if !alias.tags.is_empty() {
        table.insert("tags", Value::Array(tags_array(&alias.tags)));
    }
    Item::Value(Value::InlineTable(table))
}

fn apply_patch(table: &mut InlineTable, patch: &AliasPatch) {
    if let Some(cmd) = &patch.cmd {
        table.insert("cmd", Value::from(cmd.clone()));
    }
    if let Some(desc) = &patch.desc {
        match desc {
            Some(desc) => {
                table.insert("desc", Value::from(desc.clone()));
            }
            None => {
                table.remove("desc");
            }
        }
    }
    if let Some(tags) = &patch.tags {
        if tags.is_empty() {
            table.remove("tags");
        } else {
            table.insert("tags", Value::Array(tags_array(tags)));
        }
    }
}

fn tags_array(tags: &[String]) -> Array {
    let mut array = Array::new();
    for tag in tags {
        array.push(tag.as_str());
    }
    array
}

fn atomic_write(path: &Path, contents: &[u8]) -> Result<(), StoreError> {
    let mut file =
        AtomicWriteFile::open(path).map_err(|error| io_error("open atomic writer", path, error))?;
    file.write_all(contents)
        .map_err(|error| io_error("write file", path, error))?;
    file.commit()
        .map_err(|error| io_error("commit file", path, error))
}

fn non_empty_env(name: &str) -> Option<String> {
    env::var(name).ok().filter(|value| !value.trim().is_empty())
}

fn io_error(operation: &'static str, path: &Path, source: io::Error) -> StoreError {
    StoreError::Io {
        operation,
        path: path.to_owned(),
        source,
    }
}

fn invalid_config(path: &Path, message: impl Into<String>) -> StoreError {
    StoreError::InvalidConfig {
        path: path.to_owned(),
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::*;

    #[test]
    fn round_trip_crud_and_compact_format() {
        let directory = tempdir().unwrap();
        let store = Store::in_directory(directory.path());

        store
            .add(
                Alias::new(
                    "gs",
                    "git status",
                    Some("Git status".into()),
                    vec!["git".into()],
                )
                .unwrap(),
            )
            .unwrap();

        let contents = fs::read_to_string(store.config_path()).unwrap();
        assert!(
            contents
                .contains("gs = { cmd = \"git status\", desc = \"Git status\", tags = [\"git\"] }")
        );

        let updated = store
            .edit(
                "gs",
                AliasPatch {
                    name: None,
                    cmd: Some("git status --short".into()),
                    desc: None,
                    tags: Some(vec!["git".into(), "daily".into()]),
                },
            )
            .unwrap();
        assert_eq!(updated.cmd, "git status --short");
        assert_eq!(updated.desc.as_deref(), Some("Git status"));
        assert_eq!(updated.tags, ["git", "daily"]);
        assert!(store.backup_path().exists());

        let renamed = store
            .edit(
                "gs",
                AliasPatch {
                    name: Some("gst".into()),
                    ..AliasPatch::default()
                },
            )
            .unwrap();
        assert_eq!(renamed.name, "gst");
        assert_eq!(renamed.desc.as_deref(), Some("Git status"));
        assert_eq!(renamed.tags, ["git", "daily"]);

        let removed = store.delete("gst").unwrap();
        assert_eq!(removed.name, "gst");
        assert!(store.list().unwrap().is_empty());
    }

    #[test]
    fn preserves_comments_and_unknown_fields() {
        let directory = tempdir().unwrap();
        let store = Store::in_directory(directory.path());
        fs::write(
            store.config_path(),
            "version = 1\n\n[aliases]\n# favorite\ngs = { cmd = \"git status\", color = \"blue\" }\n",
        )
        .unwrap();

        store
            .edit(
                "gs",
                AliasPatch {
                    cmd: Some("git status --short".into()),
                    ..AliasPatch::default()
                },
            )
            .unwrap();

        let contents = fs::read_to_string(store.config_path()).unwrap();
        assert!(contents.contains("# favorite"));
        assert!(contents.contains("color = \"blue\""));
    }

    #[test]
    fn invalid_catalog_is_never_overwritten() {
        let directory = tempdir().unwrap();
        let store = Store::in_directory(directory.path());
        let invalid = "this is not TOML = [";
        fs::write(store.config_path(), invalid).unwrap();

        assert!(
            store
                .add(Alias::new("gs", "git status", None, vec![]).unwrap())
                .is_err()
        );
        assert_eq!(fs::read_to_string(store.config_path()).unwrap(), invalid);
        assert!(!store.backup_path().exists());
    }
}
