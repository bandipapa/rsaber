use std::fs::{self, File};
use std::io::Read;
use std::sync::Arc;

use crate::asset::{AssetError, AssetFileBox, AssetFileTrait, AssetManagerRc, AssetManagerTrait, AssetResult};
use crate::songinfo::SongInfo;

const PREFIX: &str = "testmap";

struct AssetManager {
    dir: String,
}

impl AssetManager {
    pub fn new(dir: &str) -> Self {
        Self {
            dir: dir.to_string(),
        }
    }
}

impl AssetManagerTrait for AssetManager {
    fn open(&self, name: &str) -> AssetResult<AssetFileBox> {
        Ok(Box::new(AssetFile::new(format!("{}/{}/{}", PREFIX, self.dir, name))))
    }
}

struct AssetFile {
    name: String,
}

impl AssetFile {
    fn new(name: String) -> Self {
        Self {
            name,
        }
    }

    fn open(name: &str) -> AssetResult<File> {
        File::open(name).map_err(|_| AssetError::NotFound) // TODO: Report actual error, or is NotFound fine?
    }
}

impl AssetFileTrait for AssetFile {
    fn read(&self) -> AssetResult<Box<dyn Read + Send + Sync>> {
        let file = Self::open(&self.name)?;
        Ok(Box::new(file))
    }

    fn read_str(&self) -> AssetResult<String> {
        let mut file = Self::open(&self.name)?;
        let mut buf = String::new();
        file.read_to_string(&mut buf).map_err(|_| AssetError::Decode)?;
        Ok(buf)
    }
}

#[test]
fn test_map() {
    for entry in fs::read_dir(PREFIX).expect("Unable to read directory").map(|entry| entry.expect("Unable to read entry")) {
        let filename = entry.file_name();
        let dir = filename.to_str().unwrap();

        println!("parse {}", dir);

        let asset_mgr: AssetManagerRc = Arc::new(AssetManager::new(dir));

        let song_info = SongInfo::load(Arc::clone(&asset_mgr)).expect("Unable to load info");
        song_info.get_bpm_info().expect("Unable to load bpm info");

        for beatmap_info in song_info.get_beatmap_infos() {
            beatmap_info.load().expect("Unable to load beatmap");
        }
    }
}
