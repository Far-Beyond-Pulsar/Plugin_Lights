//! Deferred light bins for pixels drawn through a rigid frame remap.

use helio_core::gpu_cache::GpuCache;
use helio_core::graph::ResourceBuilder;
use helio_core::{PassContext, RenderPass, ResourceKey, Result};
use helio_frame_api::{
    gbuffer,
    lighting::{VIEW_REMAPS, VIEW_REMAP_SPACES},
};
use helio_light_api::{
    ACTIVE_LIGHT_INDICES, LIGHT_LIST_SNIPPET, MAX_LIGHTS_PER_TILE, REMAPPED_TILE_LIGHTS,
    SCENE_LIGHTS, TILE_SIZE,
};

fn binding(binding: u32, buffer: &wgpu::Buffer) -> wgpu::BindGroupEntry<'_> {
    wgpu::BindGroupEntry {
        binding,
        resource: buffer.as_entire_binding(),
    }
}

pub(crate) struct RemappedLightTiles {
    pipeline: wgpu::ComputePipeline,
    layout: wgpu::BindGroupLayout,
    cache: GpuCache,
    tiles: Option<wgpu::Buffer>,
    live: bool,
}

impl RemappedLightTiles {
    pub(crate) fn new(device: &wgpu::Device) -> Self {
        let module = helio_core::shader::module_with(
            device,
            "Remapped light tiles",
            helio_core::include_wgsl!("../shaders/remapped_tiles.wgsl"),
            &[LIGHT_LIST_SNIPPET],
        );
        let pipeline = helio_core::shader::create_compute_pipeline(
            device,
            wgpu::ComputePipelineDescriptor {
                label: Some("Remapped light tiles"),
                layout: None,
                module: &module,
                entry_point: Some("bin_remapped_tiles"),
                compilation_options: Default::default(),
                cache: None,
            },
        );
        Self {
            layout: pipeline.get_bind_group_layout(0),
            pipeline,
            cache: GpuCache::new(),
            tiles: None,
            live: false,
        }
    }
}

impl RenderPass for RemappedLightTiles {
    fn name(&self) -> &'static str {
        "RemappedLightTiles"
    }

    fn scene_buffers(&self) -> &'static [&'static str] {
        &[helio_light_api::LIGHT_LIST]
    }

    fn declare_resources(&self, builder: &mut ResourceBuilder) {
        builder.read(gbuffer::SURFACE);
        builder.read(gbuffer::DEPTH);
        builder.read_optional(ACTIVE_LIGHT_INDICES);
        builder.read_optional(VIEW_REMAPS);
        builder.read_optional(VIEW_REMAP_SPACES);
        builder.write_buffer(REMAPPED_TILE_LIGHTS);
    }

    fn render_pass_descriptor<'a>(
        &'a self,
        _target: &'a wgpu::TextureView,
        _depth: &'a wgpu::TextureView,
        _resources: &'a helio_core::ResourceRegistry<'a>,
    ) -> Option<wgpu::RenderPassDescriptor<'a>> {
        None
    }

    fn execute(&mut self, ctx: &mut PassContext) -> Result<()> {
        self.live = false;
        let (Some(remaps), Some(spaces), Some(active), Some(lights), Some(surface)) = (
            ctx.registry.buffer_binding(VIEW_REMAPS),
            ctx.registry.buffer_binding(VIEW_REMAP_SPACES),
            ctx.registry
                .get::<&wgpu::Buffer>(ResourceKey::new(ACTIVE_LIGHT_INDICES)),
            ctx.scene_buffers.get(SCENE_LIGHTS.key()),
            ctx.registry
                .get::<&wgpu::TextureView>(ResourceKey::new(gbuffer::SURFACE)),
        ) else {
            return Ok(());
        };
        let x = ctx.width.div_ceil(TILE_SIZE);
        let y = ctx.height.div_ceil(TILE_SIZE);
        let bytes = u64::from(x) * u64::from(y) * u64::from(MAX_LIGHTS_PER_TILE + 1) * 4;
        if bytes == 0 || bytes > u64::from(ctx.device.limits().max_storage_buffer_binding_size) {
            return Ok(());
        }
        if self
            .tiles
            .as_ref()
            .is_none_or(|buffer| buffer.size() < bytes)
        {
            self.tiles = Some(ctx.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Remapped light tiles"),
                size: bytes,
                usage: wgpu::BufferUsages::STORAGE,
                mapped_at_creation: false,
            }));
        }
        let tiles = self.tiles.as_ref().expect("light tiles allocated");
        let texture = |binding, view| wgpu::BindGroupEntry {
            binding,
            resource: wgpu::BindingResource::TextureView(view),
        };
        self.cache.end_frame();
        let group = self.cache.bind_group(
            ctx.device,
            &wgpu::BindGroupDescriptor {
                label: Some("Remapped light tiles"),
                layout: &self.layout,
                entries: &[
                    binding(0, ctx.camera),
                    binding(1, &lights.buffer),
                    binding(2, active),
                    texture(3, surface),
                    texture(4, ctx.depth),
                    binding(5, remaps),
                    binding(6, spaces),
                    binding(7, tiles),
                ],
            },
        );
        let mut cmds = ctx.graphics_cmds();
        let mut pass = cmds.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("Remapped light tiles"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &group, &[]);
        pass.dispatch_workgroups(x, y, 1);
        self.live = true;
        Ok(())
    }

    fn publish<'a>(&self, frame: &mut helio_core::ResourceRegistry<'a>) {
        if self.live {
            if let Some(tiles) = &self.tiles {
                frame.write_buffer_binding(REMAPPED_TILE_LIGHTS, tiles, "RemappedLightTiles");
            }
        }
    }
}
