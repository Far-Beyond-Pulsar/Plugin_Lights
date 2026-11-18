//! World-space guides for the lights attached to the editor selection.

use engine_backend::scene::{SceneWorldExt as _, attachments, hierarchy_transform};
use engine_backend::subsystems::render::EditorLines;
use glam::{DVec3, Vec3};
use helio::{DebugBatch, WorldVertex};
use lights_component::{LOCAL_LIGHT_DIRECTION, LightComponent, LightType};
use pulsar_scenedb::World;

const RANGE_COLOR: [f32; 4] = [0.35, 0.73, 1.0, 0.75];
const OUTER_COLOR: [f32; 4] = [1.0, 0.65, 0.18, 0.9];
const INNER_COLOR: [f32; 4] = [1.0, 0.9, 0.5, 0.85];
const SOURCE_COLOR: [f32; 4] = [1.0, 0.88, 0.28, 1.0];
const SEGMENTS: u32 = 32;

#[derive(Default)]
pub struct SelectionGuides {
    lights: Vec<LightGuide>,
}

#[derive(PartialEq)]
struct LightGuide {
    center: DVec3,
    axis: Vec3,
    kind: LightType,
    range: f32,
    inner_angle: f32,
    outer_angle: f32,
    source_radius: f32,
    source_length: f32,
}

impl EditorLines for SelectionGuides {
    fn poll(&mut self, world: &World) -> Option<Vec<WorldVertex>> {
        let mut lights = Vec::new();
        for object in world.selection() {
            let Some(mut pose) = hierarchy_transform::world_transform_of(world, object) else {
                continue;
            };
            let center = DVec3::from_array(pose.position);
            if !center.is_finite() || !pose.rotation.iter().all(|angle| angle.is_finite()) {
                continue;
            }
            // The GPU join rotates the emission axis. Scale never changes
            // the light's authored radius, angles, or source dimensions.
            pose.scale = [1.0; 3];
            let axis = hierarchy_transform::to_matrix(&pose)
                .transform_vector3(Vec3::from_array(LOCAL_LIGHT_DIRECTION).as_dvec3())
                .as_vec3();
            for instance in attachments::instances(world, object) {
                let Some(light) = world.get::<LightComponent>(instance) else {
                    continue;
                };
                let attenuation = &light.attenuation;
                let outer_angle = positive(attenuation.outer_cone_angle).min(90.0);
                lights.push(LightGuide {
                    center,
                    axis,
                    kind: light.general.light_type,
                    range: positive(attenuation.range),
                    inner_angle: positive(attenuation.inner_cone_angle).min(outer_angle),
                    outer_angle,
                    source_radius: positive(attenuation.source_radius),
                    source_length: positive(attenuation.source_length),
                });
            }
        }
        if self.lights == lights {
            return None;
        }
        self.lights = lights;
        Some(DebugBatch::collect_lines(|draw| {
            for light in &self.lights {
                light.draw(draw);
            }
        }))
    }
}

fn positive(value: f32) -> f32 {
    if value.is_finite() {
        value.max(0.0)
    } else {
        0.0
    }
}

impl LightGuide {
    fn draw(&self, draw: &mut DebugBatch<'_>) {
        let axis = self.axis.to_array();
        if self.kind == LightType::Directional {
            // A fixed-size direction marker for an unbounded light.
            let tip = self.center + self.axis.as_dvec3();
            draw.line(self.center, tip, SOURCE_COLOR);
            draw.cone(tip, (-self.axis).to_array(), 0.25, 0.1, SOURCE_COLOR, 8);
            return;
        }
        if self.range > 0.0 {
            draw.sphere(self.center, self.range, RANGE_COLOR, SEGMENTS);
            if self.kind == LightType::Spot {
                self.draw_cone(draw, self.outer_angle, OUTER_COLOR);
                if self.inner_angle < self.outer_angle {
                    self.draw_cone(draw, self.inner_angle, INNER_COLOR);
                }
            }
        }
        if self.source_length > 0.0 {
            let half = self.axis.as_dvec3() * (0.5 * f64::from(self.source_length));
            let base = self.center - half;
            if self.source_radius > 0.0 {
                draw.cylinder(
                    base,
                    axis,
                    self.source_length,
                    self.source_radius,
                    SOURCE_COLOR,
                    16,
                );
            } else {
                draw.line(base, self.center + half, SOURCE_COLOR);
            }
        } else if self.source_radius > 0.0 {
            draw.sphere(self.center, self.source_radius, SOURCE_COLOR, 16);
        }
    }

    fn draw_cone(&self, draw: &mut DebugBatch<'_>, degrees: f32, color: [f32; 4]) {
        if degrees == 0.0 {
            draw.line(
                self.center,
                self.center + self.axis.as_dvec3() * f64::from(self.range),
                color,
            );
            return;
        }
        // Every point on the rim is one attenuation radius from the source.
        let (sin, cos) = degrees.to_radians().sin_cos();
        draw.cone(
            self.center,
            self.axis.to_array(),
            self.range * cos.max(0.0),
            self.range * sin,
            color,
            SEGMENTS,
        );
    }
}
