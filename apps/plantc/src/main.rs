//! `plantc`: the plant compiler.
//!
//! Grows species from their specs, renders previews for review, and builds
//! content-addressed `.afterplant` packages. It runs offline on the CPU and
//! never opens a window. Run `plantc help` for usage; see
//! `docs/user/PLANTS.md` and `docs/developer/PLANTS.md`.

use std::collections::BTreeMap;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;
use std::{env, fmt, fs};

use after_plants::graph::{OrganType, PlantGraph};
use after_plants::ground::{self, GROUND_LOOK_SIZE};
use after_plants::grow::{Growth, GrowthSettings, grow};
use after_plants::litter;
use after_plants::looks::Look;
use after_plants::lsys::{Limits, Neighbourhood, Program};
use after_plants::mesh;
use after_plants::package::{self, Inputs};
use after_plants::preview::{self, PreviewOptions, View};
use after_plants::quality::{self, Quality};
use after_plants::raster;
use after_plants::spec::{self, Environment, PROGRAMS, PlantSpec, SPECIES, Variant};
use after_plants::templates::{self, Templates};

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

fn run() -> Result<(), Failure> {
    let mut args: Vec<String> = env::args().skip(1).collect();
    if args.is_empty() {
        return print_usage();
    }
    let command = args.remove(0);
    match command.as_str() {
        "list" => list(),
        "check" => check(&args),
        "grow" => grow_command(&args),
        "render" => render_command(&args),
        "sheet" => sheet_command(&args),
        "atlas" => atlas_command(&args),
        "ground" => ground_command(&args),
        "build" => build_command(&args),
        "inspect" => inspect_command(&args),
        "help" | "--help" | "-h" => print_usage(),
        other => Err(format!("unknown command `{other}`; run `plantc help`").into()),
    }
}

fn print_usage() -> Result<(), Failure> {
    out!(
        "plantc: grow, preview and package procedural plants

Usage:
  plantc list
      List the built-in species and plant programs.
  plantc check <species|spec.json|program.lsys>
      Check a spec or a program and report the first problem with its line.
  plantc grow <species|spec.json> [--env ENV] [--seed N] [--years N]
      Grow one variant and print its size at every keyframe.
  plantc render <species|spec.json> --out FILE.png [--env ENV] [--seed N]
                [--age N] [--view side|three-quarter|top] [--lod 0-3]
                [--size PIXELS] [--quality draft|standard]
      Render one variant at one age, with a 1.8 m figure for scale, or
      a rod in 10 cm stripes beside a plant lower than 1.5 m.
  plantc sheet <species|spec.json> --out FILE.png [--seed N] [--size PIXELS]
      Render every keyframe age (columns) in every environment (rows).
  plantc atlas <species|spec.json> --out FILE.png
      Draw the species' organ card textures (leaves, needles, flowers) in
      their colours, one per organ, side by side.
  plantc ground --out FILE.png [--seed N] [--looks words|litter]
      Draw the 26 ground looks the terrain wears, seven to a row in the
      order of the ground's words, or with `--looks litter` the 23 canopy
      species' litters: each look's colour above its relief.
  plantc build <species|spec.json> [--out DIR] [--quality draft|standard]
               [--threads N]
      Build a .afterplant package in DIR (default `plants`) and print its
      key. A package that is already built is not built again.
  plantc inspect <package.afterplant>
      Check every object of a package and summarise it.

A species is a built-in id (see `plantc list`) or a path to a spec file.
grow, render, sheet, atlas and build accept --program FILE.lsys to try a
changed program in place of the species' built-in one.
ENV is open, edge, interior or suppressed."
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
                fs::read_to_string(path).map_err(|error| format!("cannot read {path}: {error}"))
            })
            .transpose()
    }

    fn environment(&self, spec: &PlantSpec) -> Result<Environment, String> {
        match self.flags.get("env") {
            Some(name) => Environment::from_name(name).ok_or_else(|| {
                format!("unknown environment `{name}`; use open, edge, interior or suppressed")
            }),
            None => Ok(spec.variants.environments[0]),
        }
    }

    fn quality(&self) -> Result<Quality, String> {
        let name = self.flags.get("quality").map_or("standard", String::as_str);
        quality::profile(name)
            .ok_or_else(|| format!("unknown quality `{name}`; use draft or standard"))
    }
}

fn load_spec(name: &str) -> Result<PlantSpec, String> {
    if Path::new(name).extension().is_some_and(|ext| ext == "json") {
        let text =
            fs::read_to_string(name).map_err(|error| format!("cannot read {name}: {error}"))?;
        PlantSpec::from_json(&text).map_err(|error| error.to_string())
    } else {
        PlantSpec::builtin(name).map_err(|error| error.to_string())
    }
}

fn list() -> Result<(), Failure> {
    out!("Species:");
    let width = SPECIES.iter().map(|(id, _)| id.len()).max().unwrap_or(0);
    for (id, _) in SPECIES {
        let spec = PlantSpec::builtin(id).map_err(|error| error.to_string())?;
        out!(
            "  {id:<width$}  {} ({}), program `{}`",
            spec.taxon.common_name,
            spec.taxon.scientific_name,
            spec.generator.program
        );
    }
    out!("Programs:");
    for (name, _) in PROGRAMS {
        out!("  {name}");
    }
    Ok(())
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
        let program = Program::compile(&source).map_err(|error| format!("{target}:{error}"))?;
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
    let (program, _) = spec.program().map_err(|error| error.to_string())?;
    out!(
        "{}: valid; program `{}` revision {}, {} variants, keyframes {:?}",
        spec.id,
        program.name,
        program.revision,
        spec.variant_list().len(),
        spec.growth.keyframes
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
        None => spec.program(),
    }
    .map_err(|error| error.to_string())?;
    let settings = GrowthSettings {
        seed,
        dt: spec.growth.step,
        years,
        keyframes,
        neighbourhood: neighbourhood(spec, environment),
        limits: Limits::default(),
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
    };
    for record in package::compare_allometry(&spec, &variant, &growth) {
        out!("  reference at {} years: {}", record.age, record.describe());
    }
    Ok(())
}

fn render_command(args: &[String]) -> Result<(), Failure> {
    let options = Options::parse(
        args,
        &[
            "env", "seed", "age", "view", "lod", "size", "out", "quality", "program",
        ],
    )?;
    let program = options.program()?;
    let spec = load_spec(options.one_positional("a species")?)?;
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
    let quality = options.quality()?;
    let lod = quality.lods.get(level).ok_or("`--lod` must be 0 to 3")?;
    let size: usize = options.number("size")?.unwrap_or(900);
    let growth = grow_variant(&spec, program.as_deref(), environment, seed, vec![age], age)?;
    let graph = &growth.keyframes[0];
    let looks = looks_of(&spec, &growth);
    let plant = mesh::build(
        graph,
        &looks,
        &spec.appearance,
        &lod.for_height(graph.height),
    );
    let templates = Templates::for_looks(&looks);
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
        "wrote {out}: LOD{level}, {} triangles ({} wood, {} cards)",
        plant.triangle_count(),
        plant.wood.triangle_count(),
        plant.cards.len() * 2
    );
    Ok(())
}

fn sheet_command(args: &[String]) -> Result<(), Failure> {
    let options = Options::parse(args, &["seed", "size", "out", "quality", "program"])?;
    let program = options.program()?;
    let spec = load_spec(options.one_positional("a species")?)?;
    let out = options.flags.get("out").ok_or("missing `--out FILE.png`")?;
    let seed = options.number("seed")?.unwrap_or(spec.variants.seeds[0]);
    let size: usize = options.number("size")?.unwrap_or(360);
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
        let looks = looks_of(&spec, &growth);
        let templates = Templates::for_looks(&looks);
        let tallest = growth
            .keyframes
            .iter()
            .map(|graph| graph.height)
            .fold(0.0, f64::max);
        for graph in &growth.keyframes {
            out!(
                "  {:<10} {}",
                environment.name(),
                describe(graph, &growth.organ_types)
            );
            let plant = mesh::build(
                graph,
                &looks,
                &spec.appearance,
                &quality.lods[0].for_height(graph.height),
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
    let templates = Templates::for_looks(&looks);
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
        "words" => (ground::ground_looks(seed), Vec::new()),
        "litter" => {
            let drawn: Vec<_> = (0..litter::LITTERS.len())
                .filter_map(|index| litter::litter_drawn(index, seed))
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
fn looks_of(spec: &PlantSpec, growth: &Growth) -> Vec<Look> {
    spec.appearance.looks(
        growth
            .organ_types
            .iter()
            .map(|organ| (organ.name.as_str(), organ.kind)),
    )
}

fn build_command(args: &[String]) -> Result<(), Failure> {
    let options = Options::parse(args, &["out", "quality", "threads", "program"])?;
    let program = options.program()?;
    let spec = load_spec(options.one_positional("a species")?)?;
    let quality = options.quality()?;
    let threads = options
        .number("threads")?
        .unwrap_or_else(package::default_threads)
        .max(1);
    let out = PathBuf::from(options.flags.get("out").map_or("plants", String::as_str));
    let inputs = match &program {
        Some(source) => Inputs::with_program(&spec, source, &quality),
        None => Inputs::new(&spec, &quality),
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
