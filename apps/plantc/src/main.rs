//! `plantc`: the plant compiler.
//!
//! Grows species from their specs, renders previews for review, and builds
//! content-addressed `.afterplant` packages. It runs offline on the CPU and
//! never opens a window. Run `plantc help` for usage; see
//! `docs/user/PLANTS.md` and `docs/developer/PLANTS.md`.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::OnceLock;
use std::time::Instant;
use std::{env, fmt, fs};

use plantgen::body::BodyLook;
use plantgen::checks;
use plantgen::conditions::Conditions;
use plantgen::export;
use plantgen::graph::{GraphOrgan, OrganType, PlantGraph};
use plantgen::ground::{self, GROUND_LOOK_SIZE};
use plantgen::grow::{Grower, Growth, GrowthSettings, StepTime};
use plantgen::library::{Library, ProgramHome};
use plantgen::litter;
use plantgen::looks::{self, Look, Stage};
use plantgen::lsys::{Limits, Neighbourhood, Program};
use plantgen::math::Vec3;
use plantgen::mesh::{self, PlantMesh};
use plantgen::organs;
use plantgen::package::{self, Inputs};
use plantgen::preview::{self, PreviewOptions, View};
use plantgen::quality::{self, Quality};
use plantgen::raster;
use plantgen::spec::{self, Environment, PlantSpec, Variant};
use plantgen::templates::{self, Templates};
use plantgen::traits::{Applies, Vocabulary};

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
        "traits" => traits_command(&args),
        "audit" => audit_command(&args),
        "grow" => grow_command(&args),
        "stand" => stand_command(&args),
        "render" => render_command(&args),
        "sheet" => sheet_command(&args),
        "parts" => parts_command(&args),
        "organs" => organs_command(&args),
        "year" => year_command(&args),
        "lineup" => lineup_command(&args),
        "atlas" => atlas_command(&args),
        "measure" => measure_command(&args),
        "mechanics" => mechanics_command(&args),
        "validate" => validate_command(&args),
        "ground" => ground_command(&args),
        "build" => build_command(&args),
        "export" => export_command(&args),
        "inspect" => inspect_command(&args),
        "sources" => sources_command(&args),
        "help" | "--help" | "-h" => print_usage(),
        other => Err(format!("unknown command `{other}`; run `plantc help`").into()),
    }
}

#[allow(clippy::too_many_lines)]
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
      the rank file (or species) that holds it, and for a rule on a
      look's card, the card it is for. Then every program with its
      home (the taxon it is named for, or base), what it extends, the
      files that name it and its species count. Then the general ones,
      with no taxonomic home: rules at all plants, and the parameter
      defaults of programs not named for a taxon. The list should shrink.
  plantc traits [<species>]
      Without a species, print the characters a spec may state
      (traits.json): the categories of organ, the kinds of organ, and a
      tree of organs, each with its category, its Plant Ontology term,
      where it exists, and its characters with their types. With one,
      print the characters it states, its own and those it inherits,
      each with the file that set it and the source its note cites; then
      by organ what is missing, and which organs do not apply to it.
  plantc audit [<taxon>]
      How many of the characters that apply to each species it states,
      its own or inherited (optional characters left out). Without a
      taxon, by family, with what each family's own file states; with a
      family, genus or higher rank, by species, then the characters its
      species most often lack. Then each state a file names that the
      farther file it replaces leaves out, such as a rootless species in
      a phylum with roots: often right, but worth a look.
  plantc grow <species|spec.json> [--env ENV] [--seed N] [--years N]
              [--conditions FILE|JSON [--from-age N]] [--digest] [--profile]
      Grow one variant and print its size at every keyframe. --digest
      prints the growth's digest; --profile the seconds each year took,
      by phase (drawing the string, girth and bends, the environment
      tools, keyframes, rewriting), and the parts standing.
      --conditions grows it on from --from-age years (0 when left out) in
      a place: a conditions document, as a file or inline, laid over the
      variant's, such as a substrate preset at a tree's scale or
      neighbours' crowns as envelopes; each keyframe then says how much
      of it stands in the place's solids and crowns.
  plantc render <species|spec.json> --out FILE.png [--env ENV] [--seed N]
                [--age N] [--view side|three-quarter|top] [--lod 0-3]
                [--size PIXELS] [--quality draft|standard]
                [--focus X,Y,Z [--span METRES]] [--parts on]
                [--conditions FILE|JSON [--from-age N]]
      Render one variant at one age, with a 1.8 m figure for scale, or
      a rod in 10 cm stripes beside a plant lower than 1.5 m. --focus
      frames a close-up of SPAN metres (default 0.5) round a point of
      the plant, metres from its foot with +Y up. --parts on (level 0)
      draws every flower, fruit, cone and needle shoot as its part mesh,
      as WorldLab does near the camera. --conditions grows it in a place
      as plantc grow does (the place itself is not drawn).
  plantc stand <stand.json|JSON>
      Grow plants together, each making room for the others' crowns, and
      print each one's size at the end and where its leaves lean. The
      document gives years, and plants: a list of each plant's species,
      at_m (metres east and north) and seed. It may also give tile_m (east
      and north: the stand is a tile that wraps at its edges) and
      conditions (a place every plant grows in). Seeds default to each
      spec's first.
  plantc sheet <species|spec.json> --out FILE.png [--seed N] [--size PIXELS]
      Render every keyframe age (columns) in every environment (rows).
  plantc parts <species|spec.json> --out FILE.png [--env ENV] [--seed N]
               [--size PIXELS]
      Render each organ type alone, its median size at the oldest age
      printed (drawn at least 25 cm long): a row per type, its solid
      (LOD0) left and its card (LOD1) right.
  plantc organs <species|spec.json> [--env ENV] [--seed N] [--age N]
      Count one variant's organs at an age (default its oldest keyframe)
      on a day by what each is, its term: what the plant bears on its
      axes, then what each holds, through its program's structures (a
      grass's leaves, a fern's fronds) down to what its organs' layouts
      draw: a cluster's flowers, heads or spikelets, a head's phyllaries
      and florets, a spikelet's glumes, florets and awns. Each row gives
      how many there are, how many each holder bears on average (of those
      that bear any), how many are in each phase where they have phases,
      and the program's organ and structure types counted.
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
  plantc mechanics <species|spec.json> [--env ENV] [--seed N] [--years N]
                   [--bones N] [--piece M] [--frequencies fast|exact]
                   [--modulus MPA] [--moisture SHARE]
                   [--leaf-mass KG_M2] [--rows N]
      Grow a woody species to its oldest keyframe, or N years, and print
      it as a structure of beams (plant motion S1, S2): its material,
      mass, sag under its own weight, lowest vibration modes, the
      branches a host can prune and its heaviest bones (default 512 in
      all, each at most M metres long, default 2; N rows shown, default
      12) with their bend limits. Bone frequencies are by Rayleigh's
      quotient (fast) or subspace iteration (exact, slow). A species
      that does not state its green stiffness or moisture stops it, unless
      given; unstated leaf mass leaves the leaves unweighed. --modulus
      (green modulus of elasticity in static bending, as the Wood Handbook
      gives it), --moisture (green water over dry mass) and --leaf-mass
      (fresh leaf mass per area) fill only unstated ones, and the output
      marks them as given, not the species'.
  plantc measure [<species|spec.json>...] [--generator PROGRAM] [--env ENV]
                 [--seed N] [--years N]
      Grow each species (default every one in the library; --generator
      keeps those run by one program) to its oldest keyframe, or N years,
      and print its form: crown width over depth, where it is widest,
      its departure from an ellipse (0 an orb), its lobing, crown width
      over dbh, the median leaf card length, and from the side its
      outline's fit to an ellipse (1 an ellipse) and the share of its
      outline the sky shows through.
  plantc validate [<species>...] [--seeds N] [--group sizes|counts|kinds]
      Check each species (default every one that states a character a
      check reads) against
      its characters (spec rework M8B): grow N plants (default 5) from its
      own seeds and the next ones, in its typical environment, to its
      oldest keyframe; measure every size its characters state as each
      defines it (a leaf's blade without its petiole, a head's involucre,
      a spikelet to its last floret); and judge all the plants' values
      together: pass when their middle (10th to 90th percentile) lies in
      the stated range, a warning when it leaves it, an error past the
      range's extremes or a greatest size. Counts (rays, florets,
      phyllaries, glumes, units and branches of an inflorescence,
      leaflets, pinnae) are counted on each organ that holds them and
      judged the same way (M8A); kinds (cluster and head kinds, awns, a
      spikelet's ranks, a divided leaf's pattern and its end) pass when
      every state the plants draw is one the species states. --group
      shows one of the three. Exits non-zero on any error.
  plantc atlas <species|spec.json> --out FILE.png
      Draw the species' organ card textures (leaves, needles, flowers) in
      their colours, one per organ, side by side.
  plantc ground --out FILE.png [--seed N] [--looks words|litter]
      Draw the nine plant-made ground looks (needles, leaves, thatch,
      twigs, moss, sphagnum, grass, tussock and cushion), seven to a row,
      or with `--looks litter` the 23 canopy species' litters: each look's
      colour above its relief.
  plantc build <species|spec.json> [--out DIR] [--quality draft|standard]
               [--threads N] [--profile static|motion]
      Build a .afterplant package in DIR (default `plants`) and print its
      key. A package that is already built is not built again. With
      --profile motion it also holds each keyframe's bones and branches
      and each level's binding to them (plant motion S2), under a key of
      its own; the static package is unchanged.
  plantc inspect <package.afterplant>
      Check every object of a package and summarise it.
  plantc export <species|spec.json|package.afterplant> [--out DIR]
                [--format glb] [--env ENV] [--seed N] [--age N]
                [--shade attribute|baked] [--quality draft|standard]
                [--compress none|quantize|meshopt]
                [--attributes all|wind|none]
                [--triangles N[,N,N,N]] [--max-mib N]
                [--profile static|motion]
      Write one variant at one age as glTF 2.0 binary files that need no
      PlantGen code, one per level of detail (STEM-lod0.glb to
      STEM-lod3.glb), and an index, STEM.json, in DIR (default `exports`);
      STEM is <species>-<env>-<seed>-<age>y. Without --env and --seed, the
      species' typical site and first seed; without --age, the oldest
      keyframe. The plant is grown over all its years, as its package
      grows it; given a package, every level is checked against it.
      Bark keeps its own colour, with the darkening of the plant's own
      shade as the attribute _PLANTGEN_SHADE, or with --shade baked
      darkened as packages hold it. --compress quantize stores positions,
      normals and colours as small integers and parts as instances, a
      quarter to a half smaller; such files need KHR_mesh_quantization and
      EXT_mesh_gpu_instancing, which three.js, Unity's glTFast and current
      Blender read and Godot does not. --compress meshopt also compresses
      every array (EXT_meshopt_compression, which three.js reads with its
      meshopt decoder), 3 to 4 times smaller again: the 30-year bigleaf
      maple's nearest level is 7 MiB. --attributes wind leaves out the
      growth attributes (_PLANTGEN_BIRTH, _PLANTGEN_SHED), none also the
      wind ones (_PLANTGEN_WIND, _PLANTGEN_RADIUS).
      --triangles and --max-mib draw the levels to a budget instead:
      --triangles 500k gives the nearest level at most 500 000 triangles
      (each part mesh instance counted) and each farther level a quarter
      of the one before, or 500k,100k,20k,5k one count per level;
      --max-mib 20 keeps all the level files together within 20 MiB.
      Each level draws the most detail that fits along a dial through
      the quality's levels and past the farthest: between two levels,
      thin twigs go, tubes take fewer sides and rings, part meshes get
      coarser and leaves gather into bigger cards; at each level, part
      meshes and solid organs become cards; past the farthest, down to
      the stems and a few cards. The index records each level's drawing
      and budget. A budgeted export is not checked against a package.
      --profile motion (or a motion package) also skins every mesh to the
      plant's bones (JOINTS_0, WEIGHTS_0, _PLANTGEN_ALONG), lets its parts
      ride their bones' joints, and carries the bones and branches in the
      PLANTGEN_motion extension, as a motion package holds them.
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
grow, organs, render, sheet, atlas, build and export accept --program
FILE.lsys to try a changed program in place of the species' built-in one.
organs, render, sheet, parts, lineup, build and export accept --day N (1
to 365, default 196): the day of the year organs with a season show.
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

    /// The place of `--conditions FILE` (or the document itself, given
    /// inline) and the age of `--from-age` (0 when left out) the plant
    /// grows on in it from.
    fn place(&self) -> Result<Option<(Conditions, f64)>, String> {
        let from_age = self.number::<f64>("from-age")?;
        let Some(given) = self.flags.get("conditions") else {
            return match from_age {
                Some(_) => Err("`--from-age` goes with `--conditions`".into()),
                None => Ok(None),
            };
        };
        let text = if given.trim_start().starts_with('{') {
            given.clone()
        } else {
            fs::read_to_string(given).map_err(|error| format!("cannot read {given}: {error}"))?
        };
        let place = Conditions::from_json(&text)?;
        Ok(Some((place, from_age.unwrap_or(0.0))))
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
        for (key, stated) in &inherited.traits {
            out!("  {key} = {}  ({})", stated.value, stated.file);
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
                    "  {path}{}  ({} {}: {}{place})",
                    card(rule),
                    rank.rank,
                    rank.name,
                    rank.file
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
            for (path, rule) in rules {
                homed.push(format!(
                    "  {path}{}  (species {}: {})",
                    card(rule),
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
    let by_home = library.program_homes()?;
    print_programs(library, &by_home)?;
    let general: Vec<(&String, usize)> = by_home
        .iter()
        .filter(|home| home.home.is_none())
        .map(|home| {
            let own = own_params(library.program_text(&home.name).unwrap_or_default());
            (&home.name, own.iter().filter(|(_, value)| *value).count())
        })
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

/// `plantc rules`' programs section: each program with its home, what it
/// extends and the files that name it.
fn print_programs(library: &Library, by_home: &[ProgramHome]) -> Result<(), Failure> {
    // A program is homed when it is named for a taxon of the library
    // (growth plan G2.3a); one named for none is a base.
    let params = library.program_params();
    out!("Programs, by their home ({}):", by_home.len());
    for home in by_home {
        let own = own_params(library.program_text(&home.name).unwrap_or_default());
        let valued = own.iter().filter(|(_, value)| *value).count();
        let what = match home.extends.as_deref() {
            Some(parent) => {
                let inherited = params.get(parent);
                let set = own
                    .iter()
                    .filter(|(name, _)| inherited.is_some_and(|all| all.contains_key(*name)))
                    .count();
                let added = own.len() - set;
                format!(
                    ", extends {parent}; gives values to {set} of {parent}'s parameters{}",
                    if added > 0 {
                        format!(", adds {added}")
                    } else {
                        String::new()
                    }
                )
            }
            None if valued < own.len() => {
                format!("; {} parameters, {valued} with a value", own.len())
            }
            None => format!("; {} parameters", own.len()),
        };
        out!(
            "  {}  ({}{what}; {} species)",
            home.name,
            home.home.as_deref().map_or_else(
                || "base".to_string(),
                |place| format!("named for the taxon at {place}")
            ),
            home.species.len()
        );
        if !home.named_by.is_empty() {
            out!("      named by {}", home.named_by.join("; "));
        }
    }
    // A base's defaults are the parameters it gives a value; one it
    // declares as `param name;` leaves the value to the lineages.
    Ok(())
}

/// The parameters a program's own text declares (`param name = value;` or
/// `param name;`), each with whether it gives a value.
fn own_params(text: &str) -> Vec<(&str, bool)> {
    text.lines()
        .filter_map(|line| {
            let line = line.split('#').next().unwrap_or_default().trim();
            let rest = line.strip_prefix("param ")?;
            let name = rest.split(['=', ';']).next()?.trim();
            Some((name, rest.contains('=')))
        })
        .collect()
}

/// The cards a rule on a look's card is for, as `plantc rules` names them.
fn card(rule: &serde_json::Value) -> String {
    let templates: Vec<&str> = match rule.get("template") {
        Some(serde_json::Value::String(template)) => vec![template.as_str()],
        Some(serde_json::Value::Array(templates)) => templates
            .iter()
            .filter_map(serde_json::Value::as_str)
            .collect(),
        _ => Vec::new(),
    };
    if templates.is_empty() {
        String::new()
    } else {
        format!(" on {} cards", plantgen::inherit::cards(&templates))
    }
}

fn traits_command(args: &[String]) -> Result<(), Failure> {
    let options = Options::parse(args, &[])?;
    let vocabulary = Vocabulary::builtin();
    match options.positional.as_slice() {
        [] => {
            out!(
                "{} characters on {} organs (traits.json schema {}): each organ, its category and Plant Ontology term ({}), where it exists, and its characters",
                vocabulary.traits.len(),
                vocabulary.organs.len(),
                vocabulary.schema,
                vocabulary.po_release
            );
            out!("Categories:");
            for (name, why) in &vocabulary.categories {
                out!("  {name}: {why}");
            }
            let characters = |name: &str, indent: &str| -> Result<(), Failure> {
                for (key, item) in &vocabulary.traits {
                    if item.organ == name {
                        out!(
                            "{indent}  - {key}: {}{}{}{}",
                            item.what(),
                            condition(&item.when),
                            item.at_most
                                .as_ref()
                                .map_or(String::new(), |above| format!(", at most {above}")),
                            if item.optional { " (optional)" } else { "" }
                        );
                    }
                }
                Ok(())
            };
            out!("Kinds:");
            for (name, kind) in &vocabulary.kinds {
                out!(
                    "  {name} ({}; {}): {}",
                    kind.category,
                    vocabulary.members(name).join(", "),
                    kind.why
                );
                characters(name, "  ")?;
            }
            out!("Organs:");
            for (depth, name) in vocabulary.organ_tree() {
                let organ = &vocabulary.organs[name];
                let indent = "  ".repeat(depth + 1);
                let kind = organ
                    .is_a
                    .as_ref()
                    .map_or(String::new(), |kind| format!(", a {kind}"));
                let term = organ
                    .po
                    .as_ref()
                    .map_or(String::new(), |po| format!("; {} {}", po.id, po.name));
                out!(
                    "{indent}{name} ({}{kind}{term}){}: {}",
                    organ.category,
                    condition(&organ.when),
                    organ.why
                );
                characters(name, &indent)?;
            }
            Ok(())
        }
        [id] => species_traits(vocabulary, id),
        _ => Err("`plantc traits` takes at most one species".into()),
    }
}

/// `, where a is x, y or z, or b is w`, or nothing for an empty `when`.
fn condition(when: &plantgen::traits::When) -> String {
    if when.is_empty() {
        return String::new();
    }
    let parts: Vec<String> = when
        .iter()
        .map(|(key, values)| match values.split_last() {
            Some((last, rest)) if !rest.is_empty() => {
                format!("{key} is {} or {last}", rest.join(", "))
            }
            Some((only, _)) => format!("{key} is {only}"),
            None => format!("{key} is stated"),
        })
        .collect();
    format!(", where {}", parts.join(", or "))
}

/// A species' characters by organ: each stated one with its file and
/// source, then what is missing, then the organs that do not apply.
fn species_traits(vocabulary: &Vocabulary, id: &str) -> Result<(), Failure> {
    let library = library();
    let inherited = library.inherited(id).map_err(|error| error.to_string())?;
    let spec = library.spec(id).map_err(|error| error.to_string())?;
    let traits = inherited.trait_values();
    let coverage = vocabulary.coverage(&traits);
    let own = inherited
        .traits
        .values()
        .filter(|stated| stated.file == inherited.chain[0])
        .count();
    out!(
        "{} ({}): states {} of the {} characters that apply ({:.0}%), {own} in its own spec.json; {} more may apply once what they depend on is stated",
        spec.id,
        spec.taxon.scientific_name,
        coverage.stated.len(),
        coverage.stated.len() + coverage.missing.len(),
        coverage.share() * 100.0,
        coverage.undecided.len()
    );
    let mut absent = Vec::new();
    for (_, name) in vocabulary.organ_tree() {
        let applies = vocabulary.organ_applies(name, &traits);
        if applies == Applies::No {
            absent.push(name);
            continue;
        }
        let of_organ = |list: &[String]| -> Vec<String> {
            list.iter()
                .filter(|key| vocabulary.traits[key.as_str()].organ == name)
                .cloned()
                .collect()
        };
        let missing = of_organ(&coverage.missing);
        let undecided = of_organ(&coverage.undecided);
        let stated: Vec<(&String, &plantgen::inherit::Stated)> = inherited
            .traits
            .iter()
            .filter(|(key, _)| vocabulary.traits[key.as_str()].organ == name)
            .collect();
        if applies == Applies::Undecided && stated.is_empty() {
            continue;
        }
        out!(
            "{name}{}",
            if applies == Applies::Undecided {
                " (may apply)"
            } else {
                ""
            }
        );
        for (key, stated) in stated {
            let source = stated
                .note
                .as_ref()
                .and_then(|note| note.get("source"))
                .and_then(serde_json::Value::as_str)
                .unwrap_or("no note");
            out!(
                "  {key} = {}  ({}; {source})",
                vocabulary.show(key, &stated.value),
                stated.file
            );
        }
        if !missing.is_empty() {
            out!("  missing: {}", missing.join(", "));
        }
        if !undecided.is_empty() && applies == Applies::Yes {
            out!("  may apply: {}", undecided.join(", "));
        }
    }
    let waiting: Vec<&str> = vocabulary
        .organ_tree()
        .into_iter()
        .map(|(_, name)| name)
        .filter(|name| vocabulary.organ_applies(name, &traits) == Applies::Undecided)
        .collect();
    if !waiting.is_empty() {
        out!(
            "organs that may apply once what they depend on is stated: {}",
            waiting.join(", ")
        );
    }
    if !absent.is_empty() {
        out!("organs that do not apply: {}", absent.join(", "));
    }
    Ok(())
}

#[allow(clippy::too_many_lines)]
fn audit_command(args: &[String]) -> Result<(), Failure> {
    let options = Options::parse(args, &[])?;
    let library = library();
    let vocabulary = Vocabulary::builtin();
    let taxon = match options.positional.as_slice() {
        [] => None,
        [taxon] => Some(plantgen::inherit::file_name(taxon)),
        _ => return Err("`plantc audit` takes at most one taxon".into()),
    };
    // Each species with its coverage and the files it stands on, and
    // each state a file names that the farther file it replaces does not
    // list: the nearer file, the trait, and the two statements in words.
    let mut audited = Vec::new();
    let mut unlisted: BTreeSet<(String, String, String, String, String)> = BTreeSet::new();
    for entry in library.species() {
        let inherited = library
            .inherited(&entry.id)
            .map_err(|error| error.to_string())?;
        let within = taxon.as_deref().is_none_or(|taxon| {
            entry.family == taxon
                || entry.genus == taxon
                || inherited.chain[1..].iter().any(|file| {
                    library
                        .rank(file)
                        .is_some_and(|rank| plantgen::inherit::file_name(&rank.name) == taxon)
                })
        });
        if !within {
            continue;
        }
        let traits = inherited.trait_values();
        let own = inherited
            .traits
            .values()
            .filter(|stated| stated.file == inherited.chain[0])
            .count();
        for (key, statements) in &inherited.statements {
            for pair in statements.windows(2) {
                let (farther, nearer) = (&pair[0], &pair[1]);
                if !vocabulary
                    .unlisted(key, &farther.value, &nearer.value)
                    .is_empty()
                {
                    unlisted.insert((
                        nearer.file.clone(),
                        key.clone(),
                        vocabulary.show(key, &nearer.value),
                        farther.file.clone(),
                        vocabulary.show(key, &farther.value),
                    ));
                }
            }
        }
        audited.push((entry, vocabulary.coverage(&traits), own));
    }
    if audited.is_empty() {
        return Err(format!(
            "no species of the library is in `{}`; name a family, genus or higher rank, as its rank file is named",
            taxon.unwrap_or_default()
        )
        .into());
    }
    let total = |rows: &[&(&plantgen::library::Entry, plantgen::traits::Coverage, usize)]| {
        let stated: usize = rows
            .iter()
            .map(|(_, coverage, _)| coverage.stated.len())
            .sum();
        let missing: usize = rows
            .iter()
            .map(|(_, coverage, _)| coverage.missing.len())
            .sum();
        (stated, stated + missing)
    };
    let percent = |(stated, applying): (usize, usize)| {
        if applying == 0 {
            100.0
        } else {
            #[allow(clippy::cast_precision_loss)]
            let share = stated as f64 / applying as f64;
            share * 100.0
        }
    };
    let all: Vec<_> = audited.iter().collect();
    let (stated, applying) = total(&all);
    let stating_files = library
        .ranks()
        .filter(|rank| rank.parts().any(|(_, part)| !part.traits.is_empty()))
        .count();
    match &taxon {
        None => {
            out!(
                "{} species state {stated} of the {applying} characters that apply to them ({:.0}%), own or inherited; {stating_files} rank files state characters",
                audited.len(),
                percent((stated, applying))
            );
            out!(
                "By family: species, characters stated of those that apply, and how many its family.json states:"
            );
            let mut families: BTreeMap<&str, Vec<_>> = BTreeMap::new();
            for row in &audited {
                families.entry(row.0.family.as_str()).or_default().push(row);
            }
            let width = families
                .keys()
                .map(|family| family.len())
                .max()
                .unwrap_or(0);
            for (family, rows) in &families {
                let (stated, applying) = total(rows);
                let own = library
                    .rank(&format!("{family}/family.json"))
                    .map_or(0, |rank| rank.shared.traits.len());
                out!(
                    "  {family:<width$}  {:>3} species  {stated:>5} of {applying:>5}  {:>3.0}%  family.json {own}",
                    rows.len(),
                    percent((stated, applying))
                );
            }
        }
        Some(taxon) => {
            out!(
                "{taxon}: {} species state {stated} of the {applying} characters that apply to them ({:.0}%), own or inherited",
                audited.len(),
                percent((stated, applying))
            );
            let width = audited
                .iter()
                .map(|(entry, _, _)| entry.id.len())
                .max()
                .unwrap_or(0);
            let mut lacking: BTreeMap<&str, usize> = BTreeMap::new();
            for (entry, coverage, own) in &audited {
                out!(
                    "  {:<width$}  {:>3} of {:>3}  {:>3.0}%  {own} in its spec.json",
                    entry.id,
                    coverage.stated.len(),
                    coverage.stated.len() + coverage.missing.len(),
                    coverage.share() * 100.0
                );
                for key in &coverage.missing {
                    *lacking.entry(key.as_str()).or_default() += 1;
                }
            }
            let mut lacking: Vec<(&str, usize)> = lacking.into_iter().collect();
            lacking.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
            out!(
                "Most often missing: {}",
                lacking
                    .iter()
                    .take(12)
                    .map(|(key, count)| format!("{key} ({count})"))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
    }
    if unlisted.is_empty() {
        out!("No file names a state that the farther file it replaces leaves out");
    } else {
        out!(
            "States a file names that the farther file it replaces leaves out ({}), which may be right but want a look:",
            unlisted.len()
        );
        for (file, key, value, farther, listed) in &unlisted {
            out!("  {file}: `{key}` is {value}; {farther} has {listed}");
        }
    }
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
    grow_variant_in(
        spec,
        program,
        environment,
        seed,
        keyframes,
        years,
        None,
        false,
    )
    .map(|(growth, _)| growth)
}

/// [`grow_variant`], grown on in a place from an age when `place` gives
/// one (`--conditions`, `--from-age`), and with the time each step took by
/// phase when `profile`.
#[allow(clippy::too_many_arguments)]
fn grow_variant_in(
    spec: &PlantSpec,
    program: Option<&str>,
    environment: Environment,
    seed: u64,
    keyframes: Vec<f64>,
    years: f64,
    place: Option<&(Conditions, f64)>,
    profile: bool,
) -> Result<(Growth, Vec<StepTime>), String> {
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
    let params = spec.seeded_params(&program, &params, seed);
    let failed = |error: plantgen::lsys::GrowthError| format!("{}: {error}", spec.id);
    let mut grower = Grower::new(&program, &params, &settings).map_err(failed)?;
    if profile {
        grower = grower.timed();
    }
    match place {
        Some((place, from_age)) => grower.run_in(place, *from_age, None),
        None => loop {
            match grower.step(None) {
                Ok(true) => {}
                Ok(false) => break Ok(()),
                Err(error) => break Err(error),
            }
        },
    }
    .map_err(failed)?;
    let times = grower.times().to_vec();
    Ok((grower.finish(), times))
}

/// Where a growth's time went: a row per year (its steps summed) with the
/// parts standing at its end, then the whole run by phase.
fn profile_lines(times: &[StepTime]) -> Vec<String> {
    let mut lines = vec![
        "  profile, seconds: year  segments    organs  interpret     girth     tools      keep    derive     total"
            .to_string(),
    ];
    let mut whole = StepTime::default();
    let mut index = 0;
    while index < times.len() {
        #[allow(clippy::cast_possible_truncation)]
        let year = times[index].age.floor() as i64;
        let mut row = StepTime::default();
        #[allow(clippy::cast_possible_truncation)]
        while index < times.len() && times[index].age.floor() as i64 == year {
            let time = &times[index];
            row.interpret += time.interpret;
            row.girth += time.girth;
            row.tools += time.tools;
            row.keep += time.keep;
            row.derive += time.derive;
            row.segments = time.segments;
            row.organs = time.organs;
            index += 1;
        }
        whole.interpret += row.interpret;
        whole.girth += row.girth;
        whole.tools += row.tools;
        whole.keep += row.keep;
        whole.derive += row.derive;
        lines.push(format!(
            "  {year:>22} {:>9} {:>9} {:>10.3} {:>9.3} {:>9.3} {:>9.3} {:>9.3} {:>9.3}",
            row.segments,
            row.organs,
            row.interpret,
            row.girth,
            row.tools,
            row.keep,
            row.derive,
            row.total()
        ));
    }
    let total = whole.total().max(1e-12);
    let share = |part: f64| 100.0 * part / total;
    lines.push(format!(
        "  all steps {:.2} s: interpret {:.0} %, girth {:.0} %, tools {:.0} %, keep {:.0} %, derive {:.0} %",
        whole.total(),
        share(whole.interpret),
        share(whole.girth),
        share(whole.tools),
        share(whole.keep),
        share(whole.derive)
    ));
    lines
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

/// `plantc organs`: a grown plant's census by term (spec rework M7B).
fn organs_command(args: &[String]) -> Result<(), Failure> {
    let options = Options::parse(args, &["env", "seed", "age", "day", "program"])?;
    let spec = load_spec(options.one_positional("a species")?)?;
    let source = options.program()?;
    let environment = options.environment(&spec)?;
    let seed = options.number("seed")?.unwrap_or(spec.variants.seeds[0]);
    let age = options.number("age")?.unwrap_or(spec.growth.years);
    let day = options.day()?;
    let growth = grow_variant(&spec, source.as_deref(), environment, seed, vec![age], age)?;
    let graph = growth.keyframes.last().ok_or("nothing grew")?;
    let (program, _) = match &source {
        Some(source) => spec.program_from(source),
        None => spec.program_in(library()),
    }
    .map_err(|error| error.to_string())?;
    let terms = organs::of_program(&program, &spec.appearance);
    let organ_types: Vec<(&str, &str)> = growth
        .organ_types
        .iter()
        .map(|kind| kind.name.as_str())
        .zip(terms)
        .collect();
    let structure_types: Vec<(&str, &str)> = growth
        .structure_types
        .iter()
        .map(|kind| (kind.name.as_str(), kind.term.as_str()))
        .collect();
    let (looks, _) = looks_of(&spec, &growth, day);
    let seasons: Vec<Option<&looks::Season>> = growth
        .organ_types
        .iter()
        .map(|kind| {
            spec.appearance
                .organs
                .get(&kind.name)
                .and_then(|look| look.season.as_ref())
        })
        .collect();
    let tallies = organs::census(graph, &organ_types, &structure_types, &looks, &seasons, day);
    out!(
        "{} ({}), {} environment, seed {seed}: {} years, day {day}",
        spec.taxon.common_name,
        spec.id,
        environment.name(),
        graph.age
    );
    let mut rows = Vec::new();
    census_rows(&tallies, 0, None, &mut rows);
    if rows.is_empty() {
        out!("  no organs");
    }
    let width = |column: usize| {
        rows.iter()
            .map(|row: &[String; 4]| row[column].chars().count())
            .max()
            .unwrap_or(0)
    };
    let (terms, counts, shares) = (width(0), width(1), width(2));
    for [term, count, share, notes] in &rows {
        let line = format!("  {term:<terms$}  {count:>counts$}  {share:<shares$}  {notes}");
        out!("{}", line.trim_end());
    }
    Ok(())
}

/// The rows of `plantc organs` for `tallies` at `depth`, held by
/// `holder`: each term indented, its count, its count per holder and its
/// phases and types.
fn census_rows(
    tallies: &[organs::Tally],
    depth: usize,
    holder: Option<&str>,
    rows: &mut Vec<[String; 4]>,
) {
    for tally in tallies {
        let term = tally.term.replace('_', " ");
        let share = match holder {
            Some(holder) if tally.holders > 0 => {
                #[allow(clippy::cast_precision_loss)]
                let mean = tally.count as f64 / tally.holders as f64;
                format!("{mean:.1} per {holder}")
            }
            _ => String::new(),
        };
        let phases: Vec<String> = organs::PHASES
            .iter()
            .zip(tally.phases)
            .filter(|(_, count)| *count > 0)
            .map(|(phase, count)| format!("{} {phase}", grouped(count)))
            .collect();
        let types: Vec<String> = tally
            .types
            .iter()
            .map(|(name, count)| format!("`{name}` {}", grouped(*count)))
            .collect();
        let [juvenile, adult] = tally.adult;
        let shoots = if juvenile + adult > 0 {
            format!("{} juvenile, {} adult", grouped(juvenile), grouped(adult))
        } else {
            String::new()
        };
        let notes = [shoots, phases.join(", "), types.join(", ")]
            .into_iter()
            .filter(|note| !note.is_empty())
            .collect::<Vec<_>>()
            .join("; ");
        rows.push([
            format!("{}{term}", "  ".repeat(depth)),
            grouped(tally.count),
            share,
            notes,
        ]);
        census_rows(&tally.parts, depth + 1, Some(&term), rows);
    }
}

/// `n` with its thousands set apart by commas.
fn grouped(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
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
        "{:<28} {:>5} {:>6} {:>6} {:>6} {:>6} {:>6} {:>6} {:>5} {:>6} {:>6} {:>5} {:>5}",
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
        "leaf",
        "fit",
        "sky"
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
                "{:<28} {:>5.0} {:>6.1} {:>6} {:>6.1} {:>6.2} {:>6.2} {:>6.2} {:>5.2} {:>6} {:>6} {:>5} {:>5}",
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
                cm(form.leaf_length),
                form.outline_fit
                    .map_or_else(|| "-".to_string(), |fit| format!("{fit:.2}")),
                form.see_through
                    .map_or_else(|| "-".to_string(), |sky| format!("{:.0}%", sky * 100.0))
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

/// `plantc mechanics`: a grown plant as beams and bones (plant motion S1).
#[allow(clippy::too_many_lines)]
fn mechanics_command(args: &[String]) -> Result<(), Failure> {
    use plantgen::mechanics::{self, Material, Settings};
    let options = Options::parse(
        args,
        &[
            "env",
            "seed",
            "years",
            "bones",
            "piece",
            "frequencies",
            "modulus",
            "moisture",
            "leaf-mass",
            "rows",
        ],
    )?;
    let [id] = options.positional.as_slice() else {
        return Err("`plantc mechanics` takes one species".into());
    };
    let spec = load_spec(id)?;
    let mut traits = library()
        .traits_of(&spec.id)
        .map_err(|error| error.to_string())?;
    // Fill only what the species leaves unstated, and say so.
    let mut given = Vec::new();
    for (flag, key) in [
        ("modulus", "wood_modulus_green_mpa"),
        ("moisture", "wood_moisture_green_share"),
        ("leaf-mass", "leaf_mass_fresh_kg_m2"),
    ] {
        if let Some(value) = options.number::<f64>(flag)? {
            if traits.contains_key(key) {
                out!("--{flag} ignored: {} states {key}", spec.id);
            } else {
                traits.insert(key.to_string(), value.into());
                given.push(key);
            }
        }
    }
    let material = Material::of(&traits).map_err(|missing| {
        format!(
            "{} does not state {}; give --modulus or --moisture to explore",
            spec.id,
            missing.join(", ")
        )
    })?;
    material.validate()?;
    let environment = options.environment(&spec)?;
    let seed = options.number("seed")?.unwrap_or(spec.variants.seeds[0]);
    let years = options.number("years")?.unwrap_or(spec.growth.years);
    let growth = grow_variant(&spec, None, environment, seed, vec![years], years)?;
    let Some(graph) = growth.keyframes.last() else {
        return Err(format!("{} grew nothing", spec.id).into());
    };
    let exact = match options.flags.get("frequencies").map(String::as_str) {
        None | Some("fast") => false,
        Some("exact") => true,
        Some(other) => {
            return Err(format!("--frequencies is fast or exact, not `{other}`").into());
        }
    };
    let settings = Settings {
        bones: options
            .number("bones")?
            .unwrap_or(Settings::default().bones),
        piece: options
            .number("piece")?
            .unwrap_or(Settings::default().piece),
        exact,
        ..Settings::default()
    };
    let started = Instant::now();
    let found = mechanics::mechanics(graph, &growth.organ_types, &material, &settings);
    let took = started.elapsed();
    let mark = |key: &str| if given.contains(&key) { " (given)" } else { "" };
    out!(
        "{} ({}), seed {seed}, {:.0} years, {:.1} m",
        spec.taxon.common_name,
        spec.id,
        graph.age,
        graph.height
    );
    out!(
        "wood {:.0} kg/m3 green{}, modulus {:.0} MPa{} along the grain, rupture {}, leaves {}{}",
        material.wood_density,
        mark("wood_moisture_green_share"),
        material.modulus / 1.0e6,
        mark("wood_modulus_green_mpa"),
        material.rupture.map_or_else(
            || "unstated".to_string(),
            |pa| format!("{:.0} MPa", pa / 1.0e6)
        ),
        material.leaf_mass.map_or_else(
            || "unstated (not weighed)".to_string(),
            |mass| format!("{mass:.3} kg/m2")
        ),
        mark("leaf_mass_fresh_kg_m2"),
    );
    let beams = &found.beams;
    out!(
        "{} beams, {} carried segments, {} axes, {} bones, {} branches; {:.1} kg, of which leaves {:.2} kg; {} organs unweighed",
        beams.len(),
        beams.carried,
        found.axes,
        found.bones.len(),
        found.branches.len(),
        found.mass(),
        found.leaf_mass(),
        beams.unweighed
    );
    out!(
        "own weight once more moves an end up to {:.1} mm",
        found.sag() * 1000.0
    );
    let modes: Vec<String> = found
        .modes
        .iter()
        .map(|mode| format!("{:.3}", mode.frequency))
        .collect();
    out!("lowest modes, Hz: {}", modes.join(" "));
    out!("solved in {:.1} ms", took.as_secs_f64() * 1000.0);
    // What a motion package's record adds: reach, snow, perches,
    // anchorage and wood through wood.
    let motion = plantgen::motion::Motion {
        material,
        assumed: Vec::new(),
        settings,
    };
    let started = Instant::now();
    let record = plantgen::motion::record(&found, &motion, graph, &growth.organ_types);
    let crossings = record.crossings;
    out!(
        "anchorage {:.0} kN m overturning (assumed {} N m per kg of stem); {} perches; \
         snow on {:.1} m2 of wood and {:.1} m2 of foliage",
        record.anchorage.moment / 1000.0,
        plantgen::motion::ANCHORAGE_PER_KG,
        record.perches.len(),
        record
            .bones
            .iter()
            .map(|bone| bone.snow_area.wood)
            .sum::<f64>(),
        record
            .bones
            .iter()
            .map(|bone| bone.snow_area.foliage)
            .sum::<f64>(),
    );
    out!(
        "wood through wood: {} pairs, {:.1}% of segments, deepest {:.1} cm; record in {:.1} ms",
        crossings.pairs,
        crossings.share * 100.0,
        crossings.deepest * 100.0,
        started.elapsed().as_secs_f64() * 1000.0
    );
    let rows = options.number::<usize>("rows")?.unwrap_or(12);
    let mut order: Vec<usize> = (0..found.bones.len()).collect();
    order.sort_by(|&a, &b| {
        found.bones[b]
            .mass_beyond
            .total_cmp(&found.bones[a].mass_beyond)
            .then(a.cmp(&b))
    });
    out!(
        "{:>5} {:>6} {:>5} {:>6} {:>7} {:>6} {:>9} {:>7} {:>7} {:>9} {:>6} {:>6} {:>7}",
        "bone",
        "parent",
        "depth",
        "length",
        "foot cm",
        "kg",
        "beyond kg",
        "f1 Hz",
        "f2 Hz",
        "moment Nm",
        "break",
        "bend°",
        "reach m"
    );
    for &at in order.iter().take(rows) {
        let bone = &found.bones[at];
        out!(
            "{:>5} {:>6} {:>5} {:>6.2} {:>7.1} {:>6.2} {:>9.2} {:>7.3} {:>7.3} {:>9.1} {:>6} {:>6} {:>7.2}",
            at,
            bone.parent
                .map_or_else(|| "-".to_string(), |p| p.to_string()),
            bone.depth,
            bone.length,
            bone.radius[0] * 200.0,
            bone.mass,
            bone.mass_beyond,
            bone.frequencies[0],
            bone.frequencies[1],
            bone.weight_moment,
            bone.breaking_moment.map_or_else(
                || "-".to_string(),
                |breaking| format!("{:.0}%", 100.0 * bone.weight_moment / breaking)
            ),
            bone.bend_limit.map_or_else(
                || "-".to_string(),
                |limit| format!("{:.0}", limit.to_degrees())
            ),
            record.bones[at].reach
        );
    }
    Ok(())
}

/// `plantc validate`: stated sizes, counts and kinds against grown plants
/// (spec rework M8B and M8A).
#[allow(clippy::too_many_lines)]
fn validate_command(args: &[String]) -> Result<(), Failure> {
    let options = Options::parse(args, &["seeds", "group"])?;
    let count = options.number::<usize>("seeds")?.unwrap_or(5).max(1);
    let group = options.flags.get("group").cloned();
    if let Some(group) = &group
        && !matches!(group.as_str(), "sizes" | "counts" | "kinds")
    {
        return Err(format!("--group is sizes, counts or kinds, not {group}").into());
    }
    let library = library();
    let ids: Vec<String> = if options.positional.is_empty() {
        library
            .species()
            .iter()
            .map(|entry| entry.id.clone())
            .filter(|id| {
                let (Ok(spec), Ok(traits)) = (library.spec(id), library.traits_of(id)) else {
                    return false;
                };
                checks::states_checked(&spec, &traits)
            })
            .collect()
    } else {
        options.positional.clone()
    };
    let mut tally = [0_usize; 4];
    for id in &ids {
        let spec = load_spec(id)?;
        let traits = library
            .traits_of(&spec.id)
            .map_err(|error| error.to_string())?;
        let seeds = checks::seeds_of(&spec, count);
        let report = checks::check(&spec, library, &traits, &seeds)?;
        out!(
            "{} ({}): {} plants, seeds {}, {} environment, {} years",
            spec.taxon.common_name,
            spec.id,
            report.seeds.len(),
            report
                .seeds
                .iter()
                .map(u64::to_string)
                .collect::<Vec<_>>()
                .join(", "),
            report.environment.name(),
            report.age
        );
        let finding_row = |finding: &checks::Finding| {
            let shown = |value: f64| checks::value_text(finding.character, value);
            let measured = finding.spread.map_or_else(String::new, |s| {
                let values = if finding.judged == checks::Judged::Organs {
                    format!("{} in {} plants", s.count, s.plants)
                } else {
                    format!("{} plants", s.plants)
                };
                format!(
                    "{} {} {} ({values})",
                    shown(s.q10),
                    shown(s.median),
                    shown(s.q90)
                )
            });
            [
                finding.character.to_string(),
                checks::range_text(finding.character, &finding.stated),
                measured,
                finding.verdict.word().to_string(),
                finding.reason.clone(),
            ]
        };
        let is_count = |finding: &&checks::Finding| checks::COUNTS.contains(&finding.character);
        let sections: [(&str, Vec<[String; 5]>); 3] = [
            (
                "sizes",
                report
                    .findings
                    .iter()
                    .filter(|finding| !is_count(finding))
                    .map(finding_row)
                    .collect(),
            ),
            (
                "counts",
                report
                    .findings
                    .iter()
                    .filter(is_count)
                    .map(finding_row)
                    .collect(),
            ),
            (
                "kinds",
                report
                    .kinds
                    .iter()
                    .map(|agreement| {
                        let drawn: Vec<String> = agreement
                            .drawn
                            .iter()
                            .map(|state| state.replace('|', " or "))
                            .collect();
                        let reason = match agreement.verdict {
                            checks::Verdict::Error => "draws a state the species does not state",
                            checks::Verdict::NotGrown => "nothing drawn to compare",
                            _ => "",
                        }
                        .to_string();
                        [
                            agreement.character.to_string(),
                            agreement.stated.join(" or "),
                            drawn.join(", "),
                            agreement.verdict.word().to_string(),
                            reason,
                        ]
                    })
                    .collect(),
            ),
        ];
        let shown: Vec<&(&str, Vec<[String; 5]>)> = sections
            .iter()
            .filter(|(name, rows)| {
                !rows.is_empty() && group.as_deref().is_none_or(|group| group == *name)
            })
            .collect();
        if shown.is_empty() {
            out!("  nothing stated that a check measures");
        }
        let width = |column: usize| {
            shown
                .iter()
                .flat_map(|(_, rows)| rows)
                .map(|row| row[column].chars().count())
                .max()
                .unwrap_or(0)
        };
        let (a, b, c, d) = (width(0), width(1), width(2), width(3));
        for (name, rows) in &shown {
            out!("  {name}");
            for [character, stated, measured, verdict, reason] in rows {
                let line = format!(
                    "    {character:<a$}  {stated:<b$}  {measured:<c$}  {verdict:<d$}  {reason}"
                );
                out!("{}", line.trim_end());
            }
        }
        if !report.unmeasured.is_empty() {
            out!("  not measured yet: {}", report.unmeasured.join(", "));
        }
        // A flower on a juvenile shoot is an error whatever is stated
        // (spec rework P6).
        if report.juvenile_flowers > 0 && group.is_none() {
            out!(
                "  error: {} flowers and fruit on juvenile shoots",
                grouped(u64::try_from(report.juvenile_flowers).unwrap_or(u64::MAX))
            );
            tally[3] += 1;
        }
        let verdicts = report
            .findings
            .iter()
            .filter(|finding| {
                let name = if is_count(finding) { "counts" } else { "sizes" };
                group.as_deref().is_none_or(|group| group == name)
            })
            .map(|finding| finding.verdict)
            .chain(
                report
                    .kinds
                    .iter()
                    .filter(|_| group.as_deref().is_none_or(|group| group == "kinds"))
                    .map(|agreement| agreement.verdict),
            );
        for verdict in verdicts {
            let slot = match verdict {
                checks::Verdict::Pass => 0,
                checks::Verdict::NotGrown => 1,
                checks::Verdict::Warning => 2,
                checks::Verdict::Error => 3,
            };
            tally[slot] += 1;
        }
    }
    if ids.len() > 1 {
        out!(
            "{} species: pass {}, not grown {}, warnings {}, errors {}",
            ids.len(),
            tally[0],
            tally[1],
            tally[2],
            tally[3]
        );
    }
    if tally[3] > 0 {
        return Err(format!("checks failed: {}", tally[3]).into());
    }
    Ok(())
}

fn grow_command(args: &[String]) -> Result<(), Failure> {
    // `--digest` and `--profile` take no value.
    let digest = args.iter().any(|arg| arg == "--digest");
    let profile = args.iter().any(|arg| arg == "--profile");
    let args: Vec<String> = args
        .iter()
        .filter(|arg| *arg != "--digest" && *arg != "--profile")
        .cloned()
        .collect();
    let options = Options::parse(
        &args,
        &["env", "seed", "years", "program", "conditions", "from-age"],
    )?;
    let spec = load_spec(options.one_positional("a species")?)?;
    let program = options.program()?;
    let place = options.place()?;
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
    let (growth, times) = grow_variant_in(
        &spec,
        program.as_deref(),
        environment,
        seed,
        keyframes,
        years,
        place.as_ref(),
        profile,
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
    if let Some((place, from_age)) = &place {
        out!(
            "  grown on from {from_age} years in a place, class {}",
            place.over(&Conditions::preset(environment)).class()
        );
    }
    for graph in &growth.keyframes {
        out!("  {}", describe(graph, &growth.organ_types));
        if let Some((place, _)) = &place {
            out!("      {}", place.intrusion(graph)?.describe());
        }
    }
    let variant = Variant {
        environment,
        seed,
        neighbourhood: neighbourhood(&spec, environment),
        class: None,
        substrate: None,
    };
    for record in package::compare_allometry(&spec, &variant, &growth) {
        out!("  reference at {} years: {}", record.age, record.describe());
    }
    if digest {
        out!("  digest {}", growth.digest());
    }
    if profile {
        for line in profile_lines(&times) {
            out!("{line}");
        }
    }
    Ok(())
}

/// One plant of `plantc stand`'s document, ready to grow.
type StandPlant = (PlantSpec, [f64; 2], Program, Vec<f64>, GrowthSettings);

fn stand_plant(
    plant: &serde_json::Value,
    years: f64,
    conditions: &Conditions,
) -> Result<StandPlant, Failure> {
    let id = plant["species"]
        .as_str()
        .ok_or("each plant names its species")?;
    let spec = load_spec(id)?;
    let at_m = stand_pair(&plant["at_m"], "at_m")?;
    let seed = match &plant["seed"] {
        serde_json::Value::Null => spec.variants.seeds[0],
        seed => seed
            .as_u64()
            .ok_or("a plant's seed must be a whole number")?,
    };
    let (program, params) = spec
        .program_in(library())
        .map_err(|error| error.to_string())?;
    let params = spec.seeded_params(&program, &params, seed);
    let settings = GrowthSettings {
        seed,
        dt: spec.growth.step,
        years,
        keyframes: vec![years],
        conditions: conditions.clone(),
        limits: Limits::default(),
        host: None,
    };
    Ok((spec, at_m, program, params, settings))
}

fn stand_number(value: &serde_json::Value, what: &str) -> Result<f64, String> {
    value
        .as_f64()
        .ok_or_else(|| format!("stand {what} must be a number"))
}

fn stand_pair(value: &serde_json::Value, what: &str) -> Result<[f64; 2], String> {
    match value.as_array().map(Vec::as_slice) {
        Some([a, b]) => Ok([stand_number(a, what)?, stand_number(b, what)?]),
        _ => Err(format!("stand {what} must be two numbers")),
    }
}

/// `plantc stand`: plants grown together (`plantgen::stand`).
fn stand_command(args: &[String]) -> Result<(), Failure> {
    let options = Options::parse(args, &[])?;
    let given = options.one_positional("a stand document")?;
    let text = if given.trim_start().starts_with('{') {
        given.to_string()
    } else {
        fs::read_to_string(given).map_err(|error| format!("cannot read {given}: {error}"))?
    };
    let document: serde_json::Value =
        serde_json::from_str(&text).map_err(|error| format!("invalid stand: {error}"))?;
    let years = stand_number(&document["years"], "years")?;
    let tile_m = match &document["tile_m"] {
        serde_json::Value::Null => None,
        tile => Some(stand_pair(tile, "tile_m")?),
    };
    let open = Conditions::preset(Environment::Open);
    let conditions = match &document["conditions"] {
        serde_json::Value::Null => open,
        place => Conditions::from_json(&place.to_string())?.over(&open),
    };
    let plants = document["plants"]
        .as_array()
        .ok_or("stand plants must be a list")?;
    let grown = plants
        .iter()
        .map(|plant| stand_plant(plant, years, &conditions))
        .collect::<Result<Vec<_>, _>>()?;
    let members: Vec<plantgen::stand::Member<'_>> = grown
        .iter()
        .map(
            |(_, at_m, program, params, settings)| plantgen::stand::Member {
                program,
                params,
                settings,
                at_m: *at_m,
            },
        )
        .collect();
    let started = Instant::now();
    let growths =
        plantgen::stand::grow_stand(&members, tile_m).map_err(|error| error.to_string())?;
    out!(
        "{} plants grown together for {years} years in {:.1} s{}",
        growths.len(),
        started.elapsed().as_secs_f64(),
        tile_m.map_or(String::new(), |[east, north]| format!(
            ", a tile {east} by {north} m that wraps"
        ))
    );
    for ((spec, at_m, ..), growth) in grown.iter().zip(&growths) {
        let Some(graph) = growth.keyframes.last() else {
            continue;
        };
        #[allow(clippy::cast_precision_loss)]
        let count = graph.organs.len().max(1) as f64;
        let sum = graph
            .organs
            .iter()
            .fold(Vec3::ZERO, |sum, organ| sum + organ.position);
        out!(
            "  {} at {:?}: {}; leaves lean {:.2} m east, {:.2} m north",
            spec.id,
            at_m,
            describe(graph, &growth.organ_types),
            sum.x / count,
            -sum.z / count
        );
    }
    Ok(())
}

#[allow(clippy::too_many_lines)]
fn render_command(args: &[String]) -> Result<(), Failure> {
    let options = Options::parse(
        args,
        &[
            "env",
            "seed",
            "age",
            "view",
            "lod",
            "size",
            "out",
            "quality",
            "program",
            "focus",
            "span",
            "alone",
            "day",
            "parts",
            "conditions",
            "from-age",
        ],
    )?;
    let day = options.day()?;
    let program = options.program()?;
    let place = options.place()?;
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
    let (growth, _) = grow_variant_in(
        &spec,
        program.as_deref(),
        environment,
        seed,
        vec![age],
        age,
        place.as_ref(),
        false,
    )?;
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
        let (middle, reach) = card_extent(look);
        let frame = 1.3 * reach;
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
                within: None,
                adult: None,
            }],
            structures: Vec::new(),
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
                    focus: Some((Vec3::Y * (length * middle), length * frame)),
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

/// Where an organ's card is centred along it and how far it reaches, in
/// lengths of the organ: half way and its length or width, whichever is
/// more; a cluster's card (`Look::span`) where its card frame puts it.
fn card_extent(look: &looks::Look) -> (f64, f64) {
    let (foot, length) = look.span();
    let middle = match (&look.inflorescence, look.mount()) {
        (None, _) => 0.5,
        (Some(_), looks::Mount::Facing) => foot,
        (Some(_), looks::Mount::Along) => foot + 0.5 * length,
    };
    (middle, length * look.aspect().max(1.0))
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
        // or width, whichever is more; a cluster's round its card's middle.
        let (largest, middle) = row_days
            .iter()
            .map(|&day| {
                let (looks, shares) = looks_of(&spec, &growth, day);
                let share = shares.get(index).copied().unwrap_or(1.0);
                let (middle, reach) = looks.get(index).map_or((0.5, 1.0), card_extent);
                (
                    share * reach,
                    looks
                        .get(index)
                        .and_then(|look| look.inflorescence.as_ref())
                        .map(|_| share * middle),
                )
            })
            .fold((1.0, None), |(most, at), (reach, middle)| {
                if reach > most {
                    (reach, middle)
                } else {
                    (most, at.or(middle))
                }
            });
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
                        within: None,
                        adult: None,
                    }]
                } else {
                    Vec::new()
                },
                structures: Vec::new(),
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
                    focus: Some((
                        Vec3::Y * middle.map_or(length * largest * 0.5, |middle| length * middle),
                        length * largest * 1.3,
                    )),
                },
            ));
            let name = |stage: Stage| match stage {
                Stage::Gone => "gone".to_string(),
                Stage::Bud => "bud".to_string(),
                Stage::Flower => "flower".to_string(),
                Stage::Fruit { unripe } => format!("fruit, {:.0}% unripe", unripe * 100.0),
            };
            // A cluster opening one unit after another: its first unit's
            // stage to its last's.
            let (first, last) = (name(season.stage(day)), name(season.unit_stage(day, 1.0)));
            stages.push(if first == last || season.opening <= 0.0 {
                format!("{day}: {first}")
            } else {
                format!("{day}: {first} to {last}")
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
    let options = Options::parse(
        args,
        &["out", "quality", "threads", "program", "day", "profile"],
    )?;
    let spec = load_spec(options.one_positional("a species")?)?;
    let out = PathBuf::from(options.flags.get("out").map_or("plants", String::as_str));
    build_package(&spec, &options, &out)?;
    Ok(())
}

/// Build the package of `spec` in `out` with the options `--quality`,
/// `--day`, `--program` and `--threads`, unless it is already there,
/// saying what happens; its path.
fn build_package(spec: &PlantSpec, options: &Options, out: &Path) -> Result<PathBuf, Failure> {
    let program = options.program()?;
    let quality = options.quality()?;
    let day = options.day()?;
    let threads = options
        .number("threads")?
        .unwrap_or_else(package::default_threads)
        .max(1);
    let inputs = match &program {
        Some(source) => Inputs::with_program_in(spec, library(), source, &quality, day),
        None => Inputs::on_day_in(spec, library(), &quality, day),
    }
    .map_err(|error| error.to_string())?;
    let inputs = profiled(spec, inputs, options)?;
    if let Some(path) = package::existing(out, &spec.id, &inputs.key) {
        out!(
            "{} is already built:\n  key {}\n  {}",
            spec.id,
            inputs.key,
            path.display()
        );
        return Ok(path);
    }
    let started = Instant::now();
    // Progress is best effort: a closed output shows up at the summary.
    let built = package::build(&inputs, threads, &mut |line| {
        let _ = writeln!(io::stdout(), "  {line}");
    })
    .map_err(|error| error.to_string())?;
    let path = package::write(&built, out).map_err(|error| error.to_string())?;
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
    Ok(path)
}

/// `inputs` of `spec` with motion objects built beside the meshes when
/// `--profile motion` asks, saying which material values are generic.
fn profiled(spec: &PlantSpec, inputs: Inputs, options: &Options) -> Result<Inputs, Failure> {
    match options.flags.get("profile").map(String::as_str) {
        None | Some("static") => Ok(inputs),
        Some("motion") => {
            let traits = library()
                .traits_of(&spec.id)
                .map_err(|error| error.to_string())?;
            let motion = plantgen::motion::Motion::of(&traits);
            if motion.assumed.len() > 1 {
                out!(
                    "{} does not state {}: generic values stand in",
                    spec.id,
                    motion.assumed[..motion.assumed.len() - 1].join(", ")
                );
            }
            Ok(inputs.with_motion(motion))
        }
        Some(other) => Err(format!("--profile is static or motion, not `{other}`").into()),
    }
}

#[allow(clippy::too_many_lines)]
fn export_command(args: &[String]) -> Result<(), Failure> {
    let options = Options::parse(
        args,
        &[
            "out",
            "format",
            "quality",
            "program",
            "day",
            "env",
            "seed",
            "age",
            "shade",
            "compress",
            "attributes",
            "triangles",
            "max-mib",
            "profile",
        ],
    )?;
    match options.flags.get("format").map(String::as_str) {
        None | Some("glb") => {}
        Some(other) => {
            return Err(format!("unknown format `{other}`; plantc exports glb").into());
        }
    }
    let shade = match options.flags.get("shade").map(String::as_str) {
        None | Some("attribute") => export::Shade::Attribute,
        Some("baked") => export::Shade::Baked,
        Some(other) => {
            return Err(format!("unknown shade `{other}`; use attribute or baked").into());
        }
    };
    let compression = match options.flags.get("compress").map(String::as_str) {
        None | Some("none") => export::Compression::None,
        Some("quantize") => export::Compression::Quantize,
        Some("meshopt") => export::Compression::Meshopt,
        Some(other) => {
            return Err(
                format!("unknown compression `{other}`; use none, quantize or meshopt").into(),
            );
        }
    };
    let attributes = match options.flags.get("attributes").map(String::as_str) {
        None | Some("all") => export::Attributes::All,
        Some("wind") => export::Attributes::Wind,
        Some("none") => export::Attributes::None,
        Some(other) => {
            return Err(format!("unknown attributes `{other}`; use all, wind or none").into());
        }
    };
    let budget = export::Budget {
        triangles: options
            .flags
            .get("triangles")
            .map(|given| triangle_budget(given))
            .transpose()?,
        bytes: options
            .flags
            .get("max-mib")
            .map(|given| {
                given
                    .parse::<f64>()
                    .ok()
                    .filter(|mebibytes| mebibytes.is_finite() && *mebibytes > 0.0)
                    .map(|mebibytes| {
                        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                        let bytes = (mebibytes * 1_048_576.0) as usize;
                        bytes
                    })
                    .ok_or_else(|| format!("--max-mib takes a size above 0, found `{given}`"))
            })
            .transpose()?,
    };
    let settings = export::Settings {
        shade,
        compression,
        attributes,
        budget,
    };
    let target = options.one_positional("a species or a package")?;
    let package_dir = Path::new(target);
    let started = Instant::now();
    // A package's own inputs, checked against its key; or a spec's.
    let (inputs, manifest) = if package_dir.join(package::MANIFEST).is_file() {
        for flag in ["quality", "program", "day", "profile"] {
            if options.flags.contains_key(flag) {
                return Err(format!("`--{flag}` is the package's own; {target} is built").into());
            }
        }
        let manifest = package::read_manifest(package_dir).map_err(|error| error.to_string())?;
        let quality = export::quality_of(&manifest).map_err(|error| error.to_string())?;
        let program = package::read_object(package_dir, &manifest.program.object)
            .map_err(|error| error.to_string())?;
        let inputs = export::inputs_of(&manifest, &program, &quality, library())
            .map_err(|error| error.to_string())?;
        (inputs, Some(manifest))
    } else {
        let spec = load_spec(target)?;
        let quality = options.quality()?;
        let day = options.day()?;
        let inputs = match options.program()? {
            Some(source) => Inputs::with_program_in(&spec, library(), &source, &quality, day),
            None => Inputs::on_day_in(&spec, library(), &quality, day),
        }
        .map_err(|error| error.to_string())?;
        (profiled(&spec, inputs, &options)?, None)
    };
    let spec = &inputs.spec;
    let variants = spec.variant_list();
    let mut variant = export::typical_variant(spec, library());
    let environment = options
        .flags
        .get("env")
        .map(|name| {
            Environment::from_name(name).ok_or_else(|| {
                format!("unknown environment `{name}`; use open, edge, interior or suppressed")
            })
        })
        .transpose()?;
    let seed: Option<u64> = options.number("seed")?;
    if environment.is_some() || seed.is_some() {
        let (environment, seed) = (
            environment.unwrap_or(variants[variant].environment),
            seed.unwrap_or(variants[variant].seed),
        );
        variant = variants
            .iter()
            .position(|grown| grown.environment == environment && grown.seed == seed)
            .ok_or_else(|| {
                let grown: Vec<String> = variants
                    .iter()
                    .map(|grown| format!("{} {}", grown.environment.name(), grown.seed))
                    .collect();
                format!(
                    "{} has no variant {} {seed}; it has {}",
                    spec.id,
                    environment.name(),
                    grown.join(", ")
                )
            })?;
    }
    let age = options
        .number::<f64>("age")?
        .unwrap_or_else(|| export::oldest_age(spec));
    out!(
        "growing {} {} seed {} over {} years, kept at {age}",
        spec.id,
        variants[variant].environment.name(),
        variants[variant].seed,
        spec.growth.years
    );
    let plant = export::grow(&inputs, variant, age).map_err(|error| error.to_string())?;
    let read = |sha: &str| package::read_object(package_dir, sha);
    let built = manifest.as_ref().map(|manifest| export::Built {
        manifest,
        object: &read,
    });
    let built = built.filter(|_| !budget.is_some());
    let exported = export::export(&inputs, &plant, &settings, built.as_ref())
        .map_err(|error| error.to_string())?;
    let out = PathBuf::from(options.flags.get("out").map_or("exports", String::as_str));
    let written = export::write(&exported, &out).map_err(|error| error.to_string())?;
    out!(
        "exported {} in {:.1} s{}:",
        exported.stem,
        started.elapsed().as_secs_f64(),
        if built.is_some() {
            ", every level as the package holds it"
        } else {
            ""
        }
    );
    for (level, path) in exported.levels.iter().zip(&written) {
        #[allow(clippy::cast_precision_loss)]
        let mebibytes = level.glb.len() as f64 / 1_048_576.0;
        out!(
            "  {}  {mebibytes:.1} MiB, {} triangles in {} meshes drawn{}",
            path.display(),
            level.summary.triangles,
            level.summary.instances,
            level.budget.map_or(String::new(), |given| format!(
                ", dial {:.2}{}",
                level.detail.dial,
                if given.fits { "" } else { ", over its budget" }
            ))
        );
    }
    if let Some(index) = written.last() {
        out!("  {}", index.display());
    }
    if let Some(bytes) = budget.bytes {
        let whole: usize = exported.levels.iter().map(|level| level.glb.len()).sum();
        #[allow(clippy::cast_precision_loss)]
        let (whole, bytes) = (whole as f64 / 1_048_576.0, bytes as f64 / 1_048_576.0);
        if whole <= bytes {
            out!("  {whole:.1} MiB in all, within {bytes:.1} MiB");
        } else {
            out!(
                "  {whole:.1} MiB in all, over {bytes:.1} MiB: less than the coarsest drawings take"
            );
        }
    }
    Ok(())
}

/// A triangle budget: one count for the nearest level, each farther
/// level a quarter of the one before, or a count per level separated by
/// commas; a count may end in k (thousands) or m (millions).
fn triangle_budget(given: &str) -> Result<[usize; export::LEVELS], String> {
    let count = |text: &str| {
        let text = text.trim();
        let (number, unit) = match text.char_indices().last() {
            Some((at, 'k' | 'K')) => (&text[..at], 1e3),
            Some((at, 'm' | 'M')) => (&text[..at], 1e6),
            _ => (text, 1.0),
        };
        number
            .parse::<f64>()
            .ok()
            .map(|number| number * unit)
            .filter(|count| count.is_finite() && *count >= 1.0)
            .map(|count| {
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let count = count.round() as usize;
                count
            })
            .ok_or_else(|| {
                format!(
                    "--triangles takes a count such as 500k, or {} separated by commas; found `{given}`",
                    export::LEVELS
                )
            })
    };
    let counts = given.split(',').map(count).collect::<Result<Vec<_>, _>>()?;
    match counts.as_slice() {
        [nearest] => Ok(export::Budget::quartering(*nearest)),
        _ => counts.try_into().map_err(|_| {
            format!(
                "--triangles takes one count or {} separated by commas; found `{given}`",
                export::LEVELS
            )
        }),
    }
}

fn inspect_command(args: &[String]) -> Result<(), Failure> {
    let options = Options::parse(args, &[])?;
    let path = options.one_positional("a package directory")?;
    let manifest = package::verify(Path::new(path)).map_err(|error| error.to_string())?;
    out!("{}", package::summary(&manifest).trim_end());
    out!("  every object matches its hash");
    Ok(())
}
