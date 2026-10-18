//! **Port-only: shared parts of the modern full-screen passes** (crate::ao_render, crate::sun_shadow, crate::sun_rays,
//! crate::scene_copy): the view uniform [`FxView`] and the `rerac::fx_view` WGSL module (`assets/shaders/fx_view.wgsl`:
//! positions and normals rebuilt from the scene depth), and the bind-group entries for the view's depth texture with or
//! without multisampling.

use bevy::core_pipeline::FullscreenShader;
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use bevy::render::camera::ExtractedCamera;
use bevy::render::render_resource::binding_types::{texture_depth_2d, texture_depth_2d_multisampled};
use bevy::render::render_resource::encase::UniformBuffer;
use bevy::render::render_resource::{
    BindGroupLayoutDescriptor, BindGroupLayoutEntryBuilder, BlendComponent, BlendFactor, BlendOperation, BlendState, Buffer,
    BufferInitDescriptor, BufferUsages, CachedRenderPipelineId, ColorTargetState, ColorWrites, FragmentState, MultisampleState,
    PipelineCache, RenderPipeline, RenderPipelineDescriptor, ShaderType, TextureFormat, VertexState,
};
use bevy::render::renderer::RenderDevice;
use bevy::render::view::{ExtractedView, ViewTarget};
use bevy::shader::{Shader, ShaderDefVal};
use std::hash::Hash;
use std::sync::Mutex;

const IMPORT_PATH: &str = "shaders/fx_view.wgsl";

/// `rerac::fx_view::FxView`.
#[derive(Clone, Copy, Debug, Default, ShaderType)]
pub struct FxView {
    pub clip_from_view: Mat4,
    pub view_from_clip: Mat4,
    pub world_from_view: Mat4,
    pub view_from_world: Mat4,
    /// The camera's viewport in target pixels: x, y, width, height.
    pub viewport: Vec4,
    /// The pass's own values.
    pub params: Vec4,
    pub extra: Vec4,
}

impl FxView {
    /// The view of a render-world camera (None before it has a target size).
    pub fn of(cam: &ExtractedCamera, view: &ExtractedView) -> Option<Self> {
        let size = cam.physical_target_size?;
        let (pos, vp) = cam.viewport.as_ref().map_or((UVec2::ZERO, size), |v| (v.physical_position, v.physical_size));
        let world_from_view = view.world_from_view.to_matrix();
        Some(FxView {
            clip_from_view: view.clip_from_view,
            view_from_clip: view.clip_from_view.inverse(),
            world_from_view,
            view_from_world: world_from_view.inverse(),
            viewport: Vec4::new(pos.x as f32, pos.y as f32, vp.x.max(1) as f32, vp.y.max(1) as f32),
            params: Vec4::ZERO,
            extra: Vec4::ZERO,
        })
    }

    /// A one-frame uniform buffer of these values.
    pub fn buffer(&self, device: &RenderDevice, label: &'static str) -> Buffer { uniform_buffer(device, label, self) }
}

/// A uniform buffer holding `value` (std140 layout).
pub fn uniform_buffer<T: ShaderType + bevy::render::render_resource::encase::internal::WriteInto>(device: &RenderDevice, label: &'static str, value: &T) -> Buffer {
    let mut bytes = UniformBuffer::new(Vec::<u8>::new());
    bytes.write(value).expect("uniform layout");
    device.create_buffer_with_data(&BufferInitDescriptor { label: Some(label), contents: bytes.as_ref(), usage: BufferUsages::UNIFORM })
}

/// The bind-group layout entry of the view's depth texture (`texture_depth_2d`, or `texture_depth_multisampled_2d`
/// with MSAA).
pub fn depth_entry(multisampled: bool) -> BindGroupLayoutEntryBuilder {
    if multisampled { texture_depth_2d_multisampled() } else { texture_depth_2d() }
}

/// The shader defs of a pass reading the view's depth (`MULTISAMPLED` with MSAA).
pub fn depth_defs(multisampled: bool) -> Vec<ShaderDefVal> { if multisampled { vec!["MULTISAMPLED".into()] } else { vec![] } }

/// `dst · src` on colour, the destination alpha kept: a pass's darkening factor.
pub const MULTIPLY: BlendState = BlendState {
    color: BlendComponent { src_factor: BlendFactor::Zero, dst_factor: BlendFactor::Src, operation: BlendOperation::Add },
    alpha: BlendComponent { src_factor: BlendFactor::Zero, dst_factor: BlendFactor::One, operation: BlendOperation::Add },
};

/// One full-screen draw of a pass: its fragment entry point, the target and the blend.
pub struct Fullscreen {
    pub label: &'static str,
    pub layout: BindGroupLayoutDescriptor,
    pub shader: Handle<Shader>,
    pub entry: &'static str,
    pub defs: Vec<ShaderDefVal>,
    pub format: TextureFormat,
    pub blend: Option<BlendState>,
    pub samples: u32,
}

/// The cached full-screen pipelines of a pass, by a key of its own (queued on first use).
pub struct Pipelines<K> {
    pub vertex: VertexState,
    ids: Mutex<HashMap<K, CachedRenderPipelineId>>,
}

impl<K: Eq + Hash> Pipelines<K> {
    pub fn new(fullscreen: &FullscreenShader) -> Self { Pipelines { vertex: fullscreen.to_vertex_state(), ids: Default::default() } }

    pub fn id(&self, cache: &PipelineCache, key: K, make: impl FnOnce() -> Fullscreen) -> CachedRenderPipelineId {
        let mut ids = self.ids.lock().unwrap_or_else(|e| e.into_inner());
        *ids.entry(key).or_insert_with(|| {
            let f = make();
            cache.queue_render_pipeline(RenderPipelineDescriptor {
                label: Some(f.label.into()),
                layout: vec![f.layout],
                vertex: self.vertex.clone(),
                fragment: Some(FragmentState {
                    shader: f.shader,
                    shader_defs: f.defs,
                    entry_point: Some(f.entry.into()),
                    targets: vec![Some(ColorTargetState { format: f.format, blend: f.blend, write_mask: ColorWrites::ALL })],
                }),
                multisample: MultisampleState { count: f.samples, ..default() },
                ..default()
            })
        })
    }

    /// The ready pipeline (None while it compiles).
    pub fn get<'c>(&self, cache: &'c PipelineCache, key: K, make: impl FnOnce() -> Fullscreen) -> Option<&'c RenderPipeline> {
        cache.get_render_pipeline(self.id(cache, key, make))
    }
}

/// The view's colour samples and format (the targets of a pass drawing into it).
pub fn target_of(target: &ViewTarget) -> (u32, TextureFormat) {
    (target.sampled_main_texture().map_or(1, |t| t.sample_count()), target.main_texture_format())
}

/// Keeps the `rerac::fx_view` WGSL module loaded (the passes import it by its path).
#[derive(Resource)]
struct FxViewShader(#[allow(dead_code)] Handle<Shader>);

pub struct FxViewPlugin;

impl Plugin for FxViewPlugin {
    fn build(&self, app: &mut App) {
        let import = app.world().resource::<AssetServer>().load(IMPORT_PATH);
        app.insert_resource(FxViewShader(import));
    }
}
