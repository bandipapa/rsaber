// For format description, see:
// - https://bsmg.wiki/mapping/map-format.html
// - https://github.com/Kylemc1413/SongCore/blob/master/README.md
// TODO: use &refs in #[derive(Deserialize)] structs instead of owned types
#![allow(non_camel_case_types)]

use std::fmt;
use std::ops::Range;
use std::result::{Result as result_Result};
use std::sync::Arc;

use serde::{Deserialize, Deserializer};
use serde::de::{Error as de_Error, Visitor};
use serde_json::{Error as json_Error, Value};

use crate::asset::AssetManagerRc;
use crate::model::Color;

type Result<T> = result_Result<T, Error>;

#[derive(Debug)]
pub enum Error {
    ParseError(json_Error),
    BuildError(String),
}

impl From<json_Error> for Error {
    fn from(value: json_Error) -> Self {
        Error::ParseError(value)
    }
}

// SongInfo

pub struct SongInfo {
    asset_mgr: AssetManagerRc,
    dir: String,
    author: String,
    title: String,
    song_filename: String,
    bpm_selector: BPMSelector,
    color_schemes: Box<[ColorScheme]>,
    beatmap_infos: Box<[BeatmapInfo]>,
}

impl SongInfo {
    pub fn load<S: AsRef<str>>(asset_mgr: AssetManagerRc, dir: S) -> Result<Self> {
        // From https://docs.rs/serde_json/latest/serde_json/fn.from_reader.html :
        // Note that counter to intuition, this function is usually slower than reading a file completely into memory and then applying from_str or from_slice on it.

        let buf = asset_mgr.read_file(&format!("{}/Info.dat", dir.as_ref()));
        let value: Value = serde_json::from_str(&buf)?;

        match get_version(&value)? {
            "2.0.0" | "2.1.0" => {
                let info: SongInfo_V2 = serde_json::from_value(value)?;
                Ok(info.build(asset_mgr, dir)?)
            },
            "4.0.0" | "4.0.1" => {
                let info: SongInfo_V4 = serde_json::from_value(value)?;
                Ok(info.build(asset_mgr, dir)?)
            },
            version => Err(Error::BuildError(format!("Unsupported info version: {}", version)))
        }
    }

    #[cfg(feature = "test")]
    pub fn test(asset_mgr: AssetManagerRc) -> Self {
        let beatmap_info = BeatmapInfo::test(Arc::clone(&asset_mgr));

        Self {
            asset_mgr,
            dir: "dir".to_string(),
            author: "author".to_string(),
            title: "title".to_string(),
            song_filename: "song_filename".to_string(),
            bpm_selector: BPMSelector::Fixed(1.0),
            color_schemes: Box::from([]),
            beatmap_infos: Box::from([beatmap_info]),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn new<S: AsRef<str>>(asset_mgr: AssetManagerRc, dir: S, author: String, title: String, sub_title: String, song_filename: String, bpm_selector: BPMSelector, color_schemes: Vec<ColorScheme>, beatmap_infos: Vec<BeatmapInfo>) -> Self {
        let space = if sub_title.is_empty() {
            ""
        } else {
            " "
        };

        Self {
            asset_mgr,
            dir: dir.as_ref().to_string(),
            author,
            title: format!("{}{}{}", title, space, sub_title),
            song_filename: format!("{}/{}", dir.as_ref(), song_filename),
            bpm_selector,
            color_schemes: Box::from(color_schemes),
            beatmap_infos: Box::from(beatmap_infos),
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
            BPMSelector::Mapped(filename) => BPMInfo::Mapped(BPMMap::load(Arc::clone(&self.asset_mgr), &self.dir, filename)?),
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

pub struct ColorScheme {
    color_l: Color,
    color_r: Color,
}

impl ColorScheme {
    fn new(color_l: Color, color_r: Color) -> Self {
        Self {
            color_l,
            color_r,
        }
    }

    pub fn get_color_l(&self) -> &Color {
        &self.color_l
    }

    pub fn get_color_r(&self) -> &Color {
        &self.color_r
    }
}

impl Default for ColorScheme {
    fn default() -> Self {
        let color_l = Color::from_srgb_float(0.7843137, 0.07843138, 0.07843138); // See https://bsmg.wiki/mapping/lighting-defaults.html#_1-19-0-colors .
        let color_r = Color::from_srgb_float(0.1568627, 0.5568627, 0.8235294);

        ColorScheme::new(color_l, color_r)
    }
}

pub struct BeatmapInfo {
    asset_mgr: AssetManagerRc,
    dir: String,
    characteristic: String, // TODO: map
    difficulty: String, // TODO: map
    color_scheme_index_opt: Option<u32>,
    def_color_scheme: ColorScheme,
    filename: String,
    notejump_speed: f32,
    notejump_beatoffset: f32,
    #[cfg(feature = "test")]
    test: bool,
}

impl BeatmapInfo {
    #[allow(clippy::too_many_arguments)]
    fn new<S: AsRef<str>>(asset_mgr: AssetManagerRc, dir: S, characteristic: String, difficulty: String, color_scheme_index_opt: Option<u32>, def_color_scheme: ColorScheme, filename: String, notejump_speed: f32, notejump_beatoffset: f32) -> Self {
        Self {
            asset_mgr,
            dir: dir.as_ref().to_string(),
            characteristic,
            difficulty,
            color_scheme_index_opt,
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
            dir: "dir".to_string(),
            characteristic: "characteristic".to_string(),
            difficulty: "difficulty".to_string(),
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

        Beatmap::load(Arc::clone(&self.asset_mgr), &self.dir, &self.filename)
    }

    pub fn get_characteristic(&self) -> &str {
        &self.characteristic
    }

    pub fn get_difficulty(&self) -> &str {
        &self.difficulty
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

    #[allow(dead_code)] // TODO: remove dead_code once it is used
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
    fn build<S: AsRef<str>>(self, asset_mgr: AssetManagerRc, dir: S) -> Result<SongInfo> {
        let mut color_schemes = Vec::new();
        if let Some(raw_color_schemes) = self.color_schemes {
            for raw_color_scheme in raw_color_schemes {
                let inner = raw_color_scheme.inner;
                let color_l = inner.color_l;
                let color_r = inner.color_r;
                let color_scheme = ColorScheme::new(Color::from_srgb_float(color_l.r, color_l.g, color_l.b), Color::from_srgb_float(color_r.r, color_r.g, color_r.b));
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
                }

                let beatmap_info = BeatmapInfo::new(Arc::clone(&asset_mgr), dir.as_ref(), characteristic.clone(), raw_beatmap_info.difficulty, raw_beatmap_info.color_scheme_index_opt, def_color_scheme, raw_beatmap_info.filename, raw_beatmap_info.notejump_speed, raw_beatmap_info.notejump_beatoffset);
                beatmap_infos.push(beatmap_info);
            }
        }

        Ok(SongInfo::new(asset_mgr, dir, self.author, self.title, self.sub_title, self.song_filename, BPMSelector::Fixed(self.bpm), color_schemes, beatmap_infos))
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
    difficulty: String,
    #[serde(rename = "_beatmapColorSchemeIdx")]
    color_scheme_index_opt: Option<u32>,
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
    fn build<S: AsRef<str>>(self, asset_mgr: AssetManagerRc, dir: S) -> Result<SongInfo> {
        let bpm_selector = if let Some(filename) = self.audio.bpmmap_filename {
            BPMSelector::Mapped(filename)
        } else if let Some(bpm) = self.audio.bpm {
            BPMSelector::Fixed(bpm)
        } else {
            return Err(Error::BuildError("Either bpm or audioDataFilename is required".to_string()));
        };

        let mut color_schemes = Vec::new();
        if let Some(raw_color_schemes) = self.color_schemes {
            for raw_color_scheme in raw_color_schemes {
                let color_scheme = ColorScheme::new(raw_color_scheme.color_l, raw_color_scheme.color_r);
                color_schemes.push(color_scheme);
            }
        }

        let mut beatmap_infos = Vec::new();
        for raw_beatmap_info in self.beatmap_infos {
            let beatmap_info = BeatmapInfo::new(Arc::clone(&asset_mgr), dir.as_ref(), raw_beatmap_info.characteristic, raw_beatmap_info.difficulty, raw_beatmap_info.color_scheme_index_opt, ColorScheme::default(), raw_beatmap_info.filename, raw_beatmap_info.notejump_speed, raw_beatmap_info.notejump_beatoffset);
            beatmap_infos.push(beatmap_info);
        }

        Ok(SongInfo::new(asset_mgr, dir, self.song.author, self.song.title, self.song.sub_title, self.audio.song_filename, bpm_selector, color_schemes, beatmap_infos))
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
}

#[derive(Deserialize)]
struct SongInfo_V4_BeatmapInfo {
    characteristic: String,
    difficulty: String,
    #[serde(rename = "beatmapColorSchemeIdx")]
    color_scheme_index_opt: Option<u32>,
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
    fn load<S: AsRef<str>>(asset_mgr: AssetManagerRc, dir: S, filename: S) -> Result<Self> {
        let buf = asset_mgr.read_file(&format!("{}/{}", dir.as_ref(), filename.as_ref()));
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
            version => Err(Error::BuildError(format!("Unsupported bpmmap version: {}", version)))
        }
    }

    fn new(mut ranges: Vec<BPMRange>) -> Self {
        ranges.sort_by(|range1, range2| range1.bpm.start.partial_cmp(&range2.bpm.start).expect("Unable to compare"));

        Self {
            ranges: Box::from(ranges),
        }
    }

    pub fn get_ts(&self, bpm: f32) -> Option<f32> {
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
    notes: Box<[Note]>
}

impl Beatmap {
    fn load<S: AsRef<str>>(asset_mgr: AssetManagerRc, dir: S, filename: S) -> Result<Self> {
        let buf = asset_mgr.read_file(&format!("{}/{}", dir.as_ref(), filename.as_ref()));
        let value: Value = serde_json::from_str(&buf)?;

        match get_version(&value)? {
            "2.0.0" | "2.2.0" => {
                let beatmap: Beatmap_V2 = serde_json::from_value(value)?;
                beatmap.build()
            },
            "3.0.0" | "3.3.0" => {
                let beatmap: Beatmap_V3 = serde_json::from_value(value)?;
                beatmap.build()
            },
            "4.0.0" | "4.1.0" => {
                let beatmap: Beatmap_V4 = serde_json::from_value(value)?;
                beatmap.build()
            },
            version => Err(Error::BuildError(format!("Unsupported beatmap version: {}", version)))
        }
    }

    #[cfg(feature = "test")]
    fn test() -> Result<Self> {
        let mut cut_dirs = [
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
            let note = Note::new(i as f32, 2, 1, NoteType::Right, cut_dirs.next().unwrap()).unwrap();
            notes.push(note);
        }
        
        Ok(Self {
            notes: Box::from(notes),
        })
    }

    fn new(mut notes: Vec<Note>) -> Self {
        notes.sort_by(|note1, note2| note1.bpm_pos.partial_cmp(&note2.bpm_pos).expect("Unable to compare"));

        Self {
            notes: Box::from(notes),
        }
    }

    pub fn get_notes(&self) -> &[Note] {
        &self.notes
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
    fn new(bpm_pos: f32, x: u8, y: u8, note_type: NoteType, cut_dir: NoteCutDir) -> Result<Self> {
        if x > 3 || y > 2 {
            return Err(Error::BuildError("Either note x or y invalid".to_string()));
        }

        Ok(Self {
            bpm_pos,
            x,
            y,
            note_type,
            cut_dir,
        })
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

#[derive(Deserialize)]
struct Beatmap_V2 {
    #[serde(rename = "_notes")]
    notes: Vec<Beatmap_V2_Note>,
}

impl Beatmap_V2 {
    fn build(self) -> Result<Beatmap> {
        let mut notes = Vec::new();

        for raw_note in self.notes {
            if let Some(note_type) = get_note_type(raw_note.note_type) {
                let note = Note::new(raw_note.bpm_pos, raw_note.x, raw_note.y, note_type, raw_note.cut_dir)?;
                notes.push(note);
            }
        }

        Ok(Beatmap::new(notes))
    }
}

#[derive(Deserialize)]
struct Beatmap_V2_Note { // TODO: impl validate
    #[serde(rename = "_time")]
    bpm_pos: f32,
    #[serde(rename = "_lineIndex")]
    x: u8,
    #[serde(rename = "_lineLayer")]
    y: u8,
    #[serde(rename = "_type")]
    note_type: u8,
    #[serde(rename = "_cutDirection")]
    cut_dir: NoteCutDir,
}

#[derive(Deserialize)]
struct Beatmap_V3 {
    #[serde(rename = "colorNotes")]
    notes: Vec<Beatmap_V3_Note>,
}

impl Beatmap_V3 {
    fn build(self) -> Result<Beatmap> {
        let mut notes = Vec::new();

        for raw_note in self.notes {
            if let Some(note_type) = get_note_type(raw_note.note_type) {
                let note = Note::new(raw_note.bpm_pos, raw_note.x, raw_note.y, note_type, raw_note.cut_dir)?;
                notes.push(note);
            }
        }

        Ok(Beatmap::new(notes))
    }
}

#[derive(Deserialize)]
struct Beatmap_V3_Note { // TODO: impl validate
    #[serde(rename = "b")]
    bpm_pos: f32,
    x: u8,
    y: u8,
    #[serde(rename = "c")]
    note_type: u8,
    #[serde(rename = "d")]
    cut_dir: NoteCutDir,
}

#[derive(Deserialize)]
struct Beatmap_V4 {
    #[serde(rename = "colorNotes")]
    notes: Vec<Beatmap_V4_Note>,
    #[serde(rename = "colorNotesData")]
    note_datas: Vec<Beatmap_V4_NoteData>,
}

impl Beatmap_V4 {
    fn build(self) -> Result<Beatmap> {
        let mut notes = Vec::new();

        for raw_note in self.notes {
            if let Some(raw_note_data) = self.note_datas.get(raw_note.data_index as usize) &&
               let Some(note_type) = get_note_type(raw_note_data.note_type) {
                let note = Note::new(raw_note.bpm_pos, raw_note_data.x, raw_note_data.y, note_type, raw_note_data.cut_dir)?;
                notes.push(note);
            }
        }

        Ok(Beatmap::new(notes))
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
    x: u8,
    y: u8,
    #[serde(rename = "c")]
    note_type: u8,
    #[serde(rename = "d")]
    cut_dir: NoteCutDir,
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

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
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

impl<'de> Visitor<'de> for NoteCutDirVisitor {
    type Value = NoteCutDir;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
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
                    return Err(Error::BuildError("Version should be a string".to_string()));
                }
            }
        }

        Err(Error::BuildError("Version not found".to_string()))
    } else {
        Err(Error::BuildError("Object expected at top-level".to_string()))
    }
}

fn get_note_type(raw_note_type: u8) -> Option<NoteType> {
    match raw_note_type {
        0 => Some(NoteType::Left),
        1 => Some(NoteType::Right),
        _ => None,
    }
}
