//! `plantc` as people run it: real processes, real files.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use plantgen::spec::{AllometryPoint, Environment, PlantSpec};

fn plantc(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_plantc"))
        .args(args)
        .output()
        .unwrap()
}

fn stdout(output: &Output) -> String {
    assert!(
        output.status.success(),
        "plantc failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout.clone()).unwrap()
}

fn scratch(name: &str) -> PathBuf {
    let path = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("plantc-{name}-{}", std::process::id()));
    if path.exists() {
        fs::remove_dir_all(&path).unwrap();
    }
    fs::create_dir_all(&path).unwrap();
    path
}

/// A spec file for a six-year-old Douglas-fir: one seed, in the open.
fn small_spec_file(dir: &Path) -> PathBuf {
    let mut spec = PlantSpec::builtin("pseudotsuga-menziesii").unwrap();
    spec.growth.years = 6.0;
    spec.growth.keyframes = vec![3.0, 6.0];
    spec.variants.environments = vec![Environment::Open];
    spec.variants.seeds = vec![7];
    spec.allometry = vec![AllometryPoint {
        age: 6.0,
        environment: Environment::Open,
        height: 4.0,
        dbh: None,
        tolerance: 0.9,
    }];
    let path = dir.join("young-fir.json");
    fs::write(&path, serde_json::to_string_pretty(&spec).unwrap()).unwrap();
    path
}

#[test]
fn list_and_check_name_the_built_in_species() {
    let listed = stdout(&plantc(&["list"]));
    assert!(listed.contains("pseudotsuga-menziesii"), "{listed}");
    assert!(listed.contains("Cactaceae:"), "{listed}");
    assert!(listed.contains("carnegiea-gigantea"), "{listed}");
    assert!(listed.contains("Programs:"));
    let checked = stdout(&plantc(&["check", "pseudotsuga-menziesii"]));
    assert!(!checked.trim().is_empty());
    let help = stdout(&plantc(&["help"]));
    assert!(help.contains("plantc build"));
}

/// `plantc rules` lists rules by their home and the general ones;
/// `plantc spec` shows a species' traits and what each rule did.
#[test]
#[allow(clippy::too_many_lines)]
fn rules_and_traits_say_where_they_come_from() {
    // Clusters, divided leaves and flowers in every look, homed at the
    // flowering plants, a herb's leaf lengths at the campanulids, then the
    // juvenile period's and Corner's rules at the seed plants, then the
    // families'; a rule on a look's card names the card.
    let built_in = stdout(&plantc(&["rules"]));
    let flowering = "(clade angiosperms: _ranks/clade/angiosperms.json)";
    let seed_plants = "(clade Spermatophyta: _ranks/clade/spermatophyta.json)";
    let campanulids = "(clade campanulids: _ranks/clade/campanulids.json)";
    assert!(
        built_in.starts_with(&format!(
            "Rules, by their home (362):\n  appearance.organs.*.inflorescence.branches  {flowering}\n"
        )),
        "{built_in}"
    );
    for line in [
        format!(
            "\n  appearance.organs.*.inflorescence.kind  {flowering}\n  appearance.organs.*.inflorescence.pedicel  {flowering}\n  appearance.organs.*.inflorescence.peduncle  {flowering}\n  appearance.organs.*.inflorescence.size  {flowering}\n  appearance.organs.*.season.bud  {flowering}\n"
        ),
        format!(
            "\n  appearance.organs.*.season.ripe  {flowering}\n  appearance.organs.*.shape.anther_tilt on `flower` cards  {flowering}\n  appearance.organs.*.shape.belly on `flower` cards  {flowering}\n"
        ),
        format!(
            "\n  appearance.organs.*.shape.carpels on `flower` cards  {flowering}\n  appearance.organs.*.shape.confluent on `compound` cards  {flowering}\n"
        ),
        format!(
            "\n  appearance.organs.*.shape.turn on `flower` cards  {flowering}\n  appearance.organs.basal.shape.template  {flowering}\n  appearance.organs.bloom.colour  {flowering}\n  appearance.organs.bloom.inflorescence  {flowering}\n  appearance.organs.bloom.shape.template  {flowering}\n"
        ),
        // Cards chosen from characters, and sizes for a growth form as
        // defaults that yield to a lineage's program; a vine's from its
        // leaf length and leaf habit; a stem succulent's
        // from its girth and flowers.
        format!(
            "\n  appearance.organs.leaf.shape.template  {flowering}\n  generator.params.bloom_size  {flowering}\n  generator.params.flower_size  {flowering}\n  generator.params.fruit_length  {flowering}\n  generator.params.fruit_size  {flowering}\n  generator.defaults.crown_base  (clade angiosperms: _ranks/clade/angiosperms.json, forms.decurrent_tree)\n"
        ),
        format!(
            "\n  generator.defaults.path_half  (clade angiosperms: _ranks/clade/angiosperms.json, forms.shrub)\n  generator.defaults.spur_leaf  (clade angiosperms: _ranks/clade/angiosperms.json, forms.shrub)\n  generator.defaults.flower_size  (clade angiosperms: _ranks/clade/angiosperms.json, forms.stem_succulent)\n  generator.defaults.radius_max  (clade angiosperms: _ranks/clade/angiosperms.json, forms.stem_succulent)\n  generator.defaults.leaf_size  (clade angiosperms: _ranks/clade/angiosperms.json, forms.vine)\n  generator.defaults.leaf_years  (clade angiosperms: _ranks/clade/angiosperms.json, forms.vine)\n  generator.params.basal_length  {campanulids}\n  generator.params.leaf_base  {campanulids}\n  generator.params.leaf_length  {campanulids}\n  generator.params.leaf_top  {campanulids}\n"
        ),
        // The monocots' rules sort between the campanulids' and the seed
        // plants'.
        format!(
            "\n  generator.params.adult_age  {seed_plants}\n  generator.params.apex_death  {seed_plants}\n  generator.params.apex_whorl  {seed_plants}\n  generator.params.bole_clear  {seed_plants}\n  generator.params.bud_base  {seed_plants}\n  generator.params.crown_fullness  {seed_plants}\n  generator.params.crown_ratio  {seed_plants}\n  generator.params.crown_widest  {seed_plants}\n  generator.params.dioecious  {seed_plants}\n  generator.params.dormant_buds  {seed_plants}\n  generator.params.flat_top  {seed_plants}\n  generator.params.limb_arch  {seed_plants}\n  generator.params.long_share  {seed_plants}\n  generator.params.room_height  {seed_plants}\n  generator.params.shed_ratio  {seed_plants}\n  generator.params.short_share  {seed_plants}\n  generator.params.space_density  {seed_plants}\n  generator.params.space_kill  {seed_plants}\n  generator.params.space_renew  {seed_plants}\n  generator.params.terminal_bloom  {seed_plants}\n  generator.params.unit_nodes  {seed_plants}\n"
        ),
    ] {
        assert!(built_in.contains(&line), "{line} in {built_in}");
    }
    // A monocot's strap leaves and scape, from its characters.
    assert!(
        built_in.contains(
            "\n  generator.params.basal_pieces  (clade monocots: _ranks/clade/monocots.json)\n  generator.params.scape  (clade monocots: _ranks/clade/monocots.json)\n"
        ),
        "{built_in}"
    );
    // A flower head laid out from its characters, in every look that
    // draws one.
    assert!(
        built_in.contains(
            "\n  appearance.organs.*.shape.disc on `head` cards  (family Asteraceae: asteraceae/family.json)\n  appearance.organs.*.shape.florets on `head` cards  (family Asteraceae: asteraceae/family.json)\n"
        ),
        "{built_in}"
    );
    assert!(
        built_in.contains(
            "\n  appearance.organs.*.shape.rays on `head` cards  (family Asteraceae: asteraceae/family.json)\n"
        ),
        "{built_in}"
    );
    assert!(
        built_in
            .contains("\n  generator.params.leaf_length  (family Poaceae: poaceae/family.json)\n"),
        "{built_in}"
    );
    // Leaf arrangement, homed at the vascular plants.
    assert!(
        built_in.contains(
            "\n  generator.params.leaves_per_node  (phylum Tracheophyta: _ranks/phylum/tracheophyta.json)\n"
        ),
        "{built_in}"
    );
    // Heights: herbs' stems and trees' tops at the vascular plants;
    // culms and spikelets, from their characters, at the Poales.
    let poales = "(order Poales: _ranks/order/poales.json)";
    for line in [
        "\n  generator.params.stem_height  (phylum Tracheophyta: _ranks/phylum/tracheophyta.json)\n".to_string(),
        format!("\n  appearance.organs.*.shape.awn on `spikelet` cards  {poales}\n"),
        format!("\n  appearance.organs.*.shape.ranks on `spikelet` cards  {poales}\n  generator.params.culm_height  {poales}\n"),
    ] {
        assert!(built_in.contains(&line), "{line} in {built_in}");
    }
    // Leaf outlines from shape words, at the kingdom, on three cards; a
    // compound leaf's word is its leaflets'.
    assert!(
        built_in.contains(
            "\n  appearance.organs.leaf.shape.leaflet_width on `compound` cards  (kingdom Plantae: _ranks/kingdom/plantae.json)\n  appearance.organs.leaf.shape.outline on `simple` or `sprig` or `compound` cards  (kingdom Plantae: _ranks/kingdom/plantae.json)\n"
        ),
        "{built_in}"
    );
    assert!(
        built_in.contains("General, with no taxonomic home: 20 rules at all plants;"),
        "{built_in}"
    );
    // The maples' program is named for their family, so it is homed.
    let general = built_in.lines().last().unwrap();
    assert!(general.contains("broadleaf"), "{general}");
    assert!(!general.contains("sapindaceae"), "{general}");

    let dir = scratch("rules");
    let family = dir.join("library/sapindaceae");
    fs::create_dir_all(&family).unwrap();
    fs::write(
        family.join("family.json"),
        r#"{"schema": 1, "rank": "family", "name": "Sapindaceae",
            "parent": "order/sapindales",
            "generator": {"program": "sapindaceae"},
            "evidence": {"generator.program": {"evidence": "Authored",
                    "source": "plantgen-authors", "note": "Maples grow on their family's program."},
                "traits.leaf_arrangement": {"evidence": "Authored",
                    "source": "plantgen-authors", "note": "Maples' leaves are opposite."},
                "rules.generator.params.leaves_per_node": {"evidence": "Authored",
                    "source": "plantgen-authors", "note": "Opposite leaves come in pairs."}},
            "traits": {"leaf_arrangement": "opposite"},
            "rules": {"generator.params.leaves_per_node": {
                "map": {"leaf_arrangement": {"alternate": 1, "opposite": 2}},
                "why": "opposite leaves, one pair at each node"}}}"#,
    )
    .unwrap();
    let library = dir.to_str().unwrap();
    let rules = stdout(&plantc(&["--library", library, "rules"]));
    assert!(
        rules.starts_with("Rules, by their home (363):\n"),
        "{rules}"
    );
    assert!(
        rules.contains(
            "\n  generator.params.leaves_per_node  (family Sapindaceae: sapindaceae/family.json)\n"
        ),
        "{rules}"
    );
    // The maples' genus states their leaves opposite, nearer than the
    // family, and the family's rule reads it.
    let spec = stdout(&plantc(&["--library", library, "spec", "acer-circinatum"]));
    assert!(
        spec.contains("\n  leaf_arrangement = \"opposite\"  (sapindaceae/acer/genus.json)\n"),
        "{spec}"
    );
    assert!(
        spec.contains("\n  generator.params.leaves_per_node  (sapindaceae/family.json): "),
        "{spec}"
    );
}

/// `plantc traits` prints the vocabulary as a tree of organs, and a
/// species' characters with the file and source of each; `plantc audit`
/// counts what each species states of the characters that apply to it.
#[test]
fn traits_and_audit_show_what_is_stated() {
    let vocabulary = stdout(&plantc(&["traits"]));
    for line in [
        "\nCategories:\n  axis: ",
        "\nKinds:\n  floret (organ; disc_floret, ligulate_floret, ray_floret): ",
        "\nOrgans:\n  plant (whole; PO:0000003 whole plant): the whole plant",
        "\n        capitulum (group; PO:0030121 capitulum inflorescence), where inflorescence_unit is capitulum: ",
        "\n          ray_floret (organ, a floret; PO:0025331 ray flower), where ",
        "\n            - ray_florets: a count from 1 to 200\n",
    ] {
        assert!(vocabulary.contains(line), "{line} in {vocabulary}");
    }

    let yarrow = stdout(&plantc(&["traits", "achillea-millefolium"]));
    assert!(
        yarrow.starts_with("achillea-millefolium (Achillea millefolium): states "),
        "{yarrow}"
    );
    let own = "asteraceae/achillea/achillea-millefolium/spec.json";
    for line in [
        format!("\n  ray_florets = 5 to 8 (low 3)  ({own}; fna-19)\n"),
        "\n  inflorescence_unit = capitulum  (asteraceae/family.json; vicflora)\n".to_string(),
        "\n  receptacle = paleate  (asteraceae/achillea/genus.json; fna-19)\n".to_string(),
        "\n  reproduction = flowers  (_ranks/clade/angiosperms.json; openstax-biology-2e)\n"
            .to_string(),
    ] {
        assert!(yarrow.contains(&line), "{line} in {yarrow}");
    }
    // Single flowers, spikelets and cones are not a daisy's.
    let absent = yarrow
        .lines()
        .find_map(|line| line.strip_prefix("organs that do not apply: "))
        .unwrap();
    for organ in ["flower", "spikelet", "cone"] {
        assert!(absent.split(", ").any(|name| name == organ), "{absent}");
    }

    let audit = stdout(&plantc(&["audit"]));
    assert!(audit.contains(" species state "), "{audit}");
    assert!(audit.contains("\n  asteraceae "), "{audit}");
    // Spanish moss has no roots, though the vascular plants it belongs to
    // have them: right, but worth a look.
    assert!(
        audit.contains(
            "\n  bromeliaceae/tillandsia/tillandsia-usneoides/spec.json: `anchorage` is none; \
             _ranks/phylum/tracheophyta.json has roots\n"
        ),
        "{audit}"
    );
    let grasses = stdout(&plantc(&["audit", "Poaceae"]));
    assert!(grasses.starts_with("poaceae: "), "{grasses}");
    assert!(grasses.contains("\n  dactylis-glomerata "), "{grasses}");
    assert!(grasses.contains("\nMost often missing: "), "{grasses}");
    assert!(!plantc(&["audit", "no-such-taxon"]).status.success());
    assert!(!plantc(&["traits", "no-such-species"]).status.success());
}

#[test]
fn a_library_folder_replaces_built_in_species() {
    let dir = scratch("library");
    let folder = dir.join("library/pinaceae/pseudotsuga/pseudotsuga-menziesii");
    fs::create_dir_all(&folder).unwrap();
    fs::write(
        folder.join("spec.json"),
        plantgen::library::source("pseudotsuga-menziesii")
            .replace("\"Douglas-fir\"", "\"Douglas-fir from a folder\""),
    )
    .unwrap();
    let library = dir.to_str().unwrap();
    let listed = stdout(&plantc(&["list", "--library", library]));
    assert!(
        listed.contains("Douglas-fir from a folder (Pseudotsuga menziesii), program `pinidae`, from the library folder"),
        "{listed}"
    );
    assert!(listed.contains("carnegiea-gigantea"), "{listed}");
    let checked = stdout(&plantc(&[
        "--library",
        library,
        "check",
        "pseudotsuga-menziesii",
    ]));
    assert!(
        checked.starts_with("pseudotsuga-menziesii: valid"),
        "{checked}"
    );
    // A family file above it: the spec says where each value came from.
    fs::write(
        dir.join("library/pinaceae/family.json"),
        r#"{"schema": 1, "rank": "family", "name": "Pinaceae",
            "parent": "order/pinales",
            "generator": {"params": {"nod_years": 2.0}},
            "evidence": {"generator.params.nod_years": {"evidence": "Authored",
                "source": "plantgen-authors", "note": "A test of a family's value."}}}"#,
    )
    .unwrap();
    let checked = stdout(&plantc(&[
        "--library",
        library,
        "check",
        "pseudotsuga-menziesii",
    ]));
    assert!(
        checked.contains("\n  stands on pinaceae/pseudotsuga/genus.json, pinaceae/family.json, _ranks/order/pinales.json, "),
        "{checked}"
    );
    let spec = stdout(&plantc(&[
        "--library",
        library,
        "spec",
        "pseudotsuga-menziesii",
    ]));
    let own = "pinaceae/pseudotsuga/pseudotsuga-menziesii/spec.json";
    assert!(
        spec.starts_with(&format!(
            "pseudotsuga-menziesii: {own} on pinaceae/pseudotsuga/genus.json, pinaceae/family.json, _ranks/order/pinales.json, "
        )),
        "{spec}"
    );
    assert!(
        spec.contains("  generator.params.nod_years = 2.0  (pinaceae/family.json)\n"),
        "{spec}"
    );
    assert!(
        spec.contains(&format!("  generator.program = \"pinidae\"  ({own})\n")),
        "{spec}"
    );
    assert!(
        spec.contains(&format!("evidence:\n  allometry  ({own})\n")),
        "{spec}"
    );
    let missing = plantc(&["list", "--library", dir.join("nowhere").to_str().unwrap()]);
    assert!(!missing.status.success());
    assert!(
        String::from_utf8_lossy(&missing.stderr)
            .contains("holds neither a library/ nor a programs/ folder")
    );
}

#[test]
fn grow_says_when_a_plant_has_died() {
    // Under a 30 m canopy the bigleaf maple sheds everything within five
    // years; the sizes would otherwise read as a row of zeros.
    let grown = stdout(&plantc(&[
        "grow",
        "acer-macrophyllum",
        "--env",
        "suppressed",
        "--years",
        "5",
    ]));
    assert!(
        grown.contains("age   5.0  nothing left: the plant has shed every part"),
        "{grown}"
    );
    assert!(!grown.contains("-0"), "{grown}");
}

/// `plantc organs` counts a plant by what its parts are, down to what
/// its organs' layouts draw (spec rework M7B).
#[test]
fn organs_counts_a_plant_by_what_its_parts_are() {
    // A grass: its leaves drawn as pieces, its panicles of spikelets,
    // each of two glumes and five awned florets.
    let fescue = stdout(&plantc(&["organs", "festuca-roemeri", "--age", "2"]));
    for line in [
        "Roemer's fescue (festuca-roemeri), open environment, seed 1: 2 years, day 196\n",
        "\n  leaf ",
        "\n    piece ",
        " per leaf ",
        "`blade` ",
        "\n  inflorescence ",
        "\n    spikelet ",
        "\n      glume ",
        " 2.0 per spikelet\n",
        "\n      spikelet floret ",
        "\n        awn ",
    ] {
        assert!(fescue.contains(line), "{line:?} in {fescue}");
    }
    // Yarrow's corymbs of heads, each of phyllaries, rays and disc florets,
    // the florets open from the rim.
    let yarrow = stdout(&plantc(&["organs", "achillea-millefolium", "--age", "2"]));
    for line in [
        "\n    capitulum ",
        " per inflorescence\n",
        "\n      phyllary ",
        "\n      ray floret ",
        " per capitulum  ",
        "\n      disc floret ",
        " bud, ",
        " open\n",
    ] {
        assert!(yarrow.contains(line), "{line:?} in {yarrow}");
    }
    // Fireweed in July: the lowest of each raceme's flowers in fruit, the
    // top ones open or in bud.
    let fireweed = stdout(&plantc(&[
        "organs",
        "epilobium-angustifolium",
        "--age",
        "2",
        "--day",
        "200",
    ]));
    assert!(fireweed.contains("\n    fruit "), "{fireweed}");
    assert!(fireweed.contains("\n    flower "), "{fireweed}");
    assert!(fireweed.contains(" bud, "), "{fireweed}");
}

#[test]
fn mistakes_fail_with_a_message_and_no_output() {
    for args in [
        &["grow", "no-such-species"][..],
        &["build", "pseudotsuga-menziesii", "--colour", "red"],
        &["frobnicate"],
        &["inspect", "/no/such/package.afterplant"],
    ] {
        let output = plantc(args);
        assert!(!output.status.success(), "{args:?} succeeded");
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(error.starts_with("plantc: "), "{args:?}: {error}");
        assert!(output.stdout.is_empty(), "{args:?}");
    }
}

#[test]
fn build_writes_a_package_once_and_inspect_verifies_it() {
    let dir = scratch("build");
    let spec = small_spec_file(&dir);
    let out = dir.join("plants");
    let spec = spec.to_str().unwrap();
    let out_text = out.to_str().unwrap();
    let args = [
        "build",
        spec,
        "--out",
        out_text,
        "--quality",
        "draft",
        "--threads",
        "2",
    ];
    let built = stdout(&plantc(&args));
    assert!(built.contains("built pseudotsuga-menziesii"), "{built}");
    let packages: Vec<PathBuf> = fs::read_dir(&out)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    assert_eq!(packages.len(), 1);
    let package = &packages[0];
    let name = package.file_name().unwrap().to_str().unwrap();
    assert!(
        name.starts_with("pseudotsuga-menziesii-") && name.ends_with(".afterplant"),
        "{name}"
    );
    // The printed key names the directory.
    let key = built
        .lines()
        .find_map(|line| line.trim().strip_prefix("key "))
        .unwrap();
    assert!(name.contains(&key[..16]));

    let again = stdout(&plantc(&args));
    assert!(again.contains("is already built"), "{again}");
    assert!(again.contains(key));

    let inspected = stdout(&plantc(&["inspect", package.to_str().unwrap()]));
    assert!(
        inspected.contains("every object matches its hash"),
        "{inspected}"
    );
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn render_writes_a_png() {
    let dir = scratch("render");
    let spec = small_spec_file(&dir);
    let image = dir.join("fir.png");
    stdout(&plantc(&[
        "render",
        spec.to_str().unwrap(),
        "--out",
        image.to_str().unwrap(),
        "--age",
        "6",
        "--size",
        "96",
        "--quality",
        "draft",
    ]));
    let bytes = fs::read(&image).unwrap();
    assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn lineup_and_close_ups_write_pngs() {
    let dir = scratch("lineup");
    let image = dir.join("cacti.png");
    let printed = stdout(&plantc(&[
        "lineup",
        "cochemiea-grahamii",
        "opuntia-basilaris",
        "--seeds",
        "2",
        "--age",
        "5",
        "--size",
        "64",
        "--out",
        image.to_str().unwrap(),
    ]));
    // A row of two seeds for each species.
    assert_eq!(
        printed.matches("cochemiea-grahamii").count(),
        3,
        "{printed}"
    );
    assert!(printed.contains("seed 2"), "{printed}");
    let bytes = fs::read(&image).unwrap();
    assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");

    // An old id finds the species renamed from it.
    let close = dir.join("close.png");
    let printed = stdout(&plantc(&[
        "render",
        "mammillaria-grahamii",
        "--age",
        "10",
        "--focus",
        "0,0.05,0",
        "--span",
        "0.1",
        "--size",
        "64",
        "--out",
        close.to_str().unwrap(),
    ]));
    assert!(printed.contains("areoles of solid spines"), "{printed}");
    let failed = plantc(&[
        "render",
        "cochemiea-grahamii",
        "--focus",
        "0,0.05",
        "--out",
        close.to_str().unwrap(),
    ]);
    assert!(!failed.status.success());
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn atlas_draws_the_looks_and_compares_their_areas() {
    let dir = scratch("atlas");
    let image = dir.join("maple.png");
    let printed = stdout(&plantc(&[
        "atlas",
        "acer-macrophyllum",
        "--out",
        image.to_str().unwrap(),
    ]));
    assert!(printed.contains("leaf"), "{printed}");
    assert!(printed.contains("palmate"), "{printed}");
    assert!(
        printed.contains("drawn") && printed.contains("shaded"),
        "{printed}"
    );
    // The built-in species agree, so nothing is flagged.
    assert!(!printed.contains("differs"), "{printed}");
    let bytes = fs::read(&image).unwrap();
    assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");

    // A look for an organ the program does not declare is a mistake.
    let mut spec: serde_json::Value =
        serde_json::from_str(plantgen::spec::builtin_species("acer-macrophyllum").unwrap())
            .unwrap();
    spec["appearance"]["organs"]["petal"] =
        serde_json::json!({ "shape": { "template": "flower" } });
    let path = dir.join("bad.json");
    fs::write(&path, spec.to_string()).unwrap();
    let failed = plantc(&[
        "atlas",
        path.to_str().unwrap(),
        "--out",
        image.to_str().unwrap(),
    ]);
    assert!(!failed.status.success());
    let message = String::from_utf8_lossy(&failed.stderr);
    assert!(message.contains("petal"), "{message}");
    fs::remove_dir_all(&dir).unwrap();
}
