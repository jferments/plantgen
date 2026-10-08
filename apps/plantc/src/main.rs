//! `plantc`: the plant compiler.
//!
//! Grows species from their specs, renders previews for review, and builds
//! content-addressed `.afterplant` packages. It runs offline on the CPU and
//! never opens a window. Run `plantc help` for usage; see
//! `docs/user/PLANTS.md` and `docs/developer/PLANTS.md`.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::OnceLock;
use std::time::Instant;
use std::{env, fmt, fs};

use plantgen::body::BodyLook;
use plantgen::conditions::Conditions;
use plantgen::graph::{GraphOrgan, OrganType, PlantGraph};
use plantgen::ground::{self, GROUND_LOOK_SIZE};
use plantgen::grow::{Growth, GrowthSettings, grow};
use plantgen::library::Library;
use plantgen::litter;
use plantgen::looks::{self, Look, Stage};
use plantgen::lsys::{Limits, Neighbourhood, Program};
use plantgen::math::Vec3;
use plantgen::mesh::{self, PlantMesh};
use plantgen::package::{self, Inputs};
use plantgen::preview::{self, PreviewOptions, View};
use plantgen::quality::{self, Quality};
use plantgen::raster;
use plantgen::spec::{self, Environment, PlantSpec, Variant};
use plantgen::templates::{self, Templates};

/// Why a command stopped early.
enum Failure {
    Message(String),
    /// Standard output was closed, as by `plantc … | head`: not an error.
    OutputClosed,
}

impl From<String> for Failure {
    fn from(message: String) -> Self {
        Self::Message(message)
    }
}

impl From<&str> for Failure {
    fn from(message: &str) -> Self {
        Self::Message(message.to_string())
    }
}

fn write_line(args: fmt::Arguments<'_>) -> Result<(), Failure> {
    let mut stdout = io::stdout().lock();
    stdout
        .write_fmt(args)
        .and_then(|()| stdout.write_all(b"\n"))
        .map_err(|error| match error.kind() {
            io::ErrorKind::BrokenPipe => Failure::OutputClosed,
            _ => Failure::Message(format!("cannot write to standard output: {error}")),
        })
}

/// `println!` that stops the command when standard output is closed
/// instead of panicking.
macro_rules! out {
    ($($arg:tt)*) => {
        write_line(format_args!($($arg)*))?
    };
}

fn main() -> ExitCode {
    match run() {
        Ok(()) | Err(Failure::OutputClosed) => ExitCode::SUCCESS,
        Err(Failure::Message(error)) => {
            eprintln!("plantc: {error}");
            ExitCode::FAILURE
        }
    }
}

/// The library every command reads: `--library DIR`'s, else the built-in
/// one.
static LIBRARY: OnceLock<Library> = OnceLock::new();

fn library() -> &'static Library {
    LIBRARY.get().unwrap_or_else(|| Library::builtin())
}

fn run() -> Result<(), Failure> {
    let mut args: Vec<String> = env::args().skip(1).collect();
    // `--library DIR` may stand anywhere and serves every command.
    if let Some(at) = args.iter().position(|arg| arg == "--library") {
        let folder = args
            .get(at + 1)
            .ok_or("`--library` needs a folder")?
            .clone();
        args.drain(at..=at + 1);
        let read = Library::from_dir(Path::new(&folder)).map_err(|error| error.to_string())?;
        let _ = LIBRARY.set(read);
    }
    if args.is_empty() {
        return print_usage();
    }
    let command = args.remove(0);
    match command.as_str() {
        "list" => list(),
        "check" => check(&args),
        "spec" => spec_command(&args),
        "rules" => rules_command(&args),
        "grow" => grow_command(&args),
        "render" => render_command(&args),
        "sheet" => sheet_command(&args),
        "parts" => parts_command(&args),
        "year" => year_command(&args),
        "lineup" => lineup_command(&args),
        "atlas" => atlas_command(&args),
        "measure" => measure_command(&args),
        "ground" => ground_command(&args),
        "build" => build_command(&args),
        "inspect" => inspect_command(&args),
        "sources" => sources_command(&args),
        "help" | "--help" | "-h" => print_usage(),
        other => Err(format!("unknown command `{other}`; run `plantc help`").into()),
    }
}

fn print_usage() -> Result<(), Failure> {
    out!(
        "plantc: grow, preview and package procedural plants

Usage:
  plantc list
      List the species and plant programs, by family.
  plantc check <species|spec.json|program.lsys>
      Check a spec or a program and report the first problem with its line;
      for a species, name the rank files its spec stands on. Every value of
      a spec needs an evidence note citing a source in library/sources/.
  plantc spec <species>
      Print the species' spec value by value, each with the file that set
      it: its own spec.json or a rank file above it (family.json,
      genus.json, library/_ranks/), or a rule on its traits; then each
      evidence note's file, its traits, and each rule it inherits with
      what came of it.
  plantc rules
      List every rule that turns traits into spec values, by its home:
      the rank file (or species) that holds it. Then the general ones,
      with no taxonomic home: rules at all plants, and the parameter
      defaults of programs not named for a taxon. The list should shrink.
  plantc grow <species|spec.json> [--env ENV] [--seed N] [--years N]
      Grow one variant and print its size at every keyframe.
  plantc render <species|spec.json> --out FILE.png [--env ENV] [--seed N]
                [--age N] [--view side|three-quarter|top] [--lod 0-3]
                [--size PIXELS] [--quality draft|standard]
                [--focus X,Y,Z [--span METRES]] [--parts on]
      Render one variant at one age, with a 1.8 m figure for scale, or
      a rod in 10 cm stripes beside a plant lower than 1.5 m. --focus
      frames a close-up of SPAN metres (default 0.5) round a point of
      the plant, metres from its foot with +Y up. --parts on (level 0)
      draws every flower, fruit, cone and needle shoot as its part mesh,
      as WorldLab does near the camera.
  plantc sheet <species|spec.json> --out FILE.png [--seed N] [--size PIXELS]
      Render every keyframe age (columns) in every environment (rows).
  plantc parts <species|spec.json> --out FILE.png [--env ENV] [--seed N]
               [--size PIXELS]
      Render each organ type alone, its median size at the oldest age
      printed (drawn at least 25 cm long): a row per type, its solid
      (LOD0) left and its card (LOD1) right.
  plantc year <species|spec.json> --out FILE.png [--env ENV] [--seed N]
              [--size PIXELS] [--days D,D,...|stages]
      Draw each organ type with a season through the year: a row per
      type, a column per day (default the middle of each month; `stages`
      the middle of its bud, flower, unripe, ripening and ripe stages),
      the organ alone and solid (LOD0) as it is that day, at one scale
      per row; each day's stage printed.
  plantc lineup <species|spec.json>... --out FILE.png [--age N] [--seeds N]
                [--view side|three-quarter|top] [--size PIXELS]
      Render a row per species of its first N seeds (default 4) at one
      age (default its oldest keyframe), each row at one scale.
  plantc measure [<species|spec.json>...] [--generator PROGRAM] [--env ENV]
                 [--seed N] [--years N]
      Grow each species (default every one in the library; --generator
      keeps those run by one program) to its oldest keyframe, or N years,
      and print its form: crown width over depth, where it is widest,
      its departure from an ellipse (0 an orb), its lobing, crown width
      over dbh and the median leaf card length.
  plantc atlas <species|spec.json> --out FILE.png
      Draw the species' organ card textures (leaves, needles, flowers) in
      their colours, one per organ, side by side.
  plantc ground --out FILE.png [--seed N] [--looks words|litter]
      Draw the nine plant-made ground looks (needles, leaves, thatch,
      twigs, moss, sphagnum, grass, tussock and cushion), seven to a row,
      or with `--looks litter` the 23 canopy species' litters: each look's
      colour above its relief.
  plantc build <species|spec.json> [--out DIR] [--quality draft|standard]
               [--threads N]
      Build a .afterplant package in DIR (default `plants`) and print its
      key. A package that is already built is not built again.
  plantc inspect <package.afterplant>
      Check every object of a package and summarise it.
  plantc sources [list | cite ID]
      List the sources evidence notes cite (library/sources/<id>.json),
      or every value that cites the source ID: the file holding its note,
      by its path in the library, and the note's path, one per line.

A species is an id (see `plantc list`) or a path to a spec file; a spec
file whose id is a species of the library stands on that species' rank
files.
Every command accepts --library DIR: a folder holding a species tree,
library/<family>/<genus>/<id>/spec.json, and programs, programs/<name>.lsys,
which replace built-in species and programs of the same id or name and add
to them. Its rank files (library/<family>/family.json,
library/<family>/<genus>/genus.json, library/_ranks/<rank>/<name>.json)
replace built-in ones at the same path; each species' spec is its own
spec.json merged onto those above it, the nearer file winning.
grow, render, sheet, atlas and build accept --program FILE.lsys to try a
changed program in place of the species' built-in one. render, sheet,
parts, lineup and build accept --day N (1 to 365, default 196): the day
of the year organs with a season show.
ENV is open, edge, interior or suppressed; without --env, the species'
typical site (its conditions.json) chooses."
    );
    Ok(())
}

/// `--flag value` options after the positional arguments.
struct Options {
    positional: Vec<String>,
    flags: BTreeMap<String, String>,
}

impl Options {
    fn parse(args: &[String], allowed: &[&str]) -> Result<Self, String> {
        let mut positional = Vec::new();
        let mut flags = BTreeMap::new();
        let mut index = 0;
        while index < args.len() {
            let arg = &args[index];
            if let Some(name) = arg.strip_prefix("--") {
                if !allowed.contains(&name) {
                    return Err(format!("unknown option `--{name}`; run `plantc help`"));
                }
                let value = args
                    .get(index + 1)
                    .ok_or_else(|| format!("`--{name}` needs a value"))?;
                flags.insert(name.to_string(), value.clone());
                index += 2;
            } else {
                positional.push(arg.clone());
                index += 1;
            }
        }
        Ok(Self { positional, flags })
    }

    fn one_positional(&self, what: &str) -> Result<&str, String> {
        match self.positional.as_slice() {
            [single] => Ok(single),
            [] => Err(format!("missing {what}")),
            _ => Err("too many arguments".into()),
        }
    }

    fn number<T: std::str::FromStr>(&self, name: &str) -> Result<Option<T>, String> {
        self.flags
            .get(name)
            .map(|text| {
                text.parse()
                    .map_err(|_| format!("`--{name}` expects a number, found `{text}`"))
            })
            .transpose()
    }

    /// The source of `--program FILE.lsys`, if given.
    fn program(&self) -> Result<Option<String>, String> {
        self.flags
            .get("program")
            .map(|path| {
                fs::read_to_string(path)
                    .map(|source| library().chain_of(&source).into_owned())
                    .map_err(|error| format!("cannot read {path}: {error}"))
            })
            .transpose()
    }

    fn environment(&self, spec: &PlantSpec) -> Result<Environment, String> {
        match self.flags.get("env") {
            Some(name) => Environment::from_name(name).ok_or_else(|| {
                format!("unknown environment `{name}`; use open, edge, interior or suppressed")
            }),
            None => Ok(typical_environment(spec)),
        }
    }

    /// `--day N`, the day of the year (1 to 365) organs with a season show;
    /// the packages' default day otherwise.
    fn day(&self) -> Result<f64, String> {
        let day = self.number("day")?.unwrap_or(package::DEFAULT_DAY);
        if (1.0..=365.0).contains(&day) {
            Ok(day)
        } else {
            Err(format!("`--day` must be 1 to 365, found {day}"))
        }
    }

    fn quality(&self) -> Result<Quality, String> {
        let name = self.flags.get("quality").map_or("standard", String::as_str);
        quality::profile(name)
            .ok_or_else(|| format!("unknown quality `{name}`; use draft or standard"))
    }
}

/// The environment of the species' typical site (`conditions.json`) when
/// its variants grow in it, else its first.
fn typical_environment(spec: &PlantSpec) -> Environment {
    library()
        .entry(&spec.id)
        .and_then(|entry| entry.conditions().ok().flatten())
        .and_then(|conditions| conditions.preset)
        .filter(|preset| spec.variants.environments.contains(preset))
        .unwrap_or(spec.variants.environments[0])
}

fn load_spec(name: &str) -> Result<PlantSpec, String> {
    if Path::new(name).extension().is_some_and(|ext| ext == "json") {
        let text =
            fs::read_to_string(name).map_err(|error| format!("cannot read {name}: {error}"))?;
        // On its species' rank files, if the library has its species.
        let text = library().effective_text(name, &text)?.unwrap_or(text);
        PlantSpec::from_json_in(&text, library()).map_err(|error| error.to_string())
    } else {
        library().spec(name).map_err(|error| error.to_string())
    }
}

fn list() -> Result<(), Failure> {
    let library = library();
    let width = library
        .species()
        .iter()
        .map(|entry| entry.id.len())
        .max()
        .unwrap_or(0);
    let mut family = "";
    for entry in library.species() {
        if entry.family != family {
            family = &entry.family;
            let mut name = family.to_owned();
            name[..1].make_ascii_uppercase();
            out!("{name}:");
        }
        let id = &entry.id;
        let spec = library.spec(id).map_err(|error| error.to_string())?;
        out!(
            "  {id:<width$}  {} ({}), program `{}`{}",
            spec.taxon.common_name,
            spec.taxon.scientific_name,
            spec.generator.program,
            if entry.path.is_some() {
                ", from the library folder"
            } else {
                ""
            }
        );
    }
    out!("Programs:");
    for name in library.programs() {
        out!("  {name}");
    }
    Ok(())
}

fn sources_command(args: &[String]) -> Result<(), Failure> {
    let library = library();
    match args.first().map(String::as_str) {
        None | Some("list") => {
            for source in library.sources() {
                out!(
                    "{}  tier {}, {:?}: {}, {} ({})",
                    source.id,
                    source.tier,
                    source.kind,
                    source.title,
                    source.authors,
                    source.year
                );
            }
            Ok(())
        }
        Some("cite") => {
            let id = args.get(1).ok_or("`sources cite` needs a source id")?;
            let found = library.citations(id)?;
            for citation in &found {
                out!("{} {}", citation.file, citation.path);
            }
            if library.source(id).is_none() {
                eprintln!("plantc: the library holds no source `{id}`");
            }
            eprintln!("plantc: {} values cite `{id}`", found.len());
            Ok(())
        }
        Some(other) => {
            Err(format!("unknown `sources` command `{other}`; use `list` or `cite ID`").into())
        }
    }
}

fn check(args: &[String]) -> Result<(), Failure> {
    let options = Options::parse(args, &[])?;
    let target = options.one_positional("a species, spec file or program file")?;
    if Path::new(target)
        .extension()
        .is_some_and(|ext| ext == "lsys")
    {
        let source =
            fs::read_to_string(target).map_err(|error| format!("cannot read {target}: {error}"))?;
        let program = Program::compile(&library().chain_of(&source))
            .map_err(|error| format!("{target}:{error}"))?;
        out!(
            "{target}: program `{}` revision {} is valid: {} parameters, {} symbols",
            program.name,
            program.revision,
            program.params.len(),
            program.symbols.len()
        );
        return Ok(());
    }
    let spec = load_spec(target)?;
    let (program, _) = spec
        .program_in(library())
        .map_err(|error| error.to_string())?;
    // Per-value evidence, on the spec as written: a file's own text, or a
    // species' spec as it inherits it.
    let text = if Path::new(target)
        .extension()
        .is_some_and(|ext| ext == "json")
    {
        fs::read_to_string(target).map_err(|error| format!("cannot read {target}: {error}"))?
    } else {
        library()
            .entry(target)
            .ok_or_else(|| format!("no species `{target}`"))?
            .source()
            .to_string()
    };
    let document: serde_json::Value =
        serde_json::from_str(&text).map_err(|error| format!("{target}: {error}"))?;
    PlantSpec::check_evidence(&document, library())
        .map_err(|error| format!("species `{}`: {error}", spec.id))?;
    out!(
        "{}: valid; program `{}` revision {}, {} variants, keyframes {:?}; every value has an evidence note citing a source",
        spec.id,
        program.name,
        program.revision,
        spec.variant_list().len(),
        spec.growth.keyframes
    );
    if Path::new(target)
        .extension()
        .is_none_or(|ext| ext != "json")
    {
        let inherited = library()
            .inherited(target)
            .map_err(|error| error.to_string())?;
        if inherited.chain.len() > 1 {
            out!("  stands on {}", inherited.chain[1..].join(", "));
        }
    }
    Ok(())
}

fn spec_command(args: &[String]) -> Result<(), Failure> {
    let options = Options::parse(args, &[])?;
    let id = options.one_positional("a species")?;
    let inherited = library().inherited(id).map_err(|error| error.to_string())?;
    match inherited.chain.split_first() {
        Some((own, above)) if !above.is_empty() => {
            out!("{id}: {own} on {}", above.join(", "));
        }
        _ => out!("{id}: {}", inherited.chain.join(", ")),
    }
    let spec = inherited
        .spec
        .as_object()
        .ok_or("a spec is a JSON object")?;
    for (path, file) in &inherited.origins {
        let value = plantgen::inherit::find(spec, path).map(ToString::to_string);
        out!("  {path} = {}  ({file})", value.unwrap_or_default());
    }
    if !inherited.notes.is_empty() {
        out!("evidence:");
        for (path, file) in &inherited.notes {
            out!("  {path}  ({file})");
        }
    }
    if !inherited.traits.is_empty() {
        out!("traits:");
        for (key, (value, file)) in &inherited.traits {
            out!("  {key} = {value}  ({file})");
        }
    }
    if !inherited.rules.is_empty() {
        out!("rules:");
        for (path, (file, outcome)) in &inherited.rules {
            match outcome {
                None => out!("  {path}  ({file}): set"),
                Some(reason) => out!("  {path}  ({file}): not set, {reason}"),
            }
        }
    }
    Ok(())
}

fn rules_command(args: &[String]) -> Result<(), Failure> {
    let options = Options::parse(args, &[])?;
    if !options.positional.is_empty() {
        return Err("`plantc rules` takes no arguments".into());
    }
    let library = library();
    let mut homed = Vec::new();
    let mut at_all_plants = 0;
    for rank in library.ranks() {
        for (at, part) in rank.parts() {
            for (path, rule) in &part.rules {
                if rule.is_null() {
                    continue;
                }
                let place = if at.is_empty() {
                    String::new()
                } else {
                    format!(", {at}")
                };
                homed.push(format!(
                    "  {path}  ({} {}: {}{place})",
                    rank.rank, rank.name, rank.file
                ));
                if rank.rank == "kingdom" {
                    at_all_plants += 1;
                }
            }
        }
    }
    for entry in library.species() {
        let own: serde_json::Value =
            serde_json::from_str(entry.own_source()).map_err(|error| error.to_string())?;
        if let Some(rules) = own.get("rules").and_then(serde_json::Value::as_object) {
            for path in rules.keys() {
                homed.push(format!(
                    "  {path}  (species {}: {})",
                    entry.id,
                    entry.file()
                ));
            }
        }
    }
    out!("Rules, by their home ({}):", homed.len());
    for line in &homed {
        out!("{line}");
    }
    // A program is homed when it is named for a taxon of the library.
    let mut taxa: std::collections::BTreeSet<String> = library
        .species()
        .iter()
        .flat_map(|entry| [entry.family.clone(), entry.genus.clone()])
        .collect();
    taxa.extend(
        library
            .ranks()
            .map(|rank| plantgen::inherit::file_name(&rank.name)),
    );
    let general: Vec<(&String, usize)> = library
        .program_params()
        .iter()
        .filter(|(name, _)| !taxa.contains(*name))
        .map(|(name, params)| (name, params.len()))
        .collect();
    let defaults: usize = general.iter().map(|(_, count)| count).sum();
    out!(
        "General, with no taxonomic home: {} rules at all plants; {defaults} parameter defaults in {} programs named for no taxon",
        at_all_plants,
        general.len()
    );
    out!(
        "  {}",
        general
            .iter()
            .map(|(name, count)| format!("{name} {count}"))
            .collect::<Vec<_>>()
            .join(", ")
    );
    Ok(())
}

/// The spec's neighbourhood for an environment, or the default one.
fn neighbourhood(spec: &PlantSpec, environment: Environment) -> Neighbourhood {
    spec.variant_list()
        .into_iter()
        .find(|variant| variant.environment == environment)
        .map_or_else(
            || environment.neighbourhood(),
            |variant| variant.neighbourhood,
        )
}

fn grow_variant(
    spec: &PlantSpec,
    program: Option<&str>,
    environment: Environment,
    seed: u64,
    keyframes: Vec<f64>,
    years: f64,
) -> Result<Growth, String> {
    let (program, params) = match program {
        Some(source) => spec.program_from(source),
        None => spec.program_in(library()),
    }
    .map_err(|error| error.to_string())?;
    let settings = GrowthSettings {
        seed,
        dt: spec.growth.step,
        years,
        keyframes,
        conditions: Conditions::in_neighbourhood(environment, neighbourhood(spec, environment)),
        limits: Limits::default(),
        host: spec
            .host_geometry_in(library())
            .map_err(|error| error.to_string())?,
    };
    grow(&program, &params, &settings).map_err(|error| format!("{}: {error}", spec.id))
}

fn describe(graph: &PlantGraph, types: &[OrganType]) -> String {
    if graph.segments.is_empty() && graph.organs.is_empty() {
        return format!(
            "age {:>5.1}  nothing left: the plant has shed every part",
            graph.age
        );
    }
    let (mean_light, low_light) = graph.organ_light().unwrap_or((0.0, 0.0));
    format!(
        "age {:>5.1}  height {:>5} m  dbh {:>5} cm  crown base {:>5} m  crown radius {:>5} m  \
         segments {:>6}  organs {:>6}  leaf area {:>6} m²  organ light {mean_light:.2} (lowest tenth {low_light:.2})",
        graph.age,
        package::size_text(graph.height),
        graph
            .diameter_at_breast_height()
            .map_or_else(|| "-".to_string(), |d| format!("{:.1}", d * 100.0)),
        graph
            .crown_base()
            .map_or_else(|| "-".to_string(), package::size_text),
        package::size_text(graph.crown_radius()),
        graph.segments.len(),
        graph.organs.len(),
        package::size_text(graph.leaf_area(types))
    )
}

fn measure_command(args: &[String]) -> Result<(), Failure> {
    let options = Options::parse(args, &["generator", "env", "seed", "years"])?;
    let ids: Vec<String> = if options.positional.is_empty() {
        library()
            .species()
            .iter()
            .map(|entry| entry.id.clone())
            .collect()
    } else {
        options.positional.clone()
    };
    let wanted = options.flags.get("generator");
    out!(
        "{:<28} {:>5} {:>6} {:>6} {:>6} {:>6} {:>6} {:>6} {:>5} {:>6} {:>6}",
        "species",
        "age",
        "height",
        "dbh",
        "crown",
        "w/d",
        "widest",
        "ellip",
        "lobe",
        "w/dbh",
        "leaf"
    );
    for id in &ids {
        let spec = load_spec(id)?;
        if wanted.is_some_and(|program| *program != spec.generator.program) {
            continue;
        }
        let environment = options.environment(&spec)?;
        let seed = options.number("seed")?.unwrap_or(spec.variants.seeds[0]);
        let years = options.number("years")?.unwrap_or(spec.growth.years);
        let growth = grow_variant(&spec, None, environment, seed, vec![years], years)?;
        let Some(graph) = growth.keyframes.last() else {
            continue;
        };
        let cm = |value: Option<f64>| {
            value.map_or_else(|| "-".to_string(), |v| format!("{:.1}", v * 100.0))
        };
        match plantgen::form::measure(graph, &growth.organ_types) {
            Some(form) => out!(
                "{:<28} {:>5.0} {:>6.1} {:>6} {:>6.1} {:>6.2} {:>6.2} {:>6.2} {:>5.2} {:>6} {:>6}",
                spec.id,
                graph.age,
                form.height,
                cm(form.dbh),
                form.crown_radius * 2.0,
                form.width_to_depth,
                form.widest_at,
                form.ellipse_departure,
                form.lobing,
                form.width_to_dbh
                    .map_or_else(|| "-".to_string(), |r| format!("{r:.0}")),
                cm(form.leaf_length)
            ),
            None => out!(
                "{:<28} {:>5.0} too few living organs to measure",
                spec.id,
                graph.age
            ),
        }
    }
    Ok(())
}

fn grow_command(args: &[String]) -> Result<(), Failure> {
    let options = Options::parse(args, &["env", "seed", "years", "program"])?;
    let spec = load_spec(options.one_positional("a species")?)?;
    let program = options.program()?;
    let environment = options.environment(&spec)?;
    let seed = options.number("seed")?.unwrap_or(spec.variants.seeds[0]);
    let years = options.number("years")?.unwrap_or(spec.growth.years);
    // The spec's ages, the ages it gives reference sizes for, and the end.
    let mut keyframes: Vec<f64> = spec
        .growth
        .keyframes
        .iter()
        .copied()
        .chain(
            spec.allometry
                .iter()
                .filter(|point| point.environment == environment)
                .map(|point| point.age),
        )
        .filter(|age| *age <= years)
        .collect();
    keyframes.push(years);
    keyframes.sort_by(f64::total_cmp);
    keyframes.dedup();
    let started = Instant::now();
    let growth = grow_variant(
        &spec,
        program.as_deref(),
        environment,
        seed,
        keyframes,
        years,
    )?;
    out!(
        "{} ({}), {} environment, seed {seed}: grown in {:.1} s; peak {} modules, {} segments, {} organs",
        spec.taxon.common_name,
        spec.id,
        environment.name(),
        started.elapsed().as_secs_f64(),
        growth.stats.peak_modules,
        growth.stats.peak_segments,
        growth.stats.peak_organs
    );
    for graph in &growth.keyframes {
        out!("  {}", describe(graph, &growth.organ_types));
    }
    let variant = Variant {
        environment,
        seed,
        neighbourhood: neighbourhood(&spec, environment),
        class: None,
    };
    for record in package::compare_allometry(&spec, &variant, &growth) {
        out!("  reference at {} years: {}", record.age, record.describe());
    }
    Ok(())
}

#[allow(clippy::too_many_lines)]
fn render_command(args: &[String]) -> Result<(), Failure> {
    let options = Options::parse(
        args,
        &[
            "env", "seed", "age", "view", "lod", "size", "out", "quality", "program", "focus",
            "span", "alone", "day", "parts",
        ],
    )?;
    let day = options.day()?;
    let program = options.program()?;
    let spec = load_spec(options.one_positional("a species")?)?;
    let alone = options.flags.contains_key("alone");
    let out = options.flags.get("out").ok_or("missing `--out FILE.png`")?;
    let environment = options.environment(&spec)?;
    let seed = options.number("seed")?.unwrap_or(spec.variants.seeds[0]);
    let age = options
        .number("age")?
        .unwrap_or_else(|| spec.growth.keyframes.iter().copied().fold(0.0, f64::max));
    let view = match options.flags.get("view") {
        Some(name) => View::from_name(name).ok_or_else(|| format!("unknown view `{name}`"))?,
        None => View::Side,
    };
    let level: usize = options.number("lod")?.unwrap_or(0);
    let parted = match options.flags.get("parts").map(String::as_str) {
        None | Some("off") => false,
        Some("on") => true,
        Some(other) => return Err(format!("`--parts` must be on or off, found `{other}`").into()),
    };
    if parted && level != 0 {
        return Err("`--parts` draws the nearest level: use it with `--lod 0`".into());
    }
    let quality = options.quality()?;
    let lod = quality.lods.get(level).ok_or("`--lod` must be 0 to 3")?;
    let size: usize = options.number("size")?.unwrap_or(900);
    let focus = match options.flags.get("focus") {
        Some(text) => {
            let point: Vec<f64> = text
                .split(',')
                .map(|part| part.trim().parse::<f64>())
                .collect::<Result<_, _>>()
                .map_err(|_| format!("`--focus` expects X,Y,Z in metres, found `{text}`"))?;
            let [x, y, z] = point[..] else {
                return Err(format!("`--focus` expects X,Y,Z in metres, found `{text}`").into());
            };
            let span: f64 = options.number("span")?.unwrap_or(0.5);
            Some((Vec3::new(x, y, z), span))
        }
        None => None,
    };
    let growth = grow_variant(&spec, program.as_deref(), environment, seed, vec![age], age)?;
    let (mut looks, sizes) = looks_of(&spec, &growth, day);
    let graph = &*drawn(&growth.keyframes[0], &sizes);
    let bodies = bodies_of(&spec, &growth);
    let mut plant = if parted {
        // As a renderer that draws part meshes shows the plant near the
        // camera: each organ of a type with them drawn as one.
        let parts = plantgen::parts::part_meshes(&looks);
        let types = plantgen::parts::types(&looks, &parts);
        let mut plant = mesh::build_with(
            graph,
            &looks,
            &bodies,
            &spec.appearance,
            &lod.for_height(graph.height),
            level,
            Some(&types),
        );
        plantgen::parts::place(&mut plant, &looks, &parts);
        plant
    } else {
        mesh::build(
            graph,
            &looks,
            &bodies,
            &spec.appearance,
            &lod.for_height(graph.height),
            level,
        )
    };
    // A guest is drawn on its host (plant forms F7), unless `--alone`.
    // Neither has fleshy bodies, so the templates are the organs' alone.
    let mut on = false;
    if let (Some(host), false, true) = (&spec.host, alone, bodies.is_empty()) {
        let host_spec = library()
            .spec(&host.species)
            .map_err(|error| error.to_string())?;
        let host_growth = grow_variant(
            &host_spec,
            None,
            host.environment,
            host.seed,
            vec![host.age],
            host.age,
        )?;
        let (host_looks, host_sizes) = looks_of(&host_spec, &host_growth, day);
        let host_graph = &*drawn(&host_growth.keyframes[0], &host_sizes);
        let host_plant = mesh::build(
            host_graph,
            &host_looks,
            &[],
            &host_spec.appearance,
            &lod.for_height(host_graph.height),
            level,
        );
        plant = on_host(host_plant, plant, host_looks.len());
        let mut both = host_looks;
        both.append(&mut looks);
        looks = both;
        on = true;
    }
    let templates = if on {
        // The host's trunk carries its own bark.
        let host_bark = spec
            .host
            .as_ref()
            .and_then(|host| library().spec(&host.species).ok())
            .and_then(|host| host.appearance.bark_params());
        Templates::for_plant(&looks, &[]).with_bark(host_bark)
    } else {
        templates_of(&spec, &looks, &bodies, &growth)
    };
    let image = preview::render(
        &plant,
        &templates,
        &PreviewOptions {
            width: size,
            height: size * 5 / 4,
            view,
            supersample: 2,
            figure: true,
            frame_height: None,
            focus,
        },
    );
    let png = raster::encode_png(
        image.width,
        image.height,
        &image.to_srgb8(Some(preview::SKY)),
    )
    .map_err(|error| format!("cannot encode PNG: {error}"))?;
    fs::write(out, png).map_err(|error| format!("cannot write {out}: {error}"))?;
    out!("{}", describe(graph, &growth.organ_types));
    out!(
        "wrote {out}: LOD{level}, {} triangles ({} wood, {} cards){}",
        plant.triangle_count(),
        plant.wood.triangle_count(),
        plant.cards.len() * 2,
        if plant.tufts.is_empty() {
            String::new()
        } else {
            format!(", {} areoles of solid spines", plant.tufts.len())
        }
    );
    Ok(())
}

/// A guest's mesh drawn on its host's: the host's wood and cards, then the
/// guest's, its cards' templates after the host's `host_types` organ types.
fn on_host(host: PlantMesh, guest: PlantMesh, host_types: usize) -> PlantMesh {
    let mut plant = host;
    let first = u32::try_from(plant.wood.positions.len()).unwrap_or(u32::MAX);
    let wood = guest.wood;
    plant.wood.positions.extend(wood.positions);
    plant.wood.normals.extend(wood.normals);
    plant.wood.uvs.extend(wood.uvs);
    plant.wood.colors.extend(wood.colors);
    plant.wood.births.extend(wood.births);
    plant.wood.sheds.extend(wood.sheds);
    plant.wood.levels.extend(wood.levels);
    plant
        .wood
        .indices
        .extend(wood.indices.iter().map(|index| index + first));
    let shift = u8::try_from(host_types).unwrap_or(u8::MAX);
    plant.cards.extend(guest.cards.into_iter().map(|mut card| {
        card.template = card.template.saturating_add(shift);
        card
    }));
    plant
}

fn sheet_command(args: &[String]) -> Result<(), Failure> {
    let options = Options::parse(args, &["seed", "size", "out", "quality", "program", "day"])?;
    let program = options.program()?;
    let spec = load_spec(options.one_positional("a species")?)?;
    let out = options.flags.get("out").ok_or("missing `--out FILE.png`")?;
    let seed = options.number("seed")?.unwrap_or(spec.variants.seeds[0]);
    let size: usize = options.number("size")?.unwrap_or(360);
    let day = options.day()?;
    let quality = options.quality()?;
    let mut images = Vec::new();
    for environment in &spec.variants.environments {
        let growth = grow_variant(
            &spec,
            program.as_deref(),
            *environment,
            seed,
            spec.growth.keyframes.clone(),
            spec.growth.years,
        )?;
        let (looks, sizes) = looks_of(&spec, &growth, day);
        let bodies = bodies_of(&spec, &growth);
        let templates = templates_of(&spec, &looks, &bodies, &growth);
        let tallest = growth
            .keyframes
            .iter()
            .map(|graph| graph.height)
            .fold(0.0, f64::max);
        for graph in &growth.keyframes {
            let graph = &*drawn(graph, &sizes);
            out!(
                "  {:<10} {}",
                environment.name(),
                describe(graph, &growth.organ_types)
            );
            let plant = mesh::build(
                graph,
                &looks,
                &bodies,
                &spec.appearance,
                &quality.lods[0].for_height(graph.height),
                0,
            );
            images.push(preview::render(
                &plant,
                &templates,
                &PreviewOptions {
                    width: size,
                    height: size * 3 / 2,
                    view: View::Side,
                    supersample: 2,
                    figure: true,
                    frame_height: Some(if graph.height < tallest * 0.3 {
                        graph.height
                    } else {
                        tallest
                    }),
                    focus: None,
                },
            ));
        }
    }
    let columns = spec.growth.keyframes.len();
    let (width, height, pixels) = preview::contact_sheet(&images, columns, 8);
    let png = raster::encode_png(width, height, &pixels)
        .map_err(|error| format!("cannot encode PNG: {error}"))?;
    fs::write(out, png).map_err(|error| format!("cannot write {out}: {error}"))?;
    out!(
        "wrote {out}: columns are ages {:?}, rows are environments {:?}",
        spec.growth.keyframes,
        spec.variants
            .environments
            .iter()
            .map(|environment| environment.name())
            .collect::<Vec<_>>()
    );
    Ok(())
}

#[allow(clippy::too_many_lines)]
fn parts_command(args: &[String]) -> Result<(), Failure> {
    let options = Options::parse(
        args,
        &["env", "seed", "size", "out", "quality", "program", "day"],
    )?;
    let program = options.program()?;
    let spec = load_spec(options.one_positional("a species")?)?;
    let out = options.flags.get("out").ok_or("missing `--out FILE.png`")?;
    let environment = options.environment(&spec)?;
    let day = options.day()?;
    let seed = options.number("seed")?.unwrap_or(spec.variants.seeds[0]);
    let size: usize = options.number("size")?.unwrap_or(320);
    let quality = options.quality()?;
    let age = spec.growth.keyframes.iter().copied().fold(0.0, f64::max);
    let growth = grow_variant(&spec, program.as_deref(), environment, seed, vec![age], age)?;
    let graph = &growth.keyframes[0];
    let (looks, stage_sizes) = looks_of(&spec, &growth, day);
    let bodies = bodies_of(&spec, &growth);
    let templates = templates_of(&spec, &looks, &bodies, &growth);
    let mut images = Vec::new();
    let mut rows = Vec::new();
    for (index, look) in looks.iter().enumerate() {
        // A type gone on the day is left out too.
        let stage_size = stage_sizes.get(index).copied().unwrap_or(1.0);
        if stage_size <= 0.0 {
            continue;
        }
        // The type's median size; a type the plant does not carry at its
        // oldest age is left out.
        let mut sizes: Vec<f64> = graph
            .organs
            .iter()
            .filter(|organ| usize::from(organ.organ) == index)
            .map(|organ| organ.size)
            .collect();
        if sizes.is_empty() {
            continue;
        }
        sizes.sort_by(f64::total_cmp);
        let median = sizes[sizes.len() / 2] * stage_size;
        // Drawn at least 25 cm long, so the camera keeps clear of its near
        // plane; the shape is the same at any size. Framed round its card's
        // length or width, whichever is more.
        let length = median.max(0.25);
        let frame = 1.3 * look.shape.aspect().max(1.0);
        let alone = PlantGraph {
            age,
            height: length,
            segments: Vec::new(),
            organs: vec![GraphOrgan {
                id: 1,
                organ: u16::try_from(index).map_err(|_| "too many organ types")?,
                segment: None,
                position: Vec3::ZERO,
                heading: Vec3::Y,
                left: Vec3::X,
                size: length,
                born: 0.0,
                shed: None,
                light: 1.0,
            }],
        };
        for level in [0, 1] {
            let lod = quality
                .lods
                .get(level)
                .ok_or("the quality has too few levels")?;
            let plant = mesh::build(
                &alone,
                &looks,
                &bodies,
                &spec.appearance,
                &lod.for_height(length),
                level,
            );
            images.push(preview::render(
                &plant,
                &templates,
                &PreviewOptions {
                    width: size,
                    height: size,
                    view: View::ThreeQuarter,
                    supersample: 3,
                    figure: false,
                    frame_height: None,
                    focus: Some((Vec3::Y * (length * 0.5), length * frame)),
                },
            ));
        }
        rows.push(format!(
            "{} ({}, {:.3} m)",
            look.organ,
            look.shape.name(),
            median
        ));
    }
    if images.is_empty() {
        return Err("the plant carries no organs at its oldest age".into());
    }
    let (width, height, pixels) = preview::contact_sheet(&images, 2, 8);
    let png = raster::encode_png(width, height, &pixels)
        .map_err(|error| format!("cannot encode PNG: {error}"))?;
    fs::write(out, png).map_err(|error| format!("cannot write {out}: {error}"))?;
    out!(
        "wrote {out}: a row per organ type, its solid (LOD0) left and its card (LOD1) right:\n  {}",
        rows.join("\n  ")
    );
    Ok(())
}

/// The middle of a season's bud, flower, unripe, ripening and ripe stages,
/// as days of the year.
fn stage_days(season: &looks::Season) -> Vec<f64> {
    let ripe = season.ripe + season.ripening;
    let year = |day: f64| ((day - 1.0).rem_euclid(365.0) + 1.0).round();
    [
        f64::midpoint(season.bud, season.flower),
        f64::midpoint(season.flower, season.fruit),
        f64::midpoint(season.fruit, season.ripe.max(season.fruit)),
        season.ripe + season.ripening * 0.5,
        f64::midpoint(ripe, season.fall),
    ]
    .map(year)
    .to_vec()
}

/// The middle of each month: `plantc year`'s days unless given.
const MONTHS: [f64; 12] = [
    15.0, 46.0, 74.0, 105.0, 135.0, 166.0, 196.0, 227.0, 258.0, 288.0, 319.0, 349.0,
];

#[allow(clippy::too_many_lines)]
fn year_command(args: &[String]) -> Result<(), Failure> {
    let options = Options::parse(args, &["env", "seed", "size", "out", "quality", "days"])?;
    let spec = load_spec(options.one_positional("a species")?)?;
    let out = options.flags.get("out").ok_or("missing `--out FILE.png`")?;
    let environment = options.environment(&spec)?;
    let seed = options.number("seed")?.unwrap_or(spec.variants.seeds[0]);
    let size: usize = options.number("size")?.unwrap_or(220);
    let quality = options.quality()?;
    let stages = options
        .flags
        .get("days")
        .is_some_and(|text| text == "stages");
    let days: Vec<f64> = match options.flags.get("days").filter(|_| !stages) {
        Some(text) => text
            .split(',')
            .map(|part| part.trim().parse::<f64>())
            .collect::<Result<_, _>>()
            .ok()
            .filter(|days: &Vec<f64>| {
                !days.is_empty() && days.iter().all(|day| (1.0..=365.0).contains(day))
            })
            .ok_or_else(|| format!("`--days` expects days 1 to 365, found `{text}`"))?,
        None => MONTHS.to_vec(),
    };
    let age = spec.growth.keyframes.iter().copied().fold(0.0, f64::max);
    let growth = grow_variant(&spec, None, environment, seed, vec![age], age)?;
    let graph = &growth.keyframes[0];
    let bodies = bodies_of(&spec, &growth);
    let lod = quality.lods.first().ok_or("the quality has no levels")?;
    let mut images = Vec::new();
    let mut rows = Vec::new();
    for (index, organ) in growth.organ_types.iter().enumerate() {
        let Some(season) = spec
            .appearance
            .organs
            .get(&organ.name)
            .and_then(|look| look.season.as_ref())
        else {
            continue;
        };
        let mut sizes: Vec<f64> = graph
            .organs
            .iter()
            .filter(|grown| usize::from(grown.organ) == index)
            .map(|grown| grown.size)
            .collect();
        if sizes.is_empty() {
            continue;
        }
        sizes.sort_by(f64::total_cmp);
        // Each day's organ at its stage's share of the median size in
        // fruit, framed as the fruit at least 25 cm long would be.
        let length = sizes[sizes.len() / 2].max(0.25);
        let row_days = if stages {
            stage_days(season)
        } else {
            days.clone()
        };
        // The row is framed round its largest stage, by its card's length
        // or width, whichever is more.
        let largest = row_days
            .iter()
            .map(|&day| {
                let (looks, shares) = looks_of(&spec, &growth, day);
                shares.get(index).copied().unwrap_or(1.0)
                    * looks
                        .get(index)
                        .map_or(1.0, |look| look.shape.aspect().max(1.0))
            })
            .fold(1.0, f64::max);
        let mut stages = Vec::new();
        for &day in &row_days {
            let (looks, shares) = looks_of(&spec, &growth, day);
            let share = shares.get(index).copied().unwrap_or(1.0);
            let alone = PlantGraph {
                age,
                height: length,
                segments: Vec::new(),
                organs: if share > 0.0 {
                    vec![GraphOrgan {
                        id: 1,
                        organ: u16::try_from(index).map_err(|_| "too many organ types")?,
                        segment: None,
                        position: Vec3::ZERO,
                        heading: Vec3::Y,
                        left: Vec3::X,
                        size: length * share,
                        born: 0.0,
                        shed: None,
                        light: 1.0,
                    }]
                } else {
                    Vec::new()
                },
            };
            let plant = mesh::build(
                &alone,
                &looks,
                &bodies,
                &spec.appearance,
                &lod.for_height(length),
                0,
            );
            images.push(preview::render(
                &plant,
                &templates_of(&spec, &looks, &bodies, &growth),
                &PreviewOptions {
                    width: size,
                    height: size,
                    view: View::ThreeQuarter,
                    supersample: 3,
                    figure: false,
                    frame_height: None,
                    focus: Some((Vec3::Y * (length * largest * 0.5), length * largest * 1.3)),
                },
            ));
            stages.push(match season.stage(day) {
                Stage::Gone => format!("{day}: gone"),
                Stage::Bud => format!("{day}: bud"),
                Stage::Flower => format!("{day}: flower"),
                Stage::Fruit { unripe } => {
                    format!("{day}: fruit, {:.0}% unripe", unripe * 100.0)
                }
            });
        }
        rows.push(format!("{}: {}", organ.name, stages.join("; ")));
    }
    if images.is_empty() {
        return Err(format!("{} has no organ with a season", spec.id).into());
    }
    let columns = if stages { 5 } else { days.len() };
    let (width, height, pixels) = preview::contact_sheet(&images, columns, 6);
    let png = raster::encode_png(width, height, &pixels)
        .map_err(|error| format!("cannot encode PNG: {error}"))?;
    fs::write(out, png).map_err(|error| format!("cannot write {out}: {error}"))?;
    out!(
        "wrote {out}: a row per organ type with a season, a column per day:\n  {}",
        rows.join("\n  ")
    );
    Ok(())
}

fn lineup_command(args: &[String]) -> Result<(), Failure> {
    let options = Options::parse(
        args,
        &["age", "seeds", "size", "view", "out", "quality", "day"],
    )?;
    let out = options.flags.get("out").ok_or("missing `--out FILE.png`")?;
    let day = options.day()?;
    if options.positional.is_empty() {
        return Err("name at least one species".into());
    }
    let count: usize = options.number("seeds")?.unwrap_or(4).max(1);
    let size: usize = options.number("size")?.unwrap_or(300);
    let view = match options.flags.get("view") {
        Some(name) => View::from_name(name).ok_or_else(|| format!("unknown view `{name}`"))?,
        None => View::Side,
    };
    let quality = options.quality()?;
    let mut images = Vec::new();
    for name in &options.positional {
        let spec = load_spec(name)?;
        let age = options
            .number("age")?
            .unwrap_or_else(|| spec.growth.keyframes.iter().copied().fold(0.0, f64::max));
        let environment = spec.variants.environments[0];
        // The spec's seeds, then the numbers after its largest.
        let largest = spec.variants.seeds.iter().copied().max().unwrap_or(0);
        let seeds: Vec<u64> = spec
            .variants
            .seeds
            .iter()
            .copied()
            .chain((1..).map(|extra| largest + extra))
            .take(count)
            .collect();
        let mut grown = Vec::with_capacity(seeds.len());
        for seed in seeds {
            let growth = grow_variant(&spec, None, environment, seed, vec![age], age)?;
            out!(
                "  {:<26} seed {seed:<3} {}",
                spec.id,
                describe(&growth.keyframes[0], &growth.organ_types)
            );
            grown.push(growth);
        }
        let tallest = grown
            .iter()
            .map(|growth| growth.keyframes[0].height)
            .fold(0.0, f64::max);
        for growth in &grown {
            let (looks, sizes) = looks_of(&spec, growth, day);
            let graph = &*drawn(&growth.keyframes[0], &sizes);
            let bodies = bodies_of(&spec, growth);
            let plant = mesh::build(
                graph,
                &looks,
                &bodies,
                &spec.appearance,
                &quality.lods[0].for_height(graph.height),
                0,
            );
            images.push(preview::render(
                &plant,
                &templates_of(&spec, &looks, &bodies, growth),
                &PreviewOptions {
                    width: size,
                    height: size * 3 / 2,
                    view,
                    supersample: 2,
                    figure: true,
                    frame_height: Some(tallest),
                    focus: None,
                },
            ));
        }
    }
    let (width, height, pixels) = preview::contact_sheet(&images, count, 8);
    let png = raster::encode_png(width, height, &pixels)
        .map_err(|error| format!("cannot encode PNG: {error}"))?;
    fs::write(out, png).map_err(|error| format!("cannot write {out}: {error}"))?;
    out!(
        "wrote {out}: a row per species ({}), {count} seeds each",
        options.positional.join(", ")
    );
    Ok(())
}

fn atlas_command(args: &[String]) -> Result<(), Failure> {
    let options = Options::parse(args, &["out", "program"])?;
    let program = options.program()?;
    let spec = load_spec(options.one_positional("a species")?)?;
    let out = options.flags.get("out").ok_or("missing `--out FILE.png`")?;
    let (program, params) = match &program {
        Some(source) => spec.program_from(source),
        None => spec.program(),
    }
    .map_err(|error| error.to_string())?;
    let looks = spec.appearance.looks(program.organs());
    let bodies = spec.appearance.body_looks(program.bodies());
    let named: Vec<(&str, &BodyLook)> = program.bodies().zip(&bodies).collect();
    let templates = Templates::for_plant(&looks, &named);
    let (width, height, pixels) = templates.swatches_rgba(SWATCH_BACKGROUND, 2);
    let png = raster::encode_png(width, height, &pixels)
        .map_err(|error| format!("cannot encode PNG: {error}"))?;
    fs::write(out, png).map_err(|error| format!("cannot write {out}: {error}"))?;
    let shading = spec::organ_areas(&program, &params).map_err(|error| error.to_string())?;
    out!(
        "wrote {out}: {width}x{height}, one card texture per organ, left to right.\n\
         Leaf area of an organ 1 m long, as drawn and as the program shades with it:"
    );
    for ((look, template), shades) in looks.iter().zip(&templates.templates).zip(&shading) {
        let drawn = templates::drawn_area(look, template);
        let note = if spec::areas_agree(drawn, *shades) {
            String::new()
        } else {
            format!("  differs by {:+.0}%", (shades / drawn - 1.0) * 100.0)
        };
        out!(
            "  {:<16} {:<10} drawn {drawn:.3} m²  shaded {shades:.3} m²{note}",
            look.organ,
            look.shape.name()
        );
    }
    if !named.is_empty() {
        out!(
            "Then two spine cards per body, the spine cluster face on and from the side:\n  {}",
            named
                .iter()
                .map(|(name, _)| *name)
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    Ok(())
}

fn ground_command(args: &[String]) -> Result<(), Failure> {
    let options = Options::parse(args, &["out", "seed", "looks"])?;
    if !options.positional.is_empty() {
        return Err("`ground` takes no species".into());
    }
    let out = options.flags.get("out").ok_or("missing `--out FILE.png`")?;
    let seed = options.number::<u64>("seed")?.unwrap_or(1);
    let (looks, drawn) = match options.flags.get("looks").map_or("words", String::as_str) {
        "words" => (ground::plant_looks(seed), Vec::new()),
        "litter" => {
            let drawn: Vec<_> = litter::litters()
                .iter()
                .map(|litter| litter.drawn(seed))
                .collect();
            (
                drawn.iter().map(|drawn| drawn.look.clone()).collect(),
                drawn,
            )
        }
        other => return Err(format!("`--looks` is `words` or `litter`, not `{other}`").into()),
    };
    let size = GROUND_LOOK_SIZE;
    let columns = GROUND_SHEET_COLUMNS.min(looks.len());
    let rows = looks.len().div_ceil(columns);
    let (width, height) = (size * columns, size * 2 * rows);
    let mut pixels = vec![0_u8; width * height * 4];
    for (index, look) in looks.iter().enumerate() {
        let (left, top) = (index % columns * size, index / columns * 2 * size);
        for y in 0..size {
            for x in 0..size {
                let from = (y * size + x) * 4;
                let colour = ((top + y) * width + left + x) * 4;
                let relief = ((top + size + y) * width + left + x) * 4;
                pixels[colour..colour + 3].copy_from_slice(&look.rgba[from..from + 3]);
                pixels[colour + 3] = 255;
                let height = look.rgba[from + 3];
                pixels[relief..relief + 4].copy_from_slice(&[height, height, height, 255]);
            }
        }
    }
    let png = raster::encode_png(width, height, &pixels)
        .map_err(|error| format!("cannot encode PNG: {error}"))?;
    fs::write(out, png).map_err(|error| format!("cannot write {out}: {error}"))?;
    out!("wrote {out}: {width}x{height}, colour above relief, row by row:");
    for look in &looks {
        let [r, g, b] = look.mean;
        out!(
            "  {:<8} repeats every {:.2} m, relief {:.0} mm, mean linear colour {r:.3} {g:.3} {b:.3}",
            look.name,
            look.tile_m,
            look.relief_m * 1000.0
        );
    }
    // What each litter's drawing gives, for tuning its declared mean and
    // relief: the drawn hue at the declared green, and the drawn relief.
    for drawn in &drawn {
        let [r, g, b] = drawn.drawn_mean;
        let green = f64::from(drawn.look.mean[1]) / g.max(1.0e-9);
        out!(
            "  {:<22} drawn: hue {:.3} {:.3} {:.3}, relief {:.0} mm",
            drawn.look.name,
            r * green,
            g * green,
            b * green,
            drawn.drawn_relief_m * 1000.0
        );
    }
    Ok(())
}

/// Looks to a row of the sheet `ground` draws.
const GROUND_SHEET_COLUMNS: usize = 7;

/// Light grey, linear RGB: behind the card textures `atlas` draws.
const SWATCH_BACKGROUND: [f32; 3] = [0.6, 0.6, 0.6];

/// One look per organ type of a grown plant.
fn looks_of(spec: &PlantSpec, growth: &Growth, day: f64) -> (Vec<Look>, Vec<f64>) {
    spec.appearance.looks_on(
        growth
            .organ_types
            .iter()
            .map(|organ| (organ.name.as_str(), organ.kind)),
        Some(day),
    )
}

/// `graph` as drawn with its organ types' `sizes` on a day: organs gone
/// left out, the others at their stage's size (`looks::staged`).
fn drawn<'a>(graph: &'a PlantGraph, sizes: &[f64]) -> Cow<'a, PlantGraph> {
    looks::staged(graph, sizes).map_or(Cow::Borrowed(graph), Cow::Owned)
}

/// One body look per body type of a grown plant.
fn bodies_of(spec: &PlantSpec, growth: &Growth) -> Vec<BodyLook> {
    spec.appearance
        .body_looks(growth.body_types.iter().map(String::as_str))
}

/// A grown plant's card templates: its organs', then its bodies' spines,
/// with the bark pattern of `spec` for its wood.
fn templates_of(
    spec: &PlantSpec,
    looks: &[Look],
    bodies: &[BodyLook],
    growth: &Growth,
) -> Templates {
    let named: Vec<(&str, &BodyLook)> = growth
        .body_types
        .iter()
        .map(String::as_str)
        .zip(bodies)
        .collect();
    Templates::for_plant(looks, &named).with_bark(spec.appearance.bark_params())
}

fn build_command(args: &[String]) -> Result<(), Failure> {
    let options = Options::parse(args, &["out", "quality", "threads", "program", "day"])?;
    let program = options.program()?;
    let spec = load_spec(options.one_positional("a species")?)?;
    let quality = options.quality()?;
    let day = options.day()?;
    let threads = options
        .number("threads")?
        .unwrap_or_else(package::default_threads)
        .max(1);
    let out = PathBuf::from(options.flags.get("out").map_or("plants", String::as_str));
    let inputs = match &program {
        Some(source) => Inputs::with_program_in(&spec, library(), source, &quality, day),
        None => Inputs::on_day_in(&spec, library(), &quality, day),
    }
    .map_err(|error| error.to_string())?;
    if let Some(path) = package::existing(&out, &spec.id, &inputs.key) {
        out!(
            "{} is already built:\n  key {}\n  {}",
            spec.id,
            inputs.key,
            path.display()
        );
        return Ok(());
    }
    let started = Instant::now();
    // Progress is best effort: a closed output shows up at the summary.
    let built = package::build(&inputs, threads, &mut |line| {
        let _ = writeln!(io::stdout(), "  {line}");
    })
    .map_err(|error| error.to_string())?;
    let path = package::write(&built, &out).map_err(|error| error.to_string())?;
    #[allow(clippy::cast_precision_loss)]
    let megabytes = built.size() as f64 / 1_048_576.0;
    out!(
        "built {} in {:.1} s: {} objects, {megabytes:.1} MiB\n  key {}\n  {}",
        spec.id,
        started.elapsed().as_secs_f64(),
        built.objects.len(),
        built.manifest.key,
        path.display()
    );
    let outside = built
        .manifest
        .validation
        .iter()
        .filter(|record| !record.within_tolerance)
        .count();
    if outside > 0 {
        out!(
            "  {outside} reference sizes are outside tolerance; run `plantc inspect` on the package for details"
        );
    }
    Ok(())
}

fn inspect_command(args: &[String]) -> Result<(), Failure> {
    let options = Options::parse(args, &[])?;
    let path = options.one_positional("a package directory")?;
    let manifest = package::verify(Path::new(path)).map_err(|error| error.to_string())?;
    out!("{}", package::summary(&manifest).trim_end());
    out!("  every object matches its hash");
    Ok(())
}
