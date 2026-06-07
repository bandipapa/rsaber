// For format description, see:
// - https://bsmg.wiki/mapping/map-format.html
// - https://github.com/Kylemc1413/SongCore/blob/master/README.md
// Regarding parsing:
// - There are lot of maps with invalid (e.g. negative, out of range)
//   values. To be able to load these maps, we have to use i32 type
//   in raw structs (see e.g. Beatmap_V2_Note->x, y).
// - The parsed structs contains more appropriate type
//   (see e.g. Note->x, y).
// - The parser methods (e.g. parse_note) are invoked for each
//   version separately (instead of making these checks in Note struct).
//   This is to handle different field encodings in the future.
// TODO: use &refs in #[derive(Deserialize)] structs instead of owned types
#![expect(non_camel_case_types)]

use std::fmt::{Formatter, Result as fmt_Result};
use std::ops::Range;
use std::result::{Result as result_Result};
use std::sync::Arc;

use serde::{Deserialize, Deserializer};
use serde::de::{Error as de_Error, Visitor};
use serde_json::{Error as json_Error, Value};

use crate::asset::{AssetError, AssetManagerRc};
use crate::render::model::Color;
use crate::songdef::SongDifficulty;
#[cfg(feature = "test")]
use crate::songdef::CHAR_STANDARD;

type Result<T> = result_Result<T, Error>;

#[derive(Debug)]
pub enum Error {
    Asset(AssetError),
    Parse(json_Error),
    Build(String),
}

impl From<AssetError> for Error {
    fn from(value: AssetError) -> Self {
        Error::Asset(value)
    }
}

impl From<json_Error> for Error {
    fn from(value: json_Error) -> Self {
        Error::Parse(value)
    }
}

// SongInfo

pub struct SongInfo {
    asset_mgr: AssetManagerRc,
    author: String,
    title: String,
    song_filename: String,
    bpm_selector: BPMSelector,
    color_schemes: Box<[ColorScheme]>,
    beatmap_infos: Box<[BeatmapInfo]>,
}

impl SongInfo {
    pub fn load(asset_mgr: AssetManagerRc) -> Result<Self> {
        // From https://docs.rs/serde_json/latest/serde_json/fn.from_reader.html :
        // Note that counter to intuition, this function is usually slower than reading a file completely into memory and then applying from_str or from_slice on it.

        let asset_file = asset_mgr.open("/Info.dat")?;
        let buf = asset_file.read_str()?;
        let value: Value = serde_json::from_str(&buf)?;

        match get_version(&value)? {
            "2.0.0" | "2.1.0" => {
                let info: SongInfo_V2 = serde_json::from_value(value)?;
                info.build(asset_mgr)
            },
            "4.0.0" | "4.0.1" => {
                let info: SongInfo_V4 = serde_json::from_value(value)?;
                info.build(asset_mgr)
            },
            version => Err(Error::Build(format!("Unsupported info version: {}", version)))
        }
    }

    #[cfg(feature = "test")]
    pub fn test(asset_mgr: AssetManagerRc) -> Self {
        let beatmap_info = BeatmapInfo::test(Arc::clone(&asset_mgr));

        Self {
            asset_mgr,
            author: "author".to_string(),
            title: "title".to_string(),
            song_filename: "song_filename".to_string(),
            bpm_selector: BPMSelector::Fixed(1.0),
            color_schemes: Box::from([]),
            beatmap_infos: Box::from([beatmap_info]),
        }
    }

    #[expect(clippy::too_many_arguments)]
    fn new(asset_mgr: AssetManagerRc, author: String, title: String, sub_title: String, song_filename: String, bpm_selector: BPMSelector, color_schemes: Vec<ColorScheme>, beatmap_infos: Vec<BeatmapInfo>) -> Self {
        let space = if sub_title.is_empty() {
            ""
        } else {
            " "
        };

        Self {
            asset_mgr,
            author,
            title: format!("{}{}{}", title, space, sub_title),
            song_filename: format!("/{}", song_filename),
            bpm_selector,
            color_schemes: color_schemes.into_boxed_slice(),
            beatmap_infos: beatmap_infos.into_boxed_slice(),
        }
    }

    pub fn get_author(&self) -> &str {
        &self.author
    }

    pub fn get_title(&self) -> &str {
        &self.title
    }

    pub fn get_song_filename(&self) -> &str {
        &self.song_filename
    }

    pub fn get_bpm_info(&self) -> Result<BPMInfo> {
        Ok(match &self.bpm_selector {
            BPMSelector::Fixed(bpm) => BPMInfo::Fixed(*bpm),
            BPMSelector::Mapped(filename) => BPMInfo::Mapped(BPMMap::load(Arc::clone(&self.asset_mgr), filename)?),
        })
    }

    pub fn get_color_scheme(&self, index: u32) -> Option<&ColorScheme> {
        self.color_schemes.get(index as usize)
    }

    pub fn get_beatmap_infos(&self) -> &[BeatmapInfo] {
        &self.beatmap_infos
    }
}

enum BPMSelector {
    Fixed(f32),
    Mapped(String),
}

pub enum BPMInfo {
    Fixed(f32),
    Mapped(BPMMap),
}

impl BPMInfo {
    pub fn get_ts(&self, bpm_pos: f32) -> Option<f32> {
        match self {
            BPMInfo::Fixed(bpm) => {
                Some(60.0 / bpm * bpm_pos)
            },
            BPMInfo::Mapped(bpm_map) => {
                bpm_map.get_ts(bpm_pos)
            },
        }
    }
}

pub struct ColorScheme {
    color_l: Color, // TODO: rename to note_l
    color_r: Color, // TODO: rename to note_r
    obstacle: Color,
}

impl ColorScheme {
    fn new(color_l: Color, color_r: Color, obstacle: Color) -> Self {
        Self {
            color_l,
            color_r,
            obstacle,
        }
    }

    pub fn get_color_l(&self) -> &Color {
        &self.color_l
    }

    pub fn get_color_r(&self) -> &Color {
        &self.color_r
    }

    pub fn get_obstacle(&self) -> &Color {
        &self.obstacle
    }
}

impl Default for ColorScheme {
    fn default() -> Self {
        // See https://bsmg.wiki/mapping/lighting-defaults.html#_1-19-0-colors .
        // TODO: Default colors are depending on the environment.
        
        let color_l = Color::from_srgb_float(0.7843137, 0.07843138, 0.07843138);
        let color_r = Color::from_srgb_float(0.1568627, 0.5568627, 0.8235294);
        let obstacle = Color::from_srgb_float(1.0, 0.1882353, 0.1882353);

        ColorScheme::new(color_l, color_r, obstacle)
    }
}

pub struct BeatmapInfo {
    asset_mgr: AssetManagerRc,
    characteristic: String,
    difficulty: SongDifficulty,
    color_scheme_index_opt: Option<u32>,
    def_color_scheme: ColorScheme,
    filename: String,
    notejump_speed: f32,
    notejump_beatoffset: f32,
    #[cfg(feature = "test")]
    test: bool,
}

impl BeatmapInfo {
    #[expect(clippy::too_many_arguments)]
    fn new(asset_mgr: AssetManagerRc, characteristic: String, difficulty: SongDifficulty, mut color_scheme_index_opt: Option<i32>, def_color_scheme: ColorScheme, filename: String, notejump_speed: f32, notejump_beatoffset: f32) -> Self {
        Self {
            asset_mgr,
            characteristic,
            difficulty,
            color_scheme_index_opt: color_scheme_index_opt.take_if(|color_scheme_index| *color_scheme_index >= 0).map(|color_scheme_index| color_scheme_index.try_into().unwrap()), // color_scheme_index can be negative, which is the same as not specified.
            def_color_scheme,
            filename,
            notejump_speed,
            notejump_beatoffset,
            #[cfg(feature = "test")]
            test: false,
        }
    }

    #[cfg(feature = "test")]
    fn test(asset_mgr: AssetManagerRc) -> Self {
        Self {
            asset_mgr,
            characteristic: CHAR_STANDARD.to_string(),
            difficulty: SongDifficulty::Easy,
            color_scheme_index_opt: None,
            def_color_scheme: ColorScheme::default(),
            filename: "filename".to_string(),
            notejump_speed: 1.0,
            notejump_beatoffset: 0.0,
            test: true,
        }
    }

    pub fn load(&self) -> Result<Beatmap> {
        #[cfg(feature = "test")]
        if self.test {
            return Beatmap::test();
        }

        Beatmap::load(Arc::clone(&self.asset_mgr), &self.filename)
    }

    pub fn get_characteristic(&self) -> &str {
        &self.characteristic
    }

    pub fn get_difficulty(&self) -> SongDifficulty {
        self.difficulty
    }

    pub fn get_color_scheme_index_opt(&self) -> Option<u32> {
        self.color_scheme_index_opt
    }

    pub fn get_def_color_scheme(&self) -> &ColorScheme {
        &self.def_color_scheme
    }

    pub fn get_notejump_speed(&self) -> f32 {
        self.notejump_speed
    }

    #[expect(dead_code)] // TODO: remove dead_code once it is used
    fn get_notejump_beatoffset(&self) -> f32 {
        self.notejump_beatoffset
    }
}

#[derive(Deserialize)]
struct SongInfo_V2 {
    #[serde(rename = "_songAuthorName")]
    author: String,
    #[serde(rename = "_songName")]
    title: String,
    #[serde(rename = "_songSubName")]
    sub_title: String,

    #[serde(rename = "_songFilename")]
    song_filename: String,
    #[serde(rename = "_beatsPerMinute")]
    bpm: f32, // TODO: validate > 0

    #[serde(rename = "_colorSchemes")]
    color_schemes: Option<Vec<SongInfo_V2_ColorScheme>>,

    #[serde(rename = "_difficultyBeatmapSets")]
    beatmap_info_sets: Vec<SongInfo_V2_BeatmapInfoSet>,
}

impl SongInfo_V2 {
    fn build(self, asset_mgr: AssetManagerRc) -> Result<SongInfo> {
        let mut color_schemes = Vec::new();
        if let Some(raw_color_schemes) = self.color_schemes {
            for raw_color_scheme in raw_color_schemes {
                let inner = raw_color_scheme.inner;
                let color_l = inner.color_l;
                let color_r = inner.color_r;
                let obstacle = inner.obstacle;
                let color_scheme = ColorScheme::new(Color::from_srgb_float(color_l.r, color_l.g, color_l.b), Color::from_srgb_float(color_r.r, color_r.g, color_r.b), Color::from_srgb_float(obstacle.r, obstacle.g, obstacle.b));
                color_schemes.push(color_scheme);
            }
        }

        let mut beatmap_infos = Vec::new();
        for raw_beatmap_info_set in self.beatmap_info_sets {
            let characteristic = raw_beatmap_info_set.characteristic;

            for raw_beatmap_info in raw_beatmap_info_set.beatmap_infos {
                let mut def_color_scheme = ColorScheme::default();

                if let Some(custom_data) = raw_beatmap_info.custom_data {
                    if let Some(color) = custom_data.color_l {
                        def_color_scheme.color_l = Color::from_srgb_float(color.r, color.g, color.b);
                    }
                    
                    if let Some(color) = custom_data.color_r {
                        def_color_scheme.color_r = Color::from_srgb_float(color.r, color.g, color.b);
                    }

                    if let Some(color) = custom_data.obstacle {
                        def_color_scheme.obstacle = Color::from_srgb_float(color.r, color.g, color.b);
                    }
                }

                let beatmap_info = BeatmapInfo::new(Arc::clone(&asset_mgr), characteristic.clone(), raw_beatmap_info.difficulty, raw_beatmap_info.color_scheme_index_opt, def_color_scheme, raw_beatmap_info.filename, raw_beatmap_info.notejump_speed, raw_beatmap_info.notejump_beatoffset);
                beatmap_infos.push(beatmap_info);
            }
        }

        Ok(SongInfo::new(asset_mgr, self.author, self.title, self.sub_title, self.song_filename, BPMSelector::Fixed(self.bpm), color_schemes, beatmap_infos))
    }
}

#[derive(Deserialize)]
struct SongInfo_V2_ColorScheme {
    #[serde(rename = "colorScheme")]
    inner: SongInfo_V2_ColorScheme_Inner,
}

#[derive(Deserialize)]
struct SongInfo_V2_ColorScheme_Inner {
    #[serde(rename = "saberAColor")]
    color_l: FloatColor,
    #[serde(rename = "saberBColor")]
    color_r: FloatColor,
    #[serde(rename = "obstaclesColor")]
    obstacle: FloatColor,
}

#[derive(Deserialize)]
struct SongInfo_V2_BeatmapInfoSet {
    #[serde(rename = "_beatmapCharacteristicName")]
    characteristic: String,
    #[serde(rename = "_difficultyBeatmaps")]
    beatmap_infos: Vec<SongInfo_V2_BeatmapInfo>,
}

#[derive(Deserialize)]
struct SongInfo_V2_BeatmapInfo {
    #[serde(rename = "_difficulty")]
    difficulty: SongDifficulty,
    #[serde(rename = "_beatmapColorSchemeIdx")]
    color_scheme_index_opt: Option<i32>,
    #[serde(rename = "_beatmapFilename")]
    filename: String,
    #[serde(rename = "_noteJumpMovementSpeed")]
    notejump_speed: f32,
    #[serde(rename = "_noteJumpStartBeatOffset")]
    notejump_beatoffset: f32,
    #[serde(rename = "_customData")]
    custom_data: Option<SongInfo_V2_BeatmapInfo_CustomData>,
}

#[derive(Deserialize)]
struct SongInfo_V2_BeatmapInfo_CustomData {
    #[serde(rename = "_colorLeft")]
    color_l: Option<FloatColor>,
    #[serde(rename = "_colorRight")]
    color_r: Option<FloatColor>,
    #[serde(rename = "_obstacleColor")]
    obstacle: Option<FloatColor>,
}

#[derive(Deserialize)]
struct SongInfo_V4 {
    song: SongInfo_V4_Song,
    audio: SongInfo_V4_Audio,
    #[serde(rename = "colorSchemes")]
    color_schemes: Option<Vec<SongInfo_V4_ColorScheme>>,
    #[serde(rename = "difficultyBeatmaps")]
    beatmap_infos: Vec<SongInfo_V4_BeatmapInfo>,
}

impl SongInfo_V4 {
    fn build(self, asset_mgr: AssetManagerRc) -> Result<SongInfo> {
        let bpm_selector = if let Some(filename) = self.audio.bpmmap_filename {
            BPMSelector::Mapped(filename)
        } else if let Some(bpm) = self.audio.bpm {
            BPMSelector::Fixed(bpm)
        } else {
            return Err(Error::Build("Either bpm or audioDataFilename is required".to_string()));
        };

        let mut color_schemes = Vec::new();
        if let Some(raw_color_schemes) = self.color_schemes {
            for raw_color_scheme in raw_color_schemes {
                let color_scheme = ColorScheme::new(raw_color_scheme.color_l, raw_color_scheme.color_r, raw_color_scheme.obstacle);
                color_schemes.push(color_scheme);
            }
        }

        let mut beatmap_infos = Vec::new();
        for raw_beatmap_info in self.beatmap_infos {
            let beatmap_info = BeatmapInfo::new(Arc::clone(&asset_mgr), raw_beatmap_info.characteristic, raw_beatmap_info.difficulty, raw_beatmap_info.color_scheme_index_opt, ColorScheme::default(), raw_beatmap_info.filename, raw_beatmap_info.notejump_speed, raw_beatmap_info.notejump_beatoffset);
            beatmap_infos.push(beatmap_info);
        }

        Ok(SongInfo::new(asset_mgr, self.song.author, self.song.title, self.song.sub_title, self.audio.song_filename, bpm_selector, color_schemes, beatmap_infos))
    }
}

#[derive(Deserialize)]
struct SongInfo_V4_Song {
    author: String,
    title: String,
    #[serde(rename = "subTitle")]
    sub_title: String,
}

#[derive(Deserialize)]
struct SongInfo_V4_Audio {
    #[serde(rename = "songFilename")]
    song_filename: String,
    bpm: Option<f32>, // TODO: validate > 0
    #[serde(rename = "audioDataFilename")]
    bpmmap_filename: Option<String>,
}

#[derive(Deserialize)]
struct SongInfo_V4_ColorScheme {
    #[serde(rename = "saberAColor")]
    color_l: Color,
    #[serde(rename = "saberBColor")]
    color_r: Color,
    #[serde(rename = "obstaclesColor")]
    obstacle: Color,
}

#[derive(Deserialize)]
struct SongInfo_V4_BeatmapInfo {
    characteristic: String,
    difficulty: SongDifficulty,
    #[serde(rename = "beatmapColorSchemeIdx")]
    color_scheme_index_opt: Option<i32>,
    #[serde(rename = "beatmapDataFilename")]
    filename: String,
    #[serde(rename = "noteJumpMovementSpeed")]
    notejump_speed: f32,
    #[serde(rename = "noteJumpStartBeatOffset")]
    notejump_beatoffset: f32,
}

// BPMMap

pub struct BPMMap {
    ranges: Box<[BPMRange]>,
}

impl BPMMap {
    fn load<S: AsRef<str>>(asset_mgr: AssetManagerRc, filename: S) -> Result<Self> {
        let asset_file = asset_mgr.open(&format!("/{}", filename.as_ref()))?;
        let buf = asset_file.read_str()?;
        let value: Value = serde_json::from_str(&buf)?;

        match get_version(&value)? {
            "2.0.0" => {
                let bpmmap: BPMMap_V2 = serde_json::from_value(value)?;
                Ok(bpmmap.build())
            },
            "4.0.0" => {
                let bpmmap: BPMMap_V4 = serde_json::from_value(value)?;
                Ok(bpmmap.build())
            },
            version => Err(Error::Build(format!("Unsupported bpmmap version: {}", version)))
        }
    }

    fn new(mut ranges: Vec<BPMRange>) -> Self {
        ranges.sort_by(|range1, range2| range1.bpm.start.partial_cmp(&range2.bpm.start).expect("Unable to compare"));

        Self {
            ranges: ranges.into_boxed_slice(),
        }
    }

    fn get_ts(&self, bpm: f32) -> Option<f32> {
        let index = self.ranges.partition_point(|range| range.bpm.start <= bpm); // First index, where range.bpm.start > bpm
        if index == 0 {
            return None;
        }

        let range = &self.ranges[index - 1];
        if bpm >= range.bpm.end {
            return None;
        }

        Some((bpm - range.bpm.start) / (range.bpm.end - range.bpm.start) * (range.ts.end - range.ts.start) + range.ts.start) // Map bpm to timestamp
    }
}

struct BPMRange {
    ts: Range<f32>,
    bpm: Range<f32>,
}

impl BPMRange {
    fn new(ts: Range<f32>, bpm: Range<f32>) -> Self {
        Self {
            ts,
            bpm,
        }
    }
}

#[derive(Deserialize)]
struct BPMMap_V2 {
    #[serde(rename = "_songFrequency")]
    sample_rate: u32,
    #[serde(rename = "_regions")]
    ranges: Vec<BPMMap_V2_Range>,
}

impl BPMMap_V2 {
    fn build(self) -> BPMMap {
        let ranges = Vec::from_iter(self.ranges.into_iter().map(|range| BPMRange::new(range.start_sample_pos as f32 / self.sample_rate as f32..range.end_sample_pos as f32 / self.sample_rate as f32, range.start_bpm..range.end_bpm)));
        BPMMap::new(ranges)
    }
}

#[derive(Deserialize)]
struct BPMMap_V2_Range { // TODO: impl validity checks
    #[serde(rename = "_startSampleIndex")]
    start_sample_pos: u32,
    #[serde(rename = "_endSampleIndex")]
    end_sample_pos: u32,
    #[serde(rename = "_startBeat")]
    start_bpm: f32,
    #[serde(rename = "_endBeat")]
    end_bpm: f32,
}

#[derive(Deserialize)]
struct BPMMap_V4 {
    #[serde(rename = "songFrequency")]
    sample_rate: u32,
    #[serde(rename = "bpmData")]
    ranges: Vec<BPMMap_V4_Range>,
}

impl BPMMap_V4 {
    fn build(self) -> BPMMap {
        let ranges = Vec::from_iter(self.ranges.into_iter().map(|range| BPMRange::new(range.start_sample_pos as f32 / self.sample_rate as f32..range.end_sample_pos as f32 / self.sample_rate as f32, range.start_bpm..range.end_bpm)));
        BPMMap::new(ranges)
    }
}

#[derive(Deserialize)]
struct BPMMap_V4_Range { // TODO: impl validity checks
    #[serde(rename = "si")]
    start_sample_pos: u32,
    #[serde(rename = "ei")]
    end_sample_pos: u32,
    #[serde(rename = "sb")]
    start_bpm: f32,
    #[serde(rename = "eb")]
    end_bpm: f32,
}

// Beatmap

pub struct Beatmap {
    notes: Box<[Note]>,
    obstacles: Box<[Obstacle]>,
}

impl Beatmap {
    fn load<S: AsRef<str>>(asset_mgr: AssetManagerRc, filename: S) -> Result<Self> {
        let asset_file = asset_mgr.open(&format!("/{}", filename.as_ref()))?;
        let buf = asset_file.read_str()?;
        let value: Value = serde_json::from_str(&buf)?;

        match get_version(&value)? {
            "2.0.0" | "2.2.0" => {
                let beatmap: Beatmap_V2 = serde_json::from_value(value)?;
                beatmap.build()
            },
            "3.0.0" | "3.2.0" | "3.3.0" => {
                let beatmap: Beatmap_V3 = serde_json::from_value(value)?;
                beatmap.build()
            },
            "4.0.0" | "4.1.0" => {
                let beatmap: Beatmap_V4 = serde_json::from_value(value)?;
                beatmap.build()
            },
            version => Err(Error::Build(format!("Unsupported beatmap version: {}", version)))
        }
    }

    #[cfg(feature = "test")]
    fn test() -> Result<Self> {
        let mut cut_dir_it = [
            NoteCutDir::Up,
            NoteCutDir::Down,
            NoteCutDir::Left,
            NoteCutDir::Right,
            NoteCutDir::UpLeft,
            NoteCutDir::UpRight,
            NoteCutDir::DownLeft,
            NoteCutDir::DownRight,
            NoteCutDir::Any,
        ].into_iter().cycle();

        let mut notes = Vec::new();

        for i in 0..100 {
            let note = Note::new(i as f32, 2, 1, NoteType::Right, cut_dir_it.next().unwrap());
            notes.push(note);
        }
        
        Ok(Self {
            notes: notes.into_boxed_slice(),
            obstacles: Box::from([]),
        })
    }

    fn new(mut notes: Vec<Note>, mut obstacles: Vec<Obstacle>) -> Self {
        notes.sort_by(|note1, note2| note1.bpm_pos.partial_cmp(&note2.bpm_pos).expect("Unable to compare"));
        obstacles.sort_by(|obstacle1, obstacle2| obstacle1.bpm_pos.partial_cmp(&obstacle2.bpm_pos).expect("Unable to compare"));

        Self {
            notes: notes.into_boxed_slice(),
            obstacles: obstacles.into_boxed_slice(),
        }
    }

    pub fn get_notes(&self) -> &[Note] {
        &self.notes
    }

    pub fn get_obstacles(&self) -> &[Obstacle] {
        &self.obstacles
    }
}

pub struct Note {
    bpm_pos: f32,
    x: u8,
    y: u8,
    note_type: NoteType,
    cut_dir: NoteCutDir,
}

#[derive(Clone, Copy)]
pub enum NoteType {
    Left,
    Right,
}

#[derive(Clone, Copy)]
pub enum NoteCutDir {
    Up,
    Down,
    Left,
    Right,
    UpLeft,
    UpRight,
    DownLeft,
    DownRight,
    Any,
}

impl Note {
    fn new(bpm_pos: f32, x: u8, y: u8, note_type: NoteType, cut_dir: NoteCutDir) -> Self {
        Self {
            bpm_pos,
            x,
            y,
            note_type,
            cut_dir,
        }
    }

    pub fn get_bpm_pos(&self) -> f32 {
        self.bpm_pos
    }

    pub fn get_x(&self) -> u8 {
        self.x
    }

    pub fn get_y(&self) -> u8 {
        self.y
    }

    pub fn get_note_type(&self) -> NoteType {
        self.note_type
    }

    pub fn get_cut_dir(&self) -> NoteCutDir {
        self.cut_dir
    }
}

pub struct Obstacle {
    bpm_pos: f32,
    x: u8,
    y: u8,
    duration: f32,
    width: u8,
    height: u8,
}

impl Obstacle {
    fn new(bpm_pos: f32, x: u8, y: u8, duration: f32, width: u8, height: u8) -> Self {
        Self {
            bpm_pos,
            x,
            y,
            duration,
            width,
            height,
        }
    }

    pub fn get_bpm_pos(&self) -> f32 {
        self.bpm_pos
    }

    pub fn get_x(&self) -> u8 {
        self.x
    }

    pub fn get_y(&self) -> u8 {
        self.y
    }

    pub fn get_duration(&self) -> f32 {
        self.duration
    }

    pub fn get_width(&self) -> u8 {
        self.width
    }

    pub fn get_height(&self) -> u8 {
        self.height
    }
}

#[derive(Deserialize)]
struct Beatmap_V2 {
    #[serde(rename = "_notes")]
    notes: Vec<Beatmap_V2_Note>,
    #[serde(rename = "_obstacles")]
    obstacles: Vec<Beatmap_V2_Obstacle>,
}

impl Beatmap_V2 {
    fn build(self) -> Result<Beatmap> {
        let mut notes = Vec::new();
        let mut obstacles = Vec::new();

        for raw_note in self.notes {
            let (x, y, note_type) = match parse_note(raw_note.x, raw_note.y, raw_note.note_type) {
                Ok(r) => r,
                Err(_) => continue, // TODO: provide strict mode
            };

            let note = Note::new(raw_note.bpm_pos, x, y, note_type, raw_note.cut_dir);
            notes.push(note);
        }

        for raw_obstacle in self.obstacles {
            let (mut raw_y, mut raw_height) = (raw_obstacle.y, raw_obstacle.height);

            if let Some(obstacle_type) = raw_obstacle.obstacle_type {
                // See https://bsmg.wiki/mapping/map-format/beatmap.html#obstacles-type .

                match obstacle_type { 
                    0 => (raw_y, raw_height) = (Some(0), Some(5)), // Full-height wall
                    1 => (raw_y, raw_height) = (Some(2), Some(3)), // Crouch wall
                    2 => (), // Free wall
                    _ => continue, // TODO: provide strict mode
                }
            }

            let raw_y = raw_y.ok_or(Error::Build("Obstacle y is missing".to_string()))?;
            let raw_height = raw_height.ok_or(Error::Build("Obstacle height is missing".to_string()))?;

            let (x, y, width, height) = match parse_obstacle(raw_obstacle.x, raw_y, raw_obstacle.width, raw_height) {
                Ok(r) => r,
                Err(_) => continue, // TODO: provide strict mode
            };

            let obstacle = Obstacle::new(raw_obstacle.bpm_pos, x, y, raw_obstacle.duration, width, height);
            obstacles.push(obstacle);
        }

        Ok(Beatmap::new(notes, obstacles))
    }
}

#[derive(Deserialize)]
struct Beatmap_V2_Note { // TODO: impl validate
    #[serde(rename = "_time")]
    bpm_pos: f32,
    #[serde(rename = "_lineIndex")]
    x: i32,
    #[serde(rename = "_lineLayer")]
    y: i32,
    #[serde(rename = "_type")]
    note_type: i32,
    #[serde(rename = "_cutDirection")]
    cut_dir: NoteCutDir,
}

#[derive(Deserialize)]
struct Beatmap_V2_Obstacle { // TODO: impl validate
    #[serde(rename = "_time")]
    bpm_pos: f32,
    #[serde(rename = "_lineIndex")]
    x: i32,
    #[serde(rename = "_lineLayer")]
    y: Option<i32>,
    #[serde(rename = "_duration")]
    duration: f32,
    #[serde(rename = "_width")]
    width: i32,
    #[serde(rename = "_height")]
    height: Option<i32>,
    #[serde(rename = "_type")]
    obstacle_type: Option<i32>,
}

#[derive(Deserialize)]
struct Beatmap_V3 {
    #[serde(rename = "colorNotes")]
    notes: Vec<Beatmap_V3_Note>,
    #[serde(rename = "obstacles")]
    obstacles: Vec<Beatmap_V3_Obstacle>,
}

impl Beatmap_V3 {
    fn build(self) -> Result<Beatmap> {
        let mut notes = Vec::new();
        let mut obstacles = Vec::new();

        for raw_note in self.notes {
            let (x, y, note_type) = match parse_note(raw_note.x, raw_note.y, raw_note.note_type) {
                Ok(r) => r,
                Err(_) => continue, // TODO: provide strict mode
            };

            let note = Note::new(raw_note.bpm_pos, x, y, note_type, raw_note.cut_dir);
            notes.push(note);
        }

        for raw_obstacle in self.obstacles {
            let (x, y, width, height) = match parse_obstacle(raw_obstacle.x, raw_obstacle.y, raw_obstacle.width, raw_obstacle.height) {
                Ok(r) => r,
                Err(_) => continue, // TODO: provide strict mode
            };

            let obstacle = Obstacle::new(raw_obstacle.bpm_pos, x, y, raw_obstacle.duration, width, height);
            obstacles.push(obstacle);
        }

        Ok(Beatmap::new(notes, obstacles))
    }
}

#[derive(Deserialize)]
struct Beatmap_V3_Note { // TODO: impl validate
    #[serde(rename = "b")]
    bpm_pos: f32,
    x: i32,
    y: i32,
    #[serde(rename = "c")]
    note_type: i32,
    #[serde(rename = "d")]
    cut_dir: NoteCutDir,
}

#[derive(Deserialize)]
struct Beatmap_V3_Obstacle { // TODO: impl validate
    #[serde(rename = "b")]
    bpm_pos: f32,
    x: i32,
    y: i32,
    #[serde(rename = "d")]
    duration: f32,
    #[serde(rename = "w")]
    width: i32,
    #[serde(rename = "h")]
    height: i32,
}

#[derive(Deserialize)]
struct Beatmap_V4 {
    #[serde(rename = "colorNotes")]
    notes: Vec<Beatmap_V4_Note>,
    #[serde(rename = "colorNotesData")]
    note_datas: Vec<Beatmap_V4_NoteData>,
    #[serde(rename = "obstacles")]
    obstacles: Vec<Beatmap_V4_Obstacle>,
    #[serde(rename = "obstaclesData")]
    obstacle_datas: Vec<Beatmap_V4_ObstacleData>,
}

impl Beatmap_V4 {
    fn build(self) -> Result<Beatmap> {
        let mut notes = Vec::new();
        let mut obstacles = Vec::new();

        for raw_note in self.notes {
            if let Some(raw_note_data) = self.note_datas.get(raw_note.data_index as usize) { // TODO: provide strict mode
                let (x, y, note_type) = match parse_note(raw_note_data.x, raw_note_data.y, raw_note_data.note_type) {
                    Ok(r) => r,
                    Err(_) => continue, // TODO: provide strict mode
                };

                let note = Note::new(raw_note.bpm_pos, x, y, note_type, raw_note_data.cut_dir);
                notes.push(note);
            }
        }

        for raw_obstacle in self.obstacles {
            if let Some(raw_obstacle_data) = self.obstacle_datas.get(raw_obstacle.data_index as usize) { // TODO: provide strict mode
                let (x, y, width, height) = match parse_obstacle(raw_obstacle_data.x, raw_obstacle_data.y, raw_obstacle_data.width, raw_obstacle_data.height) {
                    Ok(r) => r,
                    Err(_) => continue, // TODO: provide strict mode
                };

                let obstacle = Obstacle::new(raw_obstacle.bpm_pos, x, y, raw_obstacle_data.duration, width, height);
                obstacles.push(obstacle);
            }
        }

        Ok(Beatmap::new(notes, obstacles))
    }
}

#[derive(Deserialize)]
struct Beatmap_V4_Note { // TODO: impl validate
    #[serde(rename = "b")]
    bpm_pos: f32,
    #[serde(rename = "i")]
    data_index: u32,
}

#[derive(Deserialize)]
struct Beatmap_V4_NoteData { // TODO: impl validate
    x: i32,
    y: i32,
    #[serde(rename = "c")]
    note_type: i32,
    #[serde(rename = "d")]
    cut_dir: NoteCutDir,
}

#[derive(Deserialize)]
struct Beatmap_V4_Obstacle { // TODO: impl validate
    #[serde(rename = "b")]
    bpm_pos: f32,
    #[serde(rename = "i")]
    data_index: u32,
}

#[derive(Deserialize)]
struct Beatmap_V4_ObstacleData { // TODO: impl validate
    x: i32,
    y: i32,
    #[serde(rename = "d")]
    duration: f32,
    #[serde(rename = "w")]
    width: i32,
    #[serde(rename = "h")]
    height: i32,
}

// FloatColor

#[derive(Deserialize)]
struct FloatColor { // TODO: validate: 0 <= value <= 1
    r: f32,
    g: f32,
    b: f32,
}

// Color

impl<'de> Deserialize<'de> for Color {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> result_Result<Self, D::Error> {
        deserializer.deserialize_str(ColorVisitor)
    }
}

struct ColorVisitor;

impl<'de> Visitor<'de> for ColorVisitor {
    type Value = Color;

    fn expecting(&self, formatter: &mut Formatter) -> fmt_Result {
        formatter.write_str("valid color")
    }

    fn visit_str<E: de_Error>(self, v: &str) -> result_Result<Self::Value, E> {
        let raw_color = u32::from_str_radix(v, 16).map_err(|_| E::custom("invalid color"))?; // TODO: validate: leading +, number of digits, etc.
        Ok(Color::from_srgb_byte((raw_color >> 24) as u8, (raw_color >> 16) as u8, (raw_color >> 8) as u8))
    } 
}

// NoteCutDir

impl<'de> Deserialize<'de> for NoteCutDir {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> result_Result<Self, D::Error> {
        deserializer.deserialize_u64(NoteCutDirVisitor)
    }
}

struct NoteCutDirVisitor;

impl<'de> Visitor<'de> for NoteCutDirVisitor { // TODO: ignore note if NoteCutDir is not parseable?
    type Value = NoteCutDir;

    fn expecting(&self, formatter: &mut Formatter) -> fmt_Result {
        formatter.write_str("valid cut direction")
    }

    fn visit_u64<E: de_Error>(self, v: u64) -> result_Result<Self::Value, E> {
        match v {
            0 => Ok(NoteCutDir::Up),
            1 => Ok(NoteCutDir::Down),
            2 => Ok(NoteCutDir::Left),
            3 => Ok(NoteCutDir::Right),
            4 => Ok(NoteCutDir::UpLeft),
            5 => Ok(NoteCutDir::UpRight),
            6 => Ok(NoteCutDir::DownLeft),
            7 => Ok(NoteCutDir::DownRight),
            8 => Ok(NoteCutDir::Any),
            _ => Err(E::custom("invalid cut direction")),
        }
    }    
}

fn get_version(top_value: &Value) -> Result<&str> {
    if let Value::Object(top) = top_value {
        for key in ["_version", "version"] {
            if let Some(version_value) = top.get(key) {
                if let Value::String(version) = version_value {
                    return Ok(version);
                } else {
                    return Err(Error::Build("Version should be a string".to_string()));
                }
            }
        }

        Err(Error::Build("Version not found".to_string()))
    } else {
        Err(Error::Build("Object expected at top-level".to_string()))
    }
}

fn parse_note(raw_x: i32, raw_y: i32, raw_note_type: i32) -> Result<(u8, u8, NoteType)> {
    if !((0..=3).contains(&raw_x) && (0..=2).contains(&raw_y)) {
        return Err(Error::Build("Either note x or y invalid".to_string()));
    }

    let note_type = match raw_note_type {
        0 => NoteType::Left,
        1 => NoteType::Right,
        _ => return Err(Error::Build("Note type is invalid".to_string())),
    };

    Ok((raw_x.try_into().unwrap(), raw_y.try_into().unwrap(), note_type))
}

fn parse_obstacle(raw_x: i32, raw_y: i32, raw_width: i32, raw_height: i32) -> Result<(u8, u8, u8, u8)> {
    if !((0..=3).contains(&raw_x) && (0..=2).contains(&raw_y)) {
        return Err(Error::Build("Either obstacle x or y invalid".to_string()));
    }

    if !((0..=4).contains(&(raw_x + raw_width)) && (0..=5).contains(&(raw_y + raw_height))) {
        return Err(Error::Build("Either obstacle width or height invalid".to_string()));
    }

    Ok((raw_x.try_into().unwrap(), raw_y.try_into().unwrap(), raw_width.try_into().unwrap(), raw_height.try_into().unwrap()))
}
