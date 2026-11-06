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

use plantgen::api;
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

mod scene;

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
        "scene" => scene::scene_command(&args),
        "site" => site_command(&args),
        "cracks" => cracks_command(&args),
        "field" => field_command(&args),
        "moss" => moss_command(&args),
        "logs" => logs_command(&args),
        "render" => render_command(&args),
        "sheet" => sheet_command(&args),
        "tiers" => tiers_command(&args),
        "parts" => parts_command(&args),
        "organs" => organs_command(&args),
        "year" => year_command(&args),
        "lineup" => lineup_command(&args),
        "atlas" => atlas_command(&args),
        "measure" => measure_command(&args),
        "measure-mask" => measure_mask_command(&args),
        "mechanics" => mechanics_command(&args),
        "breakage" => breakage_command(&args),
        "validate" => validate_command(&args),
        "ground" => ground_command(&args),
        "bark" => bark_command(&args),
        "build" => build_command(&args),
        "export" => export_command(&args),
        "variants" => variants_command(&args),
        "fill" => fill_command(&args),
        "inspect" => inspect_command(&args),
        "unpack" => unpack_command(&args),
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
                [--age N] [--view side|eye|three-quarter|oblique|top|under]
                [--lod 0-3]
                [--size PIXELS] [--quality draft|standard]
                [--focus X,Y,Z [--span METRES]] [--parts on] [--figure off]
                [--conditions FILE|JSON [--from-age N]]
                [--break snap|WIND|bone:N [--fallen off]] [--leaves off]
                [--fell HEIGHT[,DEGREES] [--fallen off|notched] [--by saw|ax|beaver]] [--turn DEGREES]
      Render one variant at one age, with a 1.8 m figure for scale, or
      a rod in 10 cm stripes beside a plant lower than 1.5 m (--figure
      off draws neither, for plant-identification models). --focus
      frames a close-up of SPAN metres (default 0.5) round a point of
      the plant, metres from its foot with +Y up; --focus leaf:F,
      blade:F, bloom:F, stalk:F or stem:F frames the middle of the leaf,
      of its blade past its stalk, of the flower, fruit or cone, of the
      hairy young stem or leaf stalk, or of a piece of living stem (its
      prickles and bark), F of the
      way up the plant's (0 lowest, 1 highest),
      SPAN 0.05 unless given; --view under looks up at it from below, at
      the lower faces of its leaves; --view eye looks from a standing
      person's eye height (1.6 m), as photos of trees are taken. --parts on (level 0)
      draws every flower, fruit, cone and needle shoot as its part mesh,
      as WorldLab does near the camera; with --focus, only those within
      four spans (at least 1 m) of the focus, the rest as cards. --conditions grows it in a place
      as plantc grow does (the place itself is not drawn). --break
      draws it broken where plantc breakage finds it fails: snap at its
      stem's weakest section, WIND at every site a steady wind of WIND m/s
      at 10 m breaks, bone:N at bone N's weakest site; torn stubs stay on
      it and the pieces lie on the ground, unless --fallen off. A
      branch under half its stem's thickness breaks at its collar, a
      thicker one splits out with a strip of the stem; tough wood often
      breaks hinged, the piece hanging from its break. --leaves off
      draws the wood alone (cards and tufts left out), to see wounds
      the foliage hides. --fell fells it with a saw: a notch a third
      of the stem deep at HEIGHT m (0.3 unless given), facing DEGREES
      (0 = +X, 90 = +Z; 0 unless given), a back cut a twelfth of the
      stem higher leaving a twelfth of it as the hinge (the US Forest
      Service's conventional undercut); the stump stands with its sawn
      top and the log lies down the fall (--fallen off: the stump
      alone; --fallen notched: the tree still standing, the notch cut,
      or a beaver's band gnawed to the core). --by ax fells it with an
      ax instead (the Forest Service ax manual: a front notch half the
      stem deep, a back notch a sixth higher leaving a sixth, both with
      chopped 45-degree faces, chips round the stump); --by beaver
      gnaws it down (Morgan 1868: a band two thirds of the stem high,
      the stump and log ends left conical, chips at the foot). --turn turns the plant DEGREES about its foot
      (anticlockwise from above) before the view, --focus points with it.
  plantc stand <stand.json|JSON>
      Grow plants together, each making room for the others' crowns, and
      print each one's size at the end and where its leaves lean. The
      document gives years, and plants: a list of each plant's species,
      at_m (metres east and north) and seed. It may also give tile_m (east
      and north: the stand is a tile that wraps at its edges) and
      conditions (a place every plant grows in). Seeds default to each
      spec's first.
  plantc scene <scene.json> [--out PREFIX] [--views side,oblique,top,close]
               [--size PIXELS] [--day N] [--quality draft|standard]
               [--focus X,Y,Z [--span METRES]] [--cover HALF]
      Lay grown variants out as instances and draw them together, as a
      host's renderer draws a forest (stands plan, step 1). The document
      gives instances: each one's species, env (open, edge, interior or
      suppressed; open when left out), seed, age (years), at (metres east
      and north), yaw (degrees, anticlockwise from above) and scale. Each
      species, environment and seed grows once, to its oldest age; nothing
      grows together. --out writes PREFIX-VIEW.png for each view (close
      frames --focus, SPAN metres across, default 4). --cover prints the
      share of the square HALF metres each way from the middle that crowns
      cover seen straight down. A `lay_out` block lays members out on a
      tile by their crowns (plantgen::stand_tile); a `develop` block
      builds the stand from its stand_age and species pool (species, env,
      seeds), each species' niche and lifespan, and a site's moisture,
      heat_load, elevation_m and slope_deg (plantgen::development), with
      tile_m, seed, repeat, cover, scale, overlap and herb_radius_m.
  plantc logs [--species ID] --render FILE.png [--length M] [--width M]
              [--level 0-3] [--view VIEW] [--size PIXELS] [--seed N]
      Draw five logs of a species side by side, decay classes I (farthest)
      to V (nearest; plantgen::deadwood): barked, raised on branch stubs;
      barked, sagging; traces of bark on faded wood; bare, brown and
      oval, under moss; red-brown, flattened, sunk and half buried.
      Douglas-fir, 6 m and 0.6 m wide unless given, at level 0.
  plantc moss <site.json> [--years N] [--seed N] [--latitude DEG]
              [--box X0,Y0,Z0,X1,Y1,Z1] [--out COVER.json] [--cover COVER.json]
              [--render FILE.png [--view VIEW] [--size PIXELS] [--from south|north]]
              [--glb FILE.glb] [--focus X,Y,Z | --genus GENUS] [--span METRES]
              [--wet 0..1] [--detail tint|patch|shell|shoots|near] [--capsules yes|no]
      Grow mosses over a site's faces (plantgen::mosses): every moss genus
      the library gives a colony_form settles where its light, wetness
      and substrate fit (rain on faces turned up, damp at a wall's foot,
      shade from the sun, mortar and tiles basic, bark and wood acid) and
      spreads year by year, years 30 when left out, within --box (the
      whole site when left out), under the site's `canopy` of tree crowns
      (each centre_m [east, north], base_m, top_m, radius_m and density,
      shade per metre) when it has one. --out writes the colonies; --cover reads
      them instead of growing. --render draws the site and its mosses:
      shells of the colonies' forms, or with --detail shoots every shoot's
      leaves and capsules (near: only within --span of --focus); --wet 1
      just rained on, 0 dry (0.4 when left out); --from north looks from
      the north side. --genus looks at that genus's widest well-grown
      colony instead of --focus; a close render prints the genus of the
      colony it is centred on, for its caption. --glb writes the same
      site and moss as one glTF file (nodes `site` and `moss`, vertex
      colours, the site's frame: +Y up, -Z north) that Blender opens.
  plantc site <site.json> [--out FILE.png] [--view VIEW] [--size PIXELS]
              [--day N] [--quality draft|standard]
              [--focus X,Y,Z [--span METRES]]
      Grow plants on a mesh you hand over (sites plan S4, grow here).
      The document names mesh (an OBJ file, its path from the document's
      folder), z_up (true when the file's +Z is up), materials (each
      usemtl name as soil, rock, bark, wood or built, or an object of
      as, the material, thin_m, a thin part's thickness in metres (a
      fence's wire, a trellis's slat: open faces standing as a slab that
      thick), climb false for a surface climbers do not hold to, clear
      true for one kept clear (a window or door: shoots that reach it
      are cut back each year), and colour, sRGB [r, g, b] to draw it in;
      * for the rest, soil when left out), years, and plants: each
      one's species, at ([x, z], or [x, y, z] for the ground at or below
      y, in the mesh's units, metres), seed, years and optionally cut
      (half_m, below_m, above_m and cell_m of the box round it; from its
      size when left out). Each plant stands on the highest soil under
      it (under eaves, on the ground below them; with no soil, the
      highest surface) and grows in a substrate cut from the mesh round
      its foot; a climber climbs the mesh's rock, bark, wood and built
      faces in place of its stock host. Plants that grow in steps of one
      length grow together, each making room for the others' crowns.
      Prints each plant's size and how much of it stands in the mesh,
      and with --out renders them on the mesh (one bark for all wood),
      or with --focus a close-up of SPAN metres (default 2) round a point
      in the mesh's frame. docs/sites/house.json is a house with windows
      and a door kept clear, a picket fence and a wire fence.
      pavements lists crack tiles (JSON files, plantgen::pavement): a
      paved slab with its cracks and holes and the soil in them, standing
      in for the mesh at and below its surface. A plant set on one roots
      in a crack: on sound slab it moves to the nearest opening within
      snap_m (0.05 m when left out) or is refused. docs/sites/road.json
      is a street with its cracks at three decay levels
      (docs/sites/make_road.py).
  plantc cracks <site.json> [--years 0,2,5,10,20] [--traffic 0-1]
                [--maintenance 0-1] [--seed N] [--pool ID,ID...]
                [--explain 1] [--out FOLDER [--render 1] [--view VIEW]
                [--size PIXELS] [--quality draft|standard]
                [--focus X,Y,Z [--span METRES]]]
      Which plants come up by themselves in a site's pavements, year by
      year from bare slab (pavement-cracks plan K3, plantgen::succession):
      seed from far off and from the document's seed_sources (plants
      near the site: species, at [x, z], years), caught by each crack
      and hole; seedlings that take hold as the soil there carries them;
      growth held back by that soil; death by life span, drought,
      trampling (the tiles' traffic lanes, scaled by --traffic), shade
      and upkeep (--maintenance, a yearly chance of weeding). The pool
      is the Pacific Northwest's pavement weeds unless --pool names
      species. Prints the plants of each year in --years, by species and
      by tile; with --explain, each species' soil supply by kind of
      opening. With --out, writes the site with each year's plants
      (SITE-year-N.json, for plantc site), and with --render 1 draws it.
  plantc field <site.json> --out FIELD.json [--tile X0,Z0,X1,Z1]
               [--cell METRES] [--margin METRES] [--patch METRES]
               [--sky-rays N] [--latitude DEGREES]
      Measure a site once for stands of plants (plantgen::site_field,
      docs/sites/SITE-FIELD.md): a ground grid (height, slope, aspect,
      heat load, material, rooting depth, wetness, sky, distance to
      walls and kept-clear surfaces, under a roof), surface patches on
      walls, roofs, fences, poles and boulders by host, and the crack
      sites of its pavements. The tile is the mesh's extent unless
      --tile names it; the grid reaches --margin (20 m) past it.
  plantc tiers <species>... --out FOLDER [--seed N] [--size PIXELS] [--age YEARS]
               [--columns full,cards,far] [--view VIEW] [--focus F,S]
               [--views side,oblique,close]
      Render each species at its oldest age (or --age) from the side at
      full detail (level 0 cards), at the baked-card tiers and as a far
      impostor (`plantgen::billboards`), one PNG a species in FOLDER (with
      --views, one in a folder per view), and print each tier's
      triangles, tone, overdraw and cover (m², side and 45° down) as JSON
      lines.
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
                [--view side|eye|three-quarter|oblique|top|under] [--size PIXELS]
      Render a row per species of its first N seeds (default 4) at one
      age (default its oldest keyframe), each row at one scale.
  plantc breakage <species|spec.json> [--env ENV] [--seed N] [--years N]
                  [--crown C,N] [--z0 M] [--rows N]
      Grow a species to its oldest keyframe, or N years, and print where
      it breaks in a steady wind (plant motion S5): its wood's strength
      (stated, or estimated from its density by the Wood Handbook's
      functions), the wind at 10 m that snaps its stem and where, the wind
      that turns it over on its roots, and its N weakest break sites with
      the piece each drops. --crown sets the crown's drag, Cd = C u^-N on
      its silhouette (default as the species states, else 2.34,0.88),
      --z0 the ground's roughness (default 0.03 m).
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
  plantc measure-mask <mask.png> [--cover COVER.png] [--view side]
      Measure a plant mask (a photograph's or a render's: alpha, or
      brightness without alpha, at least half is the plant) as a grown
      plant's side view is measured, and print one JSON object: the
      outline's fit to an ellipse, the share of it the sky shows through,
      its width over height, where it is widest, its solidity (area over
      convex hull) and the share of its sky in large gaps. Rows below the
      lowest that reaches a quarter of the widest are the bare stem and
      are left out. --cover, the same size, is what hides the sky (the
      photograph's pixels that are not sky); without it, the mask. Only
      the side view is measured so far.
  plantc validate [<species>...] [--seeds N]
                  [--group sizes|counts|kinds|months]
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
      every state the plants draw is one the species states; the months
      its flowers are open and its fruit ripe in are read from its looks
      on every day of the year, passing when they are the months stated
      and warning a month off at an end. --group shows one of the four.
      Exits non-zero on any error.
  plantc atlas <species|spec.json> --out FILE.png [--organ ORGAN]
      Draw the species' organ card textures (leaves, needles, flowers) in
      their colours, one per organ, side by side, each lower face that
      differs (an underside colour, sori) beside its upper; with --organ,
      that organ's card alone (a leaf to set beside photographs).
  plantc ground --out FILE.png [--seed N] [--looks words|litter]
      Draw the nine plant-made ground looks (needles, leaves, thatch,
      twigs, moss, sphagnum, grass, tussock and cushion), seven to a row,
      or with `--looks litter` the 23 canopy species' litters: each look's
      colour above its relief.
  plantc bark --look FILE.json|--species ID --out DIR [--size N] [--period M] [--seed N]
      (each stage's tile wraps its stem a whole number of times, near M)
  plantc bark --probe up|round --out DIR [--size N] [--period M]
      Draw a bark look (the bark language's drawing record), from a file
      or a species' spec (written beside the tiles as ID.json), stage by
      stage, each at a diameter typical of it, as tiles repeating every
      M metres (default 0.5) of N texels a side (default 512): base
      colour, normal, occlusion-roughness-metallic and height maps, and a
      sheet with each stage's tile lit and a trunk wearing it.
  plantc build <species|spec.json> [--out DIR] [--quality draft|standard]
               [--threads N] [--profile static|motion] [--cards yes|no]
               [--old yes|no]
      Build a .afterplant package in DIR (default `plants`) and print its
      key. A package that is already built is not built again. With
      --profile motion it also holds each keyframe's bones and branches
      and each level's binding to them (plant motion S2), under a key of
      its own; the static package is unchanged. With --cards yes it also
      holds each keyframe's baked-card tiers (low and minimal cards, the
      far impostor; see `plantc tiers`), under a key of its own. With
      --old yes it also grows the species' old ages, for stands: each
      twice the one before past its oldest keyframe, up to the age its old
      plants commonly reach (`age_max_years`; Douglas-fir 120, 240 and 480
      years), under a key of its own.
  plantc variants <species|spec.json> --count N [--out DIR] [--first-seed N]
                 [--env ENV] [--age N] [--day N] [--quality draft|standard]
                 [--threads N] [--shade attribute|baked]
                 [--compress none|quantize|meshopt] [--attributes all|wind|none]
                 [--triangles N[,N,N,N]] [--max-mib N] [--levels tiers|package]
      Make N distinct plants of a species, ready to import: each grown from
      its own seed (from --first-seed, 1 unless given, counting up) and
      written as plantc export writes one (STEM-lod0.glb to STEM-lod3.glb
      and STEM.json), into DIR (default variants/<species>), with
      variants.json listing every plant's seed, size, files, triangles and
      a fingerprint of its bytes. Seed n is the same plant however many are
      made; the seeds the spec lists are the plants its package keeps.
      Without --env, the species' typical site; without --age, its oldest
      keyframe. --threads plants grow at once (every core unless given);
      the files do not depend on it. Levels are tiers (max, high, mid,
      low) unless --levels package; the export flags are plantc export's.
  plantc fill <community.json>... --tile X,Y --tile-m N [--seed N]
              [--years N] [--stands stands.json] [--out DIR]
              [--prototypes none|used|all] [--ages N,N,...] [--seeds N]
              [--herbs yes|no] [--edges yes|no] [--site M,H,E,S]
              [--threads N] [--quality draft|standard]
              [--day N] [--shade attribute|baked]
              [--compress none|quantize|meshopt]
              [--attributes all|wind|none] [--triangles N[,N,N,N]]
              [--max-mib N] [--levels tiers|package] [--glb LEVEL]
              [--glb-cells LEVEL,... [--cell-m N]]
              [--packages DIR [--profile static|motion] [--cards yes|no]]
              [--field SITE.json [--corner X,Z] [--cracks YEARS]
               [--moss YEARS]
               [--render PNG [--view V] [--size N] [--level N]
                [--focus X,Y,Z --span M] [--cut M] [--wet W]]]
      Fill one map tile with plant communities (each a JSON file: an id
      and members, each a species, optionally the layer it keeps to and
      its abundance; origin planted for a yard's or street's kept
      trees) as the library API's Fill does, and write
      DIR/tiles/X_Y.json (header, ground cover, forest floor and far
      canopy per stand cell, the logs on its floor, stand numbers) and
      DIR/tiles/X_Y.bin (32-byte instances;
      docs/LIBRARY-API.md). Each community's stand is developed at each of
      --ages years since disturbance (default 10,20,40,80,160,320,640);
      a stand cell takes the nearest. Without --stands (a StandMap:
      cell_m, cols and cells, each with its community, its
      years_since_disturbance and optionally its ground's moisture, 0 to
      1, for a creek's bank or a lake's shore), the first community
      covers the tile at --years since disturbance (default 100). A
      member without a niche is named on stderr: it suits every site.
      A map's cells may be as
      fine as a city's yards and verges (4 m): each reads its window of one
      stand tile laid out for its block of cells about 40 m across, so fine
      cells cost little more than coarse ones. Every prototype is measured
      first, its size kept in DIR/prototypes so a later fill grows none
      again. --prototypes used (the default) also exports, into
      DIR/prototypes with prototypes.json, each prototype the tile places
      that is not there yet; all exports every prototype; none only the
      tile. It prints the stand's numbers, each species', and each
      layer seen from above as inventory plots measure it (upper, middle
      and lower trees by thirds of the tallest tree, shrubs, herbs), then
      its forest floor: litter, moss, logs (a hectare, and by decay
      class) and herbs. --herbs no leaves herbs to the ground cover. --edges no grows
      a stand's first row beside a road or clearing as inside the stand,
      not as an edge facing the opening. --site gives the
      flat tile's moisture, heat load, elevation (m) and slope (degrees)
      for the species' niches (default 0.5,1,0,0). The plan is the
      same on every machine and thread count, and a tile meets its
      neighbours without a seam. --field fills the tile over a site
      document's mesh (as plantc site reads it), its south-west corner at
      --corner (the mesh's low x and high z unless given), measured into a
      site field first: plants stand where its ground lets them, smaller
      on shallow soil, herbs on roofs that hold soil, climbers at the feet
      of walls and fences. --cracks YEARS adds what comes up in its
      pavements in those years among the fill's species of the pavement
      pool. --moss YEARS grows mosses over the tile for those years
      (the genera the mosses list of the site names, or the library's;
      docs/sites/MOSS.md) and writes DIR/tiles/X_Y.moss.json.
      --render draws the filled tile on its mesh, each prototype at
      --level (1 unless given), with its logs and its forest floor's
      litter and moss, and its --moss moss, as wet as --wet (0 dry to 1
      just rained on, 0.4 unless given); --cut M leaves out all above M
      metres, to show the floor. --glb LEVEL (0 nearest to 3 farthest)
      also writes DIR/tiles/X_Y.glb: the whole tile as one glTF file,
      each prototype's level LEVEL once and every plant an instance of it
      (EXT_mesh_gpu_instancing), which Blender, Godot, Unity and three.js
      open as they are (Blender before 5 and Godot need --compress
      quantize or none, or `plantc unpack` afterwards: they do not read
      meshopt). --glb-cells LEVEL,... (say
      0,1,2,3) cuts the tile into cells --cell-m across (20 unless given)
      and writes each as such a file at every level listed,
      DIR/tiles/X_Y/c<col>_<row>-lod<N>.glb, with DIR/tiles/X_Y/cells.json
      saying from how far each level of each cell is enough, so a host
      that reads only glTF draws near cells fine and far ones coarse.
      --packages DIR also builds the
      packages that hold the prototypes the tile places (every one with
      --prototypes all), one per species, setting and seed with a
      keyframe at each of their ages, as plantc build builds one
      (--profile, --cards), into DIR, and lists in DIR/packages.json
      which package and keyframe draws each prototype. A wall climber's
      package grows it at the foot of a wall 6 m high, as the fill does;
      draw it cut to its wall by the instance's extra.
  plantc inspect <package.afterplant>
      Check every object of a package and summarise it.
  plantc unpack <FILE.glb|DIR> --out <FILE.glb|DIR>
      Copy a glTF file, or a folder such as a fill's output, with
      EXT_meshopt_compression undone in every .glb, so Blender 4.5 and
      other importers without a meshopt decoder read it (positions stay
      quantized, which they read). Other files are copied as they are,
      so a tile's cells.json still names its files.
  plantc export <species|spec.json|package.afterplant> [--out DIR]
                [--format glb] [--env ENV] [--seed N] [--age N]
                [--shade attribute|baked] [--quality draft|standard]
                [--compress none|quantize|meshopt]
                [--attributes all|wind|none]
                [--triangles N[,N,N,N]] [--max-mib N]
                [--profile static|motion] [--levels package|tiers]
                [--bark-look spec|FILE.json [--bark-texels N] [--bark-period M]]
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
      meshopt decoder and Blender from 5.2; `plantc unpack` undoes it for
      Blender 4.5), 3 to 4 times smaller again: the 30-year bigleaf
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
      --bark-look draws the bark from the plant's spec (`spec`: its
      stated bark look, else one made from its bark pattern) or a bark
      record (`plantc bark`): each stage baked to textures N texels a side (default 512), repeating
      near every M metres (default 0.5), as a material of its own, with
      the wood split by stage and given texture coordinates in tiles and
      tangents. Where one stage gives way to the next, the wood wears
      textures that mix them in steps of a quarter, in patches.
      --levels tiers draws the levels as tiers for games, max, high, mid
      and low: the nearest as the package draws it, then the most detail
      within an eighth, a 64th and a 512th of its triangles (high at
      least 20k and at most 500k, mid 2k to 50k, low 200 to 5k), each at
      most half the one before. Low is the plant's baked cards (its
      package's low-cards tier, every card fixed where it stands, over
      one texture) where they take fewer triangles than mid and at most
      5k: a lacy herb's own cards thin to nothing drawn that small.
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
            spec.taxon.name(),
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
        spec.taxon.name(),
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

fn measure_mask_command(args: &[String]) -> Result<(), Failure> {
    let options = Options::parse(args, &["view", "cover"])?;
    let [path] = options.positional.as_slice() else {
        return Err(
            "usage: plantc measure-mask <mask.png> [--cover COVER.png] [--view side]".into(),
        );
    };
    let view = options.flags.get("view").map_or("side", String::as_str);
    if view != "side" {
        return Err(format!("unknown view `{view}`: only `side` is measured so far").into());
    }
    let bytes = fs::read(path).map_err(|error| format!("cannot read {path}: {error}"))?;
    let mask = plantgen::form::mask_from_png(&bytes).map_err(|error| format!("{path}: {error}"))?;
    let cover = match options.flags.get("cover") {
        Some(cover_path) => {
            let bytes = fs::read(cover_path)
                .map_err(|error| format!("cannot read {cover_path}: {error}"))?;
            let cover = plantgen::form::mask_from_png(&bytes)
                .map_err(|error| format!("{cover_path}: {error}"))?;
            if (cover.width, cover.height) != (mask.width, mask.height) {
                return Err(format!(
                    "{cover_path} is {}x{}, the mask {}x{}",
                    cover.width, cover.height, mask.width, mask.height
                )
                .into());
            }
            Some(cover)
        }
        None => None,
    };
    let Some(side) = plantgen::form::measure_mask(&mask, cover.as_ref()) else {
        return Err(format!("{path}: the mask is empty").into());
    };
    let json = serde_json::json!({
        "view": view,
        "cover": cover.is_some(),
        "width": mask.width,
        "height": mask.height,
        "outline_cells": side.cells,
        "outline_fit": side.outline_fit,
        "see_through": side.see_through,
        "width_to_height": side.width_to_height,
        "widest_at": side.widest_at,
        "solidity": side.solidity,
        "large_gaps": side.large_gaps,
    });
    out!("{json}");
    Ok(())
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

/// `plantc breakage`: where a grown plant breaks in wind (plant motion S5).
#[allow(clippy::too_many_lines)]
fn breakage_command(args: &[String]) -> Result<(), Failure> {
    use plantgen::breakage::{self, Drag};
    use plantgen::motion::{self, Motion};
    let options = Options::parse(args, &["env", "seed", "years", "crown", "z0", "rows"])?;
    let [id] = options.positional.as_slice() else {
        return Err("`plantc breakage` takes one species".into());
    };
    let spec = load_spec(id)?;
    let traits = library()
        .traits_of(&spec.id)
        .map_err(|error| error.to_string())?;
    let motion = Motion::of(&traits).with_flare(&spec.appearance);
    let environment = options.environment(&spec)?;
    let seed = options.number("seed")?.unwrap_or(spec.variants.seeds[0]);
    let years = options.number("years")?.unwrap_or(spec.growth.years);
    let growth = grow_variant(&spec, None, environment, seed, vec![years], years)?;
    let Some(graph) = growth.keyframes.last() else {
        return Err(format!("{} grew nothing", spec.id).into());
    };
    let mut drag = Drag {
        z0: options.number("z0")?.unwrap_or(motion.drag.z0),
        ..motion.drag
    };
    if let Some(text) = options.flags.get("crown") {
        let numbers: Vec<f64> = text
            .split(',')
            .map(|part| part.trim().parse::<f64>())
            .collect::<Result<_, _>>()
            .map_err(|_| format!("`--crown` expects C,N, found `{text}`"))?;
        let [at_rest, streamlining] = numbers[..] else {
            return Err(format!("`--crown` expects C,N, found `{text}`").into());
        };
        drag.crown.at_rest = at_rest;
        drag.crown.streamlining = streamlining;
    }
    let found = motion.of_graph(graph, &growth.organ_types);
    let material = &found.material;
    let record = motion::record(&found, &motion, graph, &growth.organ_types);
    let started = Instant::now();
    let broken = breakage::breakage(&found, record.anchorage.moment, &drag, motion.flare);
    let took = started.elapsed();
    let said = |key: &str| {
        if motion.estimated.contains(&key) {
            " (estimated from density)"
        } else if motion.assumed.contains(&key) {
            " (assumed)"
        } else {
            " (stated)"
        }
    };
    out!(
        "{} ({}), seed {seed}, {:.0} years, {:.1} m, {:.0} kg",
        spec.taxon.name(),
        spec.id,
        graph.age,
        graph.height,
        found.mass()
    );
    out!(
        "wood {:.0} kg/m3 basic{}; rupture {:.1} MPa{}; modulus {:.0} MPa{} along the grain",
        material.wood_density / (1.0 + traits_moisture(&traits)),
        said("wood_density_basic_kg_m3"),
        material.rupture.unwrap_or(0.0) / 1.0e6,
        said("wood_rupture_green_mpa"),
        material.modulus / 1.0e6,
        said("wood_modulus_green_mpa"),
    );
    let leaf_area: f64 = found.beams.leaf_area.iter().sum();
    let exposure = drag.exposure(&found.beams);
    out!(
        "{leaf_area:.0} m2 of leaves (one side), {:.2} kg/m2{}; crown {:.1} to {:.1} m up, \
         {:.1} to {:.1} m wide, silhouette {:.0} to {:.0} m2",
        material.leaf_mass.unwrap_or(0.0),
        said("leaf_mass_fresh_kg_m2"),
        exposure.base,
        exposure.top,
        exposure.width.iter().copied().fold(f64::INFINITY, f64::min),
        exposure.width.iter().copied().fold(0.0, f64::max),
        exposure
            .silhouette
            .iter()
            .copied()
            .fold(f64::INFINITY, f64::min),
        exposure.silhouette.iter().copied().fold(0.0, f64::max),
    );
    out!(
        "drag: crown Cd {} u^-{} on its silhouette, u held to 10..{} m/s{} (Cd {:.2} at 10 m/s, \
         {:.2} at {}); wood below the crown Cd {}; z0 {} m",
        drag.crown.at_rest,
        drag.crown.streamlining,
        drag.crown.tested_to,
        if options.flags.contains_key("crown") {
            " (given)"
        } else {
            said("crown_drag")
        },
        drag.crown.coefficient(10.0),
        drag.crown.coefficient(drag.crown.tested_to),
        drag.crown.tested_to,
        drag.wood,
        drag.z0
    );
    let wind = |w: Option<f64>| {
        w.map_or_else(
            || format!("> {:.0} m/s", breakage::FASTEST),
            |w| format!("{w:.1} m/s"),
        )
    };
    let failure = &broken.failure;
    let snap_height = failure
        .snap_bone
        .and_then(|bone| {
            broken.sites[bone as usize]
                .iter()
                .find(|site| (site.along - failure.snap_along).abs() < 1.0e-12)
        })
        .map_or(0.0, |site| site.point[1]);
    out!(
        "stem snaps at {} (10 m wind), {:.1} m up; roots turn over at {} \
         (anchorage {:.0} kN m, {:.0} N m per kg of stem{}); fails first by {}",
        wind(failure.snap),
        snap_height,
        wind(failure.overturn),
        record.anchorage.moment / 1000.0,
        motion.anchorage,
        said("anchorage"),
        failure.first
    );
    let all: Vec<(usize, &breakage::Site)> = broken
        .sites
        .iter()
        .enumerate()
        .flat_map(|(bone, sites)| sites.iter().map(move |site| (bone, site)))
        .collect();
    let inside = all.iter().filter(|(_, site)| site.along > 0.0).count();
    out!(
        "{} bones, {} break sites ({} along a bone, not at its foot); found in {:.1} ms",
        found.bones.len(),
        all.len(),
        inside,
        took.as_secs_f64() * 1000.0
    );
    let rows = options.number::<usize>("rows")?.unwrap_or(12);
    let mut order = all.clone();
    order.sort_by(|a, b| {
        let key = |s: &breakage::Site| s.wind.unwrap_or(f64::INFINITY);
        key(a.1).total_cmp(&key(b.1)).then(a.0.cmp(&b.0))
    });
    out!(
        "{:>5} {:>5} {:>6} {:>7} {:>8} {:>10} {:>9} {:>6}",
        "bone",
        "depth",
        "along",
        "cm",
        "height",
        "wind m/s",
        "piece kg",
        "bones"
    );
    for (bone, site) in order.iter().take(rows) {
        out!(
            "{:>5} {:>5} {:>6.2} {:>7.1} {:>8.2} {:>10} {:>9.2} {:>6}",
            bone,
            found.bones[*bone].depth,
            site.along,
            site.radius * 200.0,
            site.point[1],
            site.wind
                .map_or_else(|| "-".to_string(), |w| format!("{w:.1}")),
            site.piece.mass,
            site.piece.bones.len()
        );
    }
    Ok(())
}

/// A species' green moisture share, as stated or the motion stand-in.
fn traits_moisture(traits: &serde_json::Map<String, serde_json::Value>) -> f64 {
    traits
        .get("wood_moisture_green_share")
        .and_then(plantgen::checks::Stated::of)
        .map_or(0.8, |stated| {
            stated
                .typical
                .unwrap_or(f64::midpoint(stated.min, stated.max))
        })
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
        spec.taxon.name(),
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
        estimated: Vec::new(),
        settings,
        anchorage: plantgen::motion::ANCHORAGE_PER_KG,
        drag: plantgen::breakage::Drag::default(),
        flare: None,
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

/// `plantc validate`: stated sizes, counts, kinds and months against grown
/// plants (spec rework M8B and M8A).
#[allow(clippy::too_many_lines)]
fn validate_command(args: &[String]) -> Result<(), Failure> {
    let options = Options::parse(args, &["seeds", "group"])?;
    let count = options.number::<usize>("seeds")?.unwrap_or(5).max(1);
    let group = options.flags.get("group").cloned();
    if let Some(group) = &group
        && !matches!(group.as_str(), "sizes" | "counts" | "kinds" | "months")
    {
        return Err(format!("--group is sizes, counts, kinds or months, not {group}").into());
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
            spec.taxon.name(),
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
        let sections: [(&str, Vec<[String; 5]>); 4] = [
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
            (
                "months",
                report
                    .timings
                    .iter()
                    .map(|timing| {
                        let reason = match timing.verdict {
                            checks::Verdict::Warning => "a month off at an end",
                            checks::Verdict::Error if timing.strays() => {
                                "drawn in months the species does not state"
                            }
                            checks::Verdict::Error => "not drawn in months the species states",
                            checks::Verdict::NotGrown => "nothing drawn to compare",
                            checks::Verdict::Pass => "",
                        }
                        .to_string();
                        [
                            timing.character.to_string(),
                            checks::months_text(&timing.stated),
                            checks::months_text(&timing.drawn),
                            timing.verdict.word().to_string(),
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
            )
            .chain(
                report
                    .timings
                    .iter()
                    .filter(|_| group.as_deref().is_none_or(|group| group == "months"))
                    .map(|timing| timing.verdict),
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
    // The spec's ages, its old ages, the ages it gives reference sizes
    // for, and the end.
    let mut keyframes: Vec<f64> = spec
        .growth
        .keyframes
        .iter()
        .copied()
        .chain(spec.old_ages())
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
        spec.taxon.name(),
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
/// The point `--focus leaf:F`, `blade:F`, `bloom:F`, `stalk:F` or
/// `stem:F` names on `plant`: the middle of the leaf card (of its blade,
/// past its stalk; of the flower, fruit or cone card; of the piece of
/// hairy young stem or leaf stalk, `mesh::PlantMesh::hair_tubes`; or of
/// the piece of living wood in `graph`, where its prickles and bark show)
/// `F` of the way up the plant's, from 0 the lowest to 1 the highest.
fn named_point(
    plant: &mesh::PlantMesh,
    graph: &plantgen::graph::PlantGraph,
    templates: Option<&Templates>,
    what: &str,
    at: f64,
) -> Result<Vec3, Failure> {
    let vector = |v: [f32; 3]| Vec3::new(f64::from(v[0]), f64::from(v[1]), f64::from(v[2]));
    let mut points: Vec<Vec3> = match what {
        "leaf" => plant.cards[..plant.bent_cards()]
            .iter()
            .map(|card| vector(card.base) + vector(card.heading) * (f64::from(card.length) * 0.5))
            .collect(),
        // The middle along the card of its texels that are leaf and not
        // stalk: a sundew's small blade at the end of its long petiole.
        "blade" => {
            let Some(templates) = templates else {
                return Err("`--focus blade:F` needs the plant's leaf templates".into());
            };
            let size = templates.size;
            let middles: Vec<f64> = templates
                .templates
                .iter()
                .map(|drawn| {
                    let (mut sum, mut count) = (0.0, 0.0_f64);
                    for at in 0..size * size {
                        if drawn.coverage[at] >= 0.5
                            && drawn.accent.get(at).copied().unwrap_or(0.0) < 0.5
                        {
                            #[allow(clippy::cast_precision_loss)]
                            let row = (at / size) as f64 + 0.5;
                            sum += row;
                            count += 1.0;
                        }
                    }
                    #[allow(clippy::cast_precision_loss)]
                    let size = size as f64;
                    if count > 0.0 { sum / count / size } else { 0.5 }
                })
                .collect();
            plant.cards[..plant.bent_cards()]
                .iter()
                .map(|card| {
                    let along = middles
                        .get(usize::from(card.template))
                        .copied()
                        .unwrap_or(0.5);
                    // On the card as it is bent, not on its straight
                    // heading: a curled blade's middle lies millimetres
                    // off that line, out of a few-millimetre frame.
                    card.bend
                        .point(
                            vector(card.base),
                            vector(card.heading),
                            vector(card.left),
                            f64::from(card.width) * 0.5,
                            f64::from(card.length),
                            0.0,
                            along,
                        )
                        .0
                })
                .collect()
        }
        // The organs drawn as part meshes, or else the flat cards after
        // the bent leaves: flowers, fruit and cones.
        "bloom" => {
            let middle = |card: &mesh::Card| {
                vector(card.base) + vector(card.heading) * (f64::from(card.length) * 0.5)
            };
            if plant.sites.is_empty() {
                plant.cards[plant.bent_cards()..]
                    .iter()
                    .map(middle)
                    .collect()
            } else {
                plant
                    .sites
                    .iter()
                    .filter_map(|&site| plant.cards.get(site as usize))
                    .map(middle)
                    .collect()
            }
        }
        "stalk" => plant
            .hair_tubes
            .iter()
            .map(|tube| (vector(tube.start) + vector(tube.end)) * 0.5)
            .collect(),
        "stem" => graph
            .segments
            .iter()
            .filter(|segment| {
                segment.body == 0
                    && segment.shed.is_none_or(|shed| shed > graph.age)
                    && segment.born <= graph.age
            })
            .map(|segment| (segment.start + segment.end) * 0.5)
            .collect(),
        _ => {
            return Err(format!(
                "`--focus` names leaf:F, blade:F, bloom:F, stalk:F or stem:F, found `{what}`"
            )
            .into());
        }
    };
    if points.is_empty() {
        return Err(format!("`--focus {what}:F`: the plant has none drawn at this level").into());
    }
    points.sort_by(|a, b| a.y.total_cmp(&b.y));
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    let index = (at.clamp(0.0, 1.0) * (points.len() - 1) as f64).round() as usize;
    Ok(points[index.min(points.len() - 1)])
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
            "break",
            "fallen",
            "figure",
            "leaves",
            "turn",
            "fell",
            "by",
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
    let breaking = options.flags.get("break").cloned();
    if parted && breaking.is_some() {
        return Err("`--break` draws without `--parts`".into());
    }
    let felling = options.flags.get("fell").cloned();
    let by = options.flags.get("by").cloned();
    if by.is_some() && felling.is_none() {
        return Err("`--by` says how `--fell` cuts; give `--fell` too".into());
    }
    if felling.is_some() && (parted || breaking.is_some()) {
        return Err("`--fell` draws without `--parts` or `--break`".into());
    }
    let notched =
        felling.is_some() && options.flags.get("fallen").map(String::as_str) == Some("notched");
    let fallen = match options.flags.get("fallen").map(String::as_str) {
        None | Some("on") => true,
        Some("off") => false,
        Some("notched") if felling.is_some() => false,
        Some(other) => return Err(format!("`--fallen` must be on or off, found `{other}`").into()),
    };
    let leaves = match options.flags.get("leaves").map(String::as_str) {
        None | Some("on") => true,
        Some("off") => false,
        Some(other) => return Err(format!("`--leaves` must be on or off, found `{other}`").into()),
    };
    let figure = match options.flags.get("figure").map(String::as_str) {
        None | Some("on") => true,
        Some("off") => false,
        Some(other) => return Err(format!("`--figure` must be on or off, found `{other}`").into()),
    };
    let quality = options.quality()?;
    let lod = quality.lods.get(level).ok_or("`--lod` must be 0 to 3")?;
    let size: usize = options.number("size")?.unwrap_or(900);
    // `--focus leaf:F` or `stalk:F` names a point once the plant is built.
    let named = options.flags.get("focus").and_then(|text| {
        let (what, at) = text.split_once(':')?;
        Some((what.to_owned(), at.trim().parse::<f64>().ok()?))
    });
    let focus = match options.flags.get("focus").filter(|_| named.is_none()) {
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
    let turn: f64 = options.number("turn")?.unwrap_or(0.0);
    let staged = drawn(&growth.keyframes[0], &sizes, &looks);
    let graph = &*if turn == 0.0 {
        staged
    } else {
        Cow::Owned(turned(&staged, turn))
    };
    let focus = focus.map(|(point, span)| (spun(point, turn), span));
    let bodies = bodies_of(&spec, &growth);
    let mut bloom = None;
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
        // The organs to draw as part meshes are known only until they
        // are placed.
        // A blade's middle needs the leaf templates, drawn later; the
        // close-up's parts are placed round its leaf card's middle.
        if let Some((what, at)) = named.as_ref() {
            let kind = if what == "blade" {
                "leaf"
            } else {
                what.as_str()
            };
            bloom = Some(named_point(&plant, graph, None, kind, *at)?);
        }
        // A close-up swaps part meshes in round what it looks at only, as
        // a renderer does near the camera; the rest keep their cards.
        let span: f64 = options
            .number("span")?
            .unwrap_or(if named.is_some() { 0.05 } else { 0.5 });
        let near = bloom
            .or(focus.map(|(point, _)| point))
            .map(|point| (point, (span * 4.0).max(1.0)));
        plantgen::parts::place_near(&mut plant, &looks, &parts, near);
        plant
    } else if let Some(how) = &felling {
        felled_mesh(
            &spec,
            graph,
            how,
            by.as_deref().unwrap_or("saw"),
            if notched {
                Felling::Notched
            } else if fallen {
                Felling::Fallen
            } else {
                Felling::Stump
            },
            &looks,
            &bodies,
            &lod.for_height(graph.height),
            level,
        )?
    } else if let Some(how) = &breaking {
        broken_mesh(
            &spec,
            graph,
            &growth.organ_types,
            how,
            fallen,
            &looks,
            &bodies,
            &lod.for_height(graph.height),
            level,
        )?
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
    plant.vary_by_plant(&looks, seed);
    if !leaves {
        // The wood alone, to see what foliage hides: a break's wounds.
        plant.cards.clear();
        plant.tufts.clear();
    }
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
        let host_graph = &*drawn(&host_growth.keyframes[0], &host_sizes, &host_looks);
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
    let focus = match named {
        Some((what, at)) => {
            let span: f64 = options.number("span")?.unwrap_or(0.05);
            let point = match bloom {
                Some(point) => point,
                None => named_point(&plant, graph, Some(&templates), &what, at)?,
            };
            Some((point, span))
        }
        None => focus,
    };
    let image = preview::render(
        &plant,
        &templates,
        &PreviewOptions {
            width: size,
            height: size * 5 / 4,
            view,
            supersample: 2,
            figure,
            fit_plant: !figure,
            frame_height: None,
            frame_reach: None,
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
    out!("{}", describe(&growth.keyframes[0], &growth.organ_types));
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

/// One plant of a site, ready to grow: its spec, foot, program,
/// parameters and settings.
type SitePlant = (PlantSpec, Vec3, Program, Vec<f64>, GrowthSettings);

/// The height a species reaches at `age` years, and how far it spreads
/// from its foot, as its facts state them: its growth targets
/// (`allometry`), else its greatest height and crown width, else 2 m.
fn site_size(spec: &PlantSpec, age: f64) -> (f64, f64) {
    let traits = library().traits_of(&spec.id).unwrap_or_default();
    let greatest = |name: &str| traits.get(name).and_then(plantgen::traits::greatest);
    let tallest = greatest("height_max_m");
    let target = spec
        .allometry
        .iter()
        .filter(|point| point.age <= age + 1.0e-9)
        .map(|point| point.height)
        .fold(None, |best: Option<f64>, height| {
            Some(best.map_or(height, |b| b.max(height)))
        });
    let height = target
        .or_else(|| {
            spec.allometry
                .iter()
                .map(|point| point.height)
                .reduce(f64::min)
        })
        .or(tallest)
        .unwrap_or(2.0)
        .min(tallest.unwrap_or(f64::INFINITY));
    let reach = greatest("crown_width_max_m").map_or(height * 0.6, |width| width * 0.5);
    (height, reach.max(height * 0.3))
}

/// `plantc moss`: grow mosses over a site and draw them.
#[allow(clippy::too_many_lines)]
fn logs_command(args: &[String]) -> Result<(), Failure> {
    let options = Options::parse(
        args,
        &[
            "species", "render", "length", "width", "level", "view", "size", "seed",
        ],
    )?;
    if !options.positional.is_empty() {
        return Err("`logs` takes its species as `--species ID`".into());
    }
    let png = options
        .flags
        .get("render")
        .ok_or("missing `--render FILE.png`")?;
    let species = options
        .flags
        .get("species")
        .map_or("pseudotsuga-menziesii", String::as_str);
    let spec = library()
        .spec(species)
        .map_err(|error| format!("{species}: {error}"))?;
    let length: f64 = options.number("length")?.unwrap_or(6.0);
    let width: f64 = options.number("width")?.unwrap_or(0.6);
    let level: usize = options.number("level")?.unwrap_or(0);
    let seed: u64 = options.number("seed")?.unwrap_or(1);
    let pixels: usize = options.number("size")?.unwrap_or(1600);
    let gap = 2.2 * width;
    let mut scene = PlantMesh::default();
    for decay in 1..=5_u8 {
        let shape = plantgen::deadwood::Shape {
            length_m: length,
            butt_m: width,
            top_m: (width - plantgen::deadwood::TAPER * length).max(0.45 * width),
            decay,
            seed: seed.wrapping_mul(31).wrapping_add(u64::from(decay)),
        };
        let drawn = plantgen::deadwood::draw(&shape, Some(&spec.appearance), level);
        let placed = api::Log {
            species: species.to_string(),
            position: [0.0, 0.0, 0.0],
            yaw: 0.0,
            pitch: 0.0,
            shape,
        };
        let offset = Vec3::new(0.0, 0.0, (f64::from(decay) - 3.0) * gap);
        scene = on_host(scene, placed_log(drawn, &placed, [0.0, 0.0], offset), 0);
    }
    let half = 0.5 * length + 1.0;
    let deep = 2.5 * gap + 1.0;
    let mut ground = mesh::Mesh::default();
    #[allow(clippy::cast_possible_truncation)]
    let litter =
        plantgen::deadwood::LITTER_SRGB.map(|byte| plantgen::texture::srgb_to_linear(byte) as f32);
    for (x, z) in [(-half, -deep), (half, -deep), (half, deep), (-half, deep)] {
        ground.push(mesh::Vertex::plain(Vec3::new(x, 0.0, z), Vec3::Y, litter));
    }
    ground.indices.extend([0, 2, 1, 0, 3, 2]);
    let templates = Templates::for_plant(&[], &[]).with_bark(spec.appearance.bark_params());
    let view = match options.flags.get("view") {
        Some(name) => View::from_name(name).ok_or_else(|| format!("unknown view `{name}`"))?,
        None => View::ThreeQuarter,
    };
    let image = preview::render_on_site(
        &scene,
        &[(&ground, raster::Material::Opaque)],
        &templates,
        &PreviewOptions {
            width: pixels,
            height: pixels * 3 / 4,
            view,
            supersample: 2,
            figure: false,
            frame_height: None,
            frame_reach: Some(half.max(deep)),
            focus: None,
            fit_plant: false,
        },
    );
    let bytes = raster::encode_png(
        image.width,
        image.height,
        &image.to_srgb8(Some(preview::SKY)),
    )
    .map_err(|error| format!("cannot encode PNG: {error}"))?;
    fs::write(png, bytes).map_err(|error| format!("cannot write {png}: {error}"))?;
    out!(
        "wrote {png}: five {species} logs, decay classes I-V, {} triangles at level {level}",
        scene.triangle_count()
    );
    Ok(())
}

fn moss_command(args: &[String]) -> Result<(), Failure> {
    use plantgen::mosses::{self, Detail, DrawOptions, MossCover, MossSettings};
    let options = Options::parse(
        args,
        &[
            "years", "seed", "latitude", "box", "out", "cover", "render", "view", "size", "from",
            "focus", "span", "wet", "detail", "capsules", "genus", "glb",
        ],
    )?;
    let given = options.one_positional("a site document")?;
    let text =
        fs::read_to_string(given).map_err(|error| format!("cannot read {given}: {error}"))?;
    let document: serde_json::Value =
        serde_json::from_str(&text).map_err(|error| format!("invalid site: {error}"))?;
    let folder = Path::new(given).parent().unwrap_or(Path::new("."));
    let site = site_mesh_of(&document, folder)?;
    let numbers = |name: &str, count: usize| -> Result<Option<Vec<f64>>, String> {
        options
            .flags
            .get(name)
            .map(|text| {
                let values: Vec<f64> = text
                    .split(',')
                    .map(|part| part.trim().parse::<f64>())
                    .collect::<Result<_, _>>()
                    .map_err(|_| format!("`--{name}` expects {count} numbers, found `{text}`"))?;
                if values.len() == count {
                    Ok(values)
                } else {
                    Err(format!(
                        "`--{name}` expects {count} numbers, found `{text}`"
                    ))
                }
            })
            .transpose()
    };
    let started = Instant::now();
    let cover = if let Some(path) = options.flags.get("cover") {
        let text =
            fs::read_to_string(path).map_err(|error| format!("cannot read {path}: {error}"))?;
        MossCover::from_json(&text).map_err(|error| format!("{path}: {error}"))?
    } else {
        let kinds = site_moss_kinds(&document)?;
        let mut settings = MossSettings::default();
        if let Some(years) = options.number::<u32>("years")? {
            settings.years = years;
        }
        if let Some(seed) = options.number::<u64>("seed")? {
            settings.seed = seed;
        }
        if let Some(latitude) = options.number::<f64>("latitude")? {
            settings.latitude_deg = latitude;
        }
        if let Some(values) = numbers("box", 6)? {
            settings.low = [values[0], values[1], values[2]];
            settings.high = [values[3], values[4], values[5]];
        }
        // A site may stand under trees: crowns as upright cylinders of
        // foliage, `centre_m` (east, north) = (x, -z) of the site's origin.
        let canopy: Vec<plantgen::conditions::Envelope> = match document.get("canopy") {
            Some(value) => serde_json::from_value(value.clone())
                .map_err(|error| format!("invalid canopy: {error}"))?,
            None => Vec::new(),
        };
        mosses::grow_under(&site, &kinds, &settings, &canopy)
    };
    out!(
        "{} colonies after {} years ({:.1} s)",
        cover.colonies.len(),
        cover.settings.years,
        started.elapsed().as_secs_f64()
    );
    for (genus, count, area) in cover.census() {
        out!("  {genus:<14} {count:>6} colonies, {:.3} m2", area);
    }
    if let Some(path) = options.flags.get("out") {
        fs::write(path, cover.to_json()?)
            .map_err(|error| format!("cannot write {path}: {error}"))?;
        out!("wrote {path}");
    }
    let render = options.flags.get("render");
    let glb = options.flags.get("glb");
    if render.is_none() && glb.is_none() {
        return Ok(());
    }
    let mut focus = numbers("focus", 3)?.map(|v| Vec3::new(v[0], v[1], v[2]));
    // `--genus G`: look at that genus's showpiece colony, so a view named
    // for a genus shows it and no other.
    if let Some(genus) = options.flags.get("genus") {
        if focus.is_some() {
            return Err("give --genus or --focus, not both".into());
        }
        let colony = cover
            .showpiece(genus)
            .ok_or_else(|| format!("the cover holds no colony of `{genus}`"))?;
        let at = cover.view_point(colony);
        focus = Some(Vec3::new(at[0], at[1], at[2]));
    }
    // Name the colony a close view is centred on, for its caption.
    if let Some(at) = focus
        && let Some((colony, distance)) = cover.nearest([at.x, at.y, at.z])
    {
        let genus = cover
            .kinds
            .get(usize::from(colony.kind))
            .map_or("?", |kind| kind.genus.as_str());
        out!(
            "centred on {genus} (colony {:.1} cm across, {:.0} years old, {:.1} cm from the focus {:.4},{:.4},{:.4})",
            colony.radius_m * 200.0,
            colony.age,
            distance * 100.0,
            at.x,
            at.y,
            at.z
        );
    }
    let span: f64 = options.number("span")?.unwrap_or(2.0);
    // `tint`: no moss triangles; the site's own surfaces coloured by the
    // moss over them, as a host tints a roof's or wall's texture.
    let tinted = options.flags.get("detail").map(String::as_str) == Some("tint");
    let detail = match options.flags.get("detail").map(String::as_str) {
        None | Some("near" | "tint") => Detail::Near,
        Some("patch") => Detail::Patch,
        Some("shell") => Detail::Shell,
        Some("shoots") => Detail::Shoots,
        Some(other) => {
            return Err(format!(
                "unknown detail `{other}`; use tint, patch, shell, shoots or near"
            )
            .into());
        }
    };
    let draw = DrawOptions {
        hydration: options.number::<f64>("wet")?.unwrap_or(0.4).clamp(0.0, 1.0),
        detail,
        near: focus.map(|at| (at, span)),
        capsules: options.flags.get("capsules").is_none_or(|v| v != "no"),
    };
    let mut moss = if tinted {
        plantgen::mesh::Mesh::default()
    } else {
        cover.draw(&site, &draw)
    };
    let (low, high) = match focus {
        Some(at) => {
            let half = Vec3::new(span, span, span) * 1.5;
            (at - half, at + half)
        }
        None => site.bounds(),
    };
    let mut ground = site.drawn(
        low - Vec3::new(1.0, 1.0, 1.0),
        high + Vec3::new(1.0, 1.0, 1.0),
        Vec3::ZERO,
    );
    if tinted {
        // A texel about three pixels across, and 5 mm at the finest.
        let across = focus.map_or_else(|| (high - low).length(), |_| span * 2.0);
        let pixels: f64 = options.number("size")?.unwrap_or(1200.0);
        let step = (across / pixels * 3.0).max(0.005);
        ground = cover.tint(draw.hydration).paint(&ground, step, low, high);
    }
    // `--glb FILE`: the site and its moss as drawn, in the site's own
    // frame (+Y up, -Z north), for Blender and other glTF readers.
    if let Some(path) = glb {
        let detail_name = options.flags.get("detail").map_or("near", String::as_str);
        let file = plantgen::gltf::coloured_glb(
            &[("site", &ground, false), ("moss", &moss, true)],
            "PlantGen moss",
            serde_json::json!({
                "years": cover.settings.years,
                "hydration": draw.hydration,
                "detail": detail_name,
            }),
        );
        fs::write(path, &file).map_err(|error| format!("cannot write {path}: {error}"))?;
        out!(
            "wrote {path}: {} triangles of moss, {} of site ({:.1} MiB)",
            moss.triangle_count(),
            ground.triangle_count(),
            f64::from(u32::try_from(file.len()).unwrap_or(u32::MAX)) / 1_048_576.0
        );
    }
    let Some(render) = render else {
        return Ok(());
    };
    // Seen from the north: the scene turned half round about the focus.
    let north = match options.flags.get("from").map(String::as_str) {
        None | Some("south") => false,
        Some("north") => true,
        Some(other) => return Err(format!("unknown side `{other}`; use south or north").into()),
    };
    let pivot = focus.unwrap_or_else(|| {
        let (a, b) = site.bounds();
        (a + b) * 0.5
    });
    if north {
        for mesh in [&mut moss, &mut ground] {
            for p in &mut mesh.positions {
                p[0] = (2.0 * pivot.x) as f32 - p[0];
                p[2] = (2.0 * pivot.z) as f32 - p[2];
            }
            for n in &mut mesh.normals {
                n[0] = -n[0];
                n[2] = -n[2];
            }
        }
    }
    let view = match options.flags.get("view") {
        Some(name) => View::from_name(name).ok_or_else(|| format!("unknown view `{name}`"))?,
        None => View::Oblique,
    };
    let pixels: usize = options.number("size")?.unwrap_or(1200);
    let framed = match focus {
        Some(at) => Some((at, span)),
        None => {
            let (a, b) = site.bounds();
            Some(((a + b) * 0.5, (b - a).length() * 0.45))
        }
    };
    let templates = Templates::for_plant(&[], &[]);
    let image = preview::render_on_site(
        &PlantMesh::default(),
        &[
            (&ground, raster::Material::Opaque),
            (&moss, raster::Material::Opaque),
        ],
        &templates,
        &PreviewOptions {
            width: pixels,
            height: pixels * 3 / 4,
            view,
            supersample: 2,
            figure: false,
            frame_height: None,
            frame_reach: None,
            focus: framed,
            fit_plant: false,
        },
    );
    let png = raster::encode_png(
        image.width,
        image.height,
        &image.to_srgb8(Some(preview::SKY)),
    )
    .map_err(|error| format!("cannot encode PNG: {error}"))?;
    fs::write(render, png).map_err(|error| format!("cannot write {render}: {error}"))?;
    out!(
        "wrote {render}: {} triangles of moss, {} of site",
        moss.triangle_count(),
        ground.triangle_count()
    );
    Ok(())
}

/// `plantc field`: a site's field, measured once (plantgen::site_field).
fn field_command(args: &[String]) -> Result<(), Failure> {
    use plantgen::site_field::{FieldSettings, build};
    let options = Options::parse(
        args,
        &[
            "out", "tile", "cell", "margin", "patch", "sky-rays", "latitude",
        ],
    )?;
    let given = options.one_positional("a site document")?;
    let out = options
        .flags
        .get("out")
        .ok_or("`plantc field` writes its field to --out FIELD.json")?;
    let text =
        fs::read_to_string(given).map_err(|error| format!("cannot read {given}: {error}"))?;
    let document: serde_json::Value =
        serde_json::from_str(&text).map_err(|error| format!("invalid site: {error}"))?;
    let folder = Path::new(given).parent().unwrap_or(Path::new("."));
    let mesh = site_mesh_of(&document, folder)?;
    let mut settings = FieldSettings::default();
    let number = |name: &str, into: &mut f64| -> Result<(), Failure> {
        if let Some(value) = options.flags.get(name) {
            *into = value
                .parse()
                .map_err(|_| format!("`--{name}` expects a number, found `{value}`"))?;
        }
        Ok(())
    };
    number("cell", &mut settings.cell_m)?;
    number("margin", &mut settings.margin_m)?;
    number("patch", &mut settings.patch_m)?;
    number("latitude", &mut settings.latitude_deg)?;
    if let Some(value) = options.flags.get("sky-rays") {
        settings.sky_rays = value
            .parse()
            .map_err(|_| format!("`--sky-rays` expects a whole number, found `{value}`"))?;
    }
    let (low, high) = match options.flags.get("tile") {
        Some(tile) => {
            let values: Vec<f64> = tile
                .split(',')
                .map(|v| v.trim().parse::<f64>())
                .collect::<Result<_, _>>()
                .map_err(|_| "`--tile` expects X0,Z0,X1,Z1")?;
            let [x0, z0, x1, z1] = values[..] else {
                return Err("`--tile` expects X0,Z0,X1,Z1".into());
            };
            ([x0, z0], [x1, z1])
        }
        None => {
            let (low, high) = mesh.bounds();
            settings.margin_m = options
                .flags
                .get("margin")
                .and_then(|m| m.parse().ok())
                .unwrap_or(0.0);
            ([low.x, low.z], [high.x, high.z])
        }
    };
    let started = std::time::Instant::now();
    let field = build(&mesh, low, high, &settings)?;
    let json = field.to_json()?;
    fs::write(out, &json).map_err(|error| format!("cannot write {out}: {error}"))?;
    let ground = field.ground.height.iter().filter(|h| h.is_some()).count();
    out!(
        "site field of {} x {} cells ({ground} on ground), {} hosts, {} patches, {} crack sites in {:.1} s; wrote {out} ({} KB)",
        field.columns,
        field.rows,
        field.hosts.len(),
        field.patches.len(),
        field.cracks.len(),
        started.elapsed().as_secs_f64(),
        json.len() / 1024
    );
    Ok(())
}

/// The mesh and pavements of a site document whose files are named from
/// `folder` (`plantc site`, `plantc cracks`).
fn site_mesh_of(
    document: &serde_json::Value,
    folder: &Path,
) -> Result<plantgen::site::SiteMesh, Failure> {
    use plantgen::site::Surface;
    let mesh_path = folder.join(
        document["mesh"]
            .as_str()
            .ok_or("a site names its mesh, an OBJ file")?,
    );
    let mut names: BTreeMap<String, Surface> = BTreeMap::new();
    if let Some(table) = document["materials"].as_object() {
        for (name, given) in table {
            names.insert(name.clone(), site_surface(name, given)?);
        }
    }
    let fallback = names
        .get("*")
        .copied()
        .unwrap_or(Surface::solid(plantgen::substrate::SOIL));
    let material_of = |name: Option<&str>| -> Result<Surface, String> {
        Ok(name
            .and_then(|name| names.get(name))
            .copied()
            .unwrap_or(fallback))
    };
    let z_up = document["z_up"].as_bool().unwrap_or(false);
    let obj = fs::read_to_string(&mesh_path)
        .map_err(|error| format!("cannot read {}: {error}", mesh_path.display()))?;
    let mut site_mesh = plantgen::site::SiteMesh::from_obj(&obj, z_up, &material_of)
        .map_err(|error| format!("{}: {error}", mesh_path.display()))?;
    for given in document["pavements"].as_array().into_iter().flatten() {
        let name = given
            .as_str()
            .ok_or("site pavements are crack tile files")?;
        let path = folder.join(name);
        let text = fs::read_to_string(&path)
            .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
        let tile = plantgen::pavement::CrackTile::from_json(&text)
            .map_err(|error| format!("{}: {error}", path.display()))?;
        let pavement = plantgen::pavement::Pavement::new(&tile)
            .map_err(|error| format!("{}: {error}", path.display()))?;
        out!(
            "pavement {name}: {} cracks, {} holes, {:.1}% open",
            tile.cracks.len(),
            tile.holes.len(),
            pavement.open_share() * 100.0
        );
        site_mesh.add_pavement(pavement);
    }
    Ok(site_mesh)
}

/// The pool `plantc cracks` draws on when none is given: the pavement
/// colonists of the Pacific Northwest the plan names (pavement-cracks
/// plan, section 6) that the library has, and the weeds of the road's
/// verges.
const PAVEMENT_POOL: [&str; 17] = [
    "poa-annua",
    "sagina-procumbens",
    "taraxacum-sect-taraxacum",
    "plantago-major",
    "polygonum-aviculare",
    "erigeron-canadensis",
    "hypochaeris-radicata",
    "cirsium-vulgare",
    "trifolium-repens",
    "holcus-lanatus",
    "dactylis-glomerata",
    "verbascum-thapsus",
    "rubus-armeniacus",
    "cytisus-scoparius",
    "buddleja-davidii",
    "alnus-rubra",
    "populus-tristis",
];

/// A species of the pool for the yearly step: its facts from the library,
/// and its size by age grown once in open ground.
/// Says which members of the communities have no niche: each suits every
/// site and stands as half tolerant of shade, so it comes up wherever its
/// community grows, under a closed canopy too.
fn warn_without_niche(communities: &[api::Community]) {
    let library = library();
    let mut missing: Vec<&str> = communities
        .iter()
        .flat_map(|community| &community.members)
        .map(|member| member.species.as_str())
        .filter(|id| {
            library
                .entry(id)
                .is_some_and(|entry| matches!(entry.niche(), Ok(None)))
        })
        .collect();
    missing.sort_unstable();
    missing.dedup();
    if !missing.is_empty() {
        eprintln!(
            "note: no niche for {}: each suits every site and stands half tolerant of shade",
            missing.join(", ")
        );
    }
}

fn colonist(id: &str, ages: &[f64]) -> Result<plantgen::succession::Colonist, String> {
    use plantgen::succession::{Colonist, Size};
    let spec = load_spec(id)?;
    let traits = library()
        .traits_of(&spec.id)
        .map_err(|error| error.to_string())?;
    let woody = matches!(
        traits.get("woodiness").and_then(serde_json::Value::as_str),
        Some("woody" | "semi")
    );
    let (program, params) = spec
        .program_in(library())
        .map_err(|error| error.to_string())?;
    let seed = spec.variants.seeds[0];
    let params = spec.seeded_params(&program, &params, seed);
    let soil = plantgen::lsys::tools::SoilNeed::of_program(&program, &params);
    let years = ages.iter().copied().fold(0.0, f64::max);
    let settings = GrowthSettings {
        seed,
        dt: spec.growth.step,
        years,
        keyframes: ages.to_vec(),
        conditions: Conditions::preset(Environment::Open),
        limits: Limits::default(),
        host: None,
    };
    let growth = plantgen::grow::grow(&program, &params, &settings)
        .map_err(|error| format!("{id}: {error}"))?;
    let sideways = |p: Vec3| p.x.hypot(p.z);
    let sizes = ages
        .iter()
        .zip(&growth.keyframes)
        .map(|(&age, graph)| Size {
            age,
            height: graph.height,
            crown: graph
                .segments
                .iter()
                .map(|segment| sideways(segment.end))
                // A leaf's tip, not where it stands: a rosette's crown is
                // its leaves' reach.
                .chain(
                    graph
                        .organs
                        .iter()
                        .map(|organ| sideways(organ.position + organ.heading * organ.size)),
                )
                .fold(0.0, f64::max),
        })
        .collect();
    Ok(Colonist::new(&spec.id, &traits, woody, soil, sizes))
}

/// `plantc cracks`: the plants that come up by themselves in a site's
/// pavements, year by year (pavement-cracks plan K3).
#[allow(clippy::too_many_lines, clippy::cast_precision_loss)]
fn cracks_command(args: &[String]) -> Result<(), Failure> {
    use plantgen::succession::{Settings, microsites, run};
    let options = Options::parse(
        args,
        &[
            "years",
            "pool",
            "traffic",
            "maintenance",
            "seed",
            "out",
            "render",
            "view",
            "size",
            "day",
            "quality",
            "focus",
            "span",
            "explain",
        ],
    )?;
    let given = options.one_positional("a site document")?;
    let text =
        fs::read_to_string(given).map_err(|error| format!("cannot read {given}: {error}"))?;
    let document: serde_json::Value =
        serde_json::from_str(&text).map_err(|error| format!("invalid site: {error}"))?;
    let folder = Path::new(given).parent().unwrap_or(Path::new("."));
    let years: Vec<u32> = options
        .flags
        .get("years")
        .map_or("0,2,5,10,20", String::as_str)
        .split(',')
        .map(|part| part.trim().parse::<u32>())
        .collect::<Result<_, _>>()
        .map_err(|_| "`--years` expects whole years, as 0,2,5,10,20")?;
    let last = years.iter().copied().max().unwrap_or(0);
    let mut pool: Vec<String> = match options.flags.get("pool") {
        Some(list) => list.split(',').map(|id| id.trim().to_owned()).collect(),
        None => PAVEMENT_POOL.iter().map(|id| (*id).to_owned()).collect(),
    };
    // Plants near the site that seed onto it: species, at [x, z], years.
    let mut sources = Vec::new();
    for (number, given) in document["seed_sources"]
        .as_array()
        .into_iter()
        .flatten()
        .enumerate()
    {
        let id = given["species"]
            .as_str()
            .ok_or_else(|| format!("seed source {number} names no species"))?;
        let at: Vec<f64> = given["at"]
            .as_array()
            .map(|values| {
                values
                    .iter()
                    .filter_map(serde_json::Value::as_f64)
                    .collect()
            })
            .unwrap_or_default();
        let [x, z] = at[..] else {
            return Err(format!("seed source {number} ({id}): at must be [x, z]").into());
        };
        let age = given["years"]
            .as_f64()
            .ok_or_else(|| format!("seed source {number} ({id}) gives no years"))?;
        let colonist = pool.iter().position(|p| p == id).unwrap_or_else(|| {
            pool.push(id.to_owned());
            pool.len() - 1
        });
        sources.push(plantgen::succession::Source {
            colonist,
            x,
            z,
            age,
        });
    }
    let settings = Settings {
        seed: options.number("seed")?.unwrap_or(1),
        traffic: options.number("traffic")?.unwrap_or(0.0),
        maintenance: options.number("maintenance")?.unwrap_or(0.0),
    };
    if !(0.0..=1.0).contains(&settings.traffic) || !(0.0..=1.0).contains(&settings.maintenance) {
        return Err("`--traffic` and `--maintenance` run from 0 to 1".into());
    }
    // The site's mesh and pavements, as `plantc site` reads them, with a
    // plant it can stand on the mesh so the document is complete.
    let started = Instant::now();
    let mut ages: Vec<f64> = [1.0, 2.0, 3.0, 5.0, 8.0, 12.0, 20.0, 30.0, 50.0]
        .into_iter()
        .filter(|age| *age <= f64::from(last.max(1)) + 1.0)
        .collect();
    if ages
        .last()
        .is_none_or(|age| *age < f64::from(last.max(1)) + 1.0)
    {
        ages.push(f64::from(last.max(1)) + 1.0);
    }
    let oldest = sources.iter().map(|s| s.age).fold(0.0, f64::max);
    if oldest > ages.last().copied().unwrap_or(0.0) {
        ages.push(oldest);
    }
    let colonists: Vec<_> = pool
        .iter()
        .map(|id| colonist(id, &ages))
        .collect::<Result<_, String>>()?;
    out!(
        "pool of {} species grown to {} years in {:.1} s",
        colonists.len(),
        ages.last().copied().unwrap_or(0.0),
        started.elapsed().as_secs_f64()
    );
    for c in &colonists {
        let (height, crown) = c.size_at(f64::from(last.max(1)));
        out!(
            "  {:<28} {:?}{}, seed by {:?}, seed bank {:?}, trampling {:?}, shade {:?}; at {} years {:.2} m tall, {:.2} m across",
            c.id,
            c.life,
            if c.woody { ", woody" } else { "" },
            c.dispersal,
            c.seedbank,
            c.trampling,
            c.shade,
            last.max(1),
            height,
            crown * 2.0
        );
    }
    let site = site_mesh_of(&document, folder)?;
    let started = Instant::now();
    let cores = std::thread::available_parallelism().map_or(1, std::num::NonZero::get);
    let places = microsites(&site, cores);
    out!(
        "{} microsites ({} m patches) in {:.1} s",
        places.len(),
        plantgen::succession::PATCH_M,
        started.elapsed().as_secs_f64()
    );
    if options.flags.contains_key("explain") {
        // Each species' supply ratio by opening: its mean over the
        // microsites of each class, a year-old plant and a 5-year-old one.
        let classes: [(&str, f64, f64); 5] = [
            ("hairline <4mm", 0.0, 0.004),
            ("crack 4-8mm", 0.004, 0.008),
            ("crack 8-15mm", 0.008, 0.015),
            ("crack >15mm", 0.015, 0.05),
            ("hole", 0.05, f64::INFINITY),
        ];
        out!(
            "supply ratio rs at 1 and 5 years by opening (microsites, mean soil depth):{}",
            classes
                .iter()
                .map(|(name, low, high)| {
                    let here: Vec<_> = places
                        .iter()
                        .filter(|p| p.width >= *low && p.width < *high)
                        .collect();
                    let depth =
                        here.iter().map(|p| p.soil_depth).sum::<f64>() / here.len().max(1) as f64;
                    format!(" {name} ({}, {:.0} mm)", here.len(), depth * 1000.0)
                })
                .collect::<String>()
        );
        for c in &colonists {
            let row: String = classes
                .iter()
                .map(|(_, low, high)| {
                    let here: Vec<_> = places
                        .iter()
                        .filter(|p| p.width >= *low && p.width < *high)
                        .collect();
                    let mean = |age: f64| {
                        here.iter().map(|p| c.supply(p, age, 1.0).0).sum::<f64>()
                            / here.len().max(1) as f64
                    };
                    format!("  {:>4.2} {:>4.2}", mean(1.0), mean(5.0))
                })
                .collect();
            out!("  {:<28}{row}", c.id);
        }
    }
    out!("{} seed sources near the site", sources.len());
    let snapshots = run(&places, &colonists, &sources, &settings, &years);
    let out_dir = options.flags.get("out").map(Path::new);
    if let Some(dir) = out_dir {
        fs::create_dir_all(dir)
            .map_err(|error| format!("cannot make {}: {error}", dir.display()))?;
    }
    let stem = Path::new(given)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("site");
    for snapshot in &snapshots {
        let mut count: BTreeMap<&str, (usize, usize, f64)> = BTreeMap::new();
        for plant in &snapshot.plants {
            let entry = count
                .entry(&colonists[plant.colonist].id)
                .or_insert((0, 0, 0.0));
            entry.0 += 1;
            if plant.age >= 2.0 {
                entry.1 += 1;
            }
            entry.2 = entry.2.max(plant.height);
        }
        out!(
            "year {}: {} plants{}",
            snapshot.year,
            snapshot.plants.len(),
            count
                .iter()
                .map(|(id, (n, older, tallest))| {
                    format!("\n  {id:<28} {n:>4} ({older:>3} past their first year), tallest {tallest:.2} m")
                })
                .collect::<String>()
        );
        for (index, name) in document["pavements"]
            .as_array()
            .into_iter()
            .flatten()
            .enumerate()
        {
            let mut here: BTreeMap<&str, usize> = BTreeMap::new();
            for plant in snapshot
                .plants
                .iter()
                .filter(|p| places[p.microsite].tile == index)
            {
                *here.entry(&colonists[plant.colonist].id).or_default() += 1;
            }
            out!(
                "  on {}: {}",
                name.as_str().unwrap_or("?"),
                if here.is_empty() {
                    "nothing".to_owned()
                } else {
                    here.iter()
                        .map(|(id, n)| format!("{n} {id}"))
                        .collect::<Vec<_>>()
                        .join(", ")
                }
            );
        }
        let Some(dir) = out_dir else {
            continue;
        };
        // The site again, with these plants, for `plantc site`.
        let mut site_doc = document.clone();
        let absolute = |name: &serde_json::Value| -> serde_json::Value {
            name.as_str()
                .map(|name| {
                    let path = folder.join(name);
                    serde_json::Value::String(
                        fs::canonicalize(&path)
                            .unwrap_or(path)
                            .to_string_lossy()
                            .into_owned(),
                    )
                })
                .unwrap_or(serde_json::Value::Null)
        };
        site_doc["mesh"] = absolute(&document["mesh"]);
        if let Some(list) = document["pavements"].as_array() {
            site_doc["pavements"] = serde_json::Value::Array(list.iter().map(absolute).collect());
        }
        site_doc["plants"] = serde_json::Value::Array(
            snapshot
                .plants
                .iter()
                .map(|plant| {
                    let place = &places[plant.microsite];
                    serde_json::json!({
                        "species": colonists[plant.colonist].id,
                        "at": [place.x, place.z],
                        "seed": plantgen::rng::hash_words(&[settings.seed, plant.microsite as u64, u64::from(plant.born)]) % 1000,
                        "years": plant.age.max(1.0),
                        "snap_m": 0.01,
                    })
                })
                .collect(),
        );
        site_doc["succession"] = serde_json::json!({
            "made_by": "plantc cracks",
            "year": snapshot.year,
            "seed": settings.seed,
            "traffic": settings.traffic,
            "maintenance": settings.maintenance,
            "generator_revision": plantgen::GENERATOR_REVISION,
        });
        let path = dir.join(format!("{stem}-year-{}.json", snapshot.year));
        fs::write(
            &path,
            serde_json::to_string_pretty(&site_doc).map_err(|error| error.to_string())? + "\n",
        )
        .map_err(|error| format!("cannot write {}: {error}", path.display()))?;
        out!("  wrote {}", path.display());
        if options.flags.contains_key("render") && !snapshot.plants.is_empty() {
            let png = dir.join(format!("{stem}-year-{}.png", snapshot.year));
            let mut site_args = vec![
                path.to_string_lossy().into_owned(),
                "--out".to_owned(),
                png.to_string_lossy().into_owned(),
            ];
            for name in ["view", "size", "day", "quality", "focus", "span"] {
                if let Some(value) = options.flags.get(name) {
                    site_args.push(format!("--{name}"));
                    site_args.push(value.clone());
                }
            }
            site_command(&site_args)?;
        }
    }
    Ok(())
}

/// `plantc site`: plants grown on a mesh (`plantgen::site`).
#[allow(clippy::too_many_lines)]
/// A site material table's entry `name`: a material's name, or an object
/// with the material (`as`), a thin part's thickness (`thin_m`), whether a
/// climber may hold to it (`climb`), whether it is kept clear of plants
/// (`clear`: what grows onto it is cut back), the colour to draw it in
/// (`colour`, sRGB 0 to 1), soil gathered on it (`soil_m`) and, where it
/// differs from its material's, how acid its face is for mosses
/// (`reaction`: acid, neutral or basic; asphalt shingles are `built`
/// and neutral).
fn site_surface(name: &str, given: &serde_json::Value) -> Result<plantgen::site::Surface, String> {
    use plantgen::site::{Surface, material_named};
    if let Some(material) = given.as_str() {
        return Ok(Surface::solid(material_named(material)?));
    }
    let table = given
        .as_object()
        .ok_or_else(|| format!("site material `{name}` must be a name or an object"))?;
    for key in table.keys() {
        if ![
            "as", "thin_m", "climb", "clear", "colour", "soil_m", "reaction",
        ]
        .contains(&key.as_str())
        {
            return Err(format!(
                "site material `{name}`: unknown field `{key}`; use as, thin_m, climb, clear, colour, soil_m, reaction"
            ));
        }
    }
    let material = table
        .get("as")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| format!("site material `{name}` names its material in `as`"))?;
    let mut surface = Surface::solid(material_named(material)?);
    if let Some(thin) = table.get("thin_m") {
        surface.thin_m = thin
            .as_f64()
            .filter(|t| t.is_finite() && *t >= 0.0)
            .ok_or_else(|| format!("site material `{name}`: thin_m must be metres, 0 or more"))?;
    }
    if let Some(climb) = table.get("climb") {
        surface.climb = climb
            .as_bool()
            .ok_or_else(|| format!("site material `{name}`: climb must be true or false"))?;
    }
    if let Some(soil) = table.get("soil_m") {
        surface.soil_m = soil
            .as_f64()
            .filter(|t| t.is_finite() && *t >= 0.0)
            .ok_or_else(|| format!("site material `{name}`: soil_m must be metres, 0 or more"))?;
    }
    if let Some(reaction) = table.get("reaction") {
        surface.reaction = Some(match reaction.as_str() {
            Some("acid") => -1,
            Some("neutral") => 0,
            Some("basic") => 1,
            _ => {
                return Err(format!(
                    "site material `{name}`: reaction must be acid, neutral or basic"
                ));
            }
        });
    }
    if let Some(clear) = table.get("clear") {
        surface.clear = clear
            .as_bool()
            .ok_or_else(|| format!("site material `{name}`: clear must be true or false"))?;
    }
    if let Some(colour) = table.get("colour") {
        let values: Vec<f64> = colour
            .as_array()
            .map(|values| {
                values
                    .iter()
                    .filter_map(serde_json::Value::as_f64)
                    .collect()
            })
            .unwrap_or_default();
        let [r, g, b] = values[..] else {
            return Err(format!(
                "site material `{name}`: colour must be [r, g, b], sRGB 0 to 1"
            ));
        };
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let linear = |v: f64| {
            plantgen::texture::srgb_to_linear((v.clamp(0.0, 1.0) * 255.0).round() as u8) as f32
        };
        surface.colour = Some([linear(r), linear(g), linear(b)]);
    }
    Ok(surface)
}

/// How near a surface kept clear a plant's stems and leaves may come
/// before they are cut back, metres.
const CLEAR_GAP_M: f64 = 0.05;

/// How far a plant set on sound pavement moves to the nearest crack, metres.
const PAVEMENT_SNAP_M: f64 = 0.05;

fn site_command(args: &[String]) -> Result<(), Failure> {
    use plantgen::site::{Cut, climbable};
    let options = Options::parse(
        args,
        &["out", "view", "size", "day", "quality", "focus", "span"],
    )?;
    let given = options.one_positional("a site document")?;
    let text =
        fs::read_to_string(given).map_err(|error| format!("cannot read {given}: {error}"))?;
    let document: serde_json::Value =
        serde_json::from_str(&text).map_err(|error| format!("invalid site: {error}"))?;
    let folder = Path::new(given).parent().unwrap_or(Path::new("."));
    let started = Instant::now();
    let site_mesh = site_mesh_of(&document, folder)?;
    let site = &site_mesh;
    let default_years = document["years"].as_f64();
    let plants = document["plants"]
        .as_array()
        .ok_or("site plants must be a list")?;
    let mut grown: Vec<SitePlant> = Vec::new();
    let mut cuts = Vec::new();
    for (number, plant) in plants.iter().enumerate() {
        let id = plant["species"]
            .as_str()
            .ok_or("each plant names its species")?;
        let spec = load_spec(id)?;
        let at: Vec<f64> = plant["at"]
            .as_array()
            .map(|values| {
                values
                    .iter()
                    .filter_map(serde_json::Value::as_f64)
                    .collect()
            })
            .unwrap_or_default();
        let (mut x, mut z, below) = match at[..] {
            [x, z] => (x, z, None),
            [x, y, z] => (x, z, Some(y)),
            _ => {
                return Err(
                    format!("plant {number} ({id}): at must be [x, z] or [x, y, z]").into(),
                );
            }
        };
        // On pavement a plant roots only in a crack or a hole: one set on
        // sound slab moves to the nearest opening within snap_m, or is
        // refused.
        if let Some(tile) = site
            .pavements()
            .iter()
            .find(|tile| tile.covers(x, z) && below.is_none_or(|b| tile.top() <= b))
        {
            let snap = plant["snap_m"].as_f64().unwrap_or(PAVEMENT_SNAP_M);
            let (cx, cz, _) = tile.nearest_open(x, z, snap).ok_or_else(|| {
                format!(
                    "plant {number} ({id}): ({x}, {z}) is sound pavement with no crack within {snap} m; nothing roots there"
                )
            })?;
            (x, z) = (cx, cz);
        }
        let (ground, material) = site.footing(x, z, below).ok_or_else(|| {
            format!("plant {number} ({id}): no upward-facing ground under ({x}, {z})")
        })?;
        let foot = Vec3::new(x, ground, z);
        let seed = match &plant["seed"] {
            serde_json::Value::Null => spec.variants.seeds[0],
            seed => seed
                .as_u64()
                .ok_or("a plant's seed must be a whole number")?,
        };
        let years = plant["years"]
            .as_f64()
            .or(default_years)
            .unwrap_or_else(|| spec.growth.keyframes.iter().copied().fold(0.0, f64::max));
        let (height, reach) = site_size(&spec, years);
        let mut cut = Cut::for_plant(height, reach);
        if let Some(given) = plant["cut"].as_object() {
            for (name, slot) in [
                ("half_m", &mut cut.half_m),
                ("below_m", &mut cut.below_m),
                ("above_m", &mut cut.above_m),
                ("cell_m", &mut cut.cell_m),
            ] {
                if let Some(value) = given.get(name) {
                    *slot = value.as_f64().ok_or_else(|| {
                        format!("plant {number} ({id}): cut {name} must be a number")
                    })?;
                }
            }
        }
        let substrate = site.substrate_round(foot, cut)?;
        let mut conditions = Conditions::preset(Environment::Open);
        conditions.substrate = Some(substrate);
        // In a crack, the soil the roots can reach (soil depth design D1).
        conditions.rooting = site.rooting(foot);
        conditions.validate()?;
        let host = site
            .host_round(foot, cut, &climbable)
            .map(std::sync::Arc::new);
        let (program, params) = spec
            .program_in(library())
            .map_err(|error| error.to_string())?;
        let params = spec.seeded_params(&program, &params, seed);
        out!(
            "  {id} on {} at ({x:.2}, {ground:.2}, {z:.2}); site cut {:.1} m round, {:.2} m cells{}",
            ["air", "soil", "rock", "bark", "wood", "built"][usize::from(material)],
            cut.half_m,
            cut.cell_m,
            if host.is_some() {
                "; surfaces to climb"
            } else {
                ""
            }
        );
        let settings = GrowthSettings {
            seed,
            dt: spec.growth.step,
            years,
            keyframes: vec![years],
            conditions,
            limits: Limits::default(),
            host,
        };
        cuts.push(cut);
        grown.push((spec, foot, program, params, settings));
    }
    if grown.is_empty() {
        return Err("a site needs at least one plant".into());
    }
    let cut_seconds = started.elapsed().as_secs_f64();
    let started = Instant::now();
    let together = grown
        .iter()
        .all(|plant| plant.4.dt.to_bits() == grown[0].4.dt.to_bits());
    let mut growths: Vec<Growth> = if together {
        let members: Vec<plantgen::stand::Member<'_>> = grown
            .iter()
            .map(
                |(_, foot, program, params, settings)| plantgen::stand::Member {
                    program,
                    params,
                    settings,
                    at_m: [foot.x, -foot.z],
                },
            )
            .collect();
        plantgen::stand::grow_stand(&members, None).map_err(|error| error.to_string())?
    } else {
        grown
            .iter()
            .map(|(spec, _, program, params, settings)| {
                let mut grower = Grower::new(program, params, settings)
                    .map_err(|error| format!("{}: {error}", spec.id))?;
                loop {
                    match grower.step(None) {
                        Ok(true) => {}
                        Ok(false) => break,
                        Err(error) => return Err(format!("{}: {error}", spec.id)),
                    }
                }
                Ok(grower.finish())
            })
            .collect::<Result<_, String>>()?
    };
    out!(
        "{} plants on {} triangles: sites cut in {cut_seconds:.1} s, grown {} in {:.1} s",
        grown.len(),
        site.triangle_count(),
        if together {
            "together"
        } else {
            "one by one (their steps differ in length)"
        },
        started.elapsed().as_secs_f64()
    );
    for ((spec, foot, _, _, settings), growth) in grown.iter().zip(&mut growths) {
        // What grew onto a surface kept clear is cut back.
        let mut trimmed = (0, 0);
        for graph in &mut growth.keyframes {
            trimmed = site.cut_back(graph, *foot, CLEAR_GAP_M);
        }
        let Some(graph) = growth.keyframes.last() else {
            continue;
        };
        let intrusion = settings.conditions.intrusion(graph)?;
        out!(
            "  {}: {}; {}{}",
            spec.id,
            describe(graph, &growth.organ_types),
            intrusion.describe(),
            if trimmed == (0, 0) {
                String::new()
            } else {
                format!(
                    "; {} segments and {} organs cut back from surfaces kept clear",
                    trimmed.0, trimmed.1
                )
            }
        );
    }
    let Some(out) = options.flags.get("out") else {
        return Ok(());
    };
    // Draw: the plants about the middle of their feet, the lowest foot at
    // height 0, on the part of the mesh round them.
    let day = options.day()?;
    let quality = options.quality()?;
    let lod = &quality.lods[0];
    let feet: Vec<Vec3> = grown.iter().map(|plant| plant.1).collect();
    let low_foot = feet.iter().map(|foot| foot.y).fold(f64::INFINITY, f64::min);
    #[allow(clippy::cast_precision_loss)]
    let middle = feet.iter().fold(Vec3::ZERO, |sum, foot| sum + *foot) * (1.0 / feet.len() as f64);
    let offset = Vec3::new(-middle.x, -low_foot, -middle.z);
    let mut scene = PlantMesh::default();
    let mut all_looks: Vec<Look> = Vec::new();
    let mut bark = None;
    let (mut low, mut high) = (
        Vec3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY),
        Vec3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY),
    );
    // Plants of one species share its looks, so a site of many plants
    // stays within the renderer's organ types.
    let mut looks_at: BTreeMap<&str, usize> = BTreeMap::new();
    for (((spec, foot, _, _, settings), growth), cut) in grown.iter().zip(&growths).zip(&cuts) {
        let (looks, sizes) = looks_of(spec, growth, day);
        let graph = &*drawn(&growth.keyframes[0], &sizes, &looks);
        let bodies = bodies_of(spec, growth);
        let mut plant = mesh::build(
            graph,
            &looks,
            &bodies,
            &spec.appearance,
            &lod.for_height(graph.height),
            0,
        );
        plant.vary_by_plant(&looks, settings.seed);
        moved(&mut plant, *foot + offset);
        let first = *looks_at.entry(spec.id.as_str()).or_insert_with(|| {
            let first = all_looks.len();
            all_looks.extend(looks);
            first
        });
        scene = on_host(scene, plant, first);
        if bark.is_none() {
            bark = spec.appearance.bark_params();
        }
        let reach = Vec3::new(cut.half_m, 0.0, cut.half_m);
        low = low.min(*foot - reach - Vec3::new(0.0, 0.3, 0.0));
        high = high.max(*foot + reach + Vec3::new(0.0, cut.above_m, 0.0));
    }
    if all_looks.len() > 255 {
        return Err("too many organ types to draw at once; draw fewer species".into());
    }
    // A view framed with `--focus` draws the site as far round it as it
    // frames, not only round the plants.
    if let (Some(text), Ok(Some(span))) = (options.flags.get("focus"), options.number("span")) {
        let values: Vec<f64> = text
            .split(',')
            .filter_map(|v| v.trim().parse().ok())
            .collect();
        if let [x, y, z] = values[..] {
            let half = Vec3::new(span, span, span) * 0.75;
            low = low.min(Vec3::new(x, y, z) - half);
            high = high.max(Vec3::new(x, y, z) + half);
        }
    }
    let ground = site.drawn(low, high, offset);
    let templates = Templates::for_plant(&all_looks, &[]).with_bark(bark);
    let view = match options.flags.get("view") {
        Some(name) => View::from_name(name).ok_or_else(|| format!("unknown view `{name}`"))?,
        None => View::ThreeQuarter,
    };
    let pixels: usize = options.number("size")?.unwrap_or(1200);
    // `--focus X,Y,Z` in the mesh's frame, framing `--span` metres.
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
            let span: f64 = options.number("span")?.unwrap_or(2.0);
            Some((Vec3::new(x, y, z) + offset, span))
        }
        None => None,
    };
    // Framed on the plants, not on all the mesh cut round them.
    let plants_reach = grown
        .iter()
        .zip(&growths)
        .filter_map(|((_, foot, ..), growth)| {
            let graph = growth.keyframes.last()?;
            let at = *foot + offset;
            graph
                .segments
                .iter()
                .map(|segment| segment.end)
                .chain(graph.organs.iter().map(|organ| organ.position))
                .map(|p| (p.x + at.x).abs().max((p.z + at.z).abs()))
                .reduce(f64::max)
        })
        .fold(0.5, f64::max);
    let image = preview::render_on_site(
        &scene,
        &[(&ground, raster::Material::Opaque)],
        &templates,
        &PreviewOptions {
            width: pixels,
            height: pixels * 3 / 4,
            view,
            supersample: 2,
            figure: true,
            frame_height: None,
            frame_reach: Some(plants_reach * 1.15),
            focus,
            fit_plant: false,
        },
    );
    let png = raster::encode_png(
        image.width,
        image.height,
        &image.to_srgb8(Some(preview::SKY)),
    )
    .map_err(|error| format!("cannot encode PNG: {error}"))?;
    fs::write(out, png).map_err(|error| format!("cannot write {out}: {error}"))?;
    out!(
        "wrote {out}: {} triangles of plants, {} of site",
        scene.triangle_count(),
        ground.triangle_count()
    );
    Ok(())
}

/// `plant` moved by `by`: its wood, cards, areoles and hairy stems.
fn moved(plant: &mut PlantMesh, by: Vec3) {
    let by = by.to_f32();
    let shift = |p: &mut [f32; 3]| {
        for (value, step) in p.iter_mut().zip(by) {
            *value += step;
        }
    };
    plant.wood.positions.iter_mut().for_each(shift);
    for card in &mut plant.cards {
        shift(&mut card.base);
    }
    for tuft in &mut plant.tufts {
        shift(&mut tuft.position);
    }
    for tube in &mut plant.hair_tubes {
        shift(&mut tube.start);
        shift(&mut tube.end);
    }
}

/// `plantc render --break HOW`: the plant broken where its break sites
/// say (plant motion S5): `snap` at its stem's weakest section, a wind
/// speed (m/s at 10 m) at every site that wind breaks, or `bone:N` at
/// bone N's weakest site. Its torn stubs drawn on it, and unless `fallen`
/// is off the pieces lying on the ground with their torn ends.
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn broken_mesh(
    spec: &PlantSpec,
    graph: &PlantGraph,
    types: &[OrganType],
    how: &str,
    fallen: bool,
    looks: &[Look],
    bodies: &[BodyLook],
    lod: &mesh::LodSpec,
    level: usize,
) -> Result<PlantMesh, Failure> {
    use plantgen::damage::{self, Break, Grain, Toughness};
    let traits = library()
        .traits_of(&spec.id)
        .map_err(|error| error.to_string())?;
    let motion = plantgen::motion::Motion::of(&traits).with_flare(&spec.appearance);
    let found = motion.of_graph(graph, types);
    let record = plantgen::motion::record(&found, &motion, graph, types);
    let broken =
        plantgen::breakage::breakage(&found, record.anchorage.moment, &motion.drag, motion.flare);
    let at = |site: &plantgen::breakage::Site| Break {
        segment: found.beams.segment[site.beam as usize],
        toward: Vec3::new(site.direction[0], site.direction[1], site.direction[2]),
        wind: site.wind.unwrap_or(0.0),
    };
    let breaks: Vec<Break> = if how == "snap" {
        let failure = &broken.failure;
        let bone = failure
            .snap_bone
            .ok_or("no wind up to the fastest searched snaps its stem")?;
        broken.sites[bone as usize]
            .iter()
            .filter(|site| (site.along - failure.snap_along).abs() < 1.0e-12)
            .map(at)
            .collect()
    } else if let Some(bone) = how.strip_prefix("bone:") {
        let bone: usize = bone
            .parse()
            .map_err(|_| format!("`--break bone:N` takes a bone number, found `{bone}`"))?;
        let sites = broken
            .sites
            .get(bone)
            .ok_or_else(|| format!("there is no bone {bone}"))?;
        sites.last().map(at).into_iter().collect()
    } else {
        let wind: f64 = how.parse().map_err(|_| {
            format!("`--break` takes snap, bone:N or a wind speed in m/s, found `{how}`")
        })?;
        // One wind for the scene: the way it blew to break the first.
        let chosen: Vec<&plantgen::breakage::Site> = broken
            .sites
            .iter()
            .flatten()
            .filter(|site| site.wind.is_some_and(|at| at <= wind))
            .collect();
        let first = chosen.iter().min_by(|a, b| {
            a.wind
                .partial_cmp(&b.wind)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let toward = first.map_or(Vec3::X, |site| {
            Vec3::new(site.direction[0], site.direction[1], site.direction[2])
        });
        chosen
            .into_iter()
            .map(|site| Break {
                toward,
                wind,
                ..at(site)
            })
            .collect()
    };
    let toughness = Toughness::of_traits(&traits);
    let damaged = damage::apply(graph, &breaks, toughness, &motion.drag);
    let mut tears = [0_usize; 3];
    for how in &damaged.tears {
        tears[match how {
            damage::Tear::Clean => 0,
            damage::Tear::Hinged => 1,
            damage::Tear::Split => 2,
        }] += 1;
    }
    out!(
        "tears: {} clean, {} hinged, {} split out of their stems",
        tears[0],
        tears[1],
        tears[2]
    );
    let grain = Grain {
        sapwood: Grain::SAPWOOD,
        heartwood: None,
        bark: spec.appearance.bark,
    };
    let mut plant = mesh::build(
        &damaged.standing,
        looks,
        bodies,
        &spec.appearance,
        lod,
        level,
    );
    for wound in &damaged.scars {
        damage::append(&mut plant.wood, &damage::scar(wound, &grain, lod.ring_edge));
        out!(
            "split scar from {:.2},{:.2},{:.2} m down {:.2} m, facing {:.0} degrees (0 = +X, 90 = +Z)",
            wound.top.x,
            wound.top.y,
            wound.top.z,
            wound.length,
            wound.out.z.atan2(wound.out.x).to_degrees()
        );
    }
    for stub in &damaged.stubs {
        damage::append(
            &mut plant.wood,
            &damage::fracture(stub, toughness, &grain, lod.ring_edge),
        );
        out!(
            "broken at {:.2},{:.2},{:.2} m, {:.0} cm across",
            stub.point.x,
            stub.point.y,
            stub.point.z,
            stub.radius * 200.0
        );
    }
    if fallen {
        let lying = plantgen::spec::Appearance {
            flare: None,
            bottle: None,
            ..spec.appearance.clone()
        };
        for piece in &damaged.pieces {
            let more = mesh::build(&piece.graph, looks, bodies, &lying, lod, level);
            damage::append(&mut plant.wood, &more.wood);
            plant.cards.extend(more.cards);
            plant.tufts.extend(more.tufts);
            damage::append(
                &mut plant.wood,
                &damage::fracture(&piece.end, toughness, &grain, lod.ring_edge),
            );
        }
    }
    out!(
        "{} breaks, {} pieces fallen{}",
        damaged.stubs.len(),
        damaged.pieces.len(),
        if fallen { "" } else { " (not drawn)" }
    );
    Ok(plant)
}

/// How far `plantc render --fell` draws the felling.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Felling {
    /// Notch and back cut sawn, the tree still standing on its hinge.
    Notched,
    /// The stump alone.
    Stump,
    /// The stump and the log lying down the fall.
    Fallen,
}

/// `plantc render --fell HEIGHT[,DEGREES]`: the plant felled with a saw at
/// HEIGHT m, falling towards DEGREES (0 = +X, 90 = +Z).
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn felled_mesh(
    spec: &PlantSpec,
    graph: &PlantGraph,
    how: &str,
    by: &str,
    stage: Felling,
    looks: &[Look],
    bodies: &[BodyLook],
    lod: &mesh::LodSpec,
    level: usize,
) -> Result<PlantMesh, Failure> {
    use plantgen::damage::{self, Grain, Toughness};
    use plantgen::felling::{self, Cut, Method};
    let numbers: Vec<f64> = how
        .split(',')
        .map(|part| part.trim().parse::<f64>())
        .collect::<Result<_, _>>()
        .map_err(|_| format!("`--fell` takes HEIGHT or HEIGHT,DEGREES, found `{how}`"))?;
    let (height, degrees) = match numbers[..] {
        [height] => (height, 0.0),
        [height, degrees] => (height, degrees),
        _ => return Err(format!("`--fell` takes HEIGHT or HEIGHT,DEGREES, found `{how}`").into()),
    };
    let traits = library()
        .traits_of(&spec.id)
        .map_err(|error| error.to_string())?;
    let motion = plantgen::motion::Motion::of(&traits);
    let toughness = Toughness::of_traits(&traits);
    let look = spec
        .appearance
        .bark_look(&spec.id)
        .map_err(|error| error.to_string())?;
    let method = match by {
        "saw" => Method::SAW,
        "ax" | "axe" => Method::AX,
        "beaver" => Method::BEAVER,
        other => {
            return Err(format!("`--by` is saw, ax or beaver, found `{other}`").into());
        }
    };
    let angle = degrees.to_radians();
    let cut = Cut {
        height,
        toward: Vec3::new(angle.cos(), 0.0, angle.sin()),
        method,
    };
    let felled = felling::fell(
        graph,
        &cut,
        look.as_ref().map(|look| &look.thickness),
        spec.appearance.flare.as_ref(),
        &motion.drag,
    )
    .ok_or_else(|| format!("{} has no stem standing {height} m up", spec.id))?;
    let grain = Grain {
        sapwood: Grain::SAPWOOD,
        heartwood: None,
        bark: spec.appearance.bark,
    };
    let mut plant = mesh::build(
        &felled.standing,
        looks,
        bodies,
        &spec.appearance,
        lod,
        level,
    );
    if stage != Felling::Notched {
        damage::append(
            &mut plant.wood,
            &felling::kerf(&felled.stump, toughness, &grain, lod.ring_edge),
        );
    }
    let d = 2.0 * felled.stump.radius;
    let how_cut = match method {
        Method::Notched(notch) => format!(
            "notch {:.0} cm deep, hinge {:.0} cm long and {:.1} cm thick, stump shot {:.1} cm",
            notch.depth * d * 100.0,
            felled.hinge.length * 100.0,
            felled.hinge.thickness * 100.0,
            felled.hinge.height * 100.0
        ),
        Method::Gnawed(gnaw) => format!(
            "gnawed band {:.0} cm high, core {:.1} cm across",
            gnaw.band * d * 100.0,
            felled.hinge.thickness * 100.0
        ),
    };
    out!(
        "felled by {by} at {height:.2} m towards {degrees:.0} degrees: stem {:.0} cm across, bark {:.1} cm, {} rings; {how_cut}",
        d * 100.0,
        felled.stump.bark * 100.0,
        felled.stump.rings,
    );
    if stage == Felling::Notched {
        // The tree as it stood, flare and all, the notch cut from it.
        let mut more = mesh::build(&felled.upright, looks, bodies, &spec.appearance, lod, level);
        felling::notch_out(&mut more.wood, &felled.notch);
        damage::append(&mut plant.wood, &more.wood);
        plant.cards.extend(more.cards);
        plant.tufts.extend(more.tufts);
        damage::append(
            &mut plant.wood,
            &felling::kerf(&felled.notch, toughness, &grain, lod.ring_edge),
        );
    }
    if stage == Felling::Fallen {
        let lying = plantgen::spec::Appearance {
            flare: None,
            bottle: None,
            ..spec.appearance.clone()
        };
        let more = mesh::build(&felled.log, looks, bodies, &lying, lod, level);
        damage::append(&mut plant.wood, &more.wood);
        plant.cards.extend(more.cards);
        plant.tufts.extend(more.tufts);
        damage::append(
            &mut plant.wood,
            &felling::kerf(&felled.butt, toughness, &grain, lod.ring_edge),
        );
        out!(
            "log {:.1} m long lies towards {degrees:.0} degrees, its butt {:.2} m from the stump's centre",
            felled
                .log
                .segments
                .iter()
                .map(|segment| segment.end.x.hypot(segment.end.z))
                .fold(0.0, f64::max),
            felled.butt.origin.x.hypot(felled.butt.origin.z)
        );
    }
    Ok(plant)
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
    plant.hair_tubes.extend(guest.hair_tubes);
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
        for grown in &growth.keyframes {
            let graph = &*drawn(grown, &sizes, &looks);
            out!(
                "  {:<10} {}",
                environment.name(),
                describe(grown, &growth.organ_types)
            );
            let mut plant = mesh::build(
                graph,
                &looks,
                &bodies,
                &spec.appearance,
                &quality.lods[0].for_height(graph.height),
                0,
            );
            plant.vary_by_plant(&looks, seed);
            images.push(preview::render(
                &plant,
                &templates,
                &PreviewOptions {
                    width: size,
                    height: size * 3 / 2,
                    view: View::Side,
                    supersample: 2,
                    figure: true,
                    fit_plant: false,
                    frame_height: Some(if graph.height < tallest * 0.3 {
                        graph.height
                    } else {
                        tallest
                    }),
                    frame_reach: None,
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
/// Pixels across the renders that measure a tier's tone.
const TONE_PIXELS: usize = 160;

fn tiers_command(args: &[String]) -> Result<(), Failure> {
    let options = Options::parse(
        args,
        &[
            "seed", "size", "out", "quality", "program", "day", "age", "view", "focus", "columns",
            "views",
        ],
    )?;
    let program = options.program()?;
    let out = options.flags.get("out").ok_or("missing `--out FOLDER`")?;
    let view = match options.flags.get("view") {
        Some(name) => View::from_name(name).ok_or_else(|| format!("unknown view `{name}`"))?,
        None => View::Side,
    };
    // `--columns full,cards,far`: which columns to draw (all by default).
    let columns: Vec<String> = options.flags.get("columns").map_or_else(
        || vec!["full".into(), "cards".into(), "far".into()],
        |text| text.split(',').map(|c| c.trim().to_owned()).collect(),
    );
    let wants = |name: &str| columns.iter().any(|c| c == name);
    // `--focus F,S`: a close-up of the point a share F up the plant, S of
    // its height across.
    let focus_share: Option<(f64, f64)> = match options.flags.get("focus") {
        Some(text) => {
            let parts: Vec<f64> = text
                .split(',')
                .map(|part| part.trim().parse::<f64>())
                .collect::<Result<_, _>>()
                .map_err(|_| format!("`--focus` expects F,S, found `{text}`"))?;
            let [f, s] = parts[..] else {
                return Err(format!("`--focus` expects F,S, found `{text}`").into());
            };
            Some((f, s))
        }
        None => None,
    };
    fs::create_dir_all(out).map_err(|error| format!("cannot make {out}: {error}"))?;
    let size: usize = options.number("size")?.unwrap_or(360);
    let day = options.day()?;
    let quality = options.quality()?;
    if options.positional.is_empty() {
        return Err("name at least one species".into());
    }
    for name in &options.positional {
        let spec = load_spec(name)?;
        let seed = options.number("seed")?.unwrap_or(spec.variants.seeds[0]);
        let age = match options.number::<f64>("age")? {
            Some(age) => age,
            None => spec.growth.keyframes.iter().copied().fold(0.0, f64::max),
        };
        let environment = spec.variants.environments[0];
        let growth = grow_variant(&spec, program.as_deref(), environment, seed, vec![age], age)?;
        let (looks, sizes) = looks_of(&spec, &growth, day);
        let bodies = bodies_of(&spec, &growth);
        let templates = templates_of(&spec, &looks, &bodies, &growth);
        let graph = &*drawn(&growth.keyframes[0], &sizes, &looks);
        let mut full = mesh::build(
            graph,
            &looks,
            &bodies,
            &spec.appearance,
            &quality.lods[if wants("full") { 0 } else { 3 }].for_height(graph.height),
            if wants("full") { 0 } else { 3 },
        );
        full.vary_by_plant(&looks, seed);
        // Every column is framed as the plant drawn whole is, so the
        // tiers share its scale.
        let full_cards = full.card_mesh();
        let reach = full
            .wood
            .positions
            .iter()
            .chain(&full_cards.positions)
            .map(|p| f64::from(p[0].hypot(p[2])))
            .fold(0.1, f64::max);
        let shot = |view: View, focus: Option<(f64, f64)>| PreviewOptions {
            width: size,
            height: size * 3 / 2,
            view,
            supersample: 2,
            figure: false,
            fit_plant: false,
            frame_height: Some(graph.height * 1.05),
            frame_reach: Some(reach),
            focus: focus.map(|(f, s)| (Vec3::new(0.0, graph.height * f, 0.0), graph.height * s)),
        };
        // `--views side,oblique,close`: one picture each, in a folder of
        // its name (close is three-quarter, focused 0.7 up and 0.45
        // across); otherwise the one `--view`.
        let shots: Vec<(Option<&str>, PreviewOptions)> = match options.flags.get("views") {
            Some(names) => names
                .split(',')
                .map(|name| match name.trim() {
                    "side" => Ok((Some("side"), shot(View::Side, None))),
                    "oblique" => Ok((Some("oblique"), shot(View::Oblique, None))),
                    "close" => Ok((Some("close"), shot(View::ThreeQuarter, Some((0.7, 0.45))))),
                    other => Err(format!("unknown view `{other}`")),
                })
                .collect::<Result<_, _>>()?,
            None => vec![(None, shot(view, focus_share))],
        };
        let mut images: Vec<Vec<raster::Image>> = vec![Vec::new(); shots.len()];
        if wants("full") {
            for (k, (_, options)) in shots.iter().enumerate() {
                images[k].push(preview::render(&full, &templates, options));
            }
        }
        let bark = templates
            .bark
            .map_or(raster::Material::Opaque, raster::Material::Bark);
        let full_tone = plantgen::billboards::tone(
            &[(&full.wood, bark), (&full_cards, raster::Material::Card)],
            &templates,
            TONE_PIXELS,
        );
        let mut rows = vec![format!(
            "{{\"species\":\"{}\",\"tier\":\"full\",\"height_m\":{:.2},\"triangles\":{},\"tone\":[{:.4},{:.4}],\"overdraw\":[{:.2},{:.2}],\"cover\":[{:.6},{:.6}]}}",
            spec.id,
            graph.height,
            full.triangle_count(),
            full_tone.tone[0],
            full_tone.tone[1],
            full_tone.overdraw[0],
            full_tone.overdraw[1],
            full_tone.cover[0],
            full_tone.cover[1]
        )];
        // A tree drawn in full is gigabytes; the tiers build their own.
        drop(full_cards);
        drop(full);
        let source = (wants("cards") || wants("far")).then(|| {
            plantgen::billboards::Source::new(
                graph,
                &looks,
                &bodies,
                &spec.appearance,
                spec.growth_form,
                &templates,
                &quality.lods,
            )
        });
        for (tier, source) in plantgen::billboards::TIERS
            .iter()
            .filter(|_| wants("cards"))
            .filter_map(|tier| Some((tier, source.as_ref()?)))
        {
            let carded = plantgen::billboards::build_from(
                source,
                graph,
                &looks,
                &bodies,
                &spec.appearance,
                spec.growth_form,
                &templates,
                &quality.lods,
                tier,
            );
            let stats = &carded.stats;
            let atlas = carded.atlas();
            let mut baked = templates.clone();
            baked.baked.clone_from(&carded.pictures);
            let tone = plantgen::billboards::tone(
                &[
                    (&carded.wood, bark),
                    (&carded.cards, raster::Material::Baked),
                ],
                &baked,
                TONE_PIXELS,
            );
            rows.push(format!(
                "{{\"species\":\"{}\",\"tier\":\"{}\",\"triangles\":{},\"wood\":{},\"cards\":{},\"card_triangles\":{},\"texels\":{},\"coverage\":{:.3},\"atlas\":[{},{}],\"atlas_fill\":{:.3},\"tone\":[{:.4},{:.4}],\"overdraw\":[{:.2},{:.2}],\"cover\":[{:.6},{:.6}]}}",
                spec.id,
                tier.name,
                stats.triangles(),
                stats.wood_triangles,
                stats.cards,
                stats.card_triangles,
                stats.texels,
                stats.coverage,
                atlas.width,
                atlas.height,
                atlas.fill(&carded.pictures),
                tone.tone[0],
                tone.tone[1],
                tone.overdraw[0],
                tone.overdraw[1],
                tone.cover[0],
                tone.cover[1]
            ));
            let plant = mesh::PlantMesh {
                wood: carded.wood.clone(),
                ..mesh::PlantMesh::default()
            };
            for (k, (_, options)) in shots.iter().enumerate() {
                images[k].push(preview::render_with(
                    &plant,
                    &[(&carded.cards, raster::Material::Baked)],
                    &baked,
                    options,
                ));
            }
        }
        if let Some(source) = source.as_ref().filter(|_| wants("far")) {
            let far = plantgen::billboards::FAR;
            let carded = plantgen::billboards::impostor_from(source, &templates, &far);
            let mut baked = templates.clone();
            baked.baked.clone_from(&carded.pictures);
            let tone = plantgen::billboards::tone(
                &[(&carded.cards, raster::Material::Baked)],
                &baked,
                TONE_PIXELS,
            );
            rows.push(format!(
                "{{\"species\":\"{}\",\"tier\":\"{}\",\"triangles\":{},\"pictures\":{},\"texels\":{},\"tone\":[{:.4},{:.4}],\"overdraw\":[{:.2},{:.2}],\"cover\":[{:.6},{:.6}]}}",
                spec.id,
                far.name,
                carded.stats.card_triangles,
                carded.pictures.len(),
                carded.stats.texels,
                tone.tone[0],
                tone.tone[1],
                tone.overdraw[0],
                tone.overdraw[1],
                tone.cover[0],
                tone.cover[1]
            ));
            for (k, (_, options)) in shots.iter().enumerate() {
                images[k].push(preview::render_with(
                    &mesh::PlantMesh::default(),
                    &[(&carded.cards, raster::Material::Baked)],
                    &baked,
                    options,
                ));
            }
        }
        for ((folder, _), images) in shots.iter().zip(&images) {
            let (width, height, pixels) = preview::contact_sheet(images, images.len(), 8);
            let png = raster::encode_png(width, height, &pixels)
                .map_err(|error| format!("cannot encode PNG: {error}"))?;
            let folder =
                folder.map_or_else(|| Path::new(out).to_path_buf(), |f| Path::new(out).join(f));
            fs::create_dir_all(&folder)
                .map_err(|error| format!("cannot make {}: {error}", folder.display()))?;
            let file = folder.join(format!("{}.png", spec.id));
            fs::write(&file, png)
                .map_err(|error| format!("cannot write {}: {error}", file.display()))?;
        }
        for row in rows {
            out!("{row}");
        }
    }
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
                    fit_plant: false,
                    frame_height: None,
                    frame_reach: None,
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
            // A fruit that hangs is borne a length above the ground, so
            // it hangs in the air as it does on the plant.
            let lift = match looks.get(index).map(|look| &look.shape) {
                Some(plantgen::looks::Shape::Fruit(fruit)) if fruit.hang > 0.0 => {
                    length * largest * 1.1
                }
                _ => 0.0,
            };
            let alone = PlantGraph {
                age,
                height: length,
                segments: Vec::new(),
                organs: if share > 0.0 {
                    vec![GraphOrgan {
                        id: 1,
                        organ: u16::try_from(index).map_err(|_| "too many organ types")?,
                        segment: None,
                        position: Vec3::Y * lift,
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
                    fit_plant: false,
                    frame_height: None,
                    frame_reach: None,
                    focus: Some((
                        Vec3::Y
                            * if lift > 0.0 {
                                // Round the solid it drew, wherever it hangs.
                                let (low, high) = plant.wood.positions.iter().fold(
                                    (f64::MAX, f64::MIN),
                                    |(low, high), p| {
                                        (low.min(f64::from(p[1])), high.max(f64::from(p[1])))
                                    },
                                );
                                if low <= high {
                                    0.5 * (low + high)
                                } else {
                                    lift
                                }
                            } else {
                                middle.map_or(length * largest * 0.5, |middle| length * middle)
                            },
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
        for &seed in &seeds {
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
        for (growth, &seed) in grown.iter().zip(&seeds) {
            let (looks, sizes) = looks_of(&spec, growth, day);
            let graph = &*drawn(&growth.keyframes[0], &sizes, &looks);
            let bodies = bodies_of(&spec, growth);
            let mut plant = mesh::build(
                graph,
                &looks,
                &bodies,
                &spec.appearance,
                &quality.lods[0].for_height(graph.height),
                0,
            );
            plant.vary_by_plant(&looks, seed);
            images.push(preview::render(
                &plant,
                &templates_of(&spec, &looks, &bodies, growth),
                &PreviewOptions {
                    width: size,
                    height: size * 3 / 2,
                    view,
                    supersample: 2,
                    figure: true,
                    fit_plant: false,
                    frame_height: Some(tallest),
                    frame_reach: None,
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
    let options = Options::parse(args, &["out", "program", "organ"])?;
    let program = options.program()?;
    let spec = load_spec(options.one_positional("a species")?)?;
    let out = options.flags.get("out").ok_or("missing `--out FILE.png`")?;
    let (program, params) = match &program {
        Some(source) => spec.program_from(source),
        None => spec.program(),
    }
    .map_err(|error| error.to_string())?;
    let mut looks = spec.appearance.looks(program.organs());
    let mut shading = spec::organ_areas(&program, &params).map_err(|error| error.to_string())?;
    // One organ's card alone (`--organ leaf`): for comparing a drawn leaf
    // with photographs of the species' leaves.
    if let Some(organ) = options.flags.get("organ") {
        let keep: Vec<bool> = looks.iter().map(|look| look.organ == *organ).collect();
        let mut kept = keep.iter();
        looks.retain(|_| *kept.next().unwrap_or(&false));
        let mut kept = keep.iter();
        shading.retain(|_| *kept.next().unwrap_or(&false));
        if looks.is_empty() {
            return Err(format!("the program draws no organ `{organ}`").into());
        }
    }
    let bodies = spec.appearance.body_looks(program.bodies());
    let named: Vec<(&str, &BodyLook)> = program.bodies().zip(&bodies).collect();
    let templates = Templates::for_plant(&looks, &named);
    let (width, height, pixels) = templates.swatches_rgba(SWATCH_BACKGROUND, 2);
    let png = raster::encode_png(width, height, &pixels)
        .map_err(|error| format!("cannot encode PNG: {error}"))?;
    fs::write(out, png).map_err(|error| format!("cannot write {out}: {error}"))?;
    out!(
        "wrote {out}: {width}x{height}, one card texture per organ, left to right,\n\
         its lower face beside it where the two are unlike.\n\
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

#[allow(clippy::too_many_lines)]
fn bark_command(args: &[String]) -> Result<(), Failure> {
    use plantgen::bark_look::{self, BarkLook};
    let options = Options::parse(
        args,
        &["look", "species", "probe", "out", "size", "period", "seed"],
    )?;
    let out = Path::new(options.flags.get("out").ok_or("missing `--out DIR`")?);
    let size = options.number::<usize>("size")?.unwrap_or(512);
    if !(16..=4096).contains(&size) || !size.is_power_of_two() {
        return Err("`--size` is a power of two from 16 to 4096".into());
    }
    let period = options.number::<f64>("period")?.unwrap_or(0.5);
    let seed = options.number::<u64>("seed")?.unwrap_or(1);
    // A probe: a metre of trunk a metre across, for checking renderers.
    if let Some(way) = options.flags.get("probe") {
        let up = match way.as_str() {
            "up" => true,
            "round" => false,
            _ => return Err("`--probe` is `up` or `round`".into()),
        };
        fs::create_dir_all(out)
            .map_err(|error| format!("cannot make {}: {error}", out.display()))?;
        let name = format!("probe-{way}");
        let file = out.join(format!("{name}.glb"));
        let glb = bark_look::glb(&bark_look::probe(size, period, up), &name, 1.0, 1.0);
        fs::write(&file, glb)
            .map_err(|error| format!("cannot write {}: {error}", file.display()))?;
        out!("wrote {}", file.display());
        return Ok(());
    }
    let look = match (options.flags.get("look"), options.flags.get("species")) {
        (Some(path), None) => {
            let text =
                fs::read_to_string(path).map_err(|error| format!("cannot read {path}: {error}"))?;
            BarkLook::from_json(&text).map_err(|error| error.to_string())?
        }
        (None, Some(name)) => {
            let spec = load_spec(name)?;
            spec.appearance
                .bark_look(&spec.id)
                .map_err(|error| error.to_string())?
                .ok_or_else(|| format!("{} states no bark", spec.id))?
        }
        _ => {
            return Err(
                "give one of `--look FILE.json`, `--species ID` or `--probe up|round`".into(),
            );
        }
    };
    fs::create_dir_all(out).map_err(|error| format!("cannot make {}: {error}", out.display()))?;
    // The record drawn, as the bark language writes it.
    let file = out.join(format!("{}.json", look.id));
    let json = serde_json::to_string_pretty(&look).map_err(|error| error.to_string())?;
    fs::write(&file, json + "\n")
        .map_err(|error| format!("cannot write {}: {error}", file.display()))?;
    let write = |name: String, width: usize, height: usize, rgba: &[u8]| -> Result<(), Failure> {
        let file = out.join(name);
        let png = raster::encode_png(width, height, rgba)
            .map_err(|error| format!("cannot encode PNG: {error}"))?;
        fs::write(&file, png)
            .map_err(|error| format!("cannot write {}: {error}", file.display()))?;
        Ok(())
    };
    // The sheet: a row a stage, the lit tile, a trunk wearing it, and its
    // base colour above its normal map at half size.
    let half = size / 2;
    let (width, row_height) = (size * 2 + half, size);
    let mut sheet = vec![255_u8; width * row_height * look.stages.len() * 4];
    let place = |sheet: &mut Vec<u8>,
                 rgba: &[u8],
                 w: usize,
                 h: usize,
                 left: usize,
                 top: usize,
                 step: usize| {
        for y in 0..h {
            for x in 0..w {
                let from = ((y * step) * (w * step) + x * step) * 4;
                let to = ((top + y) * width + left + x) * 4;
                sheet[to..to + 4].copy_from_slice(&rgba[from..from + 4]);
            }
        }
    };
    out!(
        "{} ({} stages), tiles of {size} texels over {period} m, seed {seed}:",
        look.id,
        look.stages.len()
    );
    for (index, (stage, diameter)) in look.stages.iter().zip(look.typical_diameters()).enumerate() {
        let period = bark_look::period_for(diameter, period);
        let tile = bark_look::bake(&look, stage, diameter, size, period, seed);
        let stem = format!("{}-{index}-{}", look.id, stage.name);
        write(
            format!("{stem}-basecolor.png"),
            size,
            size,
            &tile.base_colour_rgba(),
        )?;
        write(
            format!("{stem}-normal.png"),
            size,
            size,
            &tile.normal_rgba(),
        )?;
        write(format!("{stem}-orm.png"), size, size, &tile.orm_rgba())?;
        // A metre of trunk wearing it, as glTF.
        let file = out.join(format!("{stem}.glb"));
        fs::write(&file, bark_look::glb(&tile, &stem, diameter, 1.0))
            .map_err(|error| format!("cannot write {}: {error}", file.display()))?;
        write(
            format!("{stem}-height.png"),
            size,
            size,
            &tile.height_rgba(),
        )?;
        let top = index * row_height;
        place(&mut sheet, &tile.lit_rgba(), size, size, 0, top, 1);
        // The trunk: as tall as the sheet's row shows, about 1.6 diameters
        // of it, at least 0.25 m.
        let span = (diameter * 1.6).max(0.25);
        let trunk = tile.trunk_rgba(diameter, span, size, size);
        place(&mut sheet, &trunk, size, size, size, top, 1);
        place(
            &mut sheet,
            &tile.base_colour_rgba(),
            half,
            half,
            2 * size,
            top,
            2,
        );
        place(
            &mut sheet,
            &tile.normal_rgba(),
            half,
            half,
            2 * size,
            top + half,
            2,
        );
        let (low, high) = tile.relief();
        let [r, g, b] = tile.mean_albedo().map(plantgen::texture::linear_to_srgb);
        out!(
            "  {index} {:<14} at {:>5.0} cm: tile {:>4.0} mm, bark {:>5.1} mm thick, relief {:>5.1} mm, mean colour sRGB {r} {g} {b}",
            stage.name,
            diameter * 100.0,
            period * 1000.0,
            tile.thickness_m * 1000.0,
            (high - low) * 1000.0
        );
    }
    write(
        format!("{}-sheet.png", look.id),
        width,
        row_height * look.stages.len(),
        &sheet,
    )?;
    out!(
        "wrote {}: a row a stage: lit tile, trunk, base colour over normal",
        out.join(format!("{}-sheet.png", look.id)).display()
    );
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
/// `point` turned `degrees` about the vertical through the plant's foot,
/// anticlockwise seen from above.
fn spun(point: Vec3, degrees: f64) -> Vec3 {
    let (sin, cos) = degrees.to_radians().sin_cos();
    Vec3::new(
        point.x * cos + point.z * sin,
        point.y,
        -point.x * sin + point.z * cos,
    )
}

/// `graph` turned `degrees` about the vertical through its foot
/// (`plantc render --turn`), to look at a side of it a view does not face.
fn turned(graph: &PlantGraph, degrees: f64) -> PlantGraph {
    let mut out = graph.clone();
    for segment in &mut out.segments {
        segment.start = spun(segment.start, degrees);
        segment.end = spun(segment.end, degrees);
        segment.left = spun(segment.left, degrees);
    }
    for organ in &mut out.organs {
        organ.position = spun(organ.position, degrees);
        organ.heading = spun(organ.heading, degrees);
        organ.left = spun(organ.left, degrees);
    }
    out
}

fn drawn<'a>(graph: &'a PlantGraph, sizes: &[f64], looks: &[Look]) -> Cow<'a, PlantGraph> {
    looks::staged(graph, sizes, looks).map_or(Cow::Borrowed(graph), Cow::Owned)
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
        &[
            "out", "quality", "threads", "program", "day", "profile", "cards", "old",
        ],
    )?;
    let spec = load_spec(options.one_positional("a species")?)?;
    let spec = match options.flags.get("old").map(String::as_str) {
        None | Some("no") => spec,
        Some("yes") => {
            let old = spec.old_ages();
            if old.is_empty() {
                out!(
                    "{} states no lifespan past its oldest keyframe (`age_max_years`): no old ages",
                    spec.id
                );
            } else {
                let ages: Vec<String> = old.iter().map(f64::to_string).collect();
                out!("{} old ages: {} years", spec.id, ages.join(", "));
            }
            spec.grown_old()
        }
        Some(other) => return Err(format!("--old is yes or no, not `{other}`").into()),
    };
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
    let inputs = match options.flags.get("cards").map(String::as_str) {
        None | Some("no") => inputs,
        Some("yes") => inputs.with_cards(),
        Some(other) => return Err(format!("--cards is yes or no, not `{other}`").into()),
    };
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
/// The flags `plantc export` and `plantc variants` share: how bark is
/// shaded, compression, attributes, a triangle or size budget, and
/// whether the levels are the package's or tiers (`--levels`; tiers when
/// `tiers_by_default` and the flag is not given).
fn export_options(
    options: &Options,
    tiers_by_default: bool,
) -> Result<
    (
        export::Shade,
        export::Compression,
        export::Attributes,
        export::Budget,
    ),
    Failure,
> {
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
    let mut budget = export::Budget::new(
        options
            .flags
            .get("triangles")
            .map(|given| triangle_budget(given))
            .transpose()?,
        options
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
    );
    budget.tiers = match options.flags.get("levels").map(String::as_str) {
        None => tiers_by_default,
        Some("package") => false,
        Some("tiers") => true,
        Some(other) => {
            return Err(format!("unknown levels `{other}`; use package or tiers").into());
        }
    };
    Ok((shade, compression, attributes, budget))
}

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
            "levels",
            "profile",
            "bark-look",
            "bark-texels",
            "bark-period",
        ],
    )?;
    match options.flags.get("format").map(String::as_str) {
        None | Some("glb") => {}
        Some(other) => {
            return Err(format!("unknown format `{other}`; plantc exports glb").into());
        }
    }
    let (shade, compression, attributes, budget) = export_options(&options, false)?;
    // Bark from a record, or `spec` for the plant's own; the look is found
    // once the plant is.
    let bark_source = options.flags.get("bark-look").cloned();
    let bark_texels = options.number::<usize>("bark-texels")?.unwrap_or(512);
    let bark_period = options.number::<f64>("bark-period")?.unwrap_or(0.5);
    if bark_source.is_some() {
        if !(16..=4096).contains(&bark_texels) || !bark_texels.is_power_of_two() {
            return Err("`--bark-texels` is a power of two from 16 to 4096".into());
        }
        if bark_period.is_nan() || bark_period <= 0.0 {
            return Err("`--bark-period` is a length above 0, in metres".into());
        }
    } else {
        for flag in ["bark-texels", "bark-period"] {
            if options.flags.contains_key(flag) {
                return Err(format!("`--{flag}` needs `--bark-look`").into());
            }
        }
    }
    let mut settings = export::Settings {
        shade,
        compression,
        attributes,
        budget,
        bark: None,
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
    if let Some(source) = &bark_source {
        let look = if source == "spec" {
            spec.appearance
                .bark_look(&spec.id)
                .map_err(|error| error.to_string())?
                .ok_or_else(|| format!("{} states no bark", spec.id))?
        } else {
            let text = fs::read_to_string(source)
                .map_err(|error| format!("cannot read {source}: {error}"))?;
            plantgen::bark_look::BarkLook::from_json(&text).map_err(|error| error.to_string())?
        };
        // The look lives as long as the run.
        settings.bark = Some(export::BarkExport {
            look: Box::leak(Box::new(look)),
            texels: bark_texels,
            period_m: bark_period,
        });
    }
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

fn variants_command(args: &[String]) -> Result<(), Failure> {
    let options = Options::parse(
        args,
        &[
            "out",
            "count",
            "first-seed",
            "env",
            "age",
            "day",
            "quality",
            "threads",
            "shade",
            "compress",
            "attributes",
            "triangles",
            "max-mib",
            "levels",
        ],
    )?;
    let (shade, compression, attributes, budget) = export_options(&options, true)?;
    let target = options.one_positional("a species")?;
    let spec = load_spec(target)?;
    let mut generator_options = api::Options::default();
    generator_options.quality = options.quality()?;
    generator_options.day = options.day()?;
    let started = Instant::now();
    let generator = api::Generator::of_in(&spec, library(), &generator_options)
        .map_err(|error| error.to_string())?;
    let count: usize = options
        .number("count")?
        .ok_or("`plantc variants` needs `--count N`, how many plants to make")?;
    let mut variants = api::Variants::count(count);
    if let Some(first) = options.number::<u64>("first-seed")? {
        variants.first_seed = first;
    }
    if let Some(name) = options.flags.get("env") {
        variants.environment = Some(Environment::from_name(name).ok_or_else(|| {
            format!("unknown environment `{name}`; use open, edge, interior or suppressed")
        })?);
    }
    variants.age = options.number("age")?;
    if let Some(threads) = options.number::<usize>("threads")? {
        variants.threads = threads.max(1);
    }
    variants.settings = export::Settings {
        shade,
        compression,
        attributes,
        budget,
        bark: None,
    };
    let out = options
        .flags
        .get("out")
        .map_or_else(|| Path::new("variants").join(&spec.id), PathBuf::from);
    out!(
        "making {count} plants of {} from seed {}, {} at {} years, into {}",
        spec.id,
        variants.first_seed,
        variants
            .environment
            .unwrap_or_else(|| generator.default_environment())
            .name(),
        variants.age.unwrap_or_else(|| generator.default_age()),
        out.display()
    );
    let finished = std::sync::atomic::AtomicUsize::new(0);
    let set = generator
        .write_variants_reporting(&variants, &out, &|plant| {
            let done = finished.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
            let bytes: usize = plant.levels.iter().map(|level| level.bytes).sum();
            #[allow(clippy::cast_precision_loss)]
            let mebibytes = bytes as f64 / 1_048_576.0;
            // A closed output stops only the progress lines.
            let _ = write_line(format_args!(
                "  {done}/{count}  seed {}: {:.2} m tall, crown {:.2} m across, {} triangles near, \
             {mebibytes:.1} MiB",
                plant.seed,
                plant.size.height,
                plant.size.crown_radius * 2.0,
                plant.levels.first().map_or(0, |level| level.triangles)
            ));
        })
        .map_err(|error| error.to_string())?;
    out!(
        "made {} plants in {:.1} s: {}",
        set.plants.len(),
        started.elapsed().as_secs_f64(),
        out.join(api::VARIANTS_FILE).display()
    );
    if !set.duplicates.is_empty() {
        let seeds: Vec<String> = set.duplicates.iter().map(u64::to_string).collect();
        out!(
            "  drawn exactly as an earlier seed: {} (this species grows alike from those seeds)",
            seeds.join(", ")
        );
    }
    Ok(())
}

#[allow(clippy::too_many_lines)]
fn fill_command(args: &[String]) -> Result<(), Failure> {
    let options = Options::parse(
        args,
        &[
            "tile",
            "tile-m",
            "seed",
            "years",
            "stands",
            "out",
            "prototypes",
            "ages",
            "herbs",
            "edges",
            "site",
            "seeds",
            "threads",
            "quality",
            "day",
            "shade",
            "compress",
            "attributes",
            "triangles",
            "max-mib",
            "levels",
            "field",
            "corner",
            "cracks",
            "moss",
            "wet",
            "render",
            "view",
            "size",
            "level",
            "focus",
            "span",
            "cut",
            "glb",
            "glb-cells",
            "cell-m",
            "packages",
            "profile",
            "cards",
        ],
    )?;
    if options.positional.is_empty() {
        return Err("`plantc fill` needs at least one community JSON file".into());
    }
    let communities = options
        .positional
        .iter()
        .map(|path| {
            let text =
                fs::read_to_string(path).map_err(|error| format!("cannot read {path}: {error}"))?;
            api::Community::from_json(&text).map_err(|error| format!("{path}: {error}"))
        })
        .collect::<Result<Vec<_>, String>>()?;
    warn_without_niche(&communities);
    let tile = options
        .flags
        .get("tile")
        .ok_or("`plantc fill` needs `--tile X,Y`, the tile's place in the grid")?;
    let tile = match tile
        .split(',')
        .map(|part| part.trim().parse::<i32>())
        .collect::<Result<Vec<_>, _>>()
        .as_deref()
    {
        Ok([x, y]) => api::TileId { x: *x, y: *y },
        _ => return Err(format!("--tile takes X,Y as two whole numbers, found `{tile}`").into()),
    };
    let tile_m: f64 = options
        .number("tile-m")?
        .ok_or("`plantc fill` needs `--tile-m N`, the tile's side in metres")?;
    let seed: u64 = options.number("seed")?.unwrap_or(1);
    let years: f32 = options.number("years")?.unwrap_or(100.0);
    let mut settings = api::FillSettings::default();
    if let Some(ages) = options.flags.get("ages") {
        settings.stand_ages = ages
            .split(',')
            .map(|age| age.trim().parse::<f64>())
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| format!("--ages takes years separated by commas, found `{ages}`"))?;
    }
    settings.herbs = match options.flags.get("herbs").map(String::as_str) {
        None | Some("yes") => true,
        Some("no") => false,
        Some(other) => return Err(format!("--herbs takes yes or no, found `{other}`").into()),
    };
    settings.edges = match options.flags.get("edges").map(String::as_str) {
        None | Some("yes") => true,
        Some("no") => false,
        Some(other) => return Err(format!("--edges takes yes or no, found `{other}`").into()),
    };
    if let Some(seeds) = options.number::<u16>("seeds")? {
        settings.seeds = seeds;
    }
    settings.options.quality = options.quality()?;
    settings.options.day = options.day()?;
    let threads = options.number::<usize>("threads")?.unwrap_or_else(|| {
        std::thread::available_parallelism().map_or(1, std::num::NonZeroUsize::get)
    });
    let out = PathBuf::from(options.flags.get("out").map_or("fill", String::as_str));
    let started = Instant::now();
    let fill = api::Fill::new(&communities, &settings).map_err(|error| error.to_string())?;
    let grown = fill
        .measure(&[], threads, Some(&out.join("prototypes")))
        .map_err(|error| error.to_string())?;
    out!(
        "measured {} prototypes ({grown} grown, the rest kept) in {:.1} s",
        fill.prototypes().len(),
        started.elapsed().as_secs_f64()
    );
    let started = Instant::now();
    let field_site = match options.flags.get("field") {
        Some(given) => Some(field_site(&options, given, &fill, tile, tile_m)?),
        None => None,
    };
    let mut site = match &field_site {
        Some(field) => field.site.clone(),
        None => api::Site::flat(tile, tile_m).map_err(|error| error.to_string())?,
    };
    if let Some(given) = options.flags.get("site") {
        let numbers = given
            .split(',')
            .map(|part| part.trim().parse::<f64>())
            .collect::<Result<Vec<_>, _>>();
        let Ok([moisture, heat_load, elevation_m, slope_deg]) = numbers.as_deref() else {
            return Err(format!(
                "--site takes moisture,heat_load,elevation_m,slope_deg, found `{given}`"
            )
            .into());
        };
        site = site.with_factors(plantgen::niche::SiteFactors {
            moisture: *moisture,
            heat_load: *heat_load,
            elevation_m: *elevation_m,
            slope_deg: *slope_deg,
        });
    }
    let stands = match options.flags.get("stands") {
        Some(path) => {
            let text =
                fs::read_to_string(path).map_err(|error| format!("cannot read {path}: {error}"))?;
            serde_json::from_str::<api::StandMap>(&text)
                .map_err(|error| format!("{path}: {error}"))?
        }
        None => api::StandMap::uniform(tile_m, 0, years),
    };
    let plan = fill
        .tile(&site, &stands, seed)
        .map_err(|error| error.to_string())?;
    let written = plan
        .write(&out.join("tiles"))
        .map_err(|error| error.to_string())?;
    out!(
        "filled tile {},{} ({tile_m} m) with {} plants of {} prototypes in {:.2} s: {}",
        tile.x,
        tile.y,
        plan.instances.len(),
        plan.prototypes_used().len(),
        started.elapsed().as_secs_f64(),
        written.display()
    );
    let stats = &plan.stats;
    out!(
        "  stems/ha {:.0}, trees/ha (5 cm or more at breast height) {:.0}, basal area {:.1} m²/ha, canopy cover {:.0}%, leaf area index {:.2}, edge trees/ha {:.0}",
        stats.stems_per_ha,
        stats.trees_per_ha,
        stats.basal_area_m2_per_ha,
        100.0 * stats.canopy_cover,
        stats.leaf_area_index,
        stats.edge_trees_per_ha
    );
    let [litter, moss, wood, herbs] = stats.floor;
    let mut decay = [0_usize; 5];
    for log in &plan.logs {
        decay[usize::from(log.shape.decay.clamp(1, 5) - 1)] += 1;
    }
    out!(
        "  forest floor: litter {:.0}%, moss {:.0}%, logs {:.1}% ({:.0}/ha; decay classes I-V {}), herbs {:.0}%",
        100.0 * litter,
        100.0 * moss,
        100.0 * wood,
        stats.logs_per_ha,
        decay.map(|n| n.to_string()).join("/"),
        100.0 * herbs
    );
    // The floor's moss genera, each its share of all the tile's moss.
    let mut genera = vec![0.0_f64; plan.floor_mosses.len()];
    for cover in &plan.cover {
        for found in cover.floor.mosses.iter().flatten() {
            if let Some(sum) = genera.get_mut(usize::from(found.genus)) {
                *sum += f64::from(cover.floor.moss) * f64::from(found.share);
            }
        }
    }
    let all: f64 = genera.iter().sum();
    if all > 0.0 {
        let mut named: Vec<(&String, f64)> = plan.floor_mosses.iter().zip(genera).collect();
        named.sort_by(|a, b| b.1.total_cmp(&a.1));
        out!(
            "  floor mosses: {}",
            named
                .iter()
                .map(|(genus, sum)| format!("{genus} {:.0}%", 100.0 * sum / all))
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    // By species: trees 5 cm or more at breast height, their mean and
    // tallest height, and every plant.
    let area_ha = tile_m * tile_m / 10_000.0;
    let mut by_species: BTreeMap<&str, (usize, f64, f64, usize)> = BTreeMap::new();
    for instance in &plan.instances {
        let index = instance.prototype as usize;
        let species = fill.prototypes()[index].key.species.as_str();
        let entry = by_species.entry(species).or_insert((0, 0.0, 0.0, 0));
        entry.3 += 1;
        if let Some(size) = fill.size(index) {
            let scale = f64::from(instance.scale);
            if size.dbh.unwrap_or(0.0) * scale >= 0.05 {
                entry.0 += 1;
                entry.1 += size.height * scale;
                entry.2 = entry.2.max(size.height * scale);
            }
        }
    }
    for (species, (trees, heights, tallest, all)) in &by_species {
        #[allow(clippy::cast_precision_loss)]
        let (per_ha, mean, every) = (
            *trees as f64 / area_ha,
            heights / (*trees).max(1) as f64,
            *all as f64 / area_ha,
        );
        out!(
            "  {species}: {per_ha:.0} trees/ha (mean height {mean:.1} m, tallest {tallest:.1} m), {every:.0} plants/ha in all"
        );
    }
    // Each layer seen from above, as inventory plots measure them.
    let layers = fill.layer_cover(&plan);
    if layers.top_m > 0.0 || layers.shrubs > 0.0 || layers.herbs > 0.0 {
        let band = |at: usize| {
            format!(
                "{:.0}% ({:.0}/ha at {:.1} m)",
                100.0 * layers.trees[at],
                layers.stems_per_ha[at],
                layers.heights_m[at]
            )
        };
        out!(
            "  layers seen from above (tallest tree {:.1} m): upper trees {}, middle {}, lower {}; all trees {:.0}%; shrubs {:.0}%; herbs {:.0}%",
            layers.top_m,
            band(0),
            band(1),
            band(2),
            100.0 * layers.all_trees,
            100.0 * layers.shrubs,
            100.0 * layers.herbs
        );
    }
    let moss = match options.number::<u32>("moss")? {
        Some(years) => {
            let field = field_site
                .as_ref()
                .ok_or("`--moss` grows moss over a site: give `--field SITE.json`")?;
            // The tile's trees shade the walls and roofs they stand over.
            let canopy = plan.crowns(&fill, field.corner, MOSS_SHADE_LEAST_M);
            let cover = tile_moss(
                field,
                tile_m,
                years,
                options.number("seed")?.unwrap_or(1),
                &canopy,
            )?;
            let path = out
                .join("tiles")
                .join(format!("{}_{}.moss.json", tile.x, tile.y));
            fs::write(&path, cover.to_json()?)
                .map_err(|error| format!("cannot write {}: {error}", path.display()))?;
            out!("  wrote {}", path.display());
            Some(cover)
        }
        None => None,
    };
    if let Some(png) = options.flags.get("render") {
        let field = field_site
            .as_ref()
            .ok_or("`--render` draws a tile over a site: give `--field SITE.json`")?;
        render_fill(&options, png, &fill, &plan, field, moss.as_ref())?;
    }
    if let Some(dir) = options.flags.get("packages") {
        // The prototypes the tile places, or with --prototypes all every one.
        let all = options.flags.get("prototypes").map(String::as_str) == Some("all");
        let held = if all {
            Vec::new()
        } else {
            plan.prototypes_used()
        };
        if all || !held.is_empty() {
            write_fill_packages(&fill, &held, &options, Path::new(dir))?;
        }
    }
    let which = match options.flags.get("prototypes").map(String::as_str) {
        Some("none") => return Ok(()),
        None | Some("used") => plan.prototypes_used(),
        Some("all") => Vec::new(),
        Some(other) => {
            return Err(format!("unknown prototypes `{other}`; use none, used or all").into());
        }
    };
    if plan.instances.is_empty() {
        return Ok(());
    }
    let (shade, compression, attributes, budget) = export_options(&options, true)?;
    let export_settings = export::Settings {
        shade,
        compression,
        attributes,
        budget,
        bark: None,
    };
    let started = Instant::now();
    let prototypes = fill
        .write_prototypes(&which, &export_settings, threads, &out.join("prototypes"))
        .map_err(|error| error.to_string())?;
    out!(
        "wrote {} prototypes in {:.1} s: {}",
        prototypes.len(),
        started.elapsed().as_secs_f64(),
        out.join("prototypes").join(api::PROTOTYPES_FILE).display()
    );
    if let Some(level) = options.number::<usize>("glb")? {
        if level >= export::LEVELS {
            return Err(format!("--glb takes a level from 0 to {}", export::LEVELS - 1).into());
        }
        let glb = plan
            .glb(
                &prototypes,
                &out.join("prototypes"),
                level,
                compression == export::Compression::Meshopt,
            )
            .map_err(|error| error.to_string())?;
        let path = out.join("tiles").join(format!("{}_{}.glb", tile.x, tile.y));
        fs::write(&path, &glb).map_err(|error| format!("{}: {error}", path.display()))?;
        #[allow(clippy::cast_precision_loss)]
        let mib = glb.len() as f64 / 1_048_576.0;
        out!(
            "wrote the tile as one glTF file at level {level}, {mib:.1} MiB: {}",
            path.display()
        );
    }
    if let Some(list) = options.flags.get("glb-cells") {
        let levels = list
            .split(',')
            .map(|v| match v.trim().parse::<usize>() {
                Ok(level) if level < export::LEVELS => Ok(level),
                _ => Err(format!(
                    "--glb-cells takes levels from 0 to {}, not `{v}`",
                    export::LEVELS - 1
                )),
            })
            .collect::<Result<Vec<_>, _>>()?;
        let cell_m = options.number::<f64>("cell-m")?.unwrap_or(20.0);
        let files = plan
            .glb_cells(
                &prototypes,
                &out.join("prototypes"),
                cell_m,
                &levels,
                compression == export::Compression::Meshopt,
            )
            .map_err(|error| error.to_string())?;
        let tiles = out.join("tiles");
        let mut bytes = 0;
        for (name, data) in &files {
            let path = tiles.join(name);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)
                    .map_err(|error| format!("{}: {error}", parent.display()))?;
            }
            fs::write(&path, data).map_err(|error| format!("{}: {error}", path.display()))?;
            bytes += data.len();
        }
        #[allow(clippy::cast_precision_loss)]
        let mib = bytes as f64 / 1_048_576.0;
        out!(
            "wrote the tile as {} files of {cell_m} m cells at levels {list}, {mib:.1} MiB: {}",
            files.len() - 1,
            tiles
                .join(format!("{}_{}/cells.json", tile.x, tile.y))
                .display()
        );
    }
    Ok(())
}

/// The name of the file `plantc fill --packages` lists its packages in.
const FILL_PACKAGES_FILE: &str = "packages.json";

/// Build the packages that hold the fill's prototypes `which` (every one
/// when empty) into `dir`, one per growth (`Fill::packages`), as `plantc
/// build` builds a package (`--quality`, `--day`, `--threads`,
/// `--profile`, `--cards`), and list in `dir/packages.json` which package
/// and keyframe draws each prototype.
fn write_fill_packages(
    fill: &api::Fill,
    which: &[u32],
    options: &Options,
    dir: &Path,
) -> Result<(), Failure> {
    let packages = fill.packages(which).map_err(|error| error.to_string())?;
    let started = Instant::now();
    let mut listed = Vec::new();
    for package in &packages {
        let path = build_package(&package.spec, options, dir)?;
        let held: Vec<serde_json::Value> = package
            .prototypes
            .iter()
            .map(|&(index, keyframe)| {
                serde_json::json!({
                    "index": index,
                    "stem": fill.prototypes()[index as usize].key.stem(),
                    "keyframe": keyframe,
                })
            })
            .collect();
        let mut entry = serde_json::json!({
            "species": package.spec.id,
            "file": path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default(),
            "prototypes": held,
        });
        // A wall climber's package grew at the foot of a wall this high.
        if let Some(wall) = package.spec.variants.wall_m {
            entry["wall_m"] = serde_json::json!(wall);
        }
        listed.push(entry);
    }
    let list = serde_json::json!({
        "schema": 1,
        "generator_revision": plantgen::GENERATOR_REVISION,
        "packages": listed,
    });
    let path = dir.join(FILL_PACKAGES_FILE);
    let text = serde_json::to_string_pretty(&list).map_err(|error| error.to_string())?;
    fs::write(&path, text).map_err(|error| format!("{}: {error}", path.display()))?;
    out!(
        "built {} packages in {:.1} s: {}",
        packages.len(),
        started.elapsed().as_secs_f64(),
        path.display()
    );
    Ok(())
}

/// A fill's tile over a site document's mesh (`plantc fill --field`).
struct FieldSite {
    /// The site document.
    document: serde_json::Value,
    mesh: plantgen::site::SiteMesh,
    site: api::Site,
    /// The tile's south-west corner, (x, z) in the mesh's frame.
    corner: [f64; 2],
}

/// The site field of the document `given` over the tile whose south-west
/// corner is `--corner X,Z` (the mesh's low x and high z unless given),
/// and with `--cracks YEARS` the plants of the fill's species among the
/// pavement pool that come up in its pavements in those years.
fn field_site(
    options: &Options,
    given: &str,
    fill: &api::Fill,
    tile: api::TileId,
    tile_m: f64,
) -> Result<FieldSite, Failure> {
    use plantgen::site_field::{FieldSettings, build};
    use plantgen::succession::{Settings, microsites};
    let text =
        fs::read_to_string(given).map_err(|error| format!("cannot read {given}: {error}"))?;
    let document: serde_json::Value =
        serde_json::from_str(&text).map_err(|error| format!("invalid site: {error}"))?;
    let folder = Path::new(given).parent().unwrap_or(Path::new("."));
    let mesh = site_mesh_of(&document, folder)?;
    let corner = if let Some(text) = options.flags.get("corner") {
        let values: Vec<f64> = text
            .split(',')
            .map(|v| v.trim().parse::<f64>())
            .collect::<Result<_, _>>()
            .map_err(|_| "`--corner` expects X,Z")?;
        let [x, z] = values[..] else {
            return Err("`--corner` expects X,Z".into());
        };
        [x, z]
    } else {
        let (low, high) = mesh.bounds();
        [low.x, high.z]
    };
    let started = Instant::now();
    let settings = FieldSettings {
        margin_m: 0.0,
        ..FieldSettings::default()
    };
    let field = build(
        &mesh,
        [corner[0], corner[1] - tile_m],
        [corner[0] + tile_m, corner[1]],
        &settings,
    )?;
    out!(
        "site field of {} x {} cells, {} patches in {:.1} s",
        field.columns,
        field.rows,
        field.patches.len(),
        started.elapsed().as_secs_f64()
    );
    let mut site = api::Site::from_field(tile, tile_m, std::sync::Arc::new(field), corner)
        .map_err(|error| error.to_string())?;
    if let Some(years) = options.number::<u32>("cracks")? {
        let held: BTreeSet<&str> = fill
            .prototypes()
            .iter()
            .filter(|prototype| !prototype.key.on_wall)
            .map(|prototype| prototype.key.species.as_str())
            .collect();
        let pool: Vec<&str> = PAVEMENT_POOL
            .iter()
            .copied()
            .filter(|id| held.contains(id))
            .collect();
        if pool.is_empty() {
            return Err(
                "`--cracks`: the fill holds none of the pavement pool; add a community of them"
                    .into(),
            );
        }
        let started = Instant::now();
        let ages: Vec<f64> = [1.0, 2.0, 3.0, 5.0, 8.0, 12.0, 20.0, 30.0, 50.0]
            .into_iter()
            .filter(|age| *age <= f64::from(years) + 1.0)
            .chain(std::iter::once(f64::from(years) + 1.5))
            .collect();
        let colonists = pool
            .iter()
            .map(|id| colonist(id, &ages))
            .collect::<Result<Vec<_>, String>>()?;
        let cores = std::thread::available_parallelism().map_or(1, std::num::NonZero::get);
        let places = microsites(&mesh, cores);
        let settings = Settings {
            seed: options.number("seed")?.unwrap_or(1),
            traffic: 0.0,
            maintenance: 0.0,
        };
        site = site
            .with_colonists(&places, &colonists, &[], &settings, years)
            .map_err(|error| error.to_string())?;
        out!(
            "{} plants came up in the pavements in {years} years ({} species of the pool, {} microsites) in {:.1} s",
            site.colonists.len(),
            pool.len(),
            places.len(),
            started.elapsed().as_secs_f64()
        );
    }
    Ok(FieldSite {
        document,
        mesh,
        site,
        corner,
    })
}

/// `plant` (in growth's frame: +Y up, -Z north) placed as `instance` says
/// on a tile whose south-west corner is `corner` in the mesh's frame,
/// moved by `offset`: turned by its rotation (z up, x east, y north),
/// scaled, and its foot set at its position; a climber's wood and cards
/// past its wall's top and ends (`extra`, [`api::WallClip`]) left out.
/// A log drawn in its own frame ([`plantgen::deadwood::Shape`]) placed
/// as the tile lays it: turned to its yaw and tilted to its pitch about
/// the floor under its middle, in the render's frame (+x east, +y up, -z
/// north).
/// Leave out of `scene` every triangle wholly above `y`, and every card
/// and spine tuft whose foot is.
fn cut_above(scene: &mut PlantMesh, y: f32) {
    let positions = &scene.wood.positions;
    let kept: Vec<u32> = scene
        .wood
        .indices
        .chunks_exact(3)
        .filter(|triangle| triangle.iter().any(|&k| positions[k as usize][1] <= y))
        .flatten()
        .copied()
        .collect();
    scene.wood.indices = kept;
    scene.cards.retain(|card| card.base[1] <= y);
    scene.tufts.retain(|tuft| tuft.position[1] <= y);
    scene.sites.clear();
}

fn placed_log(log: mesh::Mesh, placed: &api::Log, corner: [f64; 2], offset: Vec3) -> PlantMesh {
    let (yaw, pitch) = (f64::from(placed.yaw), f64::from(placed.pitch));
    let along = Vec3::new(yaw.cos(), 0.0, -yaw.sin());
    let across = Vec3::new(yaw.sin(), 0.0, yaw.cos());
    let (sp, cp) = (pitch.sin(), pitch.cos());
    let foot = Vec3::new(
        corner[0] + f64::from(placed.position[0]),
        f64::from(placed.position[2]),
        corner[1] - f64::from(placed.position[1]),
    ) + offset;
    let turn = |v: [f32; 3]| {
        let (x, y, z) = (f64::from(v[0]), f64::from(v[1]), f64::from(v[2]));
        let (x, y) = (x * cp - y * sp, x * sp + y * cp);
        along * x + Vec3::Y * y + across * z
    };
    let mut wood = log;
    for position in &mut wood.positions {
        *position = (turn(*position) + foot).to_f32();
    }
    for normal in &mut wood.normals {
        *normal = turn(*normal).to_f32();
    }
    PlantMesh {
        wood,
        ..PlantMesh::default()
    }
}

/// The tile's forest floor drawn over its ground for a render: a grid of
/// half-metre squares a centimetre above it, each corner moss, litter or
/// bare soil by its stand cell's shares ([`api::Floor`]), in patches a few
/// metres across.
fn floor_mesh(plan: &api::TilePlan, field: &FieldSite, offset: Vec3) -> mesh::Mesh {
    const STEP_M: f64 = 0.5;
    let srgb = |c: [u8; 3]| -> [f32; 3] {
        #[allow(clippy::cast_possible_truncation)]
        c.map(|byte| plantgen::texture::srgb_to_linear(byte) as f32)
    };
    let (moss, litter, soil) = (
        plantgen::looks::Moss::default().colour,
        srgb(plantgen::deadwood::LITTER_SRGB),
        srgb([122, 104, 82]),
    );
    // Each floor moss genus's colour from afar, as its colonies draw.
    let kinds = plantgen::mosses::kinds(Library::builtin()).unwrap_or_default();
    let genus_colour: Vec<[f32; 3]> = plan
        .floor_mosses
        .iter()
        .map(|genus| {
            kinds
                .iter()
                .find(|kind| &kind.genus == genus)
                .map_or(moss, plantgen::mosses::MossKind::far_colour)
        })
        .collect();
    let tile_m = plan.tile_m;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let steps = (tile_m / STEP_M).round() as usize;
    // Smooth patches: two octaves of value noise, metres.
    let noise = |x: f64, y: f64, salt: u64| -> f64 {
        let octave = |scale: f64, salt: u64| {
            let (fx, fy) = (x / scale, y / scale);
            let (ix, iy) = (fx.floor(), fy.floor());
            let (tx, ty) = (fx - ix, fy - iy);
            #[allow(clippy::cast_possible_truncation)]
            let at = |dx: f64, dy: f64| {
                plantgen::rng::unit(plantgen::rng::hash_words(&[
                    salt,
                    (ix + dx) as i64 as u64,
                    (iy + dy) as i64 as u64,
                ]))
            };
            let (sx, sy) = (tx * tx * (3.0 - 2.0 * tx), ty * ty * (3.0 - 2.0 * ty));
            let top = at(0.0, 0.0) + (at(1.0, 0.0) - at(0.0, 0.0)) * sx;
            let bottom = at(0.0, 1.0) + (at(1.0, 1.0) - at(0.0, 1.0)) * sx;
            top + (bottom - top) * sy
        };
        0.65 * octave(3.0, salt) + 0.35 * octave(0.9, salt + 1)
    };
    let mut out = mesh::Mesh::default();
    let row = steps + 1;
    for j in 0..=steps {
        for i in 0..=steps {
            #[allow(clippy::cast_precision_loss)]
            let (east, north) = (
                (i as f64 * STEP_M).min(tile_m),
                (j as f64 * STEP_M).min(tile_m),
            );
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let cell = {
                let col = ((east / plan.cell_m) as usize).min(plan.cols - 1);
                let r = ((north / plan.cell_m) as usize).min(plan.cols - 1);
                r * plan.cols + col
            };
            let floor = plan
                .cover
                .get(cell)
                .map(|cover| cover.floor)
                .unwrap_or_default();
            let (m, l) = (f64::from(floor.moss), f64::from(floor.litter));
            let surface = (1.0 - f64::from(floor.wood)).max(1e-6);
            // Moss where the patch noise falls under its share of the
            // floor logs leave, litter over the rest as far as it reaches.
            let n = noise(east, north, 0x0f10);
            let colour = if n < m / surface {
                // Which genus: patches of each by its share.
                let mut pick = noise(east, north, 0x0f20);
                let mut colour = moss;
                for found in floor.mosses.iter().flatten() {
                    colour = genus_colour
                        .get(usize::from(found.genus))
                        .copied()
                        .unwrap_or(moss);
                    pick -= f64::from(found.share);
                    if pick < 0.0 {
                        break;
                    }
                }
                colour
            } else if n < (m + l) / surface {
                litter
            } else {
                soil
            };
            let up = field.site.height_at(east, north) + 0.01;
            let position = Vec3::new(field.corner[0] + east, up, field.corner[1] - north) + offset;
            out.push(mesh::Vertex::plain(position, Vec3::Y, colour));
        }
    }
    for j in 0..steps {
        for i in 0..steps {
            #[allow(clippy::cast_possible_truncation)]
            let a = (j * row + i) as u32;
            #[allow(clippy::cast_possible_truncation)]
            let b = a + row as u32;
            out.indices.extend([a, a + 1, b, a + 1, b + 1, b]);
        }
    }
    out
}

fn placed_instance(
    plant: &PlantMesh,
    instance: &api::Instance,
    climber: bool,
    corner: [f64; 2],
    offset: Vec3,
) -> PlantMesh {
    let [qx, qy, qz, qw] = instance.rotation;
    // Growth's frame to the instance's and back.
    let turn = |v: [f32; 3]| {
        let u = [v[0], -v[2], v[1]];
        let t = [
            2.0 * (qy * u[2] - qz * u[1]),
            2.0 * (qz * u[0] - qx * u[2]),
            2.0 * (qx * u[1] - qy * u[0]),
        ];
        let r = [
            u[0] + qw * t[0] + (qy * t[2] - qz * t[1]),
            u[1] + qw * t[1] + (qz * t[0] - qx * t[2]),
            u[2] + qw * t[2] + (qx * t[1] - qy * t[0]),
        ];
        [r[0], r[2], -r[1]]
    };
    #[allow(clippy::cast_possible_truncation)]
    let foot = [
        (corner[0] + f64::from(instance.position[0]) + offset.x) as f32,
        (f64::from(instance.position[2]) + offset.y) as f32,
        (corner[1] - f64::from(instance.position[1]) + offset.z) as f32,
    ];
    let scale = instance.scale;
    let point = |v: [f32; 3]| {
        let t = turn(v);
        [
            t[0] * scale + foot[0],
            t[1] * scale + foot[1],
            t[2] * scale + foot[2],
        ]
    };
    let clip = if climber {
        api::WallClip::from_extra(instance.extra)
    } else {
        None
    };
    let mut out = plant.clone();
    if let Some(clip) = clip {
        let positions = &plant.wood.positions;
        let mut kept = Vec::with_capacity(out.wood.indices.len());
        for triangle in plant.wood.indices.as_chunks::<3>().0 {
            if triangle
                .iter()
                .all(|index| clip.keeps(positions[*index as usize]))
            {
                kept.extend_from_slice(triangle);
            }
        }
        out.wood.indices = kept;
        out.cards.retain(|card| clip.keeps(card.base));
        out.tufts.retain(|tuft| clip.keeps(tuft.position));
    }
    out.wood.positions.iter_mut().for_each(|p| *p = point(*p));
    out.wood.normals.iter_mut().for_each(|n| *n = turn(*n));
    for colour in &mut out.wood.colors {
        if colour[3] < 0.0 {
            colour[3] *= scale;
        }
    }
    for card in &mut out.cards {
        card.base = point(card.base);
        card.heading = turn(card.heading);
        card.left = turn(card.left);
        card.length *= scale;
        card.width *= scale;
    }
    for tuft in &mut out.tufts {
        tuft.position = point(tuft.position);
    }
    out.hair_tubes.clear();
    out
}

/// The moss genera that grow on the site `document`: the library's, or
/// only those its `mosses` list names (its region's mosses; spores of the
/// others do not reach it).
fn site_moss_kinds(
    document: &serde_json::Value,
) -> Result<Vec<plantgen::mosses::MossKind>, Failure> {
    let mut kinds = plantgen::mosses::kinds(library())?;
    if kinds.is_empty() {
        return Err("the library describes no moss genus with a colony_form".into());
    }
    if let Some(names) = document.get("mosses").and_then(serde_json::Value::as_array) {
        let names: Vec<&str> = names.iter().filter_map(serde_json::Value::as_str).collect();
        if let Some(unknown) = names
            .iter()
            .find(|name| !kinds.iter().any(|kind| kind.genus == **name))
        {
            return Err(
                format!("site names moss genus `{unknown}`, which the library lacks").into(),
            );
        }
        kinds.retain(|kind| names.contains(&kind.genus.as_str()));
    }
    Ok(kinds)
}

/// The least height of a fill plant whose crown shades the moss under it,
/// metres: trees and big shrubs over roofs and walls, not the herbs.
const MOSS_SHADE_LEAST_M: f64 = 2.0;

/// The mosses `years` grow over the tile of `field` (`plantc fill
/// --moss YEARS`) under the crowns of `canopy`: the genera its site
/// names, or every genus of the library, from `seed`.
fn tile_moss(
    field: &FieldSite,
    tile_m: f64,
    years: u32,
    seed: u64,
    canopy: &[plantgen::conditions::Envelope],
) -> Result<plantgen::mosses::MossCover, Failure> {
    use plantgen::mosses::{self, MossSettings};
    let started = Instant::now();
    let kinds = site_moss_kinds(&field.document)?;
    let settings = MossSettings::over_tile(field.corner, tile_m, years, seed);
    let cover = mosses::grow_under(&field.mesh, &kinds, &settings, canopy);
    out!(
        "  moss: {} colonies after {years} years under {} crowns in {:.1} s",
        cover.colonies.len(),
        canopy.len(),
        started.elapsed().as_secs_f64()
    );
    for (genus, count, area) in cover.census() {
        out!("    {genus:<14} {count:>6} colonies, {area:.2} m²");
    }
    Ok(cover)
}

/// Draw a filled tile on its site (`plantc fill --render PNG`): the
/// site's mesh and every instance as its prototype at `--level` (1 unless
/// given), seen from `--view` (oblique unless given), with its moss if
/// grown: shoots within a twentieth of the view's span of its focus (the
/// tile's middle unless given), shells out to its span, patches beyond.
#[allow(clippy::too_many_lines)]
fn render_fill(
    options: &Options,
    png: &str,
    fill: &api::Fill,
    plan: &api::TilePlan,
    field: &FieldSite,
    moss: Option<&plantgen::mosses::MossCover>,
) -> Result<(), Failure> {
    let started = Instant::now();
    let level: usize = options.number("level")?.unwrap_or(1);
    let used = plan.prototypes_used();
    let tile_m = plan.tile_m;
    let middle_ground = field.site.height_at(0.5 * tile_m, 0.5 * tile_m);
    let offset = Vec3::new(
        -(field.corner[0] + 0.5 * tile_m),
        -middle_ground,
        -(field.corner[1] - 0.5 * tile_m),
    );
    // Each prototype drawn once, its cards' templates past the looks of
    // the species before it.
    let mut all_looks: Vec<Look> = Vec::new();
    let mut looks_at: BTreeMap<String, usize> = BTreeMap::new();
    let mut bark = None;
    let mut drawn: BTreeMap<u32, PlantMesh> = BTreeMap::new();
    for index in used {
        let (generator, plant) = fill
            .grow_prototype(index as usize)
            .map_err(|error| error.to_string())?;
        let (mut mesh, looks) = generator.draw(&plant, level);
        let species = generator.spec().id.clone();
        let first = *looks_at.entry(species).or_insert_with(|| {
            let first = all_looks.len();
            all_looks.extend_from_slice(looks);
            first
        });
        let shift = u8::try_from(first).unwrap_or(u8::MAX);
        for card in &mut mesh.cards {
            card.template = card.template.saturating_add(shift);
        }
        if bark.is_none() {
            bark = generator.spec().appearance.bark_params();
        }
        drawn.insert(index, mesh);
    }
    if all_looks.len() > 255 {
        return Err("too many organ types to draw at once; fill with fewer species".into());
    }
    out!(
        "drew {} prototypes at level {level} in {:.1} s",
        drawn.len(),
        started.elapsed().as_secs_f64()
    );
    let mut scene = PlantMesh::default();
    for instance in &plan.instances {
        let Some(plant) = drawn.get(&instance.prototype) else {
            continue;
        };
        let climber = fill.prototypes()[instance.prototype as usize].key.on_wall;
        let placed = placed_instance(plant, instance, climber, field.corner, offset);
        scene = on_host(scene, placed, 0);
    }
    // The logs, each drawn at the level, its bark its species'.
    let mut appearances: BTreeMap<String, Option<spec::Appearance>> = BTreeMap::new();
    for log in &plan.logs {
        let appearance = appearances
            .entry(log.species.clone())
            .or_insert_with(|| {
                Library::builtin()
                    .spec(&log.species)
                    .ok()
                    .map(|spec| spec.appearance)
            })
            .as_ref();
        // Logs are cheap: never coarser than level 1.
        let drawn = plantgen::deadwood::draw(&log.shape, appearance, level.min(1));
        scene = on_host(scene, placed_log(drawn, log, field.corner, offset), 0);
    }
    // `--cut M`: leave out everything more than M metres above the tile's
    // middle, so the forest floor shows from above.
    if let Some(cut) = options.number::<f32>("cut")? {
        cut_above(&mut scene, cut);
    }
    let half = 0.5 * tile_m;
    let ground = field.mesh.drawn(
        Vec3::new(field.corner[0], -1.0e9, field.corner[1] - tile_m),
        Vec3::new(field.corner[0] + tile_m, 1.0e9, field.corner[1]),
        offset,
    );
    let floor = floor_mesh(plan, field, offset);
    let templates = Templates::for_plant(&all_looks, &[]).with_bark(bark);
    let view = match options.flags.get("view") {
        Some(name) => View::from_name(name).ok_or_else(|| format!("unknown view `{name}`"))?,
        None => View::ThreeQuarter,
    };
    let pixels: usize = options.number("size")?.unwrap_or(1600);
    // `--focus X,Y,Z` in the mesh's frame, framing `--span` metres.
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
            let span: f64 = options.number("span")?.unwrap_or(4.0);
            Some((Vec3::new(x, y, z) + offset, span))
        }
        None => None,
    };
    let moss = match moss {
        Some(cover) => {
            use plantgen::mosses::{Detail, DrawOptions};
            let (at, span) = focus.unwrap_or((Vec3::ZERO, half));
            let mut mesh = cover.draw(
                &field.mesh,
                &DrawOptions {
                    hydration: options.number::<f64>("wet")?.unwrap_or(0.4).clamp(0.0, 1.0),
                    detail: Detail::Near,
                    near: Some((at - offset, span * 0.05)),
                    capsules: true,
                },
            );
            #[allow(clippy::cast_possible_truncation)]
            let shift = [offset.x as f32, offset.y as f32, offset.z as f32];
            for p in &mut mesh.positions {
                for (value, by) in p.iter_mut().zip(shift) {
                    *value += by;
                }
            }
            mesh
        }
        None => plantgen::mesh::Mesh::default(),
    };
    let image = preview::render_on_site(
        &scene,
        &[
            (&ground, raster::Material::Opaque),
            (&floor, raster::Material::Opaque),
            (&moss, raster::Material::Opaque),
        ],
        &templates,
        &PreviewOptions {
            width: pixels,
            height: pixels * 3 / 4,
            view,
            supersample: 2,
            figure: false,
            frame_height: None,
            frame_reach: Some(half * 1.05),
            focus,
            fit_plant: false,
        },
    );
    let bytes = raster::encode_png(
        image.width,
        image.height,
        &image.to_srgb8(Some(preview::SKY)),
    )
    .map_err(|error| format!("cannot encode PNG: {error}"))?;
    fs::write(png, bytes).map_err(|error| format!("cannot write {png}: {error}"))?;
    out!(
        "wrote {png}: {} triangles of plants, {} of moss, {} of site, in {:.1} s",
        scene.triangle_count(),
        moss.triangle_count(),
        ground.triangle_count(),
        started.elapsed().as_secs_f64()
    );
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

/// `plantc unpack`: copy glTF files, or a folder of them, with
/// `EXT_meshopt_compression` undone, so Blender 4.5 reads them.
fn unpack_command(args: &[String]) -> Result<(), Failure> {
    let options = Options::parse(args, &["out"])?;
    let from = PathBuf::from(options.one_positional("a .glb file or a folder")?);
    let to = PathBuf::from(
        options
            .flags
            .get("out")
            .ok_or("`plantc unpack` needs --out (a file, or a folder for a folder)")?,
    );
    if from == to {
        return Err("--out must differ from the input".to_string().into());
    }
    let mut files = Vec::new();
    if from.is_dir() {
        let mut folders = vec![from.clone()];
        while let Some(folder) = folders.pop() {
            for entry in fs::read_dir(&folder)
                .map_err(|error| format!("cannot read {}: {error}", folder.display()))?
            {
                let path = entry.map_err(|error| error.to_string())?.path();
                if path.is_dir() {
                    folders.push(path);
                } else {
                    let relative = path.strip_prefix(&from).map_err(|e| e.to_string())?;
                    files.push((path.clone(), to.join(relative)));
                }
            }
        }
    } else {
        files.push((from.clone(), to.clone()));
    }
    files.sort();
    let (mut unpacked, mut before, mut after) = (0usize, 0usize, 0usize);
    for (source, target) in &files {
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("cannot create {}: {error}", parent.display()))?;
        }
        let bytes = fs::read(source)
            .map_err(|error| format!("cannot read {}: {error}", source.display()))?;
        let written = if source.extension().is_some_and(|ext| ext == "glb") {
            let plain = plantgen::gltf::unpack(&bytes)
                .map_err(|error| format!("{}: {error}", source.display()))?;
            if plain.len() != bytes.len() || plain != bytes {
                unpacked += 1;
                before += bytes.len();
                after += plain.len();
            }
            plain
        } else {
            bytes
        };
        fs::write(target, written)
            .map_err(|error| format!("cannot write {}: {error}", target.display()))?;
    }
    #[allow(clippy::cast_precision_loss)]
    let mib = |bytes: usize| bytes as f64 / 1_048_576.0;
    out!(
        "copied {} files to {}, {unpacked} of them unpacked from meshopt ({:.1} to {:.1} MiB)",
        files.len(),
        to.display(),
        mib(before),
        mib(after)
    );
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
