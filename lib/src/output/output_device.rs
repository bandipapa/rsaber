use std::borrow::Cow;
use std::cell::RefCell;
use std::hash::{Hash, Hasher};
use std::num::NonZeroU32;
use std::rc::Rc;

use hashbrown::{Equivalent, HashMap};
use ordered_float::OrderedFloat;
use wgpu::{AddressMode, BindGroup, BindGroupDescriptor, BindGroupLayout, BindGroupLayoutDescriptor, BindGroupLayoutEntry, Buffer, BufferAddress, BufferDescriptor, BufferSize, ColorTargetState, CommandBuffer, CommandEncoder, CommandEncoderDescriptor, CompareFunction, DepthStencilState, Device, Extent3d, FilterMode, FragmentState, MipmapFilterMode, MultisampleState, PipelineLayout, PipelineLayoutDescriptor, PrimitiveState, QuerySet, QuerySetDescriptor, Queue, QueueWriteBufferView, RenderPipeline, RenderPipelineDescriptor, Sampler, SamplerBorderColor, SamplerDescriptor, ShaderModule, ShaderModuleDescriptor, ShaderSource, SubmissionIndex, TexelCopyBufferLayout, TexelCopyTextureInfo, Texture, TextureDescriptor, TextureDimension, TextureFormat, TextureUsages, VertexAttribute, VertexBufferLayout, VertexState, VertexStepMode};
use wgpu::util::{BufferInitDescriptor, DeviceExt};

use crate::asset::AssetManagerRc;
use crate::output::OutputInfo;

pub type OutputDeviceRc = Rc<OutputDevice>;

pub struct OutputDevice {
    device: Device,
    queue: Queue,
    output_info: OutputInfo,
    inner: RefCell<Inner>,
}

struct Inner {
    samplers: HashMap<OutputSamplerKey, Sampler>,
    bg_layouts: HashMap<OutputBindGroupLayoutKey, BindGroupLayout>,
    pipeline_layouts: HashMap<OutputPipelineLayoutKey, PipelineLayout>,
    shader_modules: HashMap<OutputShaderModuleKey, ShaderModule>,
    pipelines: HashMap<OutputRenderPipelineKey, RenderPipeline>,
}

// Manual hashing is implemented for *Desc and its *Key structs to ensure that
// their hashing behaviour is consistent.

pub struct OutputSamplerDesc {
    pub address_mode_u: AddressMode,
    pub address_mode_v: AddressMode,
    pub address_mode_w: AddressMode,
    pub mag_filter: FilterMode,
    pub min_filter: FilterMode,
    pub mipmap_filter: MipmapFilterMode,
    pub lod_min_clamp: f32,
    pub lod_max_clamp: f32,
    pub compare: Option<CompareFunction>,
    pub anisotropy_clamp: u16,
    pub border_color: Option<SamplerBorderColor>,
}

impl Default for OutputSamplerDesc {
    fn default() -> Self {
        // Defaults were taken from wgpu->SamplerDescriptor.

        Self {
            address_mode_u: Default::default(),
            address_mode_v: Default::default(),
            address_mode_w: Default::default(),
            mag_filter: Default::default(),
            min_filter: Default::default(),
            mipmap_filter: Default::default(),
            lod_min_clamp: 0.0,
            lod_max_clamp: 32.0,
            compare: None,
            anisotropy_clamp: 1,
            border_color: None,
        }
    }
}

impl Hash for OutputSamplerDesc {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.address_mode_u.hash(state);
        self.address_mode_v.hash(state);
        self.address_mode_w.hash(state);
        self.mag_filter.hash(state);
        self.min_filter.hash(state);
        self.mipmap_filter.hash(state);

        let lod_min_clamp = OrderedFloat::from(self.lod_min_clamp);
        lod_min_clamp.hash(state);

        let lod_max_clamp = OrderedFloat::from(self.lod_max_clamp);
        lod_max_clamp.hash(state);

        self.compare.hash(state);
        self.anisotropy_clamp.hash(state);
        self.border_color.hash(state);
    }
}

#[derive(Eq, PartialEq)]
struct OutputSamplerKey {
    address_mode_u: AddressMode,
    address_mode_v: AddressMode,
    address_mode_w: AddressMode,
    mag_filter: FilterMode,
    min_filter: FilterMode,
    mipmap_filter: MipmapFilterMode,
    lod_min_clamp: OrderedFloat<f32>,
    lod_max_clamp: OrderedFloat<f32>,
    compare: Option<CompareFunction>,
    anisotropy_clamp: u16,
    border_color: Option<SamplerBorderColor>,
}

impl Hash for OutputSamplerKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.address_mode_u.hash(state);
        self.address_mode_v.hash(state);
        self.address_mode_w.hash(state);
        self.mag_filter.hash(state);
        self.min_filter.hash(state);
        self.mipmap_filter.hash(state);
        self.lod_min_clamp.hash(state);
        self.lod_max_clamp.hash(state);
        self.compare.hash(state);
        self.anisotropy_clamp.hash(state);
        self.border_color.hash(state);
    }
}

impl Equivalent<OutputSamplerKey> for OutputSamplerDesc {
    fn equivalent(&self, key: &OutputSamplerKey) -> bool {
        self.address_mode_u == key.address_mode_u &&
        self.address_mode_v == key.address_mode_v &&
        self.address_mode_w == key.address_mode_w &&
        self.mag_filter == key.mag_filter &&
        self.min_filter == key.min_filter &&
        self.mipmap_filter == key.mipmap_filter &&
        self.lod_min_clamp == *key.lod_min_clamp &&
        self.lod_max_clamp == *key.lod_max_clamp &&
        self.compare == key.compare &&
        self.anisotropy_clamp == key.anisotropy_clamp &&
        self.border_color == key.border_color
    }
}

pub struct OutputBindGroupLayoutDesc<'a> {
    pub entries: &'a [BindGroupLayoutEntry],
}

impl Hash for OutputBindGroupLayoutDesc<'_> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.entries.hash(state);
    }
}

#[derive(Eq, PartialEq)]
struct OutputBindGroupLayoutKey {
    entries: Box<[BindGroupLayoutEntry]>,
}

impl Hash for OutputBindGroupLayoutKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.entries.hash(state);
    }
}

impl Equivalent<OutputBindGroupLayoutKey> for OutputBindGroupLayoutDesc<'_> {
    fn equivalent(&self, key: &OutputBindGroupLayoutKey) -> bool {
        self.entries.iter().eq(key.entries.iter())
    }
}

pub struct OutputPipelineLayoutDesc<'a> {
    pub bind_group_layouts: &'a [Option<&'a BindGroupLayout>],
    pub immediate_size: u32,
}

impl Hash for OutputPipelineLayoutDesc<'_> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.bind_group_layouts.hash(state);
        self.immediate_size.hash(state);
    }
}

#[derive(Eq, PartialEq)]
struct OutputPipelineLayoutKey {
    bind_group_layouts: Box<[Option<BindGroupLayout>]>,
    immediate_size: u32,
}

impl Hash for OutputPipelineLayoutKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.bind_group_layouts.hash(state);
        self.immediate_size.hash(state);
    }
}

impl Equivalent<OutputPipelineLayoutKey> for OutputPipelineLayoutDesc<'_> {
    fn equivalent(&self, key: &OutputPipelineLayoutKey) -> bool {
        self.bind_group_layouts.len() == key.bind_group_layouts.len() && // TODO: eq_by
        self.bind_group_layouts.iter().zip(key.bind_group_layouts.iter()).all(|(desc_bg_layout_opt, key_bg_layout_opt)| *desc_bg_layout_opt == key_bg_layout_opt.as_ref()) &&
        self.immediate_size == key.immediate_size
    }
}

pub struct OutputShaderModuleDesc<'a> {
    pub name: &'a str,
    pub placeholders: &'a [(&'a str, &'a str)],
}

impl Hash for OutputShaderModuleDesc<'_> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.name.hash(state);
        self.placeholders.hash(state);
    }
}

#[derive(Eq, PartialEq)]
struct OutputShaderModuleKey {
    name: String,
    placeholders: Box<[(String, String)]>,
}

impl Hash for OutputShaderModuleKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.name.hash(state);
        self.placeholders.hash(state);
    }
}

impl Equivalent<OutputShaderModuleKey> for OutputShaderModuleDesc<'_> {
    fn equivalent(&self, key: &OutputShaderModuleKey) -> bool {
        self.name == key.name &&
        self.placeholders.len() == key.placeholders.len() && // TODO: eq_by
        self.placeholders.iter().zip(key.placeholders.iter()).all(|((desc_key, desc_value), (key_key, key_value))| desc_key == key_key && desc_value == key_value)
    }
}

pub struct OutputVertexBufferLayout<'a> {
    pub array_stride: BufferAddress,
    pub step_mode: VertexStepMode,
    pub attributes: &'a [VertexAttribute],
}

impl Hash for OutputVertexBufferLayout<'_> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.array_stride.hash(state);
        self.step_mode.hash(state);
        self.attributes.hash(state);
    }
}

#[derive(Eq, PartialEq)]
struct OutputVertexBufferLayoutKey {
    array_stride: BufferAddress,
    step_mode: VertexStepMode,
    attributes: Box<[VertexAttribute]>,
}

impl Hash for OutputVertexBufferLayoutKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.array_stride.hash(state);
        self.step_mode.hash(state);
        self.attributes.hash(state);
    }
}

impl Equivalent<OutputVertexBufferLayoutKey> for OutputVertexBufferLayout<'_> {
    fn equivalent(&self, key: &OutputVertexBufferLayoutKey) -> bool {
        self.array_stride == key.array_stride &&
        self.step_mode == key.step_mode &&
        self.attributes.iter().eq(key.attributes.iter())
    }
}

pub struct OutputVertexState<'a> {
    pub buffers: &'a [Option<OutputVertexBufferLayout<'a>>],
}

impl Hash for OutputVertexState<'_> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.buffers.hash(state);
    }
}

#[derive(Eq, PartialEq)]
struct OutputVertexStateKey {
    buffers: Box<[Option<OutputVertexBufferLayoutKey>]>,
}

impl Hash for OutputVertexStateKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.buffers.hash(state);
    }
}

impl Equivalent<OutputVertexStateKey> for OutputVertexState<'_> {
    fn equivalent(&self, key: &OutputVertexStateKey) -> bool {
        self.buffers.len() == key.buffers.len() && // TODO: eq_by
        self.buffers.iter().zip(key.buffers.iter()).all(|(desc_buffer_opt, key_buffer_opt)|
            desc_buffer_opt.is_some() == key_buffer_opt.is_some() &&
            (desc_buffer_opt.is_none() || desc_buffer_opt.as_ref().unwrap().equivalent(key_buffer_opt.as_ref().unwrap()))
        )
    }
}

pub struct OutputFragmentState<'a> {
    pub targets: &'a [Option<ColorTargetState>],
}

impl Hash for OutputFragmentState<'_> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.targets.hash(state);
    }
}

#[derive(Eq, PartialEq)]
struct OutputFragmentStateKey {
    targets: Box<[Option<ColorTargetState>]>,
}

impl Hash for OutputFragmentStateKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.targets.hash(state);
    }
}

impl Equivalent<OutputFragmentStateKey> for OutputFragmentState<'_> {
    fn equivalent(&self, key: &OutputFragmentStateKey) -> bool {
        self.targets.iter().eq(key.targets.iter())
    }
}

pub struct OutputRenderPipelineDesc<'a> {
    pub layout: Option<&'a PipelineLayout>,
    pub vertex: OutputVertexState<'a>,
    pub primitive: PrimitiveState,
    pub depth_stencil: Option<DepthStencilState>,
    pub multisample: MultisampleState,
    pub fragment: Option<OutputFragmentState<'a>>,
    pub multiview_mask: Option<NonZeroU32>,
    pub shader_module: ShaderModule,
}

impl Hash for OutputRenderPipelineDesc<'_> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.layout.hash(state);
        self.vertex.hash(state);
        self.primitive.hash(state);
        self.depth_stencil.hash(state);
        self.multisample.hash(state);
        self.fragment.hash(state);
        self.multiview_mask.hash(state);
        self.shader_module.hash(state);
    }
}

#[derive(Eq, PartialEq)]
struct OutputRenderPipelineKey {
    layout: Option<PipelineLayout>,
    vertex: OutputVertexStateKey,
    primitive: PrimitiveState,
    depth_stencil: Option<DepthStencilState>,
    multisample: MultisampleState,
    fragment: Option<OutputFragmentStateKey>,
    multiview_mask: Option<NonZeroU32>,
    shader_module: ShaderModule,
}

impl Hash for OutputRenderPipelineKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.layout.hash(state);
        self.vertex.hash(state);
        self.primitive.hash(state);
        self.depth_stencil.hash(state);
        self.multisample.hash(state);
        self.fragment.hash(state);
        self.multiview_mask.hash(state);
        self.shader_module.hash(state);
    }
}

impl Equivalent<OutputRenderPipelineKey> for OutputRenderPipelineDesc<'_> {
    fn equivalent(&self, key: &OutputRenderPipelineKey) -> bool {
        self.layout == key.layout.as_ref() &&
        self.vertex.equivalent(&key.vertex) &&
        self.primitive == key.primitive &&
        self.depth_stencil == key.depth_stencil &&
        self.multisample == key.multisample &&
        self.fragment.is_some() == key.fragment.is_some() &&
        (self.fragment.is_none() || self.fragment.as_ref().unwrap().equivalent(key.fragment.as_ref().unwrap())) &&
        self.multiview_mask == key.multiview_mask &&
        self.shader_module == key.shader_module
    }
}

impl OutputDevice {
    pub fn new(device: &Device, queue: &Queue, output_info: OutputInfo) -> Self {
        // Implementation notes:
        // - Expose the needed top-level methods of wgpu device/queue, and keep 
        //   wgpu device/queue private.
        // - Cache/deduplicate gpu resources.

        Self {
            device: device.clone(),
            queue: queue.clone(),
            output_info,
            inner: RefCell::new(Inner {
                samplers: HashMap::new(),
                bg_layouts: HashMap::new(),
                pipeline_layouts: HashMap::new(),
                shader_modules: HashMap::new(),
                pipelines: HashMap::new(),
            }),
        }
    }

    pub fn get_output_info(&self) -> &OutputInfo {
        &self.output_info
    }

    // Uncached operations:

    pub fn create_query_set(&self, desc: &QuerySetDescriptor<'_>) -> QuerySet { // TODO: Provide caching here?
        self.device.create_query_set(desc)
    }

    pub fn create_buffer(&self, desc: &BufferDescriptor<'_>) -> Buffer {
        self.device.create_buffer(desc)
    }

    pub fn create_buffer_init(&self, desc: &BufferInitDescriptor<'_>) -> Buffer {
        self.device.create_buffer_init(desc)
    }

    pub fn create_bind_group(&self, desc: &BindGroupDescriptor<'_>) -> BindGroup { // TODO: Provide caching here?
        self.device.create_bind_group(desc)
    }
    
    pub fn create_command_encoder(&self, desc: &CommandEncoderDescriptor<'_>) -> CommandEncoder {
        self.device.create_command_encoder(desc)
    }

    pub fn submit<I: IntoIterator<Item = CommandBuffer>>(&self, command_buffers: I) -> SubmissionIndex {
        self.queue.submit(command_buffers)
    }

    pub fn write_buffer_with(&self, buffer: &Buffer, offset: BufferAddress, size: BufferSize) -> Option<QueueWriteBufferView> {
        self.queue.write_buffer_with(buffer, offset, size)
    }

    pub fn get_timestamp_period(&self) -> f32 {
        self.queue.get_timestamp_period()
    }

    pub fn get_texture_writer(&self) -> OutputTextureWriter {
        // The Slint render-loop runs on a separate thread, but it needs the wgpu queue to be 
        // able to write textures from that thread. Instead of putting OutputDevice into Arc,
        // expose the needed functionality through OutputTextureWriter.

        OutputTextureWriter::new(self.queue.clone())
    }

    pub fn create_texture(&self, width: u32, height: u32, layers: u32, sample_count: u32, format: TextureFormat) -> Texture {
        self.device.create_texture(&TextureDescriptor {
            label: None,
            size: Extent3d {
                width,
                height,
                depth_or_array_layers: layers,
            },
            mip_level_count: 1,
            sample_count,
            dimension: TextureDimension::D2,
            format,
            usage: TextureUsages::COPY_DST | TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING, // TODO: let the caller specify the usages?
            view_formats: &[],
        })
    }

    // Cached operations:

    pub fn clear_cache(&self) {
        let mut inner = self.inner.borrow_mut();

        // Shaders are not removed to avoid recompiling.
        // TODO: Embed SPIR-V shaders into binary (window, xr) during build.

        inner.samplers.clear();
        inner.bg_layouts.clear();
        inner.pipeline_layouts.clear();
        inner.pipelines.clear();
    }

    pub fn create_sampler(&self, desc: &OutputSamplerDesc) -> Sampler {
        let mut inner = self.inner.borrow_mut();

        if let Some(sampler) = inner.samplers.get(desc) {
            return sampler.clone();
        }

        let wgpu_desc = SamplerDescriptor {
            label: None,
            address_mode_u: desc.address_mode_u,
            address_mode_v: desc.address_mode_v,
            address_mode_w: desc.address_mode_w,
            mag_filter: desc.mag_filter,
            min_filter: desc.min_filter,
            mipmap_filter: desc.mipmap_filter,
            lod_min_clamp: desc.lod_min_clamp,
            lod_max_clamp: desc.lod_max_clamp,
            compare: desc.compare,
            anisotropy_clamp: desc.anisotropy_clamp,
            border_color: desc.border_color,
        };

        let sampler = self.device.create_sampler(&wgpu_desc);

        let key = OutputSamplerKey {
            address_mode_u: desc.address_mode_u,
            address_mode_v: desc.address_mode_v,
            address_mode_w: desc.address_mode_w,
            mag_filter: desc.mag_filter,
            min_filter: desc.min_filter,
            mipmap_filter: desc.mipmap_filter,
            lod_min_clamp: desc.lod_min_clamp.into(),
            lod_max_clamp: desc.lod_max_clamp.into(),
            compare: desc.compare,
            anisotropy_clamp: desc.anisotropy_clamp,
            border_color: desc.border_color,
        };
        inner.samplers.insert(key, sampler.clone());

        sampler
    }

    pub fn create_bind_group_layout(&self, desc: &OutputBindGroupLayoutDesc) -> BindGroupLayout {
        let mut inner = self.inner.borrow_mut();

        if let Some(bg_layout) = inner.bg_layouts.get(desc) {
            return bg_layout.clone();
        }

        let wgpu_desc = BindGroupLayoutDescriptor {
            label: None,
            entries: desc.entries,
        };

        let bg_layout = self.device.create_bind_group_layout(&wgpu_desc);

        let key = OutputBindGroupLayoutKey {
            entries: Box::from(desc.entries),
        };
        inner.bg_layouts.insert(key, bg_layout.clone());

        bg_layout
    }

    pub fn create_pipeline_layout(&self, desc: &OutputPipelineLayoutDesc) -> PipelineLayout {
        let mut inner = self.inner.borrow_mut();

        if let Some(pipeline_layout) = inner.pipeline_layouts.get(desc) {
            return pipeline_layout.clone();
        }

        let wgpu_desc = PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: desc.bind_group_layouts,
            immediate_size: desc.immediate_size,
        };

        let pipeline_layout = self.device.create_pipeline_layout(&wgpu_desc);

        let key = OutputPipelineLayoutKey {
            bind_group_layouts: desc.bind_group_layouts.iter().map(|bg_layout_opt| bg_layout_opt.cloned()).collect(),
            immediate_size: desc.immediate_size,
        };
        inner.pipeline_layouts.insert(key, pipeline_layout.clone());

        pipeline_layout
    }

    pub fn create_shader_module(&self, asset_mgr: AssetManagerRc, desc: &OutputShaderModuleDesc) -> ShaderModule {
        let mut inner = self.inner.borrow_mut();

        if let Some(shader_module) = inner.shader_modules.get(desc) {
            return shader_module.clone();
        }

        let asset_file = asset_mgr.open_or_err(desc.name);
        let source = asset_file.read_str_or_err();
        let source = desc.placeholders.iter().fold(source, |source, placeholder| source.replace(placeholder.0, placeholder.1));

        let wgpu_desc = ShaderModuleDescriptor {
            label: None,
            source: ShaderSource::Wgsl(Cow::Owned(source)),
        };

        let shader_module = self.device.create_shader_module(wgpu_desc);

        let key = OutputShaderModuleKey {
            name: desc.name.to_string(),
            placeholders: desc.placeholders.iter().map(|(key, value)| (key.to_string(), value.to_string())).collect(),
        };
        inner.shader_modules.insert(key, shader_module.clone());

        shader_module
    }

    pub fn create_render_pipeline(&self, desc: &OutputRenderPipelineDesc) -> RenderPipeline {
        let mut inner = self.inner.borrow_mut();

        if let Some(pipeline) = inner.pipelines.get(desc) {
            return pipeline.clone();
        }

        let vertex_buffers: Box<_> = desc.vertex.buffers.iter().map(|buffer_opt| buffer_opt.as_ref().map(|buffer| VertexBufferLayout {
            array_stride: buffer.array_stride,
            step_mode: buffer.step_mode,
            attributes: buffer.attributes,
        })).collect();

        let wgpu_desc = RenderPipelineDescriptor {
            label: None,
            layout: desc.layout,
            vertex: VertexState {
                module: &desc.shader_module,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &vertex_buffers,
            },
            primitive: desc.primitive,
            depth_stencil: desc.depth_stencil.clone(),
            multisample: desc.multisample,
            fragment: desc.fragment.as_ref().map(|fragment| FragmentState {
                module: &desc.shader_module,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: fragment.targets,
            }),
            multiview_mask: desc.multiview_mask,
            cache: None,
        };

        let pipeline = self.device.create_render_pipeline(&wgpu_desc);

        let key = OutputRenderPipelineKey {
            layout: desc.layout.cloned(),
            vertex: OutputVertexStateKey {
                buffers: desc.vertex.buffers.iter().map(|buffer_opt| buffer_opt.as_ref().map(|buffer| OutputVertexBufferLayoutKey {
                    array_stride: buffer.array_stride,
                    step_mode: buffer.step_mode,
                    attributes: Box::from(buffer.attributes),
                })).collect()
            },
            primitive: desc.primitive,
            depth_stencil: desc.depth_stencil.clone(),
            multisample: desc.multisample,
            fragment: desc.fragment.as_ref().map(|fragment| OutputFragmentStateKey {
                targets: Box::from(fragment.targets),
            }),
            multiview_mask: desc.multiview_mask,
            shader_module: desc.shader_module.clone(),
        };
        inner.pipelines.insert(key, pipeline.clone());

        pipeline
    }
}

pub struct OutputTextureWriter {
    queue: Queue,
}

impl OutputTextureWriter {
    fn new(queue: Queue) -> Self {
        Self { 
            queue 
        }
    }

    pub fn write_texture(&self, texture: TexelCopyTextureInfo<'_>, data: &[u8], data_layout: TexelCopyBufferLayout, size: Extent3d) {
        self.queue.write_texture(texture, data, data_layout, size);
    }    
}
