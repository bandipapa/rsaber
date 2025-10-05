use rsaber_lib::Main;
use rsaber_lib::asset::EmbedAssetManager;
use rsaber_lib::output::XROutput;

fn main() {
    let asset_mgr = EmbedAssetManager::new();
    let output = XROutput::new(openxr::Entry::linked()); // Use compiled-in OpenXR loader.
    let main = Main::new(asset_mgr, output.get_info());

    // Do XR loop.

    while output.poll(&main) {
    }
}
