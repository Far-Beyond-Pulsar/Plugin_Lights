//! What the lights plugin registers for the editor (the `editor` feature):
//! its level editor entries.

use lights_editor::level_editor;
use ui_level_editor::extend::{
    CreateEntry, EnvironmentListing, EnvironmentPreset, ObjectIconRegistration,
};

// ── Hierarchy ───────────────────────────────────────────────────────────────

// An object holding a light shows as one.
inventory::submit! {
    ObjectIconRegistration {
        origin: pulsar_plugins::origin!(),
        id: "lights",
        order: 100,
        icon: level_editor::object_icon,
    }
}

// ── Create menu ─────────────────────────────────────────────────────────────

/// The Create menu's lights ([`level_editor::MENU_LIGHTS`]).
macro_rules! menu_lights {
    ($($index:literal),*) => {$(
        inventory::submit! {
            CreateEntry {
                origin: pulsar_plugins::origin!(),
                id: level_editor::MENU_LIGHTS[$index].id,
                menu_label: Some(level_editor::MENU_LIGHTS[$index].menu_label),
                environment_label: None,
                quick_add: None,
                offered: None,
                order: level_editor::MENU_LIGHTS[$index].order,
                create: level_editor::MENU_LIGHTS[$index].create,
            }
        }
    )*};
}

menu_lights!(0, 1, 2);

// ── Environment mixer ───────────────────────────────────────────────────────

inventory::submit! {
    CreateEntry {
        origin: pulsar_plugins::origin!(),
        id: "environment_point_light",
        menu_label: None,
        environment_label: Some("Point Light"),
        quick_add: None,
        offered: None,
        order: 400,
        create: level_editor::add_environment_point_light,
    }
}

inventory::submit! {
    CreateEntry {
        origin: pulsar_plugins::origin!(),
        id: "environment_spot_light",
        menu_label: None,
        environment_label: Some("Spot Light"),
        quick_add: None,
        offered: None,
        order: 401,
        create: level_editor::add_environment_spot_light,
    }
}

// Offered while the level has no sun.
inventory::submit! {
    CreateEntry {
        origin: pulsar_plugins::origin!(),
        id: "environment_directional_light",
        menu_label: None,
        environment_label: Some("Directional Light"),
        quick_add: None,
        offered: Some(level_editor::no_sun),
        order: 402,
        create: level_editor::add_environment_directional_light,
    }
}

inventory::submit! {
    CreateEntry {
        origin: pulsar_plugins::origin!(),
        id: "environment_reflection_capture",
        menu_label: None,
        environment_label: Some("Reflection Capture"),
        quick_add: None,
        offered: None,
        order: 440,
        create: level_editor::add_reflection_capture,
    }
}

// The mixer lists the level's lights and reflection captures.
inventory::submit! {
    EnvironmentListing {
        origin: pulsar_plugins::origin!(),
        class_name: "LightComponent",
        label: "Light",
        order: 0,
        create_entry: Some("environment_point_light"),
    }
}

inventory::submit! {
    EnvironmentListing {
        origin: pulsar_plugins::origin!(),
        class_name: "ReflectionCaptureComponent",
        label: "Reflection Capture",
        order: 40,
        create_entry: Some("environment_reflection_capture"),
    }
}

// ── Starter setups ──────────────────────────────────────────────────────────

// A sky rig (a sky, when the project has one to add, with a sun and a moon)
// and a tuned sun.
inventory::submit! {
    EnvironmentPreset {
        origin: pulsar_plugins::origin!(),
        id: "clear_day",
        label: "Clear Day",
        order: 0,
        apply: level_editor::apply_clear_day,
    }
}

inventory::submit! {
    EnvironmentPreset {
        origin: pulsar_plugins::origin!(),
        id: "golden_hour",
        label: "Golden Hour",
        order: 10,
        apply: level_editor::apply_golden_hour,
    }
}

inventory::submit! {
    EnvironmentPreset {
        origin: pulsar_plugins::origin!(),
        id: "overcast",
        label: "Overcast",
        order: 20,
        apply: level_editor::apply_overcast,
    }
}
