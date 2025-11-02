use std::time::Duration;

use android_activity::{AndroidApp, InputStatus, MainEvent, PollEvent};

use rsaber_lib::Main;
use rsaber_lib::asset::EmbedAssetManager;
use rsaber_lib::output::XROutput;
use rsaber_lib::util::Stats;

#[unsafe(no_mangle)]
fn android_main(app: AndroidApp) {
    let asset_mgr = EmbedAssetManager::new();

    let xr_entry = unsafe { openxr::Entry::load() }.expect("Unable to load OpenXR");
    xr_entry.initialize_android_loader().expect("Unable to initialize android loader");

    // At the moment, use precompiled dynamic loader for OpenXR.
    // TODO: How to build it with cross-compiler?
    
    let output = XROutput::new(xr_entry);
    let stats = Stats::new("");
    let main = Main::new(asset_mgr, output.get_info(), stats);

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
            };
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
