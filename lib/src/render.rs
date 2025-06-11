use std::mem;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicI32, Ordering};

use bytemuck::{Pod, Zeroable};
use wgpu::{BindGroup, BindGroupDescriptor, BindGroupEntry, BindGroupLayoutDescriptor, BindGroupLayoutEntry, BindingType, Buffer, BufferBindingType, BufferDescriptor, BufferSize, BufferUsages, Color, CommandEncoderDescriptor, LoadOp, MapMode, Operations, PipelineLayoutDescriptor, QuerySet, QuerySetDescriptor, QueryType, RenderPassColorAttachment, RenderPassDepthStencilAttachment, RenderPassDescriptor, RenderPassTimestampWrites, ShaderStages, StoreOp};

use crate::AssetManagerRc;
use crate::audio::AudioEngineRc;
use crate::output::{Frame, OutputInfoRc, ViewMat};
use crate::scene::{GameParam, SceneInput, SceneManager};

const QUERY_COUNT: u32 = 2;
const QUERY_SIZE: u64 = QUERY_COUNT as u64 * mem::size_of::<u64>() as u64; // TODO: use constant wgpu::QUERY_SIZE.

// Keep render->Uni and modelrender->UNI_TMPL in-sync.
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
struct Uni {
    light_pos: [f32; 3],
    _unused1: f32,
    cam_pos: [f32; 3],
    _unused2: f32,
    // Rest is filled by Frame->set_view_m(). // TODO: Or have an additional, output-specific buffer?
}

pub struct Render {
    output_info: OutputInfoRc,
    query_set: QuerySet,
    query_resolve_buf: Buffer,
    query_result_buf: Buffer,
    render_time: Arc<AtomicI32>, // [us]
    uni_size: u64,
    uni_buf: Buffer,
    bg: BindGroup,
    scene_mgr: SceneManager,
}

impl Render {
    pub fn new(asset_mgr: AssetManagerRc, output_info: OutputInfoRc, audio_engine: AudioEngineRc) -> Self {
        let device = output_info.get_device();

        // Create query set to measure GPU execution time.

        let query_set = device.create_query_set(&QuerySetDescriptor {
            label: None,
            ty: QueryType::Timestamp,
            count: QUERY_COUNT,
        });

        let query_resolve_buf = device.create_buffer(&BufferDescriptor {
            label: None,
            size: QUERY_SIZE,
            usage: BufferUsages::QUERY_RESOLVE | BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });

        let query_result_buf = device.create_buffer(&BufferDescriptor {
            label: None,
            size: QUERY_SIZE,
            usage: BufferUsages::MAP_READ | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let render_time = Arc::new(AtomicI32::new(0));

        // Allocate uniform buffer.

        let uni_size = (mem::size_of::<Uni>() + mem::size_of::<ViewMat>() * output_info.get_view_len() as usize).try_into().unwrap();

        let uni_buf = device.create_buffer(&BufferDescriptor {
            label: None,
            size: uni_size,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let bg_layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: None,
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

        let bg: BindGroup = device.create_bind_group(&BindGroupDescriptor {
            label: None,
            layout: &bg_layout,
            entries: &[
                BindGroupEntry {
                    binding: 0, // See vertex/fragment shader->@binding().
                    resource: uni_buf.as_entire_binding(),
                }
            ]
        });

        // Create pipeline layout.

        let pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[
                &bg_layout, // See vertex/fragment shader->@group().
            ],
            push_constant_ranges: &[],
        });

        // Create scene manager and load start scene.

        let scene_mgr = SceneManager::new(Rc::clone(&asset_mgr), Rc::clone(&output_info), pipeline_layout, audio_engine);
        scene_mgr.load(GameParam::get_demo(asset_mgr));

        Self {
            output_info,
            query_set,
            query_resolve_buf,
            query_result_buf,
            render_time,
            uni_size,
            uni_buf,
            bg,
            scene_mgr,
        }
    }

    pub fn render<F: Frame>(&self, frame: F, scene_input: &SceneInput) {
        let queue = self.output_info.get_queue();

        // Fill uniform buffer. From https://docs.rs/wgpu/latest/wgpu/struct.Queue.html#method.write_buffer_with :
        // "Dropping the QueueWriteBufferView does not submit the transfer to the GPU immediately. The transfer begins only on the next call to Queue::submit() after the view is dropped, just before the explicitly submitted commands."

        {
            let mut uni_buf_view = queue.write_buffer_with(&self.uni_buf, 0, BufferSize::new(self.uni_size).unwrap()).unwrap();

            let uni_buf_sl: &mut [Uni] = bytemuck::cast_slice_mut(&mut uni_buf_view[..mem::size_of::<Uni>()]);
            let uni = &mut uni_buf_sl[0];

            uni.light_pos = cgmath::Point3::new(0.0, -3.0, 3.0).into(); // TODO: where should we position the light?
            uni.cam_pos = frame.get_cam_pos().into();

            frame.set_view_m(&mut uni_buf_view[mem::size_of::<Uni>()..]); // TODO: Refactor it to be typesafe?
        }

        // TODO: Display render_time statistic and use it to offset audio timestamp, moving average?

        let mut do_query = false;

        let render_time = self.render_time.load(Ordering::Relaxed);
        if render_time >= 0 {
            do_query = true;
        }

        // Do render pass.

        let device = self.output_info.get_device();
        let mut encoder = device.create_command_encoder(&CommandEncoderDescriptor {
            label: None,
        });

        {
            let color_view = frame.get_color_view();
            let multisample_view = frame.get_multisample_view();

            let mut render_pass = encoder.begin_render_pass(&RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(RenderPassColorAttachment { // See fragment shader->@location(0).
                    view: multisample_view.unwrap_or(color_view),
                    depth_slice: None,
                    resolve_target: multisample_view.map(|_| color_view),
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
                    view: frame.get_depth_view(),
                    depth_ops: Some(Operations {
                        load: LoadOp::Clear(1.0),
                        store: StoreOp::Discard,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: Some(RenderPassTimestampWrites {
                    query_set: &self.query_set,
                    beginning_of_pass_write_index: Some(0),
                    end_of_pass_write_index: Some(1),
                }),
                occlusion_query_set: None,
            });

            render_pass.set_bind_group(0, &self.bg, &[]); // See PipelineLayoutDescriptor->bind_group_layouts.
            self.scene_mgr.render(scene_input, &mut render_pass);
        }

        encoder.resolve_query_set(&self.query_set, 0..QUERY_COUNT, &self.query_resolve_buf, 0);

        if do_query {
            encoder.copy_buffer_to_buffer(&self.query_resolve_buf, 0, &self.query_result_buf, 0, None);
        }

        // Submit.

        queue.submit([encoder.finish()]);
        frame.end();

        if do_query {
            self.render_time.store(-1, Ordering::Relaxed);

            let query_result_buf = self.query_result_buf.clone();
            let render_time = Arc::clone(&self.render_time);
            let ts_period = self.output_info.get_queue().get_timestamp_period();

            self.query_result_buf.map_async(MapMode::Read, 0..QUERY_SIZE, move |r| {
                r.expect("Unable to map buffer");

                let t;

                {
                    let buf = query_result_buf.slice(0..QUERY_SIZE).get_mapped_range();
                    let values: &[u64] = bytemuck::cast_slice(&buf);
                    let start = values[0];
                    let end = values[1];
                    t = (end.wrapping_sub(start) as f64 * ts_period as f64 / 1_000.0) as i32;
                    assert!(t >= 0);
                }

                query_result_buf.unmap();
                render_time.store(t, Ordering::Relaxed);
            });
        }
    }
}
