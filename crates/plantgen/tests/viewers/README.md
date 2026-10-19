# Viewer conformance: bark in other renderers

PlantGen bakes bark into glTF's standard metallic-roughness material
(`plantgen::bark_look::glb`, `plantc bark`). This check makes sure other
renderers draw those files as PlantGen means them. It is not part of
`cargo test` because it needs Node and a browser.

```sh
cd crates/plantgen/tests/viewers
npm ci
cargo build --release -p plantc
../../../../target/release/plantc bark --probe up --out /tmp/glb --size 256 --period 0.25
../../../../target/release/plantc bark --probe round --out /tmp/glb --size 256 --period 0.25
../../../../target/release/plantc bark --look RECORD.json --out /tmp/glb
node run.mjs /tmp/glb [--out DIR] [--chromium PATH]
```

The bark records are in jferments/plantgen-claude, under
`project-files/bark/records/`. Renders and `report.json` go to `out/`.
The runner exits 1 when any check fails.

What it checks:

1. **The Khronos glTF validator** (`gltf-validator`) finds no errors in
   any file.
2. **Normal maps read the right way round.** Each probe is a metre of grey
   trunk with sine-wave relief, either up the stem (`probe-up`) or round
   it (`probe-round`). three.js and Babylon.js draw it in headless
   Chromium (`page.html`) under one light. The shading must follow the
   relief as Lambert's law says, with a correlation of at least 0.95. A
   map read upside down or mirrored, or a tangent pointing the wrong way,
   turns the correlation negative. The mean brightness must also be 0.9 to
   1.1 of Lambert's, which catches colour read in the wrong colour space.
3. **Bark looks the same in both renderers.** Over the trunk, three.js and
   Babylon.js may differ by at most 6 sRGB levels a pixel on average. Their
   mean colours may differ by at most ΔE 3 (CIE76).

The light units differ between the renderers. three.js needs intensity π
and Babylon.js needs π as well, because its PBR lights divide by π. The
probes measured both at 0.98 to 1.00 of Lambert's.

First results (2026-10-19, all 11 reference species):

- The validator found no errors or warnings in any file.
- The probes reached a correlation of 0.998 to 0.999 in both renderers.
- Mean pixel differences between the renderers were 0.8 to 4.6 levels.
- Mean colours differed by ΔE 0.1 to 3.2.
- The one failure is the bitter cherry twig, at ΔE 3.2. It is glossy
  (roughness 0.35), so the renderers' specular terms differ most there.

The check found two real bugs, both now fixed:

- The trunk's triangles faced inward.
- Its tangents ran against the texture coordinates, which flipped both
  axes of the normal map in both renderers.

Not covered yet:

- Blender and the Khronos sample viewer (neither is in the cloud
  container).
- KTX2 textures with our own mipmaps. PNG holds no mipmaps, so renderers
  make their own.
- Bark on whole exported plants. That needs bark texture coordinates on
  the wood mesh.
