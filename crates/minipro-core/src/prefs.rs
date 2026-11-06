//! Per-user preferences shared across frontends — currently just
//! device favorites, stored as `favorites.txt` next to the database
//! cache in [`crate::database::user_data_dir`].
//!
//! Format: one `name<TAB>manufacturer` entry per line, `#` comments
//! allowed. Line-oriented rather than TOML/JSON on purpose: parsing
//! stays dependency-free, and a hand-edited file degrades gracefully
//! (malformed lines are skipped, never fatal).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::database::user_data_dir;

const FAVORITES_FILE: &str = "favorites.txt";

/// A favorited device entry. The manufacturer is captured at save
/// time so the row still renders when the device isn't in the
/// connected programmer's list (favorites are model-agnostic; the
/// T76's and T48's lists differ).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Favorite {
    pub name: String,
    pub manufacturer: String,
}

/// `user_data_dir()/favorites.txt`, or `None` when the platform data
/// dir can't be determined (minimal environments).
pub fn favorites_path() -> Option<PathBuf> {
    user_data_dir().map(|d| d.join(FAVORITES_FILE))
}

/// Parse `name<TAB>manufacturer` lines from `path`. Missing file →
/// empty. Blanks, comments, and empty-name lines are skipped — a
/// hand-edited file must not nuke all favorites.
pub fn load_favorites_from(path: &Path) -> Vec<Favorite> {
    let Ok(text) = fs::read_to_string(path) else {
        return Vec::new();
    };
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| {
            let (name, mfr) = l.split_once('\t').unwrap_or((l, ""));
            let name = name.trim();
            (!name.is_empty()).then(|| Favorite {
                name: name.to_string(),
                manufacturer: mfr.trim().to_string(),
            })
        })
        .collect()
}

/// Load favorites from the per-user data dir.
pub fn load_favorites() -> Vec<Favorite> {
    favorites_path()
        .map(|p| load_favorites_from(&p))
        .unwrap_or_default()
}

/// Write `path` atomically — tmp file + rename — so a crash mid-write
/// can't truncate the store. Creates the parent dir if needed.
pub fn save_favorites_to(path: &Path, favs: &[Favorite]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut text = String::from("# minipro-rs device favorites — name<TAB>manufacturer\n");
    for f in favs {
        text.push_str(&f.name);
        text.push('\t');
        text.push_str(&f.manufacturer);
        text.push('\n');
    }
    let tmp = path.with_extension("txt.tmp");
    fs::write(&tmp, text)?;
    fs::rename(&tmp, path)
}

/// Persist favorites to the per-user data dir. No-ops when the dir is
/// undeterminable — favorites stay session-scoped, same as before.
pub fn save_favorites(favs: &[Favorite]) -> io::Result<()> {
    match favorites_path() {
        Some(p) => save_favorites_to(&p, favs),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let dir = std::env::temp_dir().join(format!("minipro-prefs-test-{}", std::process::id()));
        let path = dir.join("favorites.txt");
        let favs = vec![
            Favorite {
                name: "W25X20CL@SOIC8".into(),
                manufacturer: "Winbond".into(),
            },
            Favorite {
                name: "ATTINY85V@DIP8".into(),
                manufacturer: "Microchip".into(),
            },
        ];
        save_favorites_to(&path, &favs).unwrap();
        assert_eq!(load_favorites_from(&path), favs);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn tolerant_parse() {
        let dir = std::env::temp_dir().join(format!("minipro-prefs-test2-{}", std::process::id()));
        let path = dir.join("f.txt");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            &path,
            "# comment\n\nW25X20CL@SOIC8\tWinbond\n\t\nNAME-ONLY\n  \n",
        )
        .unwrap();
        let favs = load_favorites_from(&path);
        assert_eq!(favs.len(), 2);
        assert_eq!(favs[0].name, "W25X20CL@SOIC8");
        assert_eq!(favs[0].manufacturer, "Winbond");
        assert_eq!(favs[1].name, "NAME-ONLY");
        assert_eq!(favs[1].manufacturer, "");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_file_is_empty() {
        assert!(load_favorites_from(Path::new("/definitely/not/here.txt")).is_empty());
    }
}
