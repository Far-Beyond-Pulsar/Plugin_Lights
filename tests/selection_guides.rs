#![cfg(feature = "editor")]

use engine_backend::scene::{SceneWorldExt, SpawnObject, Transform, attachments};
use engine_backend::subsystems::render::EditorLinesRegistration;
use glam::DVec3;
use lights_plugin::lights_component::{LightComponent, LightType};
use pulsar_scenedb::World;

#[test]
fn guides_follow_multi_selection_and_clear_on_deselect() {
    let mut world = World::new();
    let parent = world
        .spawn_object(SpawnObject::new("parent").with_transform(Transform {
            position: [1.0e9, 0.0, 0.0],
            ..Default::default()
        }))
        .unwrap();
    let mut objects = Vec::new();
    for (offset, kind) in [(5.0, LightType::Point), (20.0, LightType::Spot)] {
        let object = world
            .spawn_object(
                SpawnObject::new("light")
                    .with_parent(Some(parent))
                    .with_transform(Transform {
                        position: [offset, 0.0, 0.0],
                        scale: [2.0; 3],
                        ..Default::default()
                    }),
            )
            .unwrap();
        let mut light = LightComponent::default();
        light.general.light_type = kind;
        light.attenuation.range = 4.0;
        pulsar_world_registry::attach_value(&mut world, object, light).unwrap();
        objects.push(object);
    }
    let registration = pulsar_plugins::registered::<EditorLinesRegistration>()
        .find(|registration| registration.name == "lights.selection_guides")
        .expect("lights register their editor guides");
    let mut guides = (registration.new)(&world);
    assert!(guides.poll(&world).is_none());

    world.select_all(&objects);
    let lines = guides.poll(&world).expect("both lights selected");
    for x in [1.0e9 + 5.0, 1.0e9 + 20.0] {
        let center = DVec3::new(x, 0.0, 0.0);
        assert!(lines.iter().any(|v| v.position == center + DVec3::X * 4.0));
    }
    let spot_center = DVec3::new(1.0e9 + 20.0, 0.0, 0.0);
    assert!(
        lines.iter().any(|v| v.position == spot_center),
        "spot cone apex"
    );
    assert!(
        guides.poll(&world).is_none(),
        "keep the existing line layer"
    );

    world.select(Some(objects[0]));
    let lines = guides.poll(&world).expect("second guide removed");
    assert!(lines.iter().all(|v| v.position.x <= 1.0e9 + 9.0));
    let instance = attachments::instances(&world, objects[0])[0];
    world
        .get_mut::<LightComponent>(instance)
        .unwrap()
        .attenuation
        .range = 6.0;
    let lines = guides.poll(&world).expect("the range changed");
    assert!(lines.iter().any(|v| v.position.x == 1.0e9 + 11.0));
    world.select(None);
    assert!(guides.poll(&world).expect("clear the layer").is_empty());
}
