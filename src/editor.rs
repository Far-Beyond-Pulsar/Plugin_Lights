//! What the lights plugin registers for the editor (the `editor` feature):
//! its level editor entries.

use lights_editor::level_editor;
use ui_level_editor::extend::{
    CreateEntry, EnvironmentListing, EnvironmentPreset, ObjectIconRegistration,
};

/// The Create menu's lights ([`level_editor::MENU_LIGHTS`]).
macro_rules! menu_lights {
    ($p:expr; $($index:literal),*) => {$(
        $p.add(CreateEntry {
            origin: $p.origin(),
            id: level_editor::MENU_LIGHTS[$index].id,
            menu_label: Some(level_editor::MENU_LIGHTS[$index].menu_label),
            environment_label: None,
            quick_add: None,
            offered: None,
            order: level_editor::MENU_LIGHTS[$index].order,
            create: level_editor::MENU_LIGHTS[$index].create,
        });
    )*};
}

/// Everything the plugin registers for the editor.
pub(crate) fn build(p: &mut pulsar_plugins::PluginBuilder) {
    // ── Hierarchy ───────────────────────────────────────────────────────────

    // An object holding a light shows as one.
    p.add(ObjectIconRegistration {
        origin: p.origin(),
        id: "lights",
        order: 100,
        icon: level_editor::object_icon,
    });

    // ── Create menu ─────────────────────────────────────────────────────────

    menu_lights!(p; 0, 1, 2);

    // ── Environment mixer ───────────────────────────────────────────────────

    p.add(CreateEntry {
        origin: p.origin(),
        id: "environment_point_light",
        menu_label: None,
        environment_label: Some("Point Light"),
        quick_add: None,
        offered: None,
        order: 400,
        create: level_editor::add_environment_point_light,
    });

    p.add(CreateEntry {
        origin: p.origin(),
        id: "environment_spot_light",
        menu_label: None,
        environment_label: Some("Spot Light"),
        quick_add: None,
        offered: None,
        order: 401,
        create: level_editor::add_environment_spot_light,
    });

    // Offered while the level has no sun.
    p.add(CreateEntry {
        origin: p.origin(),
        id: "environment_directional_light",
        menu_label: None,
        environment_label: Some("Directional Light"),
        quick_add: None,
        offered: Some(level_editor::no_sun),
        order: 402,
        create: level_editor::add_environment_directional_light,
    });

    p.add(CreateEntry {
        origin: p.origin(),
        id: "environment_reflection_capture",
        menu_label: None,
        environment_label: Some("Reflection Capture"),
        quick_add: None,
        offered: None,
        order: 440,
        create: level_editor::add_reflection_capture,
    });

    // The mixer lists the level's lights and reflection captures.
    p.add(EnvironmentListing {
        origin: p.origin(),
        class_name: "LightComponent",
        label: "Light",
        order: 0,
        create_entry: Some("environment_point_light"),
    });

    p.add(EnvironmentListing {
        origin: p.origin(),
        class_name: "ReflectionCaptureComponent",
        label: "Reflection Capture",
        order: 40,
        create_entry: Some("environment_reflection_capture"),
    });

    // ── Starter setups ──────────────────────────────────────────────────────

    // A sky rig (a sky, when the project has one to add, with a sun and a moon)
    // and a tuned sun.
    p.add(EnvironmentPreset {
        origin: p.origin(),
        id: "clear_day",
        label: "Clear Day",
        order: 0,
        apply: level_editor::apply_clear_day,
    });

    p.add(EnvironmentPreset {
        origin: p.origin(),
        id: "golden_hour",
        label: "Golden Hour",
        order: 10,
        apply: level_editor::apply_golden_hour,
    });

    p.add(EnvironmentPreset {
        origin: p.origin(),
        id: "overcast",
        label: "Overcast",
        order: 20,
        apply: level_editor::apply_overcast,
    });
}
