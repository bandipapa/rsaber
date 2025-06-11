use std::fs::{self, File};
use std::io::Read;
use std::rc::Rc;

use crate::{AssetManagerRc, AssetManagerTrait};
use crate::songinfo::SongInfo;

const PREFIX: &str = "../testmap/";

// TODO: AssetManager is copied from pc/src/asset.rs, move it to lib?
struct AssetManager;

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

#[test]
fn test_map() {
    let asset_mgr: AssetManagerRc = Rc::new(AssetManager::new());

    for entry in fs::read_dir(PREFIX).expect("Unable to read directory").map(|entry| entry.expect("Unable to read entry")) {
        let filename = entry.file_name();
        let dir = filename.to_str().unwrap();

        println!("parse {}", dir);

        let info = SongInfo::load(Rc::clone(&asset_mgr), dir).expect("Unable to load info");
        info.get_bpm_info().expect("Unable to load bpm info");

        for beatmap_info in info.get_beatmap_infos() {
            beatmap_info.load().expect("Unable to load beatmap");
        }
    }
}
