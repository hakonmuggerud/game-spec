//! Reading and re-rendering the RON tables the editor touches (`zones.ron`, `npcs.ron`). Each file is a one-line
//! `// name.ron — …` header comment followed by the pretty-printed value; a save keeps the header and reprints
//! the value with the same `PrettyConfig` the files were written with, so an unchanged table saves byte-identical
//! (`round_trip_is_byte_identical` below pins that). Comments inside the body are not preserved. The writing
//! itself is `save.rs`'s job (atomic, all-or-nothing).

use ron::ser::PrettyConfig;
use serde::{de::DeserializeOwned, Serialize};
use std::path::Path;

/// The pretty printer the data files use: two-space indent, tuples inline, arrays and maps one entry per line.
pub fn pretty() -> PrettyConfig {
    PrettyConfig::new().indentor("  ")
}

/// Split a data file into its header comment lines and the RON body.
pub fn split_header(text: &str) -> (String, &str) {
    let mut header = String::new();
    let mut rest = text;
    while rest.starts_with("//") {
        let end = rest.find('\n').map(|i| i + 1).unwrap_or(rest.len());
        header.push_str(&rest[..end]);
        rest = &rest[end..];
    }
    (header, rest)
}

/// Serialise `value` under the file's existing header.
pub fn render<T: Serialize>(header: &str, value: &T) -> Result<String, ron::Error> {
    let body = ron::ser::to_string_pretty(value, pretty())?;
    Ok(format!("{header}{body}\n"))
}

/// Read a table and its header.
pub fn read<T: DeserializeOwned>(path: &Path) -> Result<(String, T), String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let (header, body) = split_header(&text);
    let value = ron::from_str(body).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok((header, value))
}

#[cfg(test)]
mod tests {
    use super::*;
    use undercroft_data::{GameData, NpcTable, ZoneDef};

    fn round_trip<T: DeserializeOwned + Serialize>(name: &str) {
        let path = GameData::workspace_data_dir().join(name);
        let text = std::fs::read_to_string(&path).expect("read");
        let (header, value): (String, T) = read(&path).expect("parse");
        assert!(header.starts_with("// "), "{name}: header comment");
        let again = render(&header, &value).expect("render");
        assert_eq!(again, text, "{name}: rewriting the file is byte-identical");
    }

    #[test]
    fn round_trip_is_byte_identical() {
        round_trip::<Vec<ZoneDef>>("zones.ron");
        round_trip::<NpcTable>("npcs.ron");
    }
}
