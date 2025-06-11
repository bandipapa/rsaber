use std::fs::File;
use std::io::Read;

use rsaber_lib::AssetManagerTrait;

const PREFIX: &str = "../asset/";

pub struct AssetManager;

impl AssetManager {
    pub fn new() -> Self {
        Self {
        }
    }

    fn open(&self, name: &str) -> File {
        File::open(format!("{}{}", PREFIX, name)).expect("Unable to open asset")
    }
}

impl AssetManagerTrait for AssetManager {
    fn open_thr(&self, name: &str) -> Box<dyn Read + Send + Sync + 'static> {
        Box::new(self.open(name))
    }

    fn open(&self, name: &str) -> Box<dyn Read> {
        Box::new(self.open(name))
    }

    fn read_file(&self, name: &str) -> String {
        let mut buf = String::new();
        self.open(name).read_to_string(&mut buf).expect("I/O error");
        buf
    }
}
