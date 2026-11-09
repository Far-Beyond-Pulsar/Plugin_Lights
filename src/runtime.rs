//! What the lights plugin registers for a game (and the editor): every
//! runtime contribution, grouped by the engine seam it plugs into.

use engine_backend::scene::ObjectJoinRegistration;
use engine_backend::subsystems::render::RenderFeatureRegistration;
use engine_backend::subsystems::render::helio_renderer::ViewIconRegistration;
use helio_component::components::MovabilityRegistration;
use lights_component::{LightComponent, ReflectionCaptureComponent};
use pulsar_settings::project::rendering::SettingsGroupRegistration;

// ── Component classes ───────────────────────────────────────────────────────

pulsar_world_registry::submit_world_component!(LightComponent, ReflectionCaptureComponent);

// ── Rendering ───────────────────────────────────────────────────────────────

// The shadow views, the light binning, deferred and light-field lighting
// and the billboards, configured from the project's shadow settings.
inventory::submit! {
    RenderFeatureRegistration {
        origin: pulsar_plugins::origin!(),
        features: lights_component::render_features,
        configure: lights_component::configure_renderer,
    }
}

// The project's `shadows` settings group.
inventory::submit! {
    SettingsGroupRegistration {
        origin: pulsar_plugins::origin!(),
        group: &helio_light_api::SHADOWS,
    }
}

// The lights' join: the light list, its directional light and the editor's
// light icons, after the meshes' join (a light reads no mesh row, but the
// meshes' join is the group's first by convention).
inventory::submit! {
    ObjectJoinRegistration {
        origin: pulsar_plugins::origin!(),
        name: "lights",
        order: 100,
        owner_bounds: None,
        register_columns: lights_component::register_columns,
        derivation: lights_component::light_join,
    }
}

// A light object's billboard, which picking tests instead of a box.
inventory::submit! {
    ViewIconRegistration {
        origin: pulsar_plugins::origin!(),
        name: "lights.icons",
        icon: lights_component::light_icon,
    }
}

// ── Scene ───────────────────────────────────────────────────────────────────

// What an object's light instances author fixes the object.
inventory::submit! {
    MovabilityRegistration {
        origin: pulsar_plugins::origin!(),
        name: "lights",
        of: |world, instance| {
            world
                .get::<LightComponent>(instance)
                .map(|light| light.general.movability)
        },
    }
}

#[cfg(test)]
mod tests {
    use engine_backend::subsystems::render::helio_renderer::render_features;

    #[test]
    fn registers_its_features() {
        let ids = render_features::plugin_features().ids();
        for id in [
            "helio.lights.shadow_views",
            "helio.lights.binning",
            "helio.lights.deferred",
            "helio.hlfs",
            "helio.billboards",
        ] {
            assert!(ids.contains(&id), "{id} missing from {ids:?}");
        }
    }

    #[test]
    fn registers_no_directional_light_rule_and_its_join() {
        // Directional lights are not a level singleton (several suns).
        assert!(
            !inventory::iter::<engine_backend::scene::level_rules::LevelSingletonRegistration>
                .into_iter()
                .any(|rule| rule.class_name == lights_component::LIGHT_CLASS)
        );
        assert!(
            inventory::iter::<engine_backend::scene::helio_bridge::ObjectJoinRegistration>
                .into_iter()
                .any(|join| join.name == "lights")
        );
    }

    #[test]
    fn registers_the_reflection_capture_under_its_class_name() {
        assert_eq!(
            pulsar_world_registry::component_id_for_class(
                lights_component::REFLECTION_CAPTURE_CLASS_NAME
            ),
            Some(pulsar_scenedb::component_id::<
                lights_component::ReflectionCaptureComponent,
            >())
        );
    }

    /// Every registration of the plugin names this root, so it resolves to
    /// the plugin's id.
    #[test]
    fn every_registration_resolves_to_the_plugin() {
        let ours: Vec<_> = pulsar_plugins::contributions()
            .into_iter()
            .filter(|summary| summary.origin == pulsar_plugins::origin!())
            .collect();
        assert!(
            ours.iter()
                .any(|summary| summary.category == "component.class")
        );
        for summary in &ours {
            assert_eq!(
                pulsar_plugins::plugin_of(summary.origin).map(|plugin| plugin.id.as_str()),
                Some("pulsar.lights"),
                "{summary:?}"
            );
        }
    }
}
