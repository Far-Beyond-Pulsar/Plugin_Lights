//! Lights (`pulsar.lights`): the plugin's root crate, the one place the
//! plugin registers everything it contributes.
//!
//! The plugin's other crates hold the code: `helio-light-api` (the
//! contracts other kinds read: the light list, the shadow settings), the
//! feature crates (`helio-pass-*`: shadow views, light binning, deferred and
//! light-field lighting, billboards), `helio-light-host`, `lights_component`
//! (the light and reflection capture components, the light's source row and
//! the lights' join) and `lights_editor` (the level editor UI). None of them
//! registers a contribution; this crate imports them and registers them through its
//! [`build`], so this file and the two modules below are the whole list
//! of what the plugin brings:
//!
//! - [`runtime`]: what a game links. The component classes, the Helio
//!   features and their shadow settings group, the lights' scene join, the
//!   editor icon picking tests, and the movability a light authors.
//! - `editor` (with the `editor` feature): what the editor adds. The
//!   hierarchy icon, the Create menu's lights, the environment mixer's
//!   lights and reflection captures, and the starter setups (clear day,
//!   golden hour, overcast).
//!
//! Every registration names this crate as its origin
//! (`p.origin()`), so the plugin gate skips all of them while
//! a project disables the plugin, and the plugin catalog lists them under
//! `pulsar.lights`. Other plugins never depend on this crate (only on
//! `helio-light-api`); hosts reach it through the installed set or a
//! generated game, a game without the `editor` feature.

// The plugin's descriptor (`pulsar-plugin.toml`): linking this crate
// installs the plugin; a project enables it or not (`pulsar_plugins`).
pulsar_plugins::installed!("pulsar-plugin.toml", build);

/// Everything the plugin registers (runtime always, editor with the `editor`
/// feature).
pub fn build(p: &mut pulsar_plugins::PluginBuilder) {
    runtime::build(p);
    #[cfg(feature = "editor")]
    editor::build(p);
}

pub use lights_component;
#[cfg(feature = "editor")]
pub use lights_editor;

pub mod runtime;

#[cfg(feature = "editor")]
pub mod editor;
