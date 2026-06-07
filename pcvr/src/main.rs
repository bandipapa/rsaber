use rsaber_lib::Main;
use rsaber_lib::asset::EmbedAssetManager;
use rsaber_lib::openxr;
use rsaber_lib::output::XROutput;
use rsaber_lib::util::Stats;

fn main() {
    let asset_mgr = EmbedAssetManager::new();
    let output = XROutput::new(openxr::Entry::linked()); // Use compiled-in OpenXR loader.
    let stats = Stats::new("");
    let main = Main::new(asset_mgr, output.get_output_device(), stats);
    main.configure(output.get_width(), output.get_height());

    // Do XR loop.

    while output.poll(&main) {
    }
}
