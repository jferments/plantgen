//! The window's saved settings: how growing, the measures and the pictures
//! start, kept in one JSON file between runs.
//!
//! The file is `$XDG_CONFIG_HOME/plantlab/settings.json`, or
//! `~/.config/plantlab/settings.json` without it (`--settings FILE` names
//! another). A control that changes a saved setting saves that one setting
//! ([`Settings::save`]): it reads the file as it is now, changes that key
//! and keeps every other key, its own or not, as it was. It refuses to
//! write a file that would not load: when the file on disk does not load,
//! or the value is not one the key takes, nothing is written and the
//! window says why. Flags on the command line win over the file.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

/// One saved setting: its key in the file and the values it takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    /// The species chosen last, by id.
    Species,
    /// The species chosen lately, newest first.
    Recent,
    /// The library browser shows.
    LibraryOpen,
    /// Show the plant growing.
    ShowGrowing,
    /// Draw it every this many steps, 1 to 100.
    Every,
    /// Move the camera with the plant as it grows.
    Follow,
    /// The grid on the ground.
    Grid,
    /// The ruler beside the plant.
    Ruler,
    /// Pictures of the plant alone, not the scene.
    PlantOnly,
    /// Pictures' width, 128 to 1920 pixels.
    PictureWidth,
    /// An animation's frame, 20 to 2000 milliseconds.
    FrameMs,
    /// How long an animation holds its last frame, 0 to 10 seconds.
    HoldS,
    /// Save an animation's frames as PNGs too.
    Pngs,
}

impl Key {
    pub const ALL: [Self; 13] = [
        Self::Species,
        Self::Recent,
        Self::LibraryOpen,
        Self::ShowGrowing,
        Self::Every,
        Self::Follow,
        Self::Grid,
        Self::Ruler,
        Self::PlantOnly,
        Self::PictureWidth,
        Self::FrameMs,
        Self::HoldS,
        Self::Pngs,
    ];

    /// Its name in the file.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Species => "species",
            Self::Recent => "recent_species",
            Self::LibraryOpen => "library_open",
            Self::ShowGrowing => "show_growing",
            Self::Every => "every_steps",
            Self::Follow => "follow",
            Self::Grid => "grid",
            Self::Ruler => "ruler",
            Self::PlantOnly => "pictures_of_plant_alone",
            Self::PictureWidth => "picture_width_px",
            Self::FrameMs => "frame_ms",
            Self::HoldS => "hold_s",
            Self::Pngs => "pngs",
        }
    }

    /// Whether `value` is one this key takes.
    #[must_use]
    pub fn takes(self, value: &Value) -> bool {
        let within = |low: f64, high: f64| {
            value
                .as_f64()
                .is_some_and(|number| number.is_finite() && (low..=high).contains(&number))
        };
        let whole = |low: u64, high: u64| value.as_u64().is_some_and(|n| (low..=high).contains(&n));
        match self {
            Self::Species => value.as_str().is_some_and(|id| !id.is_empty()),
            Self::Recent => value
                .as_array()
                .is_some_and(|ids| ids.len() <= 32 && ids.iter().all(Value::is_string)),
            Self::LibraryOpen
            | Self::ShowGrowing
            | Self::Follow
            | Self::Grid
            | Self::Ruler
            | Self::PlantOnly
            | Self::Pngs => value.is_boolean(),
            Self::Every => whole(1, 100),
            Self::PictureWidth => whole(128, 1920),
            Self::FrameMs => whole(20, 2000),
            Self::HoldS => within(0.0, 10.0),
        }
    }
}

/// The settings file and what it held when last read.
#[derive(Debug, Clone, Default)]
pub struct Settings {
    /// Where it is; `None` when no folder for it is known.
    pub path: Option<PathBuf>,
    /// The file's keys, as read.
    values: Map<String, Value>,
    /// Why the file was not read or a setting was not saved; empty when
    /// all went well.
    pub message: String,
}

impl Settings {
    /// Where the settings live: `$XDG_CONFIG_HOME/plantlab/settings.json`,
    /// else `$HOME/.config/plantlab/settings.json`.
    #[must_use]
    pub fn default_path() -> Option<PathBuf> {
        let config = std::env::var_os("XDG_CONFIG_HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME")
                    .filter(|value| !value.is_empty())
                    .map(|home| PathBuf::from(home).join(".config"))
            })?;
        Some(config.join("plantlab").join("settings.json"))
    }

    /// Reads the file at `path`. A missing file is no settings; a file that
    /// does not load, or a key whose value it does not take, is left out
    /// and said in [`Settings::message`].
    #[must_use]
    pub fn load(path: Option<PathBuf>) -> Self {
        let mut settings = Self {
            path,
            ..Self::default()
        };
        let Some(path) = settings.path.clone() else {
            return settings;
        };
        match read(&path) {
            Ok(values) => {
                let wrong: Vec<&str> = Key::ALL
                    .iter()
                    .filter(|key| {
                        values
                            .get(key.name())
                            .is_some_and(|value| !key.takes(value))
                    })
                    .map(|key| key.name())
                    .collect();
                if !wrong.is_empty() {
                    settings.message = format!(
                        "{} has values PlantLab cannot use for {}; they are left out",
                        path.display(),
                        wrong.join(", ")
                    );
                }
                settings.values = values;
            }
            Err(error) => settings.message = error,
        }
        settings
    }

    /// The saved value of `key`, when the file has one this key takes.
    #[must_use]
    pub fn get(&self, key: Key) -> Option<&Value> {
        self.values.get(key.name()).filter(|value| key.takes(value))
    }

    #[must_use]
    pub fn bool(&self, key: Key) -> Option<bool> {
        self.get(key).and_then(Value::as_bool)
    }

    #[must_use]
    pub fn u32(&self, key: Key) -> Option<u32> {
        self.get(key)
            .and_then(Value::as_u64)
            .and_then(|n| u32::try_from(n).ok())
    }

    #[must_use]
    pub fn f64(&self, key: Key) -> Option<f64> {
        self.get(key).and_then(Value::as_f64)
    }

    #[must_use]
    pub fn string(&self, key: Key) -> Option<&str> {
        self.get(key).and_then(Value::as_str)
    }

    /// A list of strings; empty where the file has none.
    #[must_use]
    pub fn strings(&self, key: Key) -> Vec<String> {
        self.get(key)
            .and_then(Value::as_array)
            .map(|values| {
                values
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Saves `value` as `key`: reads the file as it is now, sets that key,
    /// keeps every other as it was, and writes the file through a
    /// temporary one, so a reader never sees half of it. Refuses, writing
    /// nothing, when the value is not one the key takes or the file on
    /// disk does not load.
    ///
    /// # Errors
    ///
    /// Why nothing was written.
    pub fn save(&mut self, key: Key, value: Value) -> Result<(), String> {
        let result = self.write(key, value);
        match &result {
            Ok(()) => self.message.clear(),
            Err(error) => self.message.clone_from(error),
        }
        result
    }

    fn write(&mut self, key: Key, value: Value) -> Result<(), String> {
        if !key.takes(&value) {
            return Err(format!("{value} is not a value for `{}`", key.name()));
        }
        let path = self
            .path
            .clone()
            .ok_or("no folder for the settings is known (set HOME or XDG_CONFIG_HOME)")?;
        let mut values = if path.exists() {
            read(&path).map_err(|error| format!("{error}; not saved"))?
        } else {
            Map::new()
        };
        values.insert(key.name().to_owned(), value);
        let text = serde_json::to_string_pretty(&Value::Object(values.clone()))
            .map_err(|error| error.to_string())?;
        // What is written must load again as the same keys.
        if serde_json::from_str::<Map<String, Value>>(&text)
            .ok()
            .as_ref()
            != Some(&values)
        {
            return Err("the settings would not load again; not saved".into());
        }
        if let Some(folder) = path.parent() {
            std::fs::create_dir_all(folder)
                .map_err(|error| format!("cannot make {}: {error}", folder.display()))?;
        }
        let temporary = path.with_extension("json.new");
        std::fs::write(&temporary, format!("{text}\n"))
            .and_then(|()| std::fs::rename(&temporary, &path))
            .map_err(|error| format!("cannot write {}: {error}", path.display()))?;
        self.values = values;
        Ok(())
    }
}

/// The file at `path` as a JSON object.
fn read(path: &Path) -> Result<Map<String, Value>, String> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Map::new()),
        Err(error) => return Err(format!("cannot read {}: {error}", path.display())),
    };
    serde_json::from_str(&text).map_err(|error| {
        format!(
            "{} does not load ({error}); fix or remove it",
            path.display()
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn folder(name: &str) -> PathBuf {
        let folder =
            std::env::temp_dir().join(format!("plantlab-settings-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        folder
    }

    #[test]
    fn saving_one_setting_keeps_every_other_key() {
        let folder = folder("keep");
        let path = folder.join("plantlab").join("settings.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            r#"{"grid": false, "someone_elses": [1, 2], "frame_ms": 100}"#,
        )
        .unwrap();
        let mut settings = Settings::load(Some(path.clone()));
        assert_eq!(settings.bool(Key::Grid), Some(false));
        settings.save(Key::Ruler, json!(false)).unwrap();
        let written: Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            written,
            json!({"grid": false, "someone_elses": [1, 2], "frame_ms": 100, "ruler": false})
        );
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn a_file_that_does_not_load_is_never_overwritten() {
        let folder = folder("broken");
        let path = folder.join("settings.json");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(&path, "{\"grid\": tru").unwrap();
        let mut settings = Settings::load(Some(path.clone()));
        assert!(settings.message.contains("does not load"));
        assert!(settings.save(Key::Grid, json!(true)).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{\"grid\": tru");
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn a_value_a_key_does_not_take_is_refused_and_ignored() {
        let folder = folder("range");
        let path = folder.join("settings.json");
        let mut settings = Settings::load(Some(path.clone()));
        assert!(settings.save(Key::PictureWidth, json!(5000)).is_err());
        assert!(settings.save(Key::Grid, json!("yes")).is_err());
        assert!(!path.exists());
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(&path, r#"{"every_steps": 0, "grid": true}"#).unwrap();
        let settings = Settings::load(Some(path));
        assert_eq!(settings.u32(Key::Every), None);
        assert_eq!(settings.bool(Key::Grid), Some(true));
        assert!(settings.message.contains("every_steps"));
        let _ = std::fs::remove_dir_all(&folder);
    }
}
