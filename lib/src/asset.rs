use std::borrow::Cow;
use std::io::{Cursor, Read};
use std::sync::Arc;

use rust_embed::Embed;

pub type AssetManagerRc = Arc<dyn AssetManagerTrait + Send + Sync>; // TODO: Or use type parameters and references?

pub trait AssetManagerTrait {
    fn open(&self, name: &str) -> Box<dyn Read + Send + Sync + 'static>;
    fn read_file(&self, name: &str) -> String;
}

#[derive(Embed)]
#[folder = "asset"]
struct Asset;

pub struct EmbedAssetManager;

impl EmbedAssetManager {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self {
        }
    }

    fn open(name: &str) -> Cow<'static, [u8]> {
        Asset::get(name).expect("Unable to open asset").data
    }
}

impl AssetManagerTrait for EmbedAssetManager {
    fn open(&self, name: &str) -> Box<dyn Read + Send + Sync + 'static> {
        let asset = Self::open(name);
        Box::new(Cursor::new(asset))
    }

    fn read_file(&self, name: &str) -> String {
        let asset = Self::open(name);
        String::from(str::from_utf8(&asset).expect("I/O error"))
    }
}
