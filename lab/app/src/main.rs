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

mod gpu;
mod render;
#[cfg(feature = "solari")]
mod solari;

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use plantgen::library::Library;
use plantgen::quality;
use plantlab_scene::{Look, Shot, View};
use render::Job;
use sha2::{Digest, Sha256};

const USAGE: &str = "\
plantlab: grow PlantGen's plants and render them

usage:
  plantlab thumbs SPECIES... --out DIR [--size PX] [OPTIONS]
      A thumbnail of each species (a library id, or `all`): DIR/ID.png and
      DIR/ID.json, what the picture shows.
  plantlab review SPECIES... --out DIR [OPTIONS]
      A review sheet of each species: its ages at one scale, the mature
      plant from three sides and close up, and its year when it has
      seasons: DIR/ID-review.png and DIR/ID-review.json.
  plantlab batch JOBS.jsonl --out DIR [--gpu I]
      Run many: one JSON object a line, {\"species\": ID, \"kind\": \"thumb\" or
      \"review\"} with any of size, day, age, view, quality and look. A
      picture whose sidecar shows the same inputs is skipped, so a stopped
      batch resumes where it stopped.
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
  --view three-quarter|side|top
  --quality draft|standard
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
    shot: Shot,
    gpu: Option<usize>,
    force: bool,
}

fn parse(args: &[String]) -> Result<Request, String> {
    let mut species = Vec::new();
    let mut out = None;
    let mut size = 512_u32;
    let mut shot = Shot::thumbnail("");
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
            "--gpu" => {
                gpu = Some(
                    value("--gpu")?
                        .parse()
                        .map_err(|_| "`--gpu` takes an index from `plantlab gpus`")?,
                );
            }
            "--force" => force = true,
            flag @ ("--day" | "--age" | "--view" | "--quality" | "--look" | "--cut-cards") => {
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
        shot,
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
            shot.quality = quality::profile(value)
                .ok_or_else(|| format!("unknown quality `{value}`; use draft or standard"))?;
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
        other => return Err(format!("unknown option `{other}`")),
    }
    Ok(())
}

/// The job for one picture of one species.
fn job(species: &str, kind: Kind, shot: &Shot, size: u32) -> Result<Job, String> {
    let library = Library::builtin();
    let shot = Shot {
        species: species.to_string(),
        ..shot.clone()
    };
    let (name, sheet) = match kind {
        Kind::Thumb => (
            species.to_string(),
            plantlab_scene::single(shot, size, size),
        ),
        Kind::Review => (
            format!("{species}-review"),
            plantlab_scene::review_sheet(&shot, library)?,
        ),
    };
    let key = key(species, kind, &sheet, library);
    Ok(Job { name, sheet, key })
}

/// What decides a picture: the species' spec, the generator's revision
/// and every tile's settings, hashed.
fn key(species: &str, kind: Kind, sheet: &plantlab_scene::Sheet, library: &Library) -> String {
    let mut hash = Sha256::new();
    hash.update(format!(
        "plantlab 1 {kind:?} {}\n",
        plantgen::GENERATOR_REVISION
    ));
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
        .map(|id| job(id, kind, &request.shot, request.size))
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
        let mut size = 512;
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
                "size" => size = parse_size(&text).map_err(at)?,
                option => apply(&mut shot, option, &text).map_err(at)?,
            }
        }
        let species = species.ok_or_else(|| at("missing \"species\"".into()))?;
        jobs.push(job(&species, kind, &shot, size).map_err(at)?);
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
        let a = job("acer-macrophyllum", Kind::Thumb, &shot, 512).expect("a job");
        let b = job("acer-macrophyllum", Kind::Thumb, &shot, 512).expect("a job");
        let c = job("acer-macrophyllum", Kind::Thumb, &shot, 256).expect("a job");
        assert_eq!(a.key, b.key);
        assert_ne!(a.key, c.key);
    }
}
