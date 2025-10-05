use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::iter;
use std::num::NonZeroU32;
use std::rc::Rc;
use std::sync::Arc;

use wgpu::{BindGroup, BindGroupLayout, BlendState, Buffer, BufferDescriptor, BufferSize, BufferUsages, ColorTargetState, ColorWrites, CompareFunction, DepthStencilState, Device, FragmentState, IndexFormat, MultisampleState, PipelineLayoutDescriptor, RenderPass, RenderPipeline, RenderPipelineDescriptor, ShaderModuleDescriptor, ShaderSource, VertexState};

use crate::asset::AssetManagerRc;
use crate::model::{InstGridBuf, InstPhongColorBuf, InstShaderImplType, InstShaderType, InstSimpleColorBuf, InstWindowBuf, Mesh};
use crate::output::OutputInfoRc;
use crate::ui::UIManagerRc;

// Keep render->Uni and modelrender->UNI_TMPL in-sync.
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

pub trait ModelFactory {
    type Model: Model + 'static;

    fn get_name() -> &'static str;
    fn get_mesh(asset_mgr: AssetManagerRc, device: &Device) -> Mesh;
    fn create(self, handle: ModelHandle, device: &Device, inst_sh_impls: &mut [InstShaderImplType], ui_manager: UIManagerRc) -> Self::Model;
}

pub trait Model {
    fn fill_simple_color(&self, _inst_index: u32, _inst_sh_buf: &mut InstSimpleColorBuf) {
        panic!("Method is not implemented");
    }

    fn fill_phong_color(&self, _inst_index: u32, _inst_sh_buf: &mut InstPhongColorBuf) {
        panic!("Method is not implemented");
    }

    fn fill_grid(&self, _inst_index: u32, _inst_sh_buf: &mut InstGridBuf) {
        panic!("Method is not implemented");
    }

    fn fill_window(&self, _inst_index: u32, _inst_sh_buf: &mut InstWindowBuf) {
        panic!("Method is not implemented");
    }
}

type ModelInfos = HashMap<String, ModelInfo>; 
type Visibles = Rc<RefCell<Box<[HashSet<u32>]>>>; // Box[inst_index]->HashSet[model_index]

pub struct ModelRegistry {
    asset_mgr: AssetManagerRc,
    output_info: OutputInfoRc,
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
    pub fn new(asset_mgr: AssetManagerRc, output_info: OutputInfoRc, ui_manager: UIManagerRc) -> Self {
        Self {
            asset_mgr,
            output_info,
            ui_manager,
            model_infos: HashMap::new(),
        }
    }

    pub fn create<F: ModelFactory>(&mut self, factory: F) -> Rc<F::Model> {
        // Models are grouped by name, so we can do instanced rendering,
        // see ModelRenderer->render().

        let device = self.output_info.get_device();

        let model_info = self.model_infos.entry(F::get_name().to_string()).or_insert_with(|| {
            let mesh = F::get_mesh(Arc::clone(&self.asset_mgr), device);
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
        let model = Rc::new(factory.create(handle, device, &mut model_info.inst_sh_impls, Rc::clone(&self.ui_manager)));

        model_info.models.push(Rc::clone(&model) as Rc<dyn Model>);

        model
    }

    pub fn build(self, uni_bg_layout: &BindGroupLayout) -> ModelRenderer {
        ModelRenderer::new(Arc::clone(&self.asset_mgr), self.output_info, self.model_infos, uni_bg_layout)
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
}

pub struct ModelRenderer {
    output_info: OutputInfoRc,
    render_infos: Box<[RenderInfo]>,
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
    pipeline: Rc<RenderPipeline>,
    bg_opt: Option<BindGroup>,
}

impl ModelRenderer {
    fn new(asset_mgr: AssetManagerRc, output_info: OutputInfoRc, model_infos: ModelInfos, uni_bg_layout: &BindGroupLayout) -> Self {
        let device = output_info.get_device();

        let mut pipelines = HashMap::new();
        let mut pipeline_layouts = HashMap::new();
        let mut shaders = HashMap::new();

        let view_len = output_info.get_view_len();
        let uni = UNI_TMPL.replace("#VIEW_LEN#", &format!("{view_len}"));

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

                let inst_buf = device.create_buffer(&BufferDescriptor {
                    label: None,
                    size: inst_size * models_len,
                    usage: BufferUsages::VERTEX | BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });

                // Create pipeline layout and bind groups.
                // TODO: Use fix number for inst_sh_impl->resources->count?, so we don't have to create different pipeline layouts because of different counts.
                // TODO: Once wgpu has bindless descriptors, then we can simplify this logic.

                let pipeline_layout_key = inst_sh_impl.get_key();

                let (bg_layout_opt, pipeline_layout) = pipeline_layouts.entry(pipeline_layout_key.clone()).or_insert_with(|| {
                    let mut bg_layouts = Vec::new();
                    bg_layouts.push(uni_bg_layout);

                    let bg_layout_opt = inst_sh_impl.create_bind_group_layout(device);
                    if let Some(bg_layout) = &bg_layout_opt {
                        bg_layouts.push(bg_layout);
                    }

                    let pipeline_layout = Rc::new(device.create_pipeline_layout(&PipelineLayoutDescriptor {
                        label: None,
                        bind_group_layouts: &bg_layouts, // See vertex/fragment shader->@group().
                        push_constant_ranges: &[],
                    }));

                    (bg_layout_opt, pipeline_layout)
                });

                let bg_opt = if let Some(bg_layout) = bg_layout_opt {
                    let bg = inst_sh_impl.create_bind_group(device, bg_layout);
                    Some(bg)
                } else {
                    None
                };

                // Lookup pipeline, make sure we don't have duplicated pipelines/shaders.

                let primitive_st_type = submesh.get_primitive_state_type();

                let pipeline = pipelines.entry((pipeline_layout_key, vertex_sh_type.clone(), inst_sh_type.clone(), primitive_st_type.clone())).or_insert_with(|| {
                    let shader = shaders.entry((vertex_sh_type.clone(), inst_sh_type.clone())).or_insert_with(|| {
                        let name = format!("shader/{}_{}.wgsl", vertex_sh_type.get_name(), inst_sh_type.get_name());

                        // Pre-process shaders, so we don't need to duplicate them for single
                        // or multiview (stereo) rendering.
                        // TODO: Embed compiled shaders into binary (window, xr)?

                        let source = asset_mgr.read_file(&name)
                            .replace("#UNI#", &uni)
                            .replace("#VIEW_INDEX_DEF#", output_info.get_view_index_def())
                            .replace("#VIEW_INDEX_VAL#", output_info.get_view_index_val());

                        Rc::new(device.create_shader_module(ShaderModuleDescriptor {
                            label: None,
                            source: ShaderSource::Wgsl(Cow::Owned(source)),
                        }))
                    });

                    Rc::new(device.create_render_pipeline(&RenderPipelineDescriptor {
                        label: None,
                        layout: Some(pipeline_layout),
                        vertex: VertexState {
                            module: shader,
                            entry_point: Some("vs_main"),
                            compilation_options: Default::default(),
                            buffers: &[
                                vertex_sh_type.get_layout(),
                                inst_sh_layout,
                            ],
                        },
                        fragment: Some(FragmentState {
                            module: shader,
                            entry_point: Some("fs_main"),
                            compilation_options: Default::default(),
                            targets: &[Some(ColorTargetState { // See fragment shader->@location().
                                format: output_info.get_color_format(),
                                blend: Some(BlendState::REPLACE),
                                write_mask: ColorWrites::ALL,
                            })],
                        }),
                        primitive: primitive_st_type.get_primitive(),
                        depth_stencil: Some(DepthStencilState {
                            format: output_info.get_depth_format(),
                            depth_write_enabled: true,
                            depth_compare: CompareFunction::Less,
                            stencil: Default::default(),
                            bias: Default::default(),
                        }),
                        multisample: MultisampleState {
                            count: output_info.get_sample_count(),
                            mask: !0,
                            alpha_to_coverage_enabled: false,
                        },
                        multiview: NonZeroU32::new(if view_len == 1 { 0 } else { view_len }),
                        cache: None
                    }))
                });

                InstShaderInfo {
                    inst_size,
                    inst_buf,
                    pipeline: Rc::clone(pipeline),
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

        // TODO: Would make sense to display some statistic, e.g. number of pipelines, vertex, index buffers, models, bindgroups, etc.

        Self {
            output_info,
            render_infos,
        }
    }

    pub fn render(&self, render_pass: &mut RenderPass) {
        let queue = self.output_info.get_queue();

        // TODO: Would make sense to display some statistic, e.g. number of draw calls, etc.

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

                    let mut inst_buf_view = queue.write_buffer_with(inst_buf, 0, BufferSize::new(total_size).unwrap()).unwrap();
                    let inst_buf_sl: &mut [u8] = &mut inst_buf_view;

                    for (model_index, inst_sh_buf) in visible_model_indexes.iter().zip(inst_buf_sl.chunks_mut(inst_size as usize)) {
                        let model = &render_info.models[*model_index as usize];

                        match submesh.get_inst_sh_type() {
                            InstShaderType::SimpleColor => {
                                let inst_sh_buf: &mut [InstSimpleColorBuf] = bytemuck::cast_slice_mut(inst_sh_buf);
                                let inst_sh_buf = &mut inst_sh_buf[0];
                                model.fill_simple_color(inst_index, inst_sh_buf);
                            },
                            InstShaderType::PhongColor => {
                                let inst_sh_buf: &mut [InstPhongColorBuf] = bytemuck::cast_slice_mut(inst_sh_buf);
                                let inst_sh_buf = &mut inst_sh_buf[0];
                                model.fill_phong_color(inst_index, inst_sh_buf);
                            },
                            InstShaderType::Grid => {
                                let inst_sh_buf: &mut [InstGridBuf] = bytemuck::cast_slice_mut(inst_sh_buf);
                                let inst_sh_buf = &mut inst_sh_buf[0];
                                model.fill_grid(inst_index, inst_sh_buf);
                            },
                            InstShaderType::Window => {
                                let inst_sh_buf: &mut [InstWindowBuf] = bytemuck::cast_slice_mut(inst_sh_buf);
                                let inst_sh_buf = &mut inst_sh_buf[0];
                                model.fill_window(inst_index, inst_sh_buf);
                            },
                        };
                    }

                    if !mesh_bound {
                        render_pass.set_vertex_buffer(0, mesh.get_vertex_buf().slice(..)); // See VertexState->buffers[0].
                        render_pass.set_index_buffer(mesh.get_index_buf().slice(..), IndexFormat::Uint16);
                        mesh_bound = true;
                    }

                    render_pass.set_pipeline(&inst_sh_info.pipeline);
                    render_pass.set_vertex_buffer(1, inst_buf.slice(..total_size)); // See VertexState->buffers[1].

                    if let Some(bg) = &inst_sh_info.bg_opt {
                        render_pass.set_bind_group(1, bg, &[]); // See PipelineLayoutDescriptor->bind_group_layouts.
                    }

                    render_pass.draw_indexed(submesh.get_indices(), submesh.get_base_vertex(), 0..visible_model_indexes_len);
                }
            }
        }
    }
}
