//! What the window draws next and what it last drew: the [`Recipe`] (the
//! settings a render is made with), and the last render of each species,
//! kept on disk so the window can show it before anything grows.
//!
//! Each species' last render is a picture of the window's scene and the
//! recipe it was made with, `ID.png` and `ID.json` in
//! `$XDG_CACHE_HOME/plantlab/renders` (`~/.cache/plantlab/renders`
//! without it). Choosing a species shows its picture and takes up its
//! recipe; a render replaces both. A cache: deleting the folder only
//! forgets the pictures.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use plantgen::quality::{self, Quality};
use plantgen::spec::Environment;
use plantlab_scene::{Look, View};
use serde_json::{Value, json};

/// How much detail a render draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Detail {
    /// The draft meshes: fast, for trying settings.
    Draft,
    /// The standard meshes, as library packages hold them.
    Standard,
    /// The standard meshes with every flower, fruit, cone and needle shoot
    /// as its part mesh on the nearest level, as full-quality exports and
    /// `plantc render --parts on` draw them.
    Full,
}

impl Detail {
    pub const ALL: [Self; 3] = [Self::Draft, Self::Standard, Self::Full];

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Standard => "standard",
            Self::Full => "full",
        }
    }

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Draft => "Draft",
            Self::Standard => "Standard",
            Self::Full => "Full",
        }
    }

    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|detail| detail.name() == name)
    }

    /// Its meshes' quality profile.
    #[must_use]
    pub const fn quality(self) -> Quality {
        match self {
            Self::Draft => quality::DRAFT,
            Self::Standard | Self::Full => quality::STANDARD,
        }
    }

    /// Whether it draws part meshes.
    #[must_use]
    pub const fn parts(self) -> bool {
        matches!(self, Self::Full)
    }
}

/// The settings a render is made with. The species, site, seed and age
/// decide the growth; the day, detail and level only how the grown plant
/// is drawn, so changing them draws it again without growing it; the
/// look and the view only how it is shown, so they change at once.
#[derive(Debug, Clone, PartialEq)]
pub struct Recipe {
    /// Years; `None` for the species' oldest keyframe.
    pub age: Option<f64>,
    pub day: f64,
    pub detail: Detail,
    pub level: usize,
    /// `None` for the species' typical site.
    pub environment: Option<Environment>,
    /// `None` for the spec's first seed.
    pub seed: Option<u64>,
    pub look: Look,
    pub view: View,
}

impl Default for Recipe {
    fn default() -> Self {
        Self {
            age: None,
            day: plantgen::package::DEFAULT_DAY,
            detail: Detail::Draft,
            level: 0,
            environment: None,
            seed: None,
            look: Look::Review,
            view: View::ThreeQuarter,
        }
    }
}

/// One setting of a [`Recipe`], for saying what changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Setting {
    Age,
    Site,
    Variant,
    Day,
    Detail,
    Level,
}

impl Setting {
    #[must_use]
    pub const fn word(self) -> &'static str {
        match self {
            Self::Age => "age",
            Self::Site => "site",
            Self::Variant => "variant",
            Self::Day => "day",
            Self::Detail => "detail",
            Self::Level => "level of detail",
        }
    }

    /// Whether a change to it grows the plant again, rather than only
    /// drawing the grown one again.
    #[must_use]
    pub const fn regrows(self) -> bool {
        matches!(self, Self::Age | Self::Site | Self::Variant)
    }
}

impl Recipe {
    /// The settings that differ between `self` and `other`, the ones that
    /// grow the plant again first. The look and view are left out: they
    /// change what is on screen at once.
    #[must_use]
    pub fn changes(&self, other: &Self) -> Vec<Setting> {
        let mut changed = Vec::new();
        let near = |a: Option<f64>, b: Option<f64>| match (a, b) {
            (Some(a), Some(b)) => (a - b).abs() < 1e-6,
            (a, b) => a.is_none() && b.is_none(),
        };
        if !near(self.age, other.age) {
            changed.push(Setting::Age);
        }
        if self.environment != other.environment {
            changed.push(Setting::Site);
        }
        if self.seed != other.seed {
            changed.push(Setting::Variant);
        }
        if (self.day - other.day).abs() > 1e-6 {
            changed.push(Setting::Day);
        }
        if self.detail != other.detail {
            changed.push(Setting::Detail);
        }
        if self.level != other.level {
            changed.push(Setting::Level);
        }
        changed
    }

    /// As JSON, for the kept render's file.
    #[must_use]
    pub fn to_json(&self) -> Value {
        json!({
            "age": self.age,
            "day": self.day,
            "detail": self.detail.name(),
            "level": self.level,
            "environment": self.environment.map(Environment::name),
            "seed": self.seed,
            "look": self.look.name(),
            "view": self.view.name(),
        })
    }

    /// Read back from [`Recipe::to_json`]; what is missing or not
    /// understood keeps its default.
    #[must_use]
    pub fn from_json(value: &Value) -> Self {
        let mut recipe = Self::default();
        if let Some(age) = value["age"].as_f64().filter(|age| *age > 0.0) {
            recipe.age = Some(age);
        }
        if let Some(day) = value["day"]
            .as_f64()
            .filter(|day| (1.0..=365.0).contains(day))
        {
            recipe.day = day;
        }
        if let Some(detail) = value["detail"].as_str().and_then(Detail::from_name) {
            recipe.detail = detail;
        }
        if let Some(level) = value["level"].as_u64().filter(|level| *level <= 3) {
            recipe.level = usize::try_from(level).unwrap_or(0);
        }
        recipe.environment = value["environment"]
            .as_str()
            .and_then(Environment::from_name);
        recipe.seed = value["seed"].as_u64();
        if let Some(look) = value["look"].as_str().and_then(Look::from_name) {
            recipe.look = look;
        }
        if let Some(view) = value["view"].as_str().and_then(View::from_name) {
            recipe.view = view;
        }
        if recipe.detail.parts() {
            recipe.level = 0;
        }
        recipe
    }
}

/// A species' last render, as kept.
#[derive(Debug, Clone, PartialEq)]
pub struct Kept {
    pub recipe: Recipe,
    /// When it was made, seconds since 1970.
    pub saved: u64,
    pub age: f64,
    pub height_m: f64,
    pub crown_width_m: f64,
    pub triangles: usize,
    /// How long it took, seconds.
    pub seconds: f64,
    pub generator_revision: u32,
}

impl Kept {
    #[must_use]
    pub fn to_json(&self) -> Value {
        json!({
            "recipe": self.recipe.to_json(),
            "saved": self.saved,
            "age": self.age,
            "height_m": self.height_m,
            "crown_width_m": self.crown_width_m,
            "triangles": self.triangles,
            "seconds": self.seconds,
            "generator_revision": self.generator_revision,
        })
    }

    #[must_use]
    pub fn from_json(value: &Value) -> Option<Self> {
        Some(Self {
            recipe: Recipe::from_json(value.get("recipe")?),
            saved: value["saved"].as_u64()?,
            age: value["age"].as_f64()?,
            height_m: value["height_m"].as_f64().unwrap_or(0.0),
            crown_width_m: value["crown_width_m"].as_f64().unwrap_or(0.0),
            triangles: value["triangles"]
                .as_u64()
                .and_then(|n| usize::try_from(n).ok())
                .unwrap_or(0),
            seconds: value["seconds"].as_f64().unwrap_or(0.0),
            generator_revision: value["generator_revision"]
                .as_u64()
                .and_then(|n| u32::try_from(n).ok())
                .unwrap_or(0),
        })
    }
}

/// The kept renders.
#[derive(Debug, Clone, Default)]
pub struct Store {
    /// Where they are kept; `None` when no folder is known.
    pub folder: Option<PathBuf>,
    pub kept: BTreeMap<String, Kept>,
    /// Bumped each time a picture is written, so the window loads it again.
    pub written: BTreeMap<String, u64>,
}

impl Store {
    /// `$XDG_CACHE_HOME/plantlab/renders`, else `$HOME/.cache/plantlab/renders`.
    #[must_use]
    pub fn default_folder() -> Option<PathBuf> {
        let cache = std::env::var_os("XDG_CACHE_HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME")
                    .filter(|value| !value.is_empty())
                    .map(|home| PathBuf::from(home).join(".cache"))
            })?;
        Some(cache.join("plantlab").join("renders"))
    }

    /// The renders kept in `folder`. Files that do not load are skipped.
    #[must_use]
    pub fn load(folder: Option<PathBuf>) -> Self {
        let mut kept = BTreeMap::new();
        if let Some(folder) = &folder
            && let Ok(files) = std::fs::read_dir(folder)
        {
            for file in files.flatten() {
                let path = file.path();
                if path.extension().is_none_or(|extension| extension != "json") {
                    continue;
                }
                let Some(id) = path.file_stem().and_then(|stem| stem.to_str()) else {
                    continue;
                };
                if let Some(entry) = std::fs::read_to_string(&path)
                    .ok()
                    .and_then(|text| serde_json::from_str::<Value>(&text).ok())
                    .and_then(|value| Kept::from_json(&value))
                {
                    kept.insert(id.to_owned(), entry);
                }
            }
        }
        Self {
            folder,
            kept,
            written: BTreeMap::new(),
        }
    }

    /// The picture of `id`'s last render.
    #[must_use]
    pub fn picture(&self, id: &str) -> Option<PathBuf> {
        Some(self.folder.as_ref()?.join(format!("{id}.png")))
    }

    /// Keep `kept` as `id`'s last render; its picture comes separately.
    ///
    /// # Errors
    ///
    /// When the file cannot be written.
    pub fn keep(&mut self, id: &str, kept: Kept) -> Result<(), String> {
        let folder = self
            .folder
            .clone()
            .ok_or("no folder for renders is known")?;
        std::fs::create_dir_all(&folder)
            .map_err(|error| format!("cannot make {}: {error}", folder.display()))?;
        let path = folder.join(format!("{id}.json"));
        let text = serde_json::to_string_pretty(&kept.to_json()).map_err(|e| e.to_string())?;
        std::fs::write(&path, format!("{text}\n"))
            .map_err(|error| format!("cannot write {}: {error}", path.display()))?;
        self.kept.insert(id.to_owned(), kept);
        Ok(())
    }

    /// Note that `id`'s picture was written again.
    pub fn wrote(&mut self, id: &str) {
        *self.written.entry(id.to_owned()).or_insert(0) += 1;
    }
}

/// Seconds since 1970.
#[must_use]
pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |time| time.as_secs())
}

/// How long ago `saved` was, in words.
#[must_use]
pub fn ago(saved: u64, now: u64) -> String {
    let seconds = now.saturating_sub(saved);
    match seconds {
        0..60 => "just now".into(),
        60..3_600 => plural(seconds / 60, "minute"),
        3_600..86_400 => plural(seconds / 3_600, "hour"),
        _ => plural(seconds / 86_400, "day"),
    }
}

fn plural(count: u64, unit: &str) -> String {
    if count == 1 {
        format!("1 {unit} ago")
    } else {
        format!("{count} {unit}s ago")
    }
}

/// A PNG's pixels, RGBA, with its width and height.
#[must_use]
pub fn read_png(path: &Path) -> Option<(usize, usize, Vec<u8>)> {
    let file = std::fs::File::open(path).ok()?;
    let mut decoder = png::Decoder::new(std::io::BufReader::new(file));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().ok()?;
    let mut buffer = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buffer).ok()?;
    let (width, height) = (info.width as usize, info.height as usize);
    let bytes = &buffer[..info.buffer_size()];
    let rgba = match info.color_type {
        png::ColorType::Rgba => bytes.to_vec(),
        png::ColorType::Rgb => bytes
            .as_chunks::<3>()
            .0
            .iter()
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect(),
        png::ColorType::GrayscaleAlpha => bytes
            .as_chunks::<2>()
            .0
            .iter()
            .flat_map(|p| [p[0], p[0], p[0], p[1]])
            .collect(),
        png::ColorType::Grayscale => bytes.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::Indexed => return None,
    };
    Some((width, height, rgba))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_recipe_reads_back_as_written() {
        let recipe = Recipe {
            age: Some(30.0),
            day: 288.0,
            detail: Detail::Standard,
            level: 2,
            environment: Some(Environment::Edge),
            seed: Some(7),
            look: Look::Photo,
            view: View::Oblique,
        };
        assert_eq!(Recipe::from_json(&recipe.to_json()), recipe);
        // Full draws part meshes, so it is always the nearest level.
        let full = Recipe {
            detail: Detail::Full,
            level: 2,
            ..Recipe::default()
        };
        assert_eq!(Recipe::from_json(&full.to_json()).level, 0);
    }

    #[test]
    fn changes_name_what_grows_again_first_and_leave_out_the_view() {
        let before = Recipe::default();
        let after = Recipe {
            day: 100.0,
            seed: Some(3),
            view: View::Top,
            look: Look::Photo,
            ..before.clone()
        };
        assert_eq!(after.changes(&before), [Setting::Variant, Setting::Day]);
        assert!(Setting::Variant.regrows() && !Setting::Day.regrows());
        assert!(before.changes(&before).is_empty());
    }

    #[test]
    fn kept_renders_survive_a_reload() {
        let folder = std::env::temp_dir().join(format!("plantlab-renders-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        let mut store = Store::load(Some(folder.clone()));
        assert!(store.kept.is_empty());
        let kept = Kept {
            recipe: Recipe::default(),
            saved: 1_000,
            age: 12.0,
            height_m: 8.5,
            crown_width_m: 4.0,
            triangles: 12_345,
            seconds: 3.5,
            generator_revision: 149,
        };
        store.keep("acer-macrophyllum", kept.clone()).unwrap();
        let again = Store::load(Some(folder.clone()));
        assert_eq!(again.kept.get("acer-macrophyllum"), Some(&kept));
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn ages_read_as_words() {
        assert_eq!(ago(100, 130), "just now");
        assert_eq!(ago(0, 60), "1 minute ago");
        assert_eq!(ago(0, 7_300), "2 hours ago");
        assert_eq!(ago(0, 3 * 86_400), "3 days ago");
    }
}
