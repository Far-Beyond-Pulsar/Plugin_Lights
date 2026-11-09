//! Lights (`pulsar.lights`): the editor crate -- the level editor's light
//! icon, Create menu entries, environment mixer entries and starter setups
//! ([`level_editor`]). It registers nothing itself: the plugin's root crate
//! (`lights_plugin`, with its `editor` feature) submits these through the
//! level editor's extension points.

// The level editor UI's text (`locales/`).
rust_i18n::i18n!("locales", fallback = "en");

pub mod level_editor;
