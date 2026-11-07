# Plugin_Lights

Lights and reflection captures: the light list, shadow views, the lighting models, IES profiles and light icons.

`pulsar.lights` ("Lights") is a plugin of
[Pulsar-Native](https://github.com/Far-Beyond-Pulsar/Pulsar-Native), the engine and editor that renders with
[Helio](https://github.com/Far-Beyond-Pulsar/Helio). Pulsar-Native vendors this repository as a git submodule
at `plugins/vendor/lights`.

## What it owns

Every object kind is a plugin. A plugin owns its SceneDB components (their
data and scriptable methods; the editor calls the same methods scripts do),
the contracts other kinds read (its `api` crate: graph keys, row ABIs, WGSL
snippets), the Helio feature that renders it (passes, scene derivations,
settings), its file formats and its editor UI. Helio knows no object kind,
and other kinds meet this one only through frame contracts, extension points,
draw phases and api crates.

| Crate | Path | What |
|---|---|---|
| `helio-light-api` | `api/` | The lights' public contract: the light list and its directional light, the screen-tile binning, the shadow views' matrices, budget, settings and dirty flags, the sun's occlusion pages, and their WGSL (the light row, the forward light function, the shadow atlas). The one crate another kind reads a light concept from. |
| `lights_component` | `component/` | Lights (pulsar.lights): the light and reflection capture components, the light's source row, its join, its editor icon and the registrations of the plugin's Helio features (shadow views, light binning, deferred and light-field lighting, billboards) and settings |
| `lights_plugin` | `editor/` | Lights (pulsar.lights): the editor plugin -- its metadata, over the plugin's component crate |
| `lights-examples` | `examples/` | Lights (pulsar.lights): the examples whose subject is the lights (IES profiles, many lights, light icons), over Helio's presets with the meshes' and the lights' features |
| `helio-pass-billboard` | `feature/billboard/` | Helio pass: camera-facing billboards (light icons) in the overlay phase |
| `helio-pass-light-cull` | `feature/cull/` | Helio render pass: tiled light culling (Forward+ style) |
| `helio-pass-deferred-light` | `feature/deferred/` | Helio pass: the deferred lighting model, over the frame's lighting inputs |
| `helio-pass-hlfs` | `feature/hlfs/` | Helio render pass: Hierarchical Light-Field Sampling (HLFS) |
| `helio-pass-shadow-matrix` | `feature/shadows/` | Helio pass: the shadow views' matrices |
| `helio-light-host` | `host/` | The lights' Helio features and row components for standalone Helio hosts (pulsar.lights): the plugins' examples, tests and demos. Engines register their own components and install the lights' join. |

- **Runtime module** (what a game built for a project links): `lights_component`.
- **Editor module** (what the editor links in addition): `lights_plugin`.
- **Requires**: nothing. A project enables or disables the plugin in its
  `Pulsar.toml` (`[plugins]`); disabled, the kind is absent and the frame
  draws without it.
- **Reads other kinds through**: `helio-mats` (materials).
- **Its standalone hosts also compose**: `helio-mats` (materials); `helio-mesh-api, helio-mesh-host, helio-pass-forward-lit, helio-vg-stream` (meshes) (demos and GPU tests
  draw a whole scene; the plugin itself does not depend on these).

## Building and testing

Like [Plugin_Physics](https://github.com/Far-Beyond-Pulsar/Plugin_Physics), this plugin's crates are members
of Pulsar-Native's workspace: their dependencies are `workspace = true`, and
Helio comes from Pulsar-Native's `crates/renderer/helio` submodule. It builds
only inside a Pulsar-Native checkout:

```bash
git clone --recursive https://github.com/Far-Beyond-Pulsar/Pulsar-Native
cd Pulsar-Native
cargo check -p helio-light-api -p lights_component -p lights_plugin -p lights-examples -p helio-pass-billboard -p helio-pass-light-cull -p helio-pass-deferred-light -p helio-pass-hlfs -p helio-pass-shadow-matrix -p helio-light-host --tests
cargo test -p helio-light-api -p lights_component -p helio-pass-billboard -p helio-pass-light-cull -p helio-pass-deferred-light -p helio-pass-hlfs -p helio-pass-shadow-matrix
```

Suites that render real frames need a GPU; CI leaves them out by name
(Pulsar-Native's `.config/nextest.toml`, `ci` profile). This repository's CI
checks the plugin inside a Pulsar-Native checkout: rustfmt, clippy and the
`ci` profile's tests.

A change that spans this plugin and Pulsar-Native (or Helio) merges
dependency-first: merge here, re-pin `plugins/vendor/lights` in Pulsar-Native to
this repository's `main`, then merge the Pulsar-Native PR (Pulsar-Native's
`.agents/CRATES.md`, "Submodule pins").

## Architecture

- [Every object kind is a plugin](https://github.com/Far-Beyond-Pulsar/Helio/blob/main/docs/architecture/plugin-migration.md): the
  rule, what Helio is, the plugin table and the "As built" notes of each
  phase.
- [The layered rewrite](https://github.com/Far-Beyond-Pulsar/Helio/blob/main/docs/architecture/layered-rewrite.md): layers, buffer
  contracts, extension points and why the renderer has this shape.
- [The `helio-architecture` skill](https://github.com/Far-Beyond-Pulsar/Helio/blob/main/.agents/skills/helio-architecture/SKILL.md): required reading for any change
  here (see `AGENTS.md`).
- Pulsar-Native's `.agents/PLUGIN_SYSTEM.md` and `.agents/CRATES.md`: plugin
  descriptors, the installed set and the submodule pins.

History before 2026-11-02 (when the plugin moved into Pulsar-Native) is in
Helio's repository; this repository's history starts with that move
(`git subtree split` of `plugins/vendor/lights`).

## License

MIT, as the rest of Pulsar (`LICENSE.md`).
