use std::cell::RefCell;
use std::collections::HashSet;
use std::ffi::CString;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use ash::vk::Handle;
use cgmath::{Angle, Deg, Matrix4, Quaternion, Rad, Rotation3, Vector3, Zero};
use wgpu::{Device, DeviceDescriptor, Extent3d, Features, Instance, Queue, TextureDescriptor, TextureDimension, TextureFormat, TextureUsages, TextureView};

#[cfg(target_os = "android")]
use android_activity::AndroidApp;

use crate::{APP_NAME, APP_VERSION_MAJOR, APP_VERSION_MINOR, APP_VERSION_PATCH, Main};
use crate::scene::{SceneInput, ScenePose};
use crate::output::{DEPTH_FORMAT, NEAR_Z, FAR_Z, Frame, OutputInfo, ViewMat, create_texture, get_default_features, get_default_limits, get_sample_count};

type OutputViewMat = [ViewMat; 2];

const WGPU_FORMATS: [TextureFormat; 2] = [TextureFormat::Bgra8UnormSrgb, TextureFormat::Rgba8UnormSrgb];
const NOTRUNNING_SLEEP: f32 = 0.1; // [s]
const MY_TO_OPENXR_M: Matrix4<f32> = Matrix4::new( // my -> openxr
    1.0, 0.0, 0.0, 0.0,
    0.0, 0.0, -1.0, 0.0,
    0.0, 1.0, 0.0, 0.0,
    0.0, 0.0, 0.0, 1.0
);

pub struct XROutput {
    // wgpu
    device: Device,
    queue: Queue,
    color_format: TextureFormat,
    color_views: Box<[TextureView]>,
    depth_view: TextureView,
    sample_count: u32,
    multisample_view: Option<TextureView>,
    width: u32,
    height: u32,
    xr_session: openxr::Session<openxr::Vulkan>,
    inner: RefCell<Inner>,
    xr_space: openxr::Space,
    xr_action_set: openxr::ActionSet,
    xr_left_space: openxr::Space,
    xr_right_space: openxr::Space,
    xr_left_click: openxr::Action<bool>,
    xr_right_click: openxr::Action<bool>,
    xr_left_haptic: openxr::Action<openxr::Haptic>,
    xr_right_haptic: openxr::Action<openxr::Haptic>,
    xr_inst: openxr::Instance,
}

struct Inner {
    state: State,
    event_buf: openxr::EventDataBuffer,
    xr_waiter: openxr::FrameWaiter,
    xr_stream: openxr::FrameStream<openxr::Vulkan>,
    xr_swapchain: openxr::Swapchain<openxr::Vulkan>,
}

#[derive(Clone, Copy, PartialEq)]
enum State {
    Stopped,
    Ready,
    Visible,
    Focused,
    Exit,
}

#[allow(clippy::large_enum_variant)]
enum Begin<'a> {
    NoRender,
    Frame((XRFrame<'a>, Option<XRPose>, Option<XRPose>)),
}

impl XROutput {
    pub fn new(xr_entry: openxr::Entry) -> Self {
        // This code is based on:
        // - https://openxr-tutorial.com/index.html
        // - https://github.com/rust-mobile/rust-android-examples/blob/main/na-openxr-wgpu/src/lib.rs
        // - https://github.com/philpax/wgpu-openxr-example

        let app_version_major: u8 = APP_VERSION_MAJOR.parse().unwrap();
        let app_version_minor: u8 = APP_VERSION_MINOR.parse().unwrap();
        let app_version_patch: u8 = APP_VERSION_PATCH.parse().unwrap();

        let app_version = (app_version_major as u32) << 24 | (app_version_minor as u32) << 16 | app_version_patch as u32;

        let wgpu_hal_flags = wgpu::InstanceFlags::default();

        // Use fullly qualified names for Vulkan/OpenXR, since they have
        // similar named structs.

        // Load Vulkan.

        let vk_entry = unsafe { ash::Entry::load() }.expect("Unable to load Vulkan");
        let drop_guard = Arc::new(Mutex::new(DropGuard::new(vk_entry.clone())));

        // Create OpenXR instance:
        // - Don't let OpenXR to create vulkan instance and device (khr_vulkan_enable2).
        // - Instead, we create it manually (khr_vulkan_enable), since we need to tell
        //   wgpu which extensions are actually enabled.
        
        let xr_app_info = openxr::ApplicationInfo {
            application_name: APP_NAME,
            application_version: app_version,
            engine_name: APP_NAME,
            engine_version: app_version,
            ..Default::default()
        };

        let xr_ext_avail = xr_entry.enumerate_extensions().expect("Unable to query OpenXR extensions");

        let mut xr_ext = openxr::ExtensionSet::default();
        xr_ext.khr_vulkan_enable = true;

        if xr_ext_avail.fb_display_refresh_rate {
            xr_ext.fb_display_refresh_rate = true;
        }

        #[cfg(target_os = "android")]
        {
            xr_ext.khr_android_create_instance = true;
        }

        let xr_inst = xr_entry.create_instance(&xr_app_info, &xr_ext, &[]).expect("Unable to create OpenXR instance");
        let xr_system = xr_inst.system(openxr::FormFactor::HEAD_MOUNTED_DISPLAY).expect("OpenXR system() failed, make sure the headset is connected");

        // Check Vulkan/OpenXR compatibility.

        let xr_gfx_req = xr_inst.graphics_requirements::<openxr::Vulkan>(xr_system).expect("OpenXR graphics_requirements() failed");
        let vk_version = unsafe { vk_entry.try_enumerate_instance_version() }.expect("Vulkan try_enumerate_instance_version() failed").unwrap_or(ash::vk::API_VERSION_1_0);
        let vk_version_conv = openxr::Version::new(ash::vk::api_version_major(vk_version).try_into().unwrap(), ash::vk::api_version_minor(vk_version).try_into().unwrap(), ash::vk::api_version_patch(vk_version));

        // TODO: versions on my system: vk_version_conv = 1.4.309, xr_gfx_req.max_api_version_supported = 1.2.0
        // So at the moment, don't check for max_api_version_supported.

        if vk_version_conv < xr_gfx_req.min_api_version_supported {
            panic!("Vulkan version {} mismatch, OpenXR min supported version = {}", vk_version_conv, xr_gfx_req.min_api_version_supported);
        }

        // Create Vulkan instance:
        // - Query wgpu required extensions.
        // - Query OpenXR required extensions.

        let vk_app_name = CString::new(APP_NAME).unwrap();

        let vk_app_info = ash::vk::ApplicationInfo::default()
            .application_name(&vk_app_name)
            .application_version(app_version)
            .engine_name(&vk_app_name)
            .engine_version(app_version);

        let wgpu_exts = wgpu::hal::vulkan::Instance::desired_extensions(&vk_entry, vk_version, wgpu_hal_flags).expect("wgpu desired_extensions() failed").into_iter().map(|s| s.to_str().unwrap());
        let xr_exts_str = xr_inst.vulkan_legacy_instance_extensions(xr_system).expect("OpenXR vulkan_legacy_instance_extensions() failed");
        let xr_exts = xr_exts_str.split_ascii_whitespace();

        let exts: HashSet<_> = HashSet::from_iter(wgpu_exts.chain(xr_exts)); // Deduplicate.
        let exts_c: Box<[_]> = exts.into_iter().map(|s| CString::new(s).unwrap()).collect();
        let exts_c_ptr: Box<[_]> = exts_c.iter().map(|s| s.as_ptr()).collect();

        let vk_inst_create_info = ash::vk::InstanceCreateInfo::default()
            .application_info(&vk_app_info)
            .enabled_extension_names(&exts_c_ptr);

        let vk_inst = unsafe { vk_entry.create_instance(&vk_inst_create_info, None) }.expect("Unable to create Vulkan instance");
        drop_guard.lock().unwrap().set_vk_inst(vk_inst.clone());

        // Get suitable Vulkan physical device.

        let vk_phys_dev_handle = unsafe { xr_inst.vulkan_graphics_device(xr_system, vk_inst.handle().as_raw() as _) }.expect("OpenXR vulkan_graphics_device() failed");
        let vk_phys_dev = ash::vk::PhysicalDevice::from_raw(vk_phys_dev_handle as _);

        // Find graphics queue.

        let vk_queue_families = unsafe { vk_inst.get_physical_device_queue_family_properties(vk_phys_dev) };
        let vk_queue_family_index = vk_queue_families
            .into_iter()
            .enumerate()
            .find_map(|(family_index, family)| {
                if family.queue_flags.contains(ash::vk::QueueFlags::GRAPHICS) {
                    Some(family_index.try_into().unwrap())
                } else {
                    None
                }
            })
            .expect("Unable to find suitable graphics queue");

        let vk_queue_create_info = ash::vk::DeviceQueueCreateInfo::default()
            .queue_family_index(vk_queue_family_index)
            .queue_priorities(&[1.0]);
        let vk_queue_create_infos = [vk_queue_create_info];

        // Init wgpu.

        let wgpu_hal_exts: Vec<_> = exts_c.into_iter().map(|s| Box::leak(Box::new(s)).as_c_str()).collect(); // TODO: How to do it without leak?

        #[allow(unused_assignments)]
        #[allow(unused_mut)]
        let mut android_sdk_version = 0;
        #[cfg(target_os = "android")]
        {
            android_sdk_version = AndroidApp::sdk_version().try_into().unwrap();
        }

        // Dummy closure is created to hold drop_guard.

        let drop_callback: Option<wgpu::hal::DropCallback> = {
            let drop_guard = Arc::clone(&drop_guard);
            Some(Box::new(move || { let _ = Arc::strong_count(&drop_guard); }))
        };
        let wgpu_hal_inst = unsafe { wgpu::hal::vulkan::Instance::from_raw(vk_entry, vk_inst.clone(), vk_version, android_sdk_version, None, wgpu_hal_exts, wgpu_hal_flags, Default::default(), false, drop_callback) }.expect("wgpu from_raw() failed");
        let wgpu_hal_adapter = wgpu_hal_inst.expose_adapter(vk_phys_dev).expect("wgpu expose_adapter() failed");

        // Create Vulkan device:
        // - Query wgpu required extensions.
        // - Query OpenXR required extensions.

        let wgpu_features = get_default_features() | Features::MULTIVIEW | Features::MULTISAMPLE_ARRAY;

        let wgpu_exts = wgpu_hal_adapter.adapter.required_device_extensions(wgpu_features).into_iter().map(|s| s.to_str().unwrap());
        let xr_exts_str = xr_inst.vulkan_legacy_device_extensions(xr_system).expect("OpenXR vulkan_legacy_device_extensions() failed");
        let xr_exts = xr_exts_str.split_ascii_whitespace();

        let mut exts: HashSet<_> = HashSet::from_iter(wgpu_exts.chain(xr_exts)); // Deduplicate.
        exts.insert(ash::khr::multiview::NAME.to_str().unwrap()); // TODO: Why do we need it to put multiview into exts, since we have it in wgpu_features?
        let exts_c: Box<[_]> = exts.iter().map(|s| CString::new(*s).unwrap()).collect();
        let exts_c_ptr: Box<[_]> = exts_c.iter().map(|s| s.as_ptr()).collect();

        let vk_dev_create_info = ash::vk::DeviceCreateInfo::default()
            .queue_create_infos(&vk_queue_create_infos)
            .enabled_extension_names(&exts_c_ptr);

        let wgpu_phys_exts: Box<[_]> = exts_c.into_iter().map(|s| Box::leak(Box::new(s)).as_c_str()).collect(); // TODO: How to do it without leak?
        let mut wgpu_phys_features = wgpu_hal_adapter.adapter.physical_device_features(&wgpu_phys_exts, wgpu_features);
        let vk_dev_create_info2 = wgpu_phys_features.add_to_device_create(vk_dev_create_info);

        let vk_dev = unsafe { vk_inst.create_device(vk_phys_dev, &vk_dev_create_info2, None) }.expect("Vulkan create_device() failed");
        drop_guard.lock().unwrap().set_vk_dev(vk_dev.clone());

        // Create OpenXR session.

        let xr_session_create_info = openxr::vulkan::SessionCreateInfo {
            instance: vk_inst.handle().as_raw() as _,
            physical_device: vk_phys_dev_handle,
            device: vk_dev.handle().as_raw() as _,
            queue_family_index: vk_queue_family_index,
            queue_index: 0,
        };

        let (xr_session, xr_waiter, xr_stream) = unsafe { xr_inst.create_session_with_guard::<openxr::Vulkan>(xr_system, &xr_session_create_info, Box::new(Arc::clone(&drop_guard))) }.expect("Unable to create OpenXR session");

        // Set display refresh rate to max.

        if xr_ext.fb_display_refresh_rate {
            let rates = xr_session.enumerate_display_refresh_rates().expect("Unable to query display refresh rates");
            if let Some(rate) = rates.into_iter().reduce(f32::max) {
                xr_session.request_display_refresh_rate(rate).expect("Unable to set display refresh rate");
            }
        }

        // Query color formats.

        let xr_formats = xr_session.enumerate_swapchain_formats().expect("OpenXR enumerate_swapchain_formats() failed");
        let mut format_info = None;

        for xr_format in xr_formats { // TODO: how to figure out wgpu TextureFormat from raw format?
            format_info = WGPU_FORMATS.iter().find_map(|wgpu_format| {
                if wgpu_hal_adapter.adapter.texture_format_as_raw(*wgpu_format).as_raw() == xr_format as i32 {
                    Some((xr_format, *wgpu_format))
                } else {
                    None
                }
            });

            if format_info.is_some() {
                break;
            }
        };

        let format_info = format_info.expect("Unable to select swapchain format");
        let color_format = format_info.1;

        // Create wgpu device.

        // Dummy closure is created to hold drop_guard.

        let drop_callback: Option<wgpu::hal::DropCallback> = {
            let drop_guard = Arc::clone(&drop_guard);
            Some(Box::new(move || { let _ = Arc::strong_count(&drop_guard); }))
        };
        let wgpu_hal_dev = unsafe { wgpu_hal_adapter.adapter.device_from_raw(vk_dev.clone(), drop_callback, &wgpu_phys_exts, wgpu_features, &Default::default(), vk_queue_family_index, 0) }.expect("wgpu device_from_raw() failed");
        
        let wgpu_inst = unsafe { Instance::from_hal::<wgpu::hal::vulkan::Api>(wgpu_hal_inst) };
        let wgpu_adapter = unsafe { wgpu_inst.create_adapter_from_hal(wgpu_hal_adapter) };

        let mut wgpu_limits = get_default_limits();
        wgpu_limits.max_multiview_view_count = 2;

        let device_desc = DeviceDescriptor {
            required_features: wgpu_features,
            required_limits: wgpu_limits,
            ..Default::default()
        };
        let (device, queue) = unsafe { wgpu_adapter.create_device_from_hal(wgpu_hal_dev, &device_desc) }.expect("wgpu create_device_from_hal() failed");

        // Setup swapchain.

        let xr_views = xr_inst.enumerate_view_configuration_views(xr_system, openxr::ViewConfigurationType::PRIMARY_STEREO).expect("OpenXR enumerate_view_configuration_views() failed");
        assert!(xr_views.len() == 2); // Make sure we have stereo configuration.
        assert!(xr_views[0] == xr_views[1]);

        let mut width = xr_views[0].recommended_image_rect_width;
        let mut height = xr_views[0].recommended_image_rect_height;

        // TODO: Use array/hashmap to search for tweaks.
        let xr_system_prop = xr_inst.system_properties(xr_system).expect("OpenXR system_properties() failed");
        if xr_system_prop.vendor_id == 10291 && xr_system_prop.system_name == "Oculus Quest2" {
            // Tweak Quest 2 resolution from default (1440x1584) to maximum.

            width = 1832;
            height = 1920;
        } else if xr_system_prop.vendor_id == 10462 && xr_system_prop.system_name == "SteamVR/OpenXR : playstation_vr2" {
            // Tweak view size, since OpenXR reported values are incorrect:
            // - recommended_image_rect_width: 4080
            // - recommended_image_rect_height: 4160

            width = 2000;
            height = 2040;
        }
        
        let xr_swapchain_create_info = openxr::SwapchainCreateInfo {
            create_flags: openxr::SwapchainCreateFlags::EMPTY,
            usage_flags: openxr::SwapchainUsageFlags::COLOR_ATTACHMENT,
            format: format_info.0,
            sample_count: 1,
            width,
            height,
            face_count: 1,
            array_size: 2,
            mip_count: 1,
        };

        let xr_swapchain = xr_session.create_swapchain(&xr_swapchain_create_info).expect("OpenXR create_swapchain() failed");
        let xr_swapchain_imgs = xr_swapchain.enumerate_images().expect("OpenXR enumerate_images() failed");

        let wgpu_color_descr_hal = wgpu::hal::TextureDescriptor {
            label: None,
            size: Extent3d {
                width,
                height,
                depth_or_array_layers: 2,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: color_format,
            usage: wgpu::wgt::TextureUses::COLOR_TARGET,
            memory_flags: wgpu::hal::MemoryFlags::empty(),
            view_formats: vec![],
        };

        let wgpu_color_descr = TextureDescriptor {
            label: None,
            size: Extent3d {
                width,
                height,
                depth_or_array_layers: 2,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: color_format,
            usage: TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        };

        let wgpu_hal_dev = unsafe { device.as_hal::<wgpu::hal::vulkan::Api>().unwrap() };

        let color_views = xr_swapchain_imgs.into_iter().map(|texture_raw| {
            let texture_handle = ash::vk::Image::from_raw(texture_raw);
            let texture_hal = unsafe { wgpu_hal_dev.texture_from_raw(texture_handle, &wgpu_color_descr_hal, Some(Box::new(|| {})), wgpu::hal::vulkan::TextureMemory::External) }; // Don't take ownership of the texture. // TODO: use dropguard?
            let texture = unsafe { device.create_texture_from_hal::<wgpu::hal::vulkan::Api>(texture_hal, &wgpu_color_descr) };
            texture.create_view(&Default::default())
        }).collect();

        // Setup multisample buffer.

        let sample_count = get_sample_count(&wgpu_adapter, color_format);

        let multisample_view = if sample_count > 1 {
            let texture = create_texture(&device, width, height, 2, sample_count, color_format);
            Some(texture.create_view(&Default::default()))
        } else {
            None
        };

        // Setup depth buffer.

        let depth_texture = create_texture(&device, width, height, 2, sample_count, DEPTH_FORMAT);
        let depth_view = depth_texture.create_view(&Default::default());

        // Setup input.

        let xr_space = xr_session.create_reference_space(openxr::ReferenceSpaceType::STAGE, openxr::Posef::IDENTITY).expect("OpenXR create_reference_space() failed");

        let xr_action_set = xr_inst.create_action_set("input", "Input", 0).expect("OpenXR create_action_set() failed");

        let xr_left_action = xr_action_set.create_action::<openxr::Posef>("left_hand", "Left Hand", &[]).expect("OpenXR create_action() failed");
        let xr_right_action = xr_action_set.create_action::<openxr::Posef>("right_hand", "Right Hand", &[]).expect("OpenXR create_action() failed");
        let xr_left_click = xr_action_set.create_action::<bool>("left_click", "Left Click", &[]).expect("OpenXR create_action() failed");
        let xr_right_click = xr_action_set.create_action::<bool>("right_click", "Right Click", &[]).expect("OpenXR create_action() failed");

        let xr_left_haptic = xr_action_set.create_action::<openxr::Haptic>("left_haptic", "Left Haptic", &[]).expect("OpenXR create_action() failed");
        let xr_right_haptic = xr_action_set.create_action::<openxr::Haptic>("right_haptic", "Right Haptic", &[]).expect("OpenXR create_action() failed");

        xr_inst.suggest_interaction_profile_bindings(
            xr_inst.string_to_path("/interaction_profiles/khr/simple_controller").expect("OpenXR string_to_path() failed"),
            &[
                openxr::Binding::new(
                    &xr_left_action,
                    xr_inst.string_to_path("/user/hand/left/input/aim/pose").expect("OpenXR string_to_path() failed"),
                ),
                openxr::Binding::new(
                    &xr_right_action,
                    xr_inst.string_to_path("/user/hand/right/input/aim/pose").expect("OpenXR string_to_path() failed"),
                ),
                openxr::Binding::new(
                    &xr_left_click,
                    xr_inst.string_to_path("/user/hand/left/input/select/click").expect("OpenXR string_to_path() failed"),
                ),
                openxr::Binding::new(
                    &xr_right_click,
                    xr_inst.string_to_path("/user/hand/right/input/select/click").expect("OpenXR string_to_path() failed"),
                ),
                openxr::Binding::new(
                    &xr_left_haptic,
                    xr_inst.string_to_path("/user/hand/left/output/haptic").expect("OpenXR string_to_path() failed"),
                ),
                openxr::Binding::new(
                    &xr_right_haptic,
                    xr_inst.string_to_path("/user/hand/right/output/haptic").expect("OpenXR string_to_path() failed"),
                )
            ]).expect("OpenXR suggest_interaction_profile_bindings() failed");

        xr_session.attach_action_sets(&[&xr_action_set]).expect("OpenXR attach_action_sets() failed");

        let xr_left_space = xr_left_action.create_space(xr_session.clone(), openxr::Path::NULL, openxr::Posef::IDENTITY).expect("OpenXR create_space() failed");
        let xr_right_space = xr_right_action.create_space(xr_session.clone(), openxr::Path::NULL, openxr::Posef::IDENTITY).expect("OpenXR create_space() failed");

        Self {
            device,
            queue,
            color_format,
            color_views,
            depth_view,
            sample_count,
            multisample_view,
            width,
            height,
            xr_session,
            inner: RefCell::new(Inner {
                state: State::Stopped,
                event_buf: openxr::EventDataBuffer::new(),
                xr_waiter,
                xr_stream,
                xr_swapchain,
            }),
            xr_space,
            xr_action_set,
            xr_left_space,
            xr_right_space,
            xr_left_click,
            xr_right_click,
            xr_left_haptic,
            xr_right_haptic,
            xr_inst,
        }
    }

    pub fn get_info(&self) -> OutputInfo { // TODO: prepare it it new and don't create new instance everytime?
        OutputInfo::new(&self.device, &self.queue, self.color_format, DEPTH_FORMAT, self.sample_count, 2, "@builtin(view_index) view_index: u32,", "in.view_index")
    }

    pub fn poll(&self, main: &Main) -> bool {
        // This is a common main loop method, which is shared by all VR targets.

        let inner = &mut *self.inner.borrow_mut();

        let old_state = inner.state;
        assert!(!matches!(old_state, State::Exit)); // Once exited, no more poll is possible.

        self.poll_impl(inner);
        let new_state = inner.state;

        match new_state {
            State::Stopped => {
                thread::sleep(Duration::from_secs_f32(NOTRUNNING_SLEEP));
            },
            State::Ready | State::Visible | State::Focused => {
                match self.begin(inner) {
                    Begin::NoRender => (),
                    Begin::Frame((frame, pose_l_opt, pose_r_opt)) => {
                        let scene_input = SceneInput {
                            pose_l_opt: pose_l_opt.as_ref().map(|pose| pose as &dyn ScenePose),
                            pose_r_opt: pose_r_opt.as_ref().map(|pose| pose as &dyn ScenePose),
                        };

                        main.render(frame, &scene_input);
                    },
                }
            },
            State::Exit => {
                return false;
            },
        };
        
        if old_state != new_state {
            let audio_engine = main.get_audio_engine();

            if matches!(new_state, State::Focused) {
                audio_engine.start();
            } else {
                audio_engine.pause();
            }
        }

        true
    }

    fn poll_impl(&self, inner: &mut Inner) {
        while let Some(event) = self.xr_inst.poll_event(&mut inner.event_buf).expect("OpenXR poll_event() failed") {
            match event {
                openxr::Event::SessionStateChanged(event) => {
                    match event.state() {
                        openxr::SessionState::READY => {
                            self.xr_session.begin(openxr::ViewConfigurationType::PRIMARY_STEREO).expect("OpenXR begin() failed");
                            inner.state = State::Ready;
                        },
                        openxr::SessionState::STOPPING => {
                            self.xr_session.end().expect("OpenXR end() failed");
                            inner.state = State::Stopped;
                        },
                        openxr::SessionState::FOCUSED => {
                            inner.state = State::Focused;
                        },
                        openxr::SessionState::VISIBLE => {
                            inner.state = State::Visible;
                        },
                        openxr::SessionState::EXITING | openxr::SessionState::LOSS_PENDING => {
                            inner.state = State::Exit;
                        },
                        _ => (),
                    }
                },
                openxr::Event::InstanceLossPending(_) => {
                    inner.state = State::Exit;
                },
                _ => (),
            };
        }
    }

    fn begin<'a>(&'a self, inner: &'a mut Inner) -> Begin<'a> {
        let xr_stream = &mut inner.xr_stream;

        let frame_state = inner.xr_waiter.wait().expect("OpenXR wait() failed");
        xr_stream.begin().expect("OpenXR begin() failed");

        let display_t = frame_state.predicted_display_time;

        if !frame_state.should_render { // See openxr::SessionState::SYNCHRONIZED.
            xr_stream.end(display_t, openxr::EnvironmentBlendMode::OPAQUE, &[]).expect("OpenXR end() failed");
            return Begin::NoRender;
        }

        // Acquire next image from swapchain.

        let xr_swapchain = &mut inner.xr_swapchain;
        let color_index = xr_swapchain.acquire_image().expect("OpenXR acquire_image() failed");
        xr_swapchain.wait_image(openxr::Duration::INFINITE).expect("OpenXR wait_image() failed");
        let color_view = self.color_views[color_index as usize].clone();

        // Handle input.

        self.xr_session.sync_actions(&[(&self.xr_action_set).into()]).expect("OpenXR sync_actions() failed");

        let focused = matches!(inner.state, State::Focused);

        let left_location = self.xr_left_space.locate(&self.xr_space, display_t).expect("OpenXR locate() failed");
        let right_location = self.xr_right_space.locate(&self.xr_space, display_t).expect("OpenXR locate() failed");

        let click_state_l = self.xr_left_click.state(&self.xr_session, openxr::Path::NULL).expect("OpenXR state() failed");
        let click_state_r = self.xr_right_click.state(&self.xr_session, openxr::Path::NULL).expect("OpenXR state() failed");

        let pose_l_opt = self.calc_pose(focused, &left_location, &click_state_l, &self.xr_left_haptic);
        let pose_r_opt = self.calc_pose(focused, &right_location, &click_state_r, &self.xr_right_haptic);
        
        // Calculate view matrices.

        let (_, views) = self.xr_session.locate_views(openxr::ViewConfigurationType::PRIMARY_STEREO, display_t, &self.xr_space).expect("OpenXR locate_views() failed");
        assert!(views.len() == 2);

        let mut view_m = [Matrix4::zero().into(), Matrix4::zero().into()];
        let mut cam_pos = Vector3::zero();

        for (view, view_m_single) in views.iter().zip(view_m.iter_mut()) {
            let pose = view.pose;

            // We are doing the pose matrix inversion manually, since it is trivial.

            let pos = pose.position;
            let pos_m = Matrix4::from_translation(Vector3::new(-pos.x, -pos.y, -pos.z));

            let rot = pose.orientation;
            let rot_m = Matrix4::from(Quaternion::new(rot.w, rot.x, rot.y, rot.z).conjugate());

            let cam_m = rot_m * pos_m;
            let proj_m = perspective(&view.fov, NEAR_Z, FAR_Z);
            let view_m_calc = proj_m * cam_m * MY_TO_OPENXR_M;

            *view_m_single = view_m_calc.into();

            cam_pos.x += pos.x / 2.0;
            cam_pos.y -= pos.z / 2.0;
            cam_pos.z += pos.y / 2.0;
        }

        let frame = XRFrame::new(xr_swapchain, xr_stream, &self.xr_space, self.width, self.height, display_t, views, color_view, self.multisample_view.clone(), self.depth_view.clone(), view_m, cam_pos);
        Begin::Frame((frame, pose_l_opt, pose_r_opt))
    }

    fn calc_pose(&self, focused: bool, location: &openxr::SpaceLocation, click_state: &openxr::ActionState<bool>, haptic: &openxr::Action<openxr::Haptic>) -> Option<XRPose> {
        if focused && location.location_flags.contains(openxr::SpaceLocationFlags::POSITION_VALID | openxr::SpaceLocationFlags::ORIENTATION_VALID) && click_state.is_active {
            let offset = Quaternion::from_angle_x(Deg(-45.0));

            let pos = location.pose.position;
            let rot = location.pose.orientation;
            let my_rot = Quaternion::new(rot.w, rot.x, -rot.z, rot.y) * offset;

            Some(XRPose::new(&Vector3::new(pos.x, -pos.z, pos.y), &my_rot, click_state.current_state, self.xr_session.clone(), haptic.clone()))
        } else {
            None
        }
    }
}

struct DropGuard {
    vk_inst: Option<ash::Instance>,
    vk_dev: Option<ash::Device>,
    _vk_entry: ash::Entry, // Make sure it is dropped last.
}

impl DropGuard {
    fn new(vk_entry: ash::Entry) -> Self {
        Self {
            vk_inst: None,
            vk_dev: None,
            _vk_entry: vk_entry,
        }
    }

    fn set_vk_inst(&mut self, vk_inst: ash::Instance) {
        assert!(self.vk_inst.is_none());
        self.vk_inst = Some(vk_inst);
    }

    fn set_vk_dev(&mut self, vk_dev: ash::Device) {
        assert!(self.vk_dev.is_none());
        self.vk_dev = Some(vk_dev);
    }
}

impl Drop for DropGuard {
    fn drop(&mut self) {
        // Implementation notes:
        // - OpenXR/wgpu are not going to take the ownership of the Vulkan handles created in XROutput->new().
        // - DropGuard is wrapped into an Arc, which is referenced by drop guards of OpenXR/wgpu. The handles
        //   are actually dropped, once the Arc reference count reaches 0.

        if let Some(vk_dev) = &self.vk_dev {
            // From https://docs.rs/ash/latest/ash/struct.Instance.html#method.create_device :
            // The application must not destroy the parent Instance object before first destroying the returned Device child object. Device does not implement drop semantics and can only be destroyed via destroy_device().
            
            unsafe { vk_dev.destroy_device(None) };
        }

        if let Some(vk_inst) = &self.vk_inst {
            // From https://docs.rs/ash/latest/ash/struct.Entry.html#method.create_instance :
            // Instance does not implement drop semantics and can only be destroyed via destroy_instance().
            
            unsafe { vk_inst.destroy_instance(None) };
        }

        // From https://docs.rs/ash/latest/ash/struct.Entry.html#method.load :
        // No Vulkan functions loaded directly or indirectly from this Entry may be called after it is dropped.
    }
}

struct XRFrame<'a> {
    xr_swapchain: &'a mut openxr::Swapchain<openxr::Vulkan>,
    xr_stream: &'a mut openxr::FrameStream<openxr::Vulkan>,
    xr_space: &'a openxr::Space,
    width: u32,
    height: u32,
    display_t: openxr::Time,
    views: Vec<openxr::View>,
    color_view: TextureView,
    multisample_view: Option<TextureView>,
    depth_view: TextureView,
    view_m: OutputViewMat,
    cam_pos: Vector3<f32>,
}

impl<'a> XRFrame<'a> {
    #[allow(clippy::too_many_arguments)]
    fn new(xr_swapchain: &'a mut openxr::Swapchain<openxr::Vulkan>, xr_stream: &'a mut openxr::FrameStream<openxr::Vulkan>, xr_space: &'a openxr::Space, width: u32, height: u32, display_t: openxr::Time, views: Vec<openxr::View>, color_view: TextureView, multisample_view: Option<TextureView>, depth_view: TextureView, view_m: OutputViewMat, cam_pos: Vector3<f32>) -> Self {
        Self {
            xr_swapchain,
            xr_stream,
            xr_space,
            width,
            height,
            display_t,
            views,
            color_view,
            multisample_view,
            depth_view,
            view_m,
            cam_pos,
        }
    }
}

impl<'a> Frame for XRFrame<'a> {
    fn get_color_view(&self) -> &TextureView {
        &self.color_view
    }

    fn get_multisample_view(&self) -> Option<&TextureView> {
        self.multisample_view.as_ref()
    }

    fn get_depth_view(&self) -> &TextureView {
        &self.depth_view
    }

    fn get_cam_pos(&self) -> Vector3<f32> {
        self.cam_pos
    }

    fn set_view_m(&self, buf: &mut [u8]) {
        let buf_sl: &mut [OutputViewMat] = bytemuck::cast_slice_mut(buf);
        let view_m = &mut buf_sl[0];
        *view_m = self.view_m;
    }

    fn end(self) {
        self.xr_swapchain.release_image().expect("OpenXR release_image() failed");

        let rect = openxr::Rect2Di { // TODO: Precreate this object?
            offset: openxr::Offset2Di {
                x: 0,
                y: 0,
            },
            extent: openxr::Extent2Di {
                width: self.width.try_into().unwrap(),
                height: self.height.try_into().unwrap(),
            }
        };

        let views = [
            openxr::CompositionLayerProjectionView::new()
                .pose(self.views[0].pose)
                .fov(self.views[0].fov)
                .sub_image(openxr::SwapchainSubImage::new()
                    .swapchain(self.xr_swapchain)
                    .image_array_index(0)
                    .image_rect(rect)
                ),
            openxr::CompositionLayerProjectionView::new()
                .pose(self.views[1].pose)
                .fov(self.views[1].fov)
                .sub_image(openxr::SwapchainSubImage::new()
                    .swapchain(self.xr_swapchain)
                    .image_array_index(1)
                    .image_rect(rect)
                ),
        ];

        let layer = openxr::CompositionLayerProjection::new()
            .space(self.xr_space)
            .views(&views);

        self.xr_stream.end(self.display_t, openxr::EnvironmentBlendMode::OPAQUE, &[&layer]).expect("OpenXR end() failed");
    }
}

struct XRPose {
    pos: Vector3<f32>,
    rot: Quaternion<f32>,
    click: bool,
    xr_session: openxr::Session<openxr::Vulkan>,
    haptic: openxr::Action<openxr::Haptic>,
}

impl XRPose {
    fn new(pos: &Vector3<f32>, rot: &Quaternion<f32>, click: bool, xr_session: openxr::Session<openxr::Vulkan>, haptic: openxr::Action<openxr::Haptic>) -> Self {
        Self {
            pos: *pos,
            rot: *rot,
            click,
            xr_session,
            haptic,
        }
    }
}

impl ScenePose for XRPose {
    fn get_pos(&self) -> &Vector3<f32> {
        &self.pos
    }

    fn get_rot(&self) -> &Quaternion<f32> {
        &self.rot
    }

    fn get_click(&self) -> bool {
        self.click
    }

    fn get_render(&self) -> bool {
        true
    }

    fn apply_haptic(&self) {
        let event = openxr::HapticVibration::new().duration(openxr::Duration::MIN_HAPTIC).frequency(openxr::FREQUENCY_UNSPECIFIED).amplitude(1.0);
        self.haptic.apply_feedback(&self.xr_session, openxr::Path::NULL, &event).expect("OpenXR apply_feedback() failed");
    }
}

fn perspective(fov: &openxr::Fovf, near: f32, far: f32) -> Matrix4<f32> {
    // Calculate projection matrix.
    // Taken from https://github.com/KhronosGroup/OpenXR-SDK/blob/main/src/common/xr_linear.h->XrMatrix4x4f_CreateProjectionFov.

    let tan_left = Rad(fov.angle_left).tan();
    let tan_right = Rad(fov.angle_right).tan();
    let tan_up = Rad(fov.angle_up).tan();
    let tan_down = Rad(fov.angle_down).tan();

    let tan_width = tan_right - tan_left;
    let tan_height = tan_up - tan_down;

    Matrix4::new(
        2.0 / tan_width, 0.0, 0.0, 0.0,
        0.0, 2.0 / tan_height, 0.0, 0.0,
        (tan_right + tan_left) / tan_width, (tan_up + tan_down) / tan_height, -far / (far - near), -1.0,
        0.0, 0.0, -(far * near) / (far - near), 0.0
    )
}
