use std::any::TypeId;
use std::cell::RefCell;
use std::collections::HashSet;
use std::iter;
use std::mem;
use std::rc::Rc;
use std::sync::Arc;

use bytemuck::{Pod, Zeroable};
use cgmath::{InnerSpace, Vector3};
use hashbrown::HashMap;
use wgpu::{BindGroup, BindGroupDescriptor, BindGroupEntry, BindGroupLayoutEntry, BindingResource, BindingType, Buffer, BufferBinding, BufferBindingType, BufferDescriptor, BufferSize, BufferUsages, Color, ColorTargetState, ColorWrites, CommandEncoder, CompareFunction, DepthStencilState, IndexFormat, LoadOp, MultisampleState, Operations, QueueWriteBufferView, RenderPass, RenderPassColorAttachment, RenderPassDepthStencilAttachment, RenderPassDescriptor, RenderPipeline, ShaderStages, StoreOp, TextureView};

use crate::asset::AssetManagerRc;
use crate::macros::render_node;
use crate::output::{Frame, OutputBindGroupLayoutDesc, OutputDeviceRc, OutputFragmentState, OutputPipelineLayoutDesc, OutputRenderPipelineDesc, OutputShaderModuleDesc, OutputVertexState, ViewMat};
use crate::render::{RenderNodeBuildWithParam, RenderNodeExec};
use crate::render::model::{InstGridBuf, InstObstacleBuf, InstPhongColorBuf, InstShaderImplType, InstShaderSize, InstShaderType, InstSimpleColorBuf, InstWindowBuf, Mesh};
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

pub trait ModelFactory {
    type Model: Model + 'static;

    fn get_id() -> TypeId {
        TypeId::of::<Self::Model>()
    }

    fn get_mesh(asset_mgr: AssetManagerRc, output_device: OutputDeviceRc) -> Mesh;
    fn create(self, handle: ModelHandle, output_device: OutputDeviceRc, inst_sh_impls: &mut [InstShaderImplType], ui_manager: UIManagerRc) -> Self::Model;
}

pub trait Model {
    fn get_pos(&self, _inst_index: u16) -> Vector3<f32> {
        panic!("Method is not implemented");
    }

    fn fill_simple_color(&self, _inst_index: u16) -> InstSimpleColorBuf {
        panic!("Method is not implemented");
    }

    fn fill_phong_color(&self, _inst_index: u16) -> InstPhongColorBuf {
        panic!("Method is not implemented");
    }

    fn fill_grid(&self, _inst_index: u16) -> InstGridBuf {
        panic!("Method is not implemented");
    }

    fn fill_window(&self, _inst_index: u16) -> InstWindowBuf {
        panic!("Method is not implemented");
    }

    fn fill_obstacle(&self, _inst_index: u16) -> InstObstacleBuf {
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
        // Models are grouped, so we can do instanced rendering,
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

    pub fn set_visible(&self, inst_index: u16, visible: bool) {
        let model_indexes = &mut self.visibles.borrow_mut()[inst_index as usize]; // Use HashSet to make visibility changes fast.

        if visible {
            model_indexes.insert(self.model_index);
        } else {
            model_indexes.remove(&self.model_index);
        }
    }

    pub fn get_visible(&self, inst_index: u16) -> bool {
        let model_indexes = &self.visibles.borrow()[inst_index as usize];

        model_indexes.contains(&self.model_index)
    }
}

#[render_node(struct=ModelRendererInOut,field=in_out,out(out_color))]
pub struct ModelRenderer {
    output_device: OutputDeviceRc,
    stats: StatsRc,
    uni_size: u64,
    uni_bg: BindGroup,
    render_infos: Box<[RenderInfo]>,
    buf: Buffer,
    inner: RefCell<Inner>,
}

struct RenderInfo {
    mesh: Mesh,
    models: Box<[Rc<dyn Model>]>, // [model_index]
    visibles: Visibles,
    inst_sh_infos: Box<[InstShaderInfo]>, // [inst_index]
}

struct InstShaderInfo {
    inst_size: u64,
    pipeline: RenderPipeline,
    bg_opt: Option<BindGroup>,
}

struct Inner {
    offsets: Box<[Box<[u64]>]>, // [render_info_index][inst_index]
    sort_infos: Box<[SortInfo]>,
    render_buf_opt: Option<RenderBuf>,
}

struct RenderBuf {
    multisample_view_opt: Option<TextureView>,
    depth_view: TextureView,
}

impl ModelRenderer {
    fn new(asset_mgr: AssetManagerRc, output_device: OutputDeviceRc, stats: StatsRc, model_infos: ModelInfos, in_out: ModelRendererInOut) -> Self {
        let output_info = output_device.get_output_info();
        let mut buf_size = 0;

        // Setup uniform buffer.

        let uni_size = (mem::size_of::<Uni>() + mem::size_of::<ViewMat>() * output_info.get_view_len() as usize).try_into().unwrap();
        buf_size += uni_size;

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

        let mut sort_infos_len = 0;

        let render_infos: Box<[RenderInfo]> = model_infos.into_values().map(|model_info| {
            let mesh = model_info.mesh;
            let vertex_sh_type = mesh.get_vertex_sh_type();

            let models = model_info.models.into_boxed_slice();
            let models_len = models.len() as u64;

            let inst_sh_infos = mesh.get_submeshes().iter().zip(model_info.inst_sh_impls.iter()).map(|(submesh, inst_sh_impl)| {
                let inst_sh_type = submesh.get_inst_sh_type();
                let inst_sh_layout = inst_sh_type.get_layout();
                let inst_size = inst_sh_layout.array_stride;

                buf_size += inst_size * models_len;

                if inst_sh_type.is_sorted() {
                    sort_infos_len += models_len;
                }

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

                // Create pipeline. We are using Reverse-Z to improve depth buffer precision, see
                // - https://developer.nvidia.com/content/depth-precision-visualized
                // - https://iolite-engine.com/blog_posts/reverse_z_cheatsheet

                let shader_pipeline_config = inst_sh_type.get_pipeline_config();

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
                        depth_write_enabled: Some(shader_pipeline_config.get_depth_write_enabled()),
                        depth_compare: Some(CompareFunction::Greater),
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
                            blend: Some(*shader_pipeline_config.get_blend_state()),
                            write_mask: ColorWrites::ALL,
                        })],
                    }),
                    multiview_mask: output_info.get_view_mask(),
                    shader_module,
                });

                InstShaderInfo {
                    inst_size,
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

        // Allocate buffer. A single buffer is used for both uniform and instance data.

        let buf = output_device.create_buffer(&BufferDescriptor {
            label: None,
            size: buf_size,
            usage: BufferUsages::COPY_DST | BufferUsages::UNIFORM | BufferUsages::VERTEX,
            mapped_at_creation: false,
        });

        // Setup uniform bind group.

        let uni_bg = output_device.create_bind_group(&BindGroupDescriptor {
            label: None,
            layout: &uni_bg_layout,
            entries: &[
                BindGroupEntry {
                    binding: 0, // See vertex/fragment shader->@binding().
                    resource: BindingResource::Buffer(BufferBinding {
                        buffer: &buf,
                        offset: 0,
                        size: BufferSize::new(uni_size),
                    }),
                }
            ]
        });

        // Allocate offset and sort buffer.

        let offsets = Box::from_iter(render_infos.iter().map(|render_info|
            Box::from_iter(iter::repeat_n(0, render_info.inst_sh_infos.len()))
        ));

        let sort_infos = Box::from_iter(iter::repeat_n(Default::default(), sort_infos_len as usize));

        let inner = Inner {
            offsets,
            sort_infos,
            render_buf_opt: None,
        };

        Self {
            output_device,
            stats,
            uni_size,
            uni_bg,
            render_infos,
            buf,
            inner: RefCell::new(inner),
            in_out,
        }
    }

    #[expect(clippy::too_many_arguments)]
    fn fill_and_draw<T: Iterator<Item = u32>>(render_pass: &mut RenderPass, buf: &Buffer, buf_view: &mut QueueWriteBufferView, render_infos: &[RenderInfo], offsets: &mut [Box<[u64]>], current_render_info_index_opt: &mut Option<u16>, render_info_index: u16, inst_index: u16, model_index_it: T, model_index_it_len: u32, stat_draw_calls: &mut u32) {
        let render_info = &render_infos[render_info_index as usize];
        let mesh = &render_info.mesh;
        let submesh = &mesh.get_submeshes()[inst_index as usize];
        let inst_sh_info = &render_info.inst_sh_infos[inst_index as usize];

        let offset_insts = &mut offsets[render_info_index as usize];
        let offset = &mut offset_insts[inst_index as usize];
        let end = *offset + model_index_it_len as u64 * inst_sh_info.inst_size;

        macro_rules! fill_inst_buf {
            ($(($inst_sh_type:ident, $method:ident)),*) => {
                match submesh.get_inst_sh_type() {
                    $(
                        InstShaderType::$inst_sh_type => {
                            let (inst_buf_arr, _) = buf_view.slice(*offset as usize..end as usize).into_chunks::<{InstShaderSize::$inst_sh_type}>();

                            inst_buf_arr.write_iter(model_index_it.map(|model_index| {
                                let model = &render_info.models[model_index as usize];
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
            (Obstacle, fill_obstacle)
        );

        if current_render_info_index_opt.is_none_or(|current_render_info_index| current_render_info_index != render_info_index) {
            render_pass.set_vertex_buffer(0, mesh.get_vertex_buf().slice(..)); // See VertexState->buffers[0].
            render_pass.set_index_buffer(mesh.get_index_buf().slice(..), IndexFormat::Uint16);

            current_render_info_index_opt.replace(render_info_index);
        }

        render_pass.set_pipeline(&inst_sh_info.pipeline);
        render_pass.set_vertex_buffer(1, buf.slice(*offset..end)); // See VertexState->buffers[1].

        if let Some(bg) = &inst_sh_info.bg_opt {
            render_pass.set_bind_group(1, bg, &[]); // See OutputPipelineLayoutDescriptor->bind_group_layouts.
        }

        render_pass.draw_indexed(submesh.get_indices(), submesh.get_base_vertex(), 0..model_index_it_len);

        *offset = end;
        *stat_draw_calls += 1;
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

        let mut inner = self.inner.borrow_mut();
        inner.render_buf_opt = Some(RenderBuf {
            multisample_view_opt,
            depth_view,
        });
    }

    fn render(&self, encoder: &mut CommandEncoder, frame: &dyn Frame) {
        let inner = &mut *self.inner.borrow_mut();

        // Setup render pass.

        let render_buf = inner.render_buf_opt.as_ref().unwrap();
        let output_info = self.output_device.get_output_info();
        let multisample_view_opt = render_buf.multisample_view_opt.as_ref();
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
                    store: multisample_view_opt.map_or(StoreOp::Store, |_| StoreOp::Discard), // MSAA: Discard MSAA buffer, retain resolve buffer.
                },
            })],
            depth_stencil_attachment: Some(RenderPassDepthStencilAttachment {
                view: &render_buf.depth_view,
                depth_ops: Some(Operations {
                    load: LoadOp::Clear(0.0),
                    store: StoreOp::Discard,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: output_info.get_view_mask(),
        });

        // Count number of visible instances to be rendered.

        let mut stat_inst_num = 0;
        let mut current_offset = self.uni_size;

        for (render_info, offset_insts) in self.render_infos.iter().zip(inner.offsets.iter_mut()) {
            let visibles = render_info.visibles.borrow();

            for ((inst_sh_info, offset), model_indexes) in render_info.inst_sh_infos.iter().zip(offset_insts.iter_mut()).zip(visibles.iter()) {
                let model_indexes_len = model_indexes.len();

                *offset = current_offset;
                current_offset += model_indexes_len as u64 * inst_sh_info.inst_size;

                stat_inst_num += model_indexes_len as u32;
            }
        }

        // Map buffer:
        // - From https://docs.rs/wgpu/latest/wgpu/struct.Queue.html#method.write_buffer_with :
        //   "Dropping the QueueWriteBufferView does not submit the transfer to the GPU immediately. The transfer begins only on the next call to Queue::submit() after the view is dropped, just before the explicitly submitted commands."
        // - Don't map full buffer, only the part which is large enough to hold the uploaded data.

        let mut buf_view = self.output_device.write_buffer_with(&self.buf, 0, BufferSize::new(current_offset).unwrap()).unwrap();

        // Fill uniform buffer.
        // TODO: Refactor it to be typesafe (e.g. Uni<Frame::OutputViewMat>)

        let cam_pos = frame.get_cam_pos();

        let mut uni = Uni::zeroed();
        uni.light_pos = cgmath::Point3::new(0.0, -3.0, 3.0).into(); // TODO: where should we position the light?
        uni.cam_pos = (*cam_pos).into();

        let mut uni_buf_sl = buf_view.slice(..mem::size_of::<Uni>());
        uni_buf_sl.copy_from_slice(bytemuck::cast_slice(&[uni]));
        
        let mut uni_buf_sl = buf_view.slice(mem::size_of::<Uni>()..self.uni_size as usize);
        uni_buf_sl.copy_from_slice(frame.get_view_m());

        render_pass.set_bind_group(0, &self.uni_bg, &[]); // See OutputPipelineLayoutDescriptor->bind_group_layouts.

        // Do render.
        // TODO: implement frustum culling: don't submit objects to GPU which are outside the view frustum.
        // TODO: implement dirty flag: only upload what is changed.

        let mut stat_draw_calls = 0;
        let mut current_render_info_index_opt = None;

        let sort_infos = &mut inner.sort_infos;
        let mut sort_infos_len = 0;

        for (render_info_index, render_info) in self.render_infos.iter().enumerate() {
            let render_info_index = render_info_index as u16;
            let visibles = render_info.visibles.borrow();

            for (inst_index, (submesh, model_indexes)) in render_info.mesh.get_submeshes().iter().zip(visibles.iter()).enumerate() {
                let model_index_it_len = model_indexes.len() as u32;
                if model_index_it_len > 0 {
                    let inst_index = inst_index as u16;

                    if !submesh.get_inst_sh_type().is_sorted() {
                        // Sorting is not needed (e.g. opaque objects), draw them.

                        let model_index_it = model_indexes.iter().copied();
                        Self::fill_and_draw(&mut render_pass, &self.buf, &mut buf_view, &self.render_infos, &mut inner.offsets, &mut current_render_info_index_opt, render_info_index, inst_index, model_index_it, model_index_it_len, &mut stat_draw_calls);
                    } else {
                        // Sorting is needed (e.g. transparent objects), collect them.

                        let end = sort_infos_len + model_index_it_len as usize;

                        for (model_index, sort_info) in model_indexes.iter().zip(sort_infos[sort_infos_len..end].iter_mut()) {
                            let model = &render_info.models[*model_index as usize];
                            let pos = model.get_pos(inst_index);
                            let d = (pos - cam_pos).magnitude2(); // It is faster than magnitude().

                            *sort_info = SortInfo::new(render_info_index, inst_index, *model_index, d);
                        }

                        sort_infos_len = end;
                    }
                }
            }
        }

        // Sort back-to-front. If distance is equal, then sort by render_info_index, inst_index
        // and model_index, so equal distance models are rendered in the same order, reducing flicker.

        let sort_infos = &mut sort_infos[..sort_infos_len];
        sort_infos.sort_unstable_by(|sort_info1, sort_info2|
            sort_info1.d.total_cmp(&sort_info2.d).reverse()
            .then_with(|| sort_info1.render_info_index.cmp(&sort_info2.render_info_index))
            .then_with(|| sort_info1.inst_index.cmp(&sort_info2.inst_index))
            .then_with(|| sort_info1.model_index.cmp(&sort_info2.model_index))
        );

        // Draw.

        let mut sort_infos_index = 0;

        while sort_infos_index < sort_infos_len {
            let sort_info = &sort_infos[sort_infos_index];
            let render_info_index = sort_info.render_info_index;
            let inst_index = sort_info.inst_index;

            // As long as render_info_index and inst_index are the same, we can do
            // instancing.

            let start = sort_infos_index + 1;
            let end = sort_infos[start..].iter().take_while(|sort_info_cmp| render_info_index == sort_info_cmp.render_info_index && inst_index == sort_info_cmp.inst_index).count() + start;

            let model_index_it = sort_infos[sort_infos_index..end].iter().map(|sort_info| sort_info.model_index);
            let model_index_it_len = (end - sort_infos_index) as u32;

            Self::fill_and_draw(&mut render_pass, &self.buf, &mut buf_view, &self.render_infos, &mut inner.offsets, &mut current_render_info_index_opt, render_info_index, inst_index, model_index_it, model_index_it_len, &mut stat_draw_calls);
            sort_infos_index = end;
        }

        // Update stats.

        {
            let mut stats_inner = self.stats.get_inner_mut();
            stats_inner.draw_calls += stat_draw_calls;
            stats_inner.inst_num += stat_inst_num;
            stats_inner.buf_upload += current_offset as u32;
        }
    }
}

#[derive(Clone, Default)]
struct SortInfo {
    render_info_index: u16,
    inst_index: u16,
    model_index: u32,
    d: f32,
}

impl SortInfo {
    fn new(render_info_index: u16, inst_index: u16, model_index: u32, d: f32) -> Self {
        Self {
            render_info_index,
            inst_index,
            model_index,
            d,
        }
    }
}
