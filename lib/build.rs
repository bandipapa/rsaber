use slint_build::{CompilerConfiguration, EmbedResourcesKind};

fn main() {
    let config = CompilerConfiguration::new()
        .embed_resources(EmbedResourcesKind::EmbedForSoftwareRenderer)
        .with_style("fluent-light".to_string());
    slint_build::compile_with_config("ui/ui.slint", config).expect("Unable to compile slint resources");
}
