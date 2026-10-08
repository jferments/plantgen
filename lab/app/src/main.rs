//! `plantlab`: grow PlantGen's plants under chosen conditions and look at
//! them. This first version renders without a window: `plantlab thumbs`
//! draws each species' thumbnail on the GPU (a 4090 or lavapipe) and
//! writes a PNG and a JSON sidecar per species. The window comes later
//! (design: `PLANTLAB-DESIGN.md` in the project's workspace).
//!
//! PlantLab simulates nothing itself: every plant is grown by PlantGen's
//! own code ([`plantgen::drawing`]), exactly as `plantc` grows it, and
//! turned into draw-ready data by `plantlab-scene`. This program only
//! draws that data.

mod render;

use std::path::PathBuf;
use std::process::ExitCode;

use plantgen::library::Library;
use plantgen::quality;
use plantlab_scene::{Look, Shot, View};

const USAGE: &str = "\
plantlab: grow PlantGen's plants and render them

usage:
  plantlab thumbs SPECIES... --out DIR [--size PX] [--day N] [--age YEARS]
                  [--view three-quarter|side|top] [--quality draft|standard]
                  [--look review|photo]
      Render a thumbnail of each species (a library id, or `all`) on the
      GPU: DIR/ID.png and DIR/ID.json, what the picture shows. The review
      look (default) is fixed so species compare fairly; the photo look
      adds soft shadows, sky light, ambient occlusion and a tone map.
  plantlab help
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
        Some("thumbs") => thumbs(&args[1..]),
        None | Some("help" | "-h" | "--help") => {
            print!("{USAGE}");
            Ok(())
        }
        Some(other) => Err(format!("unknown command `{other}`; run `plantlab help`")),
    }
}

/// What `thumbs` was asked for.
struct Thumbs {
    species: Vec<String>,
    out: PathBuf,
    size: u32,
    shot: Shot,
}

fn parse_thumbs(args: &[String]) -> Result<Thumbs, String> {
    let mut species = Vec::new();
    let mut out = None;
    let mut size = 512_u32;
    let mut shot = Shot::thumbnail("");
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        let mut value = |flag: &str| {
            rest.next()
                .cloned()
                .ok_or_else(|| format!("`{flag}` needs a value"))
        };
        match arg.as_str() {
            "--out" => out = Some(PathBuf::from(value("--out")?)),
            "--size" => {
                size = value("--size")?
                    .parse()
                    .ok()
                    .filter(|px| (64..=4096).contains(px))
                    .ok_or("`--size` must be 64 to 4096 pixels")?;
            }
            "--day" => {
                shot.day = value("--day")?
                    .parse()
                    .ok()
                    .filter(|day| (1.0..=365.0).contains(day))
                    .ok_or("`--day` must be 1 to 365")?;
            }
            "--age" => {
                shot.age = Some(
                    value("--age")?
                        .parse()
                        .ok()
                        .filter(|age: &f64| *age > 0.0)
                        .ok_or("`--age` must be a positive number of years")?,
                );
            }
            "--view" => {
                let name = value("--view")?;
                shot.view =
                    View::from_name(&name).ok_or_else(|| format!("unknown view `{name}`"))?;
            }
            "--look" => {
                let name = value("--look")?;
                shot.look = Look::from_name(&name)
                    .ok_or_else(|| format!("unknown look `{name}`; use review or photo"))?;
            }
            "--quality" => {
                let name = value("--quality")?;
                shot.quality = quality::profile(&name)
                    .ok_or_else(|| format!("unknown quality `{name}`; use draft or standard"))?;
            }
            flag if flag.starts_with("--") => return Err(format!("unknown flag `{flag}`")),
            id => species.push(id.to_string()),
        }
    }
    let library = Library::builtin();
    if species.iter().any(|id| id == "all") {
        species = library
            .species()
            .iter()
            .map(|entry| entry.id.clone())
            .collect();
    }
    if species.is_empty() {
        return Err("name at least one species, or `all`".into());
    }
    let out = out.ok_or("missing `--out DIR`")?;
    Ok(Thumbs {
        species,
        out,
        size,
        shot,
    })
}

fn thumbs(args: &[String]) -> Result<(), String> {
    let request = parse_thumbs(args)?;
    std::fs::create_dir_all(&request.out)
        .map_err(|error| format!("cannot create {}: {error}", request.out.display()))?;
    let jobs = request
        .species
        .iter()
        .map(|id| render::Job {
            name: id.clone(),
            shot: Shot {
                species: id.clone(),
                ..request.shot.clone()
            },
        })
        .collect();
    render::run(jobs, request.out, request.size, request.size)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(text: &str) -> Vec<String> {
        text.split_whitespace().map(str::to_string).collect()
    }

    #[test]
    fn thumbs_reads_its_flags() {
        let parsed = parse_thumbs(&args(
            "acer-macrophyllum --out /tmp/x --size 256 --day 120 --view side --look photo",
        ))
        .expect("parses");
        assert_eq!(parsed.species, ["acer-macrophyllum"]);
        assert_eq!(parsed.size, 256);
        assert!((parsed.shot.day - 120.0).abs() < 1e-12);
        assert_eq!(parsed.shot.view, View::Side);
        assert_eq!(parsed.shot.look, Look::Photo);
    }

    #[test]
    fn thumbs_refuses_what_it_cannot_do() {
        assert!(parse_thumbs(&args("--out /tmp/x")).is_err());
        assert!(parse_thumbs(&args("x --out /tmp/x --size 9")).is_err());
        assert!(parse_thumbs(&args("x --out /tmp/x --day 400")).is_err());
        assert!(parse_thumbs(&args("x --out /tmp/x --wat")).is_err());
        assert!(parse_thumbs(&args("x")).is_err());
    }

    #[test]
    fn all_means_every_library_species() {
        let parsed = parse_thumbs(&args("all --out /tmp/x")).expect("parses");
        assert_eq!(parsed.species.len(), Library::builtin().species().len());
    }
}
