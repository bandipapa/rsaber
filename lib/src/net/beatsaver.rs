// See https://api.beatsaver.com/docs/index.html for API description.
use std::sync::LazyLock;

use reqwest::{Client, Error as reqwest_Error};
use serde::Deserialize;
use url::Url;

use crate::net::Request;
use crate::songdef::SongDifficulty;

static API: LazyLock<Url> = LazyLock::new(|| Url::parse("https://beatsaver.com/api/").expect("Invalid url"));

pub struct BeatSaverSearchRequest {
    query: String,
    order: String,
    ascending: bool,
}

impl BeatSaverSearchRequest {
    pub fn new<S: AsRef<str>>(query: S, order: S, ascending: bool) -> Self { // TODO: separate type parameters for query, order.
        Self {
            query: query.as_ref().to_string(),
            order: order.as_ref().to_string(),
            ascending,
        }
    }
}

impl Request for BeatSaverSearchRequest {
    type Response = BeatSaverSearchResponse;
    type Error = reqwest_Error;

    async fn exec(self, client: Client) -> Result<Self::Response, Self::Error> {
        let url = API.join("search/text/0").expect("Invalid url");

        client.get(url)
            .query(&[
                ("q", self.query.as_str()),
                ("order", self.order.as_str()),
                ("ascending", if self.ascending { "true" } else { "false" }),
                ("pageSize", "100"), // TODO: at the moment it is hardcoded, implement paging on UI?
            ])
            .send()
            .await?
            .error_for_status()?
            .json()
            .await
    }
}

#[derive(Deserialize)]
pub struct BeatSaverSearchResponse {
    docs: Vec<BeatSaverSong>,
}

impl BeatSaverSearchResponse {
    pub fn get_songs(&self) -> &[BeatSaverSong] {
        &self.docs
    }
}

#[derive(Deserialize)]
pub struct BeatSaverSong {
    name: String,
    uploader: BeatSaverSongUploader,
    metadata: BeatSaverSongMetadata,
    stats: BeatSaverSongStats,
    versions: Vec<BeatSaverSongVersion>,
}

impl BeatSaverSong {
    pub fn get_name(&self) -> &str {
        &self.name
    }

    pub fn get_uploader(&self) -> &BeatSaverSongUploader {
        &self.uploader
    }

    pub fn get_metadata(&self) -> &BeatSaverSongMetadata {
        &self.metadata
    }

    pub fn get_stats(&self) -> &BeatSaverSongStats {
        &self.stats
    }

    pub fn get_published_version(&self) -> Option<&BeatSaverSongVersion> {
        self.versions.iter().find(|version| version.get_state() == "Published")
    }
}

#[derive(Deserialize)]
pub struct BeatSaverSongUploader {
    name: String,
}

impl BeatSaverSongUploader {
    pub fn get_name(&self) -> &str {
        &self.name
    }
}

#[derive(Deserialize)]
pub struct BeatSaverSongMetadata {
    bpm: f32, // TODO: validate > 0
    duration: i32, // TODO: validate > 0
}

impl BeatSaverSongMetadata {
    pub fn get_bpm(&self) -> f32 {
        self.bpm
    }

    pub fn get_duration(&self) -> i32 {
        self.duration
    }
}

#[derive(Deserialize)]
pub struct BeatSaverSongStats {
    score: f32, // TODO: validate >= 0
}

impl BeatSaverSongStats {
    pub fn get_score(&self) -> f32 {
        self.score
    }
}

#[derive(Deserialize)]
pub struct BeatSaverSongVersion {
    state: String,
    #[serde(rename = "coverURL")]
    cover_url: Url,
    #[serde(rename = "previewURL")]
    preview_url: Url,
    #[serde(rename = "downloadURL")]
    download_url: Url,
    #[serde(rename = "diffs")]
    variants: Vec<BeatSaverSongVariant>,
}

impl BeatSaverSongVersion {
    pub fn get_state(&self) -> &str {
        &self.state
    }

    pub fn get_cover_url(&self) -> &Url {
        &self.cover_url
    }

    pub fn get_preview_url(&self) -> &Url {
        &self.preview_url
    }

    pub fn get_download_url(&self) -> &Url {
        &self.download_url
    }

    pub fn get_variants(&self) -> &[BeatSaverSongVariant] {
        &self.variants
    }
}

#[derive(Deserialize)]
pub struct BeatSaverSongVariant {
    characteristic: String,
    difficulty: SongDifficulty,
}

impl BeatSaverSongVariant {
    pub fn get_characteristic(&self) -> &str {
        &self.characteristic
    }

    pub fn get_difficulty(&self) -> SongDifficulty {
        self.difficulty
    }
}
