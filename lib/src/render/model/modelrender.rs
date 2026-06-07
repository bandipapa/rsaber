use std::any::TypeId;
use std::cell::RefCell;
use std::collections::HashSet;
use std::{iter, mem};
use std::rc::Rc;
use std::sync::Arc;

use bytemuck::{Pod, Zeroable};
use hashbrown::HashMap;
use wgpu::{BindGroup, BindGroupDescriptor, BindGroupEntry, BindGroupLayoutEntry, BindingType, BlendState, Buffer, BufferBindingType, BufferDescriptor, BufferSize, BufferUsages, Color, ColorTargetState, ColorWrites, CommandEncoder, CompareFunction, DepthStencilState, IndexFormat, LoadOp, MultisampleState, Operations, RenderPassColorAttachment, RenderPassDepthStencilAttachment, RenderPassDescriptor, RenderPipeline, ShaderStages, StoreOp, TextureView};

use crate::asset::AssetManagerRc;
use crate::macros::render_node;
use crate::output::{Frame, OutputBindGroupLayoutDesc, OutputDeviceRc, OutputFragmentState, OutputPipelineLayoutDesc, OutputRenderPipelineDesc, OutputShaderModuleDesc, OutputVertexState, ViewMat};
use crate::render::{RenderNodeBuildWithParam, RenderNodeExec};
use crate::render::model::{InstGridBuf, InstOutlineBoxBuf, InstPhongColorBuf, InstShaderImplType, InstShaderSize, InstShaderType, InstSimpleColorBuf, InstWindowBuf, Mesh};
use crate::ui::UIManagerRc;
use crate::util::StatsRc;

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
struct Uni {
    light_pos: [f32; 3],
    _unused1: f32,
    cam_pos: [f32; 3],
    _unused2: f32,
    // Rest is filled by Frame->get_view_m(). // TODO: Or have an additional, output-specific buffer?
}

// TODO: make it model-specific, like vertex/inst shader?
const UNI_TMPL: &str = "
struct Uni {
    light_pos: vec3<f32>,
    _unused1: f32,
    cam_pos: vec3<f32>,
    _unused2: f32,
    view_m: array<mat4x4<f32>, #VIEW_LEN#>,
}

@group(0) @binding(0) var<uniform> uni: Uni;
";

pub trait ModelFactory: 'static { // The static lifetime is required by TypeId.
    type Model: Model + 'static;

    fn get_id() -> TypeId {
        TypeId::of::<Self>()
    }

    fn get_mesh(asset_mgr: AssetManagerRc, output_device: OutputDeviceRc) -> Mesh;
    fn create(self, handle: ModelHandle, output_device: OutputDeviceRc, inst_sh_impls: &mut [InstShaderImplType], ui_manager: UIManagerRc) -> Self::Model;
}

pub trait Model {
    fn fill_simple_color(&self, _inst_index: u32) -> InstSimpleColorBuf {
        panic!("Method is not implemented");
    }

    fn fill_phong_color(&self, _inst_index: u32) -> InstPhongColorBuf {
        panic!("Method is not implemented");
    }

    fn fill_grid(&self, _inst_index: u32) -> InstGridBuf {
        panic!("Method is not implemented");
    }

    fn fill_window(&self, _inst_index: u32) -> InstWindowBuf {
        panic!("Method is not implemented");
    }

    fn fill_outlinebox(&self, _inst_index: u32) -> InstOutlineBoxBuf {
        panic!("Method is not implemented");
    }
}

type ModelInfos = HashMap<TypeId, ModelInfo>;
type Visibles = Rc<RefCell<Box<[HashSet<u32>]>>>; // Box[inst_index]->HashSet[model_index]

pub struct ModelRegistry {
    asset_mgr: AssetManagerRc,
    output_device: OutputDeviceRc,
    ui_manager: UIManagerRc,
    model_infos: ModelInfos,
}

struct ModelInfo {
    mesh: Mesh,
    inst_sh_impls: Box<[InstShaderImplType]>,
    models: Vec<Rc<dyn Model>>, // [model_index]
    visibles: Visibles,
}

impl ModelRegistry {
    // TODO: Implement cache, since ModelRegistry/Obj is going to reload/compile assets on instantiation.
    pub fn new(asset_mgr: AssetManagerRc, output_device: OutputDeviceRc, ui_manager: UIManagerRc) -> Self {
        Self {
            asset_mgr,
            output_device,
            ui_manager,
            model_infos: HashMap::new(),
        }
    }

    pub fn create<F: ModelFactory>(&mut self, factory: F) -> Rc<F::Model> {
        // Models are grouped by name, so we can do instanced rendering,
        // see ModelRenderer->render().

        let model_info = self.model_infos.entry(F::get_id()).or_insert_with(|| {
            let mesh = F::get_mesh(Arc::clone(&self.asset_mgr), Rc::clone(&self.output_device));
            let submeshes = mesh.get_submeshes();

            let inst_sh_impls = submeshes.iter().map(|submesh| submesh.get_inst_sh_type().create_impl()).collect();
            let visibles = Rc::new(RefCell::new(iter::repeat_with(HashSet::new).take(submeshes.len()).collect()));

            ModelInfo {
                mesh,
                inst_sh_impls,
                models: Vec::new(),
                visibles,
            }
        });

        let model_index = model_info.models.len().try_into().unwrap();
        let handle = ModelHandle::new(Rc::clone(&model_info.visibles), model_index);
        let model = Rc::new(factory.create(handle, Rc::clone(&self.output_device), &mut model_info.inst_sh_impls, Rc::clone(&self.ui_manager)));

        model_info.models.push(Rc::clone(&model) as Rc<dyn Model>);

        model
    }
}

pub struct ModelHandle {
    visibles: Visibles,
    model_index: u32,
}

impl ModelHandle {
    fn new(visibles: Visibles, model_index: u32) -> Self {
        Self {
            visibles,
            model_index,
        }
    }

    pub fn set_visible(&self, inst_index: u32, visible: bool) {
        let model_indexes = &mut self.visibles.borrow_mut()[inst_index as usize]; // Use HashSet to make visibility changes fast.

        if visible {
            model_indexes.insert(self.model_index);
        } else {
            model_indexes.remove(&self.model_index);
        }
    }

    pub fn get_visible(&self, inst_index: u32) -> bool {
        let model_indexes = &self.visibles.borrow()[inst_index as usize];

        model_indexes.contains(&self.model_index)
    }
}

#[render_node(struct=ModelRendererInOut,field=in_out,out(out_color))]
pub struct ModelRenderer {
    output_device: OutputDeviceRc,
    stats: StatsRc,
    uni_size: u64,
    uni_buf: Buffer,
    uni_bg: BindGroup,
    render_infos: Box<[RenderInfo]>,
    inner_opt: RefCell<Option<Inner>>,
}

struct RenderInfo {
    mesh: Mesh,
    models: Box<[Rc<dyn Model>]>, // [model_index]
    visibles: Visibles,
    inst_sh_infos: Box<[InstShaderInfo]>, // [inst_index]
}

struct InstShaderInfo {
    inst_size: u64,
    inst_buf: Buffer,
    pipeline: RenderPipeline,
    bg_opt: Option<BindGroup>,
}

struct Inner {
    multisample_view_opt: Option<TextureView>,
    depth_view: TextureView,
}

impl ModelRenderer {
    fn new(asset_mgr: AssetManagerRc, output_device: OutputDeviceRc, stats: StatsRc, model_infos: ModelInfos, in_out: ModelRendererInOut) -> Self {
        let output_info = output_device.get_output_info();

        // Allocate uniform buffer.

        let uni_size = (mem::size_of::<Uni>() + mem::size_of::<ViewMat>() * output_info.get_view_len() as usize).try_into().unwrap();

        let uni_buf = output_device.create_buffer(&BufferDescriptor {
            label: None,
            size: uni_size,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let uni_bg_layout = output_device.create_bind_group_layout(&OutputBindGroupLayoutDesc {
            entries: &[
                BindGroupLayoutEntry {
                    binding: 0, // See vertex/fragment shader->@binding().
                    visibility: ShaderStages::VERTEX | ShaderStages::FRAGMENT,
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }
            ]
        });

        let uni_bg = output_device.create_bind_group(&BindGroupDescriptor {
            label: None,
            layout: &uni_bg_layout,
            entries: &[
                BindGroupEntry {
                    binding: 0, // See vertex/fragment shader->@binding().
                    resource: uni_buf.as_entire_binding(),
                }
            ]
        });

        // Determine placeholders.

        let common_asset_file = asset_mgr.open_or_err("/shader/common.wgsl"); // TODO: cache?
        let common_source = common_asset_file.read_str_or_err();

        let view_len = output_info.get_view_len();
        let uni = UNI_TMPL.replace("#VIEW_LEN#", &format!("{view_len}"));

        let (view_index_def, view_index_val) = if output_info.get_view_len() == 1 {
            ("", "0")
        } else {
            ("@builtin(view_index) view_index: u32,", "in.view_index")
        };

        let shader_placeholders = [
            ("#COMMON#", common_source.as_str()),
            ("#UNI#", uni.as_str()),
            ("#VIEW_INDEX_DEF#", view_index_def),
            ("#VIEW_INDEX_VAL#", view_index_val),
        ];

        // Setup pipelines.

        let render_infos = model_infos.into_values().map(|model_info| {
            let mesh = model_info.mesh;
            let vertex_sh_type = mesh.get_vertex_sh_type();

            let models = model_info.models.into_boxed_slice();
            let models_len = models.len() as u64;

            let inst_sh_infos = mesh.get_submeshes().iter().zip(model_info.inst_sh_impls.iter()).map(|(submesh, inst_sh_impl)| {
                let inst_sh_type = submesh.get_inst_sh_type();
                let inst_sh_layout = inst_sh_type.get_layout();                
                let inst_size = inst_sh_layout.array_stride;

                // Allocate buffer.

                let inst_buf = output_device.create_buffer(&BufferDescriptor {
                    label: None,
                    size: inst_size * models_len,
                    usage: BufferUsages::VERTEX | BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });

                // Create bind groups.
                // TODO: Use fix number for inst_sh_impl->resources->count?, so we don't have to create different pipeline layouts because of different counts.
                // TODO: Once wgpu has bindless descriptors, then we can simplify this logic.

                let mut bg_layouts = Vec::new();
                bg_layouts.push(Some(&uni_bg_layout));

                let bg_layout_opt = inst_sh_impl.create_bind_group_layout(Rc::clone(&output_device));
                let mut bg_opt = None;
                if let Some(bg_layout) = &bg_layout_opt {
                    bg_layouts.push(Some(bg_layout));
                    bg_opt = Some(inst_sh_impl.create_bind_group(Rc::clone(&output_device), bg_layout));
                }

                let pipeline_layout = output_device.create_pipeline_layout(&OutputPipelineLayoutDesc {
                    bind_group_layouts: &bg_layouts, // See vertex/fragment shader->@group().
                    immediate_size: 0,
                });

                // Create shader.

                let shader_name = format!("/shader/{}_{}.wgsl", vertex_sh_type.get_name(), inst_sh_type.get_name());
                let shader_module = output_device.create_shader_module(Arc::clone(&asset_mgr), &OutputShaderModuleDesc {
                    name: &shader_name,
                    placeholders: &shader_placeholders,
                });

                // Create pipeline.

                let pipeline = output_device.create_render_pipeline(&OutputRenderPipelineDesc {
                    layout: Some(&pipeline_layout),
                    vertex: OutputVertexState {
                        buffers: &[
                            Some(vertex_sh_type.get_layout()),
                            Some(inst_sh_layout),
                        ],
                    },
                    primitive: *submesh.get_primitive_state(),
                    depth_stencil: Some(DepthStencilState {
                        format: output_info.get_depth_format(),
                        depth_write_enabled: Some(true),
                        depth_compare: Some(CompareFunction::Less),
                        stencil: Default::default(),
                        bias: Default::default(),
                    }),
                    multisample: MultisampleState {
                        count: output_info.get_sample_count(),
                        mask: !0,
                        alpha_to_coverage_enabled: false,
                    },
                    fragment: Some(OutputFragmentState {
                        targets: &[Some(ColorTargetState { // See fragment shader->@location().
                            format: output_info.get_color_format(),
                            blend: Some(BlendState::REPLACE),
                            write_mask: ColorWrites::ALL,
                        })],
                    }),
                    multiview_mask: output_info.get_view_mask(),
                    shader_module,
                });

                InstShaderInfo {
                    inst_size,
                    inst_buf,
                    pipeline,
                    bg_opt,
                }
            }).collect();

            RenderInfo {
                mesh,
                models,
                visibles: model_info.visibles,
                inst_sh_infos,
            }
        }).collect();

        Self {
            output_device,
            stats,
            uni_size,
            uni_buf,
            uni_bg,
            render_infos,
            inner_opt: RefCell::new(None),
            in_out,
        }
    }
}

impl RenderNodeBuildWithParam for ModelRenderer {
    type Param = ModelRegistry;

    fn build(asset_mgr: AssetManagerRc, output_device: OutputDeviceRc, stats: StatsRc, param: Self::Param, in_out: Self::InOut) -> Self {
        ModelRenderer::new(asset_mgr, output_device, stats, param.model_infos, in_out)
    }
}

impl RenderNodeExec for ModelRenderer {
    fn configure(&self, width: u32, height: u32) {
        // Setup multisample buffer.

        let output_info = self.output_device.get_output_info();
        let sample_count = output_info.get_sample_count();
        let view_len = output_info.get_view_len();
        let color_format = output_info.get_color_format();

        let multisample_view_opt = if sample_count > 1 {
            let texture = self.output_device.create_texture(width, height, view_len, sample_count, color_format);
            Some(texture.create_view(&Default::default()))
        } else {
            None
        };

        // Setup depth buffer.

        let depth_texture = self.output_device.create_texture(width, height, view_len, sample_count, output_info.get_depth_format());
        let depth_view = depth_texture.create_view(&Default::default());

        // Setup color buffer.

        self.in_out.out_color.set_texture(|| self.output_device.create_texture(width, height, view_len, 1, color_format));

        let mut inner_opt = self.inner_opt.borrow_mut();
        *inner_opt = Some(Inner {
            multisample_view_opt,
            depth_view,
        });
    }

    fn render(&self, encoder: &mut CommandEncoder, frame: &dyn Frame) {
        let inner_opt = self.inner_opt.borrow();
        let inner = inner_opt.as_ref().unwrap();

        // Setup render pass.

        let output_info = self.output_device.get_output_info();
        let multisample_view_opt = inner.multisample_view_opt.as_ref();
        let color_view = &self.in_out.out_color.get_view();

        let mut render_pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: None,
            color_attachments: &[Some(RenderPassColorAttachment { // See fragment shader->@location(0).
                view: multisample_view_opt.unwrap_or(color_view),
                depth_slice: None,
                resolve_target: multisample_view_opt.map(|_| color_view),
                ops: Operations {
                    load: LoadOp::Clear(Color {
                        r: 0.0,
                        g: 0.0,
                        b: 0.0,
                        a: 1.0,
                    }),
                    store: StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(RenderPassDepthStencilAttachment {
                view: &inner.depth_view,
                depth_ops: Some(Operations {
                    load: LoadOp::Clear(1.0),
                    store: StoreOp::Discard,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: output_info.get_view_mask(),
        });

        let mut stat_draw_calls = 0;
        let mut stat_inst_num = 0;
        let mut stat_buf_upload = 0;

        // Fill uniform buffer. From https://docs.rs/wgpu/latest/wgpu/struct.Queue.html#method.write_buffer_with :
        // "Dropping the QueueWriteBufferView does not submit the transfer to the GPU immediately. The transfer begins only on the next call to Queue::submit() after the view is dropped, just before the explicitly submitted commands."
        // TODO: Refactor it to be typesafe (e.g. Uni<Frame::OutputViewMat>)

        {
            let mut uni_buf_view = self.output_device.write_buffer_with(&self.uni_buf, 0, BufferSize::new(self.uni_size).unwrap()).unwrap();

            let mut uni = Uni::zeroed();
            uni.light_pos = cgmath::Point3::new(0.0, -3.0, 3.0).into(); // TODO: where should we position the light?
            uni.cam_pos = (*frame.get_cam_pos()).into();

            let mut uni_buf_sl = uni_buf_view.slice(..mem::size_of::<Uni>());
            uni_buf_sl.copy_from_slice(bytemuck::cast_slice(&[uni]));
            
            let mut uni_buf_sl = uni_buf_view.slice(mem::size_of::<Uni>()..);
            uni_buf_sl.copy_from_slice(frame.get_view_m());

            stat_buf_upload += self.uni_size as u32;
        }

        // Do render.
        // TODO: implement frustum culling: don't submit objects to GPU which are outside the view frustum.
        // TODO: implement dirty flag: only upload what is changed.

        render_pass.set_bind_group(0, &self.uni_bg, &[]); // See OutputPipelineLayoutDescriptor->bind_group_layouts.
        
        for render_info in &self.render_infos {
            let mesh = &render_info.mesh;
            let mut mesh_bound = false;

            let visibles = render_info.visibles.borrow();

            for (inst_index, ((visible_model_indexes, inst_sh_info), submesh)) in visibles.iter().zip(render_info.inst_sh_infos.iter()).zip(mesh.get_submeshes().iter()).enumerate() {
                let visible_model_indexes_len: u32 = visible_model_indexes.len().try_into().unwrap();

                if visible_model_indexes_len > 0 {
                    let inst_index: u32 = inst_index.try_into().unwrap();
                    let inst_size = inst_sh_info.inst_size;
                    let inst_buf = &inst_sh_info.inst_buf;
                    let total_size = visible_model_indexes_len as u64 * inst_size;

                    // Don't map full buffer, only the part which is large enough to hold the visible models.

                    let mut inst_buf_view = self.output_device.write_buffer_with(inst_buf, 0, BufferSize::new(total_size).unwrap()).unwrap();

                    macro_rules! fill_inst_buf {
                        ($(($inst_sh_type:ident, $method:ident)),*) => {
                            match submesh.get_inst_sh_type() {
                                $(
                                    InstShaderType::$inst_sh_type => {
                                        let (inst_buf_arr, _) = inst_buf_view.slice(..).into_chunks::<{InstShaderSize::$inst_sh_type}>();

                                        inst_buf_arr.write_iter(visible_model_indexes.iter().map(|model_index| {
                                            let model = &render_info.models[*model_index as usize];
                                            let inst_buf_single = model.$method(inst_index);
                                            bytemuck::cast(inst_buf_single)
                                        }));
                                    },
                                )*
                            }
                        }
                    }

                    fill_inst_buf!(
                        (SimpleColor, fill_simple_color),
                        (PhongColor, fill_phong_color),
                        (Grid, fill_grid),
                        (Window, fill_window),
                        (OutlineBox, fill_outlinebox)
                    );

                    if !mesh_bound {
                        render_pass.set_vertex_buffer(0, mesh.get_vertex_buf().slice(..)); // See VertexState->buffers[0].
                        render_pass.set_index_buffer(mesh.get_index_buf().slice(..), IndexFormat::Uint16);
                        mesh_bound = true;
                    }

                    render_pass.set_pipeline(&inst_sh_info.pipeline);
                    render_pass.set_vertex_buffer(1, inst_buf.slice(..total_size)); // See VertexState->buffers[1].

                    if let Some(bg) = &inst_sh_info.bg_opt {
                        render_pass.set_bind_group(1, bg, &[]); // See OutputPipelineLayoutDescriptor->bind_group_layouts.
                    }

                    render_pass.draw_indexed(submesh.get_indices(), submesh.get_base_vertex(), 0..visible_model_indexes_len);

                    stat_draw_calls += 1;
                    stat_inst_num += visible_model_indexes_len;
                    stat_buf_upload += total_size as u32;
                }
            }
        }

        // Update stats.

        {
            let mut stats_inner = self.stats.get_inner_mut();
            stats_inner.draw_calls += stat_draw_calls;
            stats_inner.inst_num += stat_inst_num;
            stats_inner.buf_upload += stat_buf_upload;
        }
    }
}
