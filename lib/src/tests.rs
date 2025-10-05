use std::fs::{self, File};
use std::io::Read;
use std::sync::Arc;

use crate::asset::{AssetManagerRc, AssetManagerTrait};
use crate::songinfo::SongInfo;

const PREFIX: &str = "testmap/";

struct AssetManager;

impl AssetManager {
    pub fn new() -> Self {
        Self {
        }
    }

    fn open(name: &str) -> File {
        File::open(format!("{}{}", PREFIX, name)).expect("Unable to open asset")
    }
}

impl AssetManagerTrait for AssetManager {
    fn open(&self, name: &str) -> Box<dyn Read + Send + Sync + 'static> {
        let asset = Self::open(name);
        Box::new(asset)
    }

    fn read_file(&self, name: &str) -> String {
        let mut asset = Self::open(name);
        let mut buf = String::new();
        asset.read_to_string(&mut buf).expect("I/O error");
        buf
    }
}

#[test]
fn test_map() {
    let asset_mgr: AssetManagerRc = Arc::new(AssetManager::new());

    for entry in fs::read_dir(PREFIX).expect("Unable to read directory").map(|entry| entry.expect("Unable to read entry")) {
        let filename = entry.file_name();
        let dir = filename.to_str().unwrap();

        println!("parse {}", dir);

        let info = SongInfo::load(Arc::clone(&asset_mgr), dir).expect("Unable to load info");
        info.get_bpm_info().expect("Unable to load bpm info");

        for beatmap_info in info.get_beatmap_infos() {
            beatmap_info.load().expect("Unable to load beatmap");
        }
    }
}
