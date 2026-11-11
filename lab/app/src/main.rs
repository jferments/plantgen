//! `plantlab`: grow PlantGen's plants under chosen conditions and look at
//! them. It renders without a window for now, on the GPU (a 4090 or
//! lavapipe): thumbnails, review sheets and batches of both, each picture
//! with a JSON sidecar that says what it shows. The window comes later
//! (design: `PLANTLAB-DESIGN.md` in the project's workspace).
//!
//! PlantLab simulates nothing itself: every plant is grown by PlantGen's
//! own code ([`plantgen::drawing`]), exactly as `plantc` grows it, and
//! turned into draw-ready data by `plantlab-scene`. This program only
//! draws that data.

mod gif;
mod gpu;
mod measure;
mod record;
mod render;
mod settings;
#[cfg(feature = "solari")]
mod solari;
mod window;

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use plantgen::library::Library;
use plantgen::quality;
use plantgen::spec::Environment;
use plantlab_scene::{Look, Shot, View};
use render::Job;
use sha2::{Digest, Sha256};

const USAGE: &str = "\
plantlab: grow PlantGen's plants and render them

usage:
  plantlab open [SPECIES] [--age YEARS] [--live STEPS] [--no-grid] [--no-ruler]
                [--out DIR] [--record] [--plant-only] [--window WxH]
                [--page NAME] [--overlay] [--settings FILE] [--renders DIR]
                [--generate] [--capture FILE [--capture-after SECONDS]]
      The window: a top bar (the menus, the chosen species and its order
      and family, search and settings), the library at the left (every
      species by group, order, family and genus, with a search and a
      filter by growth form; L), the plant in the middle with its view
      and tools over it, and the inspector at the right (I) with its
      pages: Render (age, site, variant, the day with each organ's year
      drawn as a strip, detail Draft, Standard or Full, and Generate),
      Readings, Pictures, Spec, Bark and Keys; --page readings opens one.
      Choosing a species grows nothing: its last render shows until
      Generate (Enter), or --generate for the first. The views (1-4) and
      the looks change the plant on screen without growing it again; a
      new day, detail or level draws the grown plant again. Each
      species' last render (a picture and its settings) is kept in DIR
      (default $XDG_CACHE_HOME/plantlab/renders, else
      ~/.cache/plantlab/renders) and its settings come back with it.
      The palette (Ctrl+K) finds any species or command; O, or
      --overlay, shows the plant alone. The plant is shown growing every
      STEPS steps (default 10, 0 for none). --age grows YEARS old. A
      grid under the plant and a ruler beside it measure it (G and R).
      Snapshots (P), recorded growth (--record) and GIFs (S), of the
      scene or of the plant alone on a clear background (--plant-only),
      go to DIR (default plantlab-pictures). Drag to turn, middle-drag or
      Shift-drag to pan, scroll to zoom, F to frame the plant again.
      Settings are kept in ~/.config/plantlab/settings.json (--settings
      FILE for another); flags win over them. --window sets the window's
      size (default 1400x900). --capture saves a picture of the window
      once the plant stands, or after SECONDS with --capture-after, then
      closes it.
  plantlab thumbs SPECIES... --out DIR [--size PX] [--height PX] [--clear]
                 [OPTIONS]
      A thumbnail of each species (a library id, or `all`): DIR/ID.png and
      DIR/ID.json, what the picture shows. It is square unless --height
      sets its height (--size is then its width). --clear draws the plant
      alone on a transparent background, without its ground and scale: a
      cut-out for a web page or a sprite sheet.
  plantlab review SPECIES... --out DIR [OPTIONS]
      A review sheet of each species: its ages at one scale, the mature
      plant from four sides and close up, and its year when it has
      seasons: DIR/ID-review.png and DIR/ID-review.json.
  plantlab batch JOBS.jsonl --out DIR [--gpu I]
      Run many: one JSON object a line, {\"species\": ID, \"kind\": \"thumb\" or
      \"review\"} with any of size, height, day, age, view, quality, look,
      env, seed, lod, focus (\"X,Y,Z\" or [X, Y, Z, SPAN]), span and scale
      (on or off), \"background\": \"clear\" for a cut-out, and \"name\",
      the files' stem (default the species, so name each picture when a
      species has several). A picture whose sidecar shows the same inputs
      is skipped, so a stopped batch resumes where it stopped.
  plantlab solari SPECIES --out DIR [--size PX] [--mode realtime|pathtrace]
                  [--frames N] [--cut-cards N] [OPTIONS]
      A spike: one plant lit by Bevy's experimental ray tracing (Solari),
      on an RTX-class GPU, written as DIR/ID-solari.png. Leaf cards are cut
      into triangles (24 by 24 cells unless --cut-cards says otherwise).
      Needs a build with `--features solari`.
  plantlab gpus
      List the GPUs, for --gpu.
  plantlab help

options:
  --day N              day of the year, 1 to 365 (default 196)
  --age YEARS          the plant's age (default: its oldest keyframe)
  --view three-quarter|side|oblique|top
                       oblique (or above) looks down at 45 degrees from the south-east
  --focus X,Y,Z        a close-up round this point, metres from the plant's
                       foot with +Y up (`thumbs` and `batch`)
  --span METRES        how much of it the close-up shows, bottom to top
                       (default 0.5)
  --quality draft|standard|full
                       full is standard with part meshes (as --parts on)
  --parts on|off       draw flowers, fruit, cones and needle shoots as their
                       part meshes, as full-quality exports do near the camera
  --env typical|open|edge|interior|suppressed
                       the site it grows on (default: its typical site)
  --seed N             the variant's seed (default: the spec's first)
  --lod 0-3            the level of detail, 0 the nearest
  --scale on|off       the 1.8 m figure or the rod beside it (default on)
  --look review|photo  review (default) is fixed so species compare fairly;
                       photo adds soft shadows, sky light, ambient occlusion,
                       a tone map and supersampling
  --cut-cards N        draw leaf cards cut into triangles on an N by N grid,
                       as ray tracing needs them, instead of cut by texture
  --gpu I              render on GPU I of `plantlab gpus`
  --force              render even what is already rendered
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("plantlab: {message}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: &[String]) -> Result<(), String> {
    match args.first().map(String::as_str) {
        Some("thumbs") => pictures(&args[1..], Kind::Thumb),
        Some("review") => pictures(&args[1..], Kind::Review),
        Some("batch") => batch(&args[1..]),
        Some("solari") => solari(&args[1..]),
        Some("open") => open(&args[1..]),
        Some("gpus") => {
            for line in gpu::list() {
                println!("{line}");
            }
            Ok(())
        }
        None | Some("help" | "-h" | "--help") => {
            print!("{USAGE}");
            Ok(())
        }
        Some(other) => Err(format!("unknown command `{other}`; run `plantlab help`")),
    }
}

/// What a picture is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Thumb,
    Review,
}

/// What a command line asked for.
struct Request {
    species: Vec<String>,
    out: PathBuf,
    size: u32,
    height: Option<u32>,
    shot: Shot,
    clear: bool,
    gpu: Option<usize>,
    force: bool,
}

fn parse(args: &[String]) -> Result<Request, String> {
    let mut species = Vec::new();
    let mut out = None;
    let mut size = 512_u32;
    let mut height = None;
    let mut shot = Shot::thumbnail("");
    let mut clear = false;
    let mut gpu = None;
    let mut force = false;
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        let mut value = |flag: &str| {
            rest.next()
                .cloned()
                .ok_or_else(|| format!("`{flag}` needs a value"))
        };
        match arg.as_str() {
            "--out" => out = Some(PathBuf::from(value("--out")?)),
            "--size" => size = parse_size(&value("--size")?)?,
            "--height" => height = Some(parse_size(&value("--height")?)?),
            "--clear" => clear = true,
            "--gpu" => {
                gpu = Some(
                    value("--gpu")?
                        .parse()
                        .map_err(|_| "`--gpu` takes an index from `plantlab gpus`")?,
                );
            }
            "--force" => force = true,
            flag @ ("--day" | "--age" | "--view" | "--quality" | "--look" | "--cut-cards"
            | "--focus" | "--span" | "--parts" | "--env" | "--seed" | "--lod"
            | "--scale") => {
                apply(&mut shot, flag.trim_start_matches("--"), &value(flag)?)?;
            }
            flag if flag.starts_with("--") => return Err(format!("unknown flag `{flag}`")),
            id => species.push(id.to_string()),
        }
    }
    if species.iter().any(|id| id == "all") {
        species = Library::builtin()
            .species()
            .iter()
            .map(|entry| entry.id.clone())
            .collect();
    }
    if species.is_empty() {
        return Err("name at least one species, or `all`".into());
    }
    let out = out.ok_or("missing `--out DIR`")?;
    Ok(Request {
        species,
        out,
        size,
        height,
        shot,
        clear,
        gpu,
        force,
    })
}

fn parse_size(text: &str) -> Result<u32, String> {
    text.parse()
        .ok()
        .filter(|px| (64..=4096).contains(px))
        .ok_or_else(|| "the size must be 64 to 4096 pixels".to_string())
}

/// Metres a close-up shows from the bottom of the picture to the top, as
/// `plantc render --focus` frames it.
const FOCUS_SPAN: f64 = 0.5;

/// Set one of a shot's options from its text.
fn apply(shot: &mut Shot, name: &str, value: &str) -> Result<(), String> {
    match name {
        "day" => {
            shot.day = value
                .parse()
                .ok()
                .filter(|day| (1.0..=365.0).contains(day))
                .ok_or("the day must be 1 to 365")?;
        }
        "age" => {
            shot.age = Some(
                value
                    .parse()
                    .ok()
                    .filter(|age: &f64| *age > 0.0)
                    .ok_or("the age must be a positive number of years")?,
            );
        }
        "view" => {
            shot.view = View::from_name(value).ok_or_else(|| format!("unknown view `{value}`"))?;
        }
        "quality" => {
            // Full is the standard meshes with part meshes, as full-quality
            // exports draw them.
            let (profile, parts) = match value {
                "full" => (Some(quality::STANDARD), true),
                other => (quality::profile(other), false),
            };
            shot.quality = profile
                .ok_or_else(|| format!("unknown quality `{value}`; use draft, standard or full"))?;
            shot.parts = parts;
        }
        "parts" => {
            shot.parts = match value {
                "on" | "true" => true,
                "off" | "false" => false,
                _ => return Err("`--parts` takes on or off".into()),
            };
        }
        "env" => {
            shot.environment = match value {
                "typical" => None,
                other => Some(Environment::from_name(other).ok_or_else(|| {
                    format!(
                        "unknown site `{other}`; use typical, open, edge, interior or \
                             suppressed"
                    )
                })?),
            };
        }
        "seed" => {
            shot.seed = Some(value.parse().map_err(|_| "`--seed` takes a whole number")?);
        }
        "cut-cards" => {
            shot.cut_cards = value
                .parse()
                .ok()
                .filter(|cells| *cells <= 64)
                .ok_or("`--cut-cards` takes a grid of 0 to 64 cells a side")?;
        }
        "look" => {
            shot.look = Look::from_name(value)
                .ok_or_else(|| format!("unknown look `{value}`; use review or photo"))?;
        }
        other => return apply_framing(shot, other, value),
    }
    Ok(())
}

/// The options that frame a picture: its level of detail, a close-up and the scale figure.
fn apply_framing(shot: &mut Shot, name: &str, value: &str) -> Result<(), String> {
    match name {
        "lod" => {
            shot.level = value
                .parse()
                .ok()
                .filter(|level| *level <= 3)
                .ok_or("the level of detail must be 0 to 3")?;
        }
        "focus" => {
            // "X,Y,Z" or, from a batch's JSON, [X, Y, Z] or [X, Y, Z, SPAN]
            let numbers: Vec<f64> = value
                .trim_start_matches('[')
                .trim_end_matches(']')
                .split(',')
                .map(|part| part.trim().parse())
                .collect::<Result<_, _>>()
                .map_err(|_| "`--focus` takes X,Y,Z in metres")?;
            let span = shot.focus.map_or(FOCUS_SPAN, |(_, span)| span);
            let (point, span) = match numbers[..] {
                [x, y, z] => ([x, y, z], span),
                [x, y, z, span] if span > 0.0 => ([x, y, z], span),
                _ => {
                    return Err(
                        "`--focus` takes X,Y,Z in metres, then optionally a span above 0".into(),
                    );
                }
            };
            shot.focus = Some((point, span));
        }
        "span" => {
            let span = value
                .parse()
                .ok()
                .filter(|span: &f64| *span > 0.0)
                .ok_or("`--span` takes a positive number of metres")?;
            let point = shot.focus.map_or([0.0; 3], |(point, _)| point);
            shot.focus = Some((point, span));
        }
        "scale" => {
            shot.scale = match value {
                "on" | "true" => true,
                "off" | "false" => false,
                _ => return Err("the scale is on or off".into()),
            };
        }
        other => return Err(format!("unknown option `{other}`")),
    }
    Ok(())
}

/// How one picture is framed and kept: its size in pixels, whether its
/// background is clear, and its files' stem when not the default.
#[derive(Debug, Clone, Default)]
struct Picture {
    width: u32,
    height: Option<u32>,
    clear: bool,
    name: Option<String>,
}

impl Picture {
    fn square(size: u32) -> Self {
        Self {
            width: size,
            ..Self::default()
        }
    }
}

/// The job for one picture of one species.
fn job(species: &str, kind: Kind, shot: &Shot, picture: &Picture) -> Result<Job, String> {
    let library = Library::builtin();
    let mut shot = Shot {
        species: species.to_string(),
        ..shot.clone()
    };
    if picture.clear {
        // Nothing beside the plant: frame it alone.
        shot.scale = false;
    }
    let (name, sheet) = match kind {
        Kind::Thumb => (
            species.to_string(),
            plantlab_scene::single(shot, picture.width, picture.height.unwrap_or(picture.width)),
        ),
        Kind::Review => (
            format!("{species}-review"),
            plantlab_scene::review_sheet(&shot, library)?,
        ),
    };
    let name = picture.name.clone().unwrap_or(name);
    let key = key(species, kind, &sheet, picture.clear, library);
    Ok(Job {
        name,
        sheet,
        key,
        clear: picture.clear,
    })
}

/// What decides a picture: the species' spec, the generator's revision,
/// every tile's settings and the background, hashed.
fn key(
    species: &str,
    kind: Kind,
    sheet: &plantlab_scene::Sheet,
    clear: bool,
    library: &Library,
) -> String {
    let mut hash = Sha256::new();
    hash.update(format!(
        "plantlab 1 {kind:?} {}\n",
        plantgen::GENERATOR_REVISION
    ));
    if clear {
        hash.update("clear background\n");
    }
    if let Some(entry) = library.entry(species) {
        hash.update(entry.source());
    }
    for tile in &sheet.tiles {
        hash.update(format!("{:?} {:?}\n", tile.rect, tile.shot));
    }
    let mut text = String::with_capacity(64);
    for byte in hash.finalize() {
        let _ = write!(text, "{byte:02x}");
    }
    text
}

/// Whether `out` already holds the picture `job` would make.
fn rendered(out: &Path, job: &Job) -> bool {
    let sidecar = out.join(format!("{}.json", job.name));
    let picture = out.join(format!("{}.png", job.name));
    picture.exists()
        && std::fs::read_to_string(sidecar)
            .is_ok_and(|text| text.contains(&format!("\"key\": \"{}\"", job.key)))
}

fn run_jobs(jobs: Vec<Job>, out: PathBuf, gpu: Option<usize>, force: bool) -> Result<(), String> {
    std::fs::create_dir_all(&out)
        .map_err(|error| format!("cannot create {}: {error}", out.display()))?;
    let total = jobs.len();
    let jobs: Vec<Job> = jobs
        .into_iter()
        .filter(|job| force || !rendered(&out, job))
        .collect();
    if jobs.len() < total {
        println!(
            "{} of {total} already rendered with the same inputs; skipped",
            total - jobs.len()
        );
    }
    if jobs.is_empty() {
        return Ok(());
    }
    render::run(jobs, out, gpu)
}

fn pictures(args: &[String], kind: Kind) -> Result<(), String> {
    let request = parse(args)?;
    let jobs = request
        .species
        .iter()
        .map(|id| {
            let picture = Picture {
                width: request.size,
                height: request.height,
                clear: request.clear,
                name: None,
            };
            job(id, kind, &request.shot, &picture)
        })
        .collect::<Result<Vec<_>, _>>()?;
    run_jobs(jobs, request.out, request.gpu, request.force)
}

#[cfg(feature = "solari")]
fn solari(args: &[String]) -> Result<(), String> {
    let mut rest = Vec::new();
    let mut mode = solari::Mode::Pathtrace;
    let mut frames = None;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--mode" => {
                mode = match iter.next().map(String::as_str) {
                    Some("realtime") => solari::Mode::Realtime,
                    Some("pathtrace") => solari::Mode::Pathtrace,
                    _ => return Err("`--mode` is realtime or pathtrace".into()),
                };
            }
            "--frames" => {
                frames = Some(
                    iter.next()
                        .and_then(|value| value.parse().ok())
                        .ok_or("`--frames` takes a number")?,
                );
            }
            _ => rest.push(arg.clone()),
        }
    }
    let mut request = parse(&rest)?;
    if request.gpu.is_some() {
        return Err(
            "`solari` lets Bevy choose the GPU (WGPU_ADAPTER_NAME picks one by name)".into(),
        );
    }
    let [species] = request.species.as_slice() else {
        return Err("`solari` takes one species".into());
    };
    if request.shot.cut_cards == 0 {
        request.shot.cut_cards = 24;
    }
    solari::run(solari::Photo {
        shot: Shot {
            species: species.clone(),
            ..request.shot
        },
        out: request.out,
        size: request.size,
        mode,
        frames: frames.unwrap_or(match mode {
            solari::Mode::Realtime => 120,
            solari::Mode::Pathtrace => 1_000,
        }),
    })
}

#[cfg(not(feature = "solari"))]
fn solari(_args: &[String]) -> Result<(), String> {
    Err("this plantlab was built without Solari:          `cargo run --release -p plantlab --features solari -- solari ...`"
        .into())
}

fn open(args: &[String]) -> Result<(), String> {
    let mut options = window::Options {
        species: None,
        age: None,
        live: None,
        grid: None,
        ruler: None,
        out: PathBuf::from("plantlab-pictures"),
        record: false,
        plant_only: None,
        capture: None,
        size: (1400, 900),
        panels: Vec::new(),
        overlay: false,
        settings: None,
        renders: None,
        generate: false,
    };
    let mut capture = None;
    let mut after = None;
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--age" | "--years" => {
                let age: f64 = rest
                    .next()
                    .and_then(|value| value.parse().ok())
                    .ok_or("`--age` needs a number of years")?;
                if !(age > 0.0 && age.is_finite()) {
                    return Err("`--age` needs a number of years above 0".into());
                }
                options.age = Some(age);
            }
            "--no-grid" => options.grid = Some(false),
            "--record" => options.record = true,
            "--plant-only" => options.plant_only = Some(true),
            "--no-ruler" => options.ruler = Some(false),
            "--window" => {
                let text = rest.next().ok_or("`--window` needs WIDTHxHEIGHT")?;
                options.size = text
                    .split_once('x')
                    .and_then(|(w, h)| Some((w.parse().ok()?, h.parse().ok()?)))
                    .filter(|&(w, h): &(u32, u32)| {
                        (320..=8192).contains(&w) && (240..=8192).contains(&h)
                    })
                    .ok_or("`--window` needs WIDTHxHEIGHT, such as 1500x900")?;
            }
            "--page" | "--panels" => {
                let text = rest.next().ok_or("`--page` needs a page's name")?;
                options.panels = text.split(',').map(str::to_owned).collect();
            }
            "--generate" => options.generate = true,
            "--renders" => {
                options.renders = Some(PathBuf::from(
                    rest.next().ok_or("`--renders` needs a folder")?,
                ));
            }
            "--overlay" => options.overlay = true,
            "--settings" => {
                options.settings = Some(PathBuf::from(
                    rest.next().ok_or("`--settings` needs a file")?,
                ));
            }
            "--out" => {
                options.out = PathBuf::from(rest.next().ok_or("`--out` needs a folder")?);
            }
            "--live" => {
                options.live = Some(
                    rest.next()
                        .and_then(|value| value.parse().ok())
                        .ok_or("`--live` needs a whole number of steps (0 for none)")?,
                );
            }
            "--capture-after" => {
                after = Some(
                    rest.next()
                        .and_then(|value| value.parse().ok())
                        .ok_or("`--capture-after` needs a number of seconds")?,
                );
            }
            "--capture" => {
                capture = Some(PathBuf::from(
                    rest.next().ok_or("`--capture` needs a file")?,
                ));
            }
            flag if flag.starts_with("--") => return Err(format!("unknown flag `{flag}`")),
            id => options.species = Some(id.to_string()),
        }
    }
    options.capture = capture.map(|path| (path, after));
    window::run(options)
}

/// Read a batch file: one job a line.
fn read_batch(text: &str) -> Result<Vec<Job>, String> {
    let mut jobs = Vec::new();
    for (number, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let at = |message: String| format!("line {}: {message}", number + 1);
        let value: serde_json::Value =
            serde_json::from_str(line).map_err(|error| at(error.to_string()))?;
        let object = value
            .as_object()
            .ok_or_else(|| at("expected a JSON object".into()))?;
        let mut shot = Shot::thumbnail("");
        let mut kind = Kind::Thumb;
        let mut picture = Picture::square(512);
        let mut species = None;
        for (name, value) in object {
            let text = match value {
                serde_json::Value::String(text) => text.clone(),
                other => other.to_string(),
            };
            match name.as_str() {
                "species" => species = Some(text),
                "kind" => {
                    kind = match text.as_str() {
                        "thumb" => Kind::Thumb,
                        "review" => Kind::Review,
                        other => return Err(at(format!("unknown kind `{other}`"))),
                    };
                }
                "size" => picture.width = parse_size(&text).map_err(at)?,
                "height" => picture.height = Some(parse_size(&text).map_err(at)?),
                "background" => {
                    picture.clear = match text.as_str() {
                        "clear" => true,
                        "scene" => false,
                        other => {
                            return Err(at(format!(
                                "unknown background `{other}`; use clear or scene"
                            )));
                        }
                    };
                }
                "name" => {
                    let fine = !text.is_empty()
                        && text
                            .chars()
                            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
                        && !text.starts_with('.');
                    if !fine {
                        return Err(at(format!(
                            "the name `{text}` must be letters, digits, '-', '_' or '.'"
                        )));
                    }
                    picture.name = Some(text);
                }
                option => apply(&mut shot, option, &text).map_err(at)?,
            }
        }
        let species = species.ok_or_else(|| at("missing \"species\"".into()))?;
        jobs.push(job(&species, kind, &shot, &picture).map_err(at)?);
    }
    Ok(jobs)
}

fn batch(args: &[String]) -> Result<(), String> {
    let mut file = None;
    let mut out = None;
    let mut gpu = None;
    let mut force = false;
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--out" => out = rest.next().map(PathBuf::from),
            "--gpu" => {
                gpu = Some(
                    rest.next()
                        .and_then(|value| value.parse().ok())
                        .ok_or("`--gpu` takes an index from `plantlab gpus`")?,
                );
            }
            "--force" => force = true,
            flag if flag.starts_with("--") => return Err(format!("unknown flag `{flag}`")),
            path => file = Some(PathBuf::from(path)),
        }
    }
    let file = file.ok_or("name the batch file")?;
    let out = out.ok_or("missing `--out DIR`")?;
    let text = std::fs::read_to_string(&file)
        .map_err(|error| format!("cannot read {}: {error}", file.display()))?;
    run_jobs(read_batch(&text)?, out, gpu, force)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(text: &str) -> Vec<String> {
        text.split_whitespace().map(str::to_string).collect()
    }

    #[test]
    fn a_command_line_reads_its_flags() {
        let parsed = parse(&args(
            "acer-macrophyllum --out /tmp/x --size 256 --day 120 --view side --look photo --gpu 1",
        ))
        .expect("parses");
        assert_eq!(parsed.species, ["acer-macrophyllum"]);
        assert_eq!(parsed.size, 256);
        assert!((parsed.shot.day - 120.0).abs() < 1e-12);
        assert_eq!(parsed.shot.view, View::Side);
        assert_eq!(parsed.shot.look, Look::Photo);
        assert_eq!(parsed.gpu, Some(1));
    }

    #[test]
    fn a_close_up_reads_its_point_and_span_and_the_view_from_above() {
        let parsed = parse(&args(
            "acer-macrophyllum --out /tmp/x --view above --focus 1,6.5,-0.5 --span 3",
        ))
        .expect("parses");
        assert_eq!(parsed.shot.view, View::Oblique);
        assert_eq!(parsed.shot.focus, Some(([1.0, 6.5, -0.5], 3.0)));
        assert!(parse(&args("x --out /tmp/x --focus 1,2")).is_err());
        assert!(parse(&args("x --out /tmp/x --span 0")).is_err());
    }

    #[test]
    fn a_command_line_refuses_what_it_cannot_do() {
        assert!(parse(&args("--out /tmp/x")).is_err());
        assert!(parse(&args("x --out /tmp/x --size 9")).is_err());
        assert!(parse(&args("x --out /tmp/x --day 400")).is_err());
        assert!(parse(&args("x --out /tmp/x --wat")).is_err());
        assert!(parse(&args("x")).is_err());
    }

    #[test]
    fn all_means_every_library_species() {
        let parsed = parse(&args("all --out /tmp/x")).expect("parses");
        assert_eq!(parsed.species.len(), Library::builtin().species().len());
    }

    #[test]
    fn a_batch_reads_one_job_a_line_and_names_bad_lines() {
        let jobs = read_batch(
            "# a comment\n\
             {\"species\": \"acer-macrophyllum\"}\n\
             {\"species\": \"acer-macrophyllum\", \"size\": 256, \"look\": \"photo\"}\n",
        )
        .expect("reads");
        assert_eq!(jobs.len(), 2);
        assert_eq!(jobs[0].name, "acer-macrophyllum");
        assert_ne!(jobs[0].key, jobs[1].key);
        let error = read_batch("{\"species\": \"x\", \"kind\": \"poster\"}").err();
        assert!(error.is_some_and(|e| e.starts_with("line 1:")));
    }

    #[test]
    fn the_same_inputs_give_the_same_key() {
        let shot = Shot::thumbnail("");
        let a = job(
            "acer-macrophyllum",
            Kind::Thumb,
            &shot,
            &Picture::square(512),
        )
        .expect("a job");
        let b = job(
            "acer-macrophyllum",
            Kind::Thumb,
            &shot,
            &Picture::square(512),
        )
        .expect("a job");
        let c = job(
            "acer-macrophyllum",
            Kind::Thumb,
            &shot,
            &Picture::square(256),
        )
        .expect("a job");
        assert_eq!(a.key, b.key);
        assert_ne!(a.key, c.key);
    }

    #[test]
    fn a_batch_names_its_cut_outs_and_their_conditions() {
        let jobs = read_batch(
            "{\"species\": \"acer-macrophyllum\", \"name\": \"maple.env-edge\", \"env\": \"edge\", \
              \"lod\": 2, \"seed\": 3, \"size\": 320, \"height\": 400, \"background\": \"clear\"}\n\
             {\"species\": \"acer-macrophyllum\", \"size\": 320, \"height\": 400}\n",
        )
        .expect("reads");
        let [cut, scene] = jobs.as_slice() else {
            panic!("two jobs");
        };
        assert_eq!(cut.name, "maple.env-edge");
        assert!(cut.clear && !scene.clear);
        let close =
            read_batch("{\"species\": \"x\", \"focus\": [0, 2.5, 0.2, 0.6]}").expect("reads");
        assert_eq!(
            close[0].sheet.tiles[0].shot.focus,
            Some(([0.0, 2.5, 0.2], 0.6))
        );
        assert_ne!(cut.key, scene.key);
        let shot = &cut.sheet.tiles[0].shot;
        assert_eq!(shot.environment, Some(Environment::Edge));
        assert_eq!((shot.level, shot.seed, shot.scale), (2, Some(3), false));
        assert_eq!((cut.sheet.width, cut.sheet.height), (320, 400));
        assert!((shot.aspect - 0.8).abs() < 1e-12);
        for bad in [
            "{\"species\": \"x\", \"name\": \"../up\"}",
            "{\"species\": \"x\", \"env\": \"moon\"}",
            "{\"species\": \"x\", \"lod\": 4}",
            "{\"species\": \"x\", \"background\": \"pink\"}",
        ] {
            assert!(read_batch(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_command_line_asks_for_a_cut_out() {
        let parsed = parse(&args(
            "acer-macrophyllum --out /tmp/x --size 320 --height 400 --clear --env interior --lod 1 --scale off",
        ))
        .expect("parses");
        assert!(parsed.clear);
        assert_eq!(parsed.height, Some(400));
        assert_eq!(parsed.shot.environment, Some(Environment::Interior));
        assert_eq!(parsed.shot.level, 1);
        assert!(!parsed.shot.scale);
        let close = parse(&args(
            "carnegiea-gigantea --out /tmp/x --focus 0,2.5,0.2,0.6",
        ))
        .expect("parses");
        assert_eq!(close.shot.focus, Some(([0.0, 2.5, 0.2], 0.6)));
        let close =
            parse(&args("carnegiea-gigantea --out /tmp/x --focus 0,2.5,0")).expect("parses");
        assert_eq!(close.shot.focus, Some(([0.0, 2.5, 0.0], FOCUS_SPAN)));
        assert!(parse(&args("x --out /tmp/x --focus 1,2")).is_err());
        assert!(parse(&args("x --out /tmp/x --focus 1,2,3,0")).is_err());
    }
}
