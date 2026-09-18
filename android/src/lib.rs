use std::time::Duration;

use android_activity::{AndroidApp, InputStatus, MainEvent, PollEvent};

use rsaber_lib::Main;
use rsaber_lib::asset::EmbedAssetManager;
use rsaber_lib::openxr;
use rsaber_lib::output::XROutput;
use rsaber_lib::util::Stats;

#[unsafe(no_mangle)]
fn android_main(app: AndroidApp) {
    let asset_mgr = EmbedAssetManager::new();

    // At the moment, use precompiled dynamic loader for OpenXR.
    // TODO: How to build it with cross-compiler?

    let xr_platform_info = unsafe { openxr::AndroidPlatformInfo::new(app.vm_as_ptr(), app.activity_as_ptr()) };
    let xr_entry = unsafe { openxr::Entry::load(&xr_platform_info) }.expect("Unable to load OpenXR");
    let output = XROutput::new(xr_entry, &xr_platform_info);
    
    let stats = Stats::new("");

    let main = Main::new(asset_mgr, output.get_output_device(), stats);
    main.configure(output.get_width(), output.get_height());

    let mut terminate = false;

    loop {
        // Poll android events.

        app.poll_events(Some(Duration::from_secs(0)), |event| { 
            match event {
                PollEvent::Main(event) => {
                    match event {
                        MainEvent::InputAvailable => {
                            let mut it = app.input_events_iter().unwrap();
                            while it.next(|_| InputStatus::Unhandled) {
                            }
                        },
                        MainEvent::TerminateWindow {..} => {
                            terminate = true;
                        },
                        _ => (),
                    }
                },
                _ => (),
            }
        });

        if terminate {
            break;
        }

        // Do XR loop.

        if !output.poll(&main) {
            break;
        }
    }
}
