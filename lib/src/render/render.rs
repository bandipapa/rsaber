use std::cell::RefCell;
use std::mem;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicI32, Ordering};
use std::time::Instant;

use wgpu::{Buffer, BufferDescriptor, BufferUsages, CommandEncoderDescriptor, MapMode, QuerySet, QuerySetDescriptor, QueryType};

use crate::asset::AssetManagerRc;
use crate::audio::AudioEngineRc;
use crate::output::{Frame, OutputDeviceRc};
use crate::scene::{MenuParam, SceneInput, SceneManager};
use crate::util::StatsRc;

const QUERY_COUNT: u32 = 2;
const QUERY_SIZE: u64 = QUERY_COUNT as u64 * mem::size_of::<u64>() as u64; // TODO: use constant wgpu::QUERY_SIZE.

pub struct Render {
    output_device: OutputDeviceRc,
    stats: StatsRc,
    query_set: QuerySet,
    query_resolve_buf: Buffer,
    query_result_buf: Buffer,
    frame_time: Arc<AtomicI32>, // [ms]
    scene_mgr: SceneManager,
    inner: RefCell<Inner>,
}

struct Inner {
    instant: Instant,
    fps: u32,
}

impl Render {
    pub fn new(asset_mgr: AssetManagerRc, output_device: OutputDeviceRc, stats: StatsRc, audio_engine: AudioEngineRc) -> Self {
        // Create query set to measure GPU execution time.

        let query_set = output_device.create_query_set(&QuerySetDescriptor {
            label: None,
            ty: QueryType::Timestamp,
            count: QUERY_COUNT,
        });

        let query_resolve_buf = output_device.create_buffer(&BufferDescriptor {
            label: None,
            size: QUERY_SIZE,
            usage: BufferUsages::QUERY_RESOLVE | BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });

        let query_result_buf = output_device.create_buffer(&BufferDescriptor {
            label: None,
            size: QUERY_SIZE,
            usage: BufferUsages::MAP_READ | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let frame_time = Arc::new(AtomicI32::new(0));

        // Create scene manager and load start scene.

        let scene_mgr = SceneManager::new(Arc::clone(&asset_mgr), Rc::clone(&output_device), Arc::clone(&stats), audio_engine);
        scene_mgr.load(MenuParam::new()).expect("Unable to load scene");

        let inner = Inner {
            instant: Instant::now(),
            fps: 0,
        };

        Self {
            output_device,
            stats,
            query_set,
            query_resolve_buf,
            query_result_buf,
            frame_time,
            scene_mgr,
            inner: RefCell::new(inner),
        }
    }

    pub fn configure(&self, width: u32, height: u32) {
        self.scene_mgr.configure(width, height);
    }

    pub fn render<F: Frame>(&self, frame: F, scene_input: &SceneInput) {
        // Update stats.

        let mut do_query = false;

        {
            let mut inner = self.inner.borrow_mut();
            inner.fps += 1;

            let mut stats_inner = self.stats.get_inner_mut();

            let instant = Instant::now();
            if instant.duration_since(inner.instant).as_secs_f32() >= 1.0 {
                stats_inner.fps = inner.fps;

                inner.fps = 0;
                inner.instant = instant;
            }

            // TODO: Use frame_time statistic to offset audio timestamp, moving average?

            let frame_time = self.frame_time.load(Ordering::Relaxed);
            if frame_time >= 0 {
                stats_inner.frame_time = frame_time.try_into().unwrap();

                do_query = true;
            }

            stats_inner.draw_calls = 0;
            stats_inner.inst_num = 0;
            stats_inner.buf_upload = 0;
        }

        // Do render.

        let mut encoder = self.output_device.create_command_encoder(&CommandEncoderDescriptor {
            label: None,
        });

        // TODO: From https://docs.rs/wgpu/latest/wgpu/struct.CommandEncoder.html#method.write_timestamp:
        // "Since commands within a command recorder may be reordered, there is no strict guarantee that
        // timestamps are taken after all commands recorded so far and all before all commands recorded
        // after. This may depend both on the backend and the driver."

        encoder.write_timestamp(&self.query_set, 0);
        self.scene_mgr.render(scene_input, &mut encoder, &frame);
        encoder.write_timestamp(&self.query_set, 1);

        encoder.resolve_query_set(&self.query_set, 0..QUERY_COUNT, &self.query_resolve_buf, 0);

        if do_query {
            encoder.copy_buffer_to_buffer(&self.query_resolve_buf, 0, &self.query_result_buf, 0, None);
        }

        // Submit.

        self.output_device.submit([encoder.finish()]);

        // Finish & consume frame.

        frame.end();

        if do_query {
            self.frame_time.store(-1, Ordering::Relaxed);

            self.query_result_buf.map_async(MapMode::Read, 0..QUERY_SIZE, {
                let query_result_buf = self.query_result_buf.clone();
                let frame_time = Arc::clone(&self.frame_time);
                let ts_period = self.output_device.get_timestamp_period();

                move |r| { // TODO: use map_buffer_on_submit?
                    r.expect("Unable to map buffer");

                    let t;

                    {
                        let buf = query_result_buf.slice(0..QUERY_SIZE).get_mapped_range().expect("get_mapped_range() failed");
                        let values: &[u64] = bytemuck::cast_slice(&buf);
                        let start = values[0];
                        let end = values[1];
                        t = (end.wrapping_sub(start) as f64 * ts_period as f64 / 1_000_000.0) as i32;
                        assert!(t >= 0);
                    }

                    query_result_buf.unmap();
                    frame_time.store(t, Ordering::Relaxed);
                }
            });
        }
    }
}
