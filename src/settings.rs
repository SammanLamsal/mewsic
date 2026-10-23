use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::queues::{read_json, write_json};

/// User preferences edited on the settings page.
/// `serde(default)` lets files written before a field existed still load.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// MPD's music directory as seen from this machine, used to reach the files behind MPD URIs.
    pub music_dir: Option<PathBuf>,
}

pub struct SettingsStore {
    file: PathBuf,
}

impl SettingsStore {
    pub fn new(config_dir: &Path) -> SettingsStore {
        SettingsStore {
            file: config_dir.join("settings.json"),
        }
    }

    pub fn load(&self) -> Settings {
        read_json(&self.file).unwrap_or_default()
    }

    pub fn save(&self, settings: &Settings) -> anyhow::Result<()> {
        write_json(&self.file, settings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_fields_fall_back_to_defaults() {
        let settings: Settings = serde_json::from_str("{}").unwrap();
        assert_eq!(settings, Settings::default());
    }
}
