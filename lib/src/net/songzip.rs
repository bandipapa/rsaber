use std::collections::HashMap;
use std::io::{Cursor, Read};
use std::sync::Arc;

use reqwest::{Client, Error as reqwest_Error};
use url::Url;
use zip::ZipArchive;
use zip::result::ZipError;

use crate::asset::{AssetError, AssetFileBox, AssetFileTrait, AssetManagerRc, AssetManagerTrait, AssetResult};
use crate::net::Request;

pub struct SongZipRequest {
    url: Url,
}

impl SongZipRequest {
    pub fn new(url: Url) -> Self {
        Self {
            url,
        }
    }
}

impl Request for SongZipRequest {
    type Response = AssetManagerRc;
    type Error = SongZipError;

    async fn exec(self, client: Client) -> Result<Self::Response, Self::Error> {
        let buf = client.get(self.url)
            .send()
            .await?
            .error_for_status()?
            .bytes()
            .await?;

        // Extract zip.
        // TODO: At the moment it is in-memory, extract into disk?
        // TODO: Place limit on uncompressed size?

        let mut zip = ZipArchive::new(Cursor::new(buf))?;
        let names: Box<[_]> = zip.file_names().map(|name| name.to_string()).collect();
        let mut content = HashMap::new();

        for name in names {
            let mut file = zip.by_name(&name)?;

            let mut buf = Vec::new();
            file.read_to_end(&mut buf).map_err(ZipError::Io)?;

            content.insert(format!("/{}", name.to_lowercase()), Arc::from(buf)); // Store filename as case-insensitive.
        }

        Ok(Arc::new(AssetManager::new(content)))
    }
}

type Content = HashMap<String, BufRc>;
type BufRc = Arc<[u8]>;

struct AssetManager {
    content: Content,
}

impl AssetManager {
    fn new(content: Content) -> Self {
        Self {
            content,
        }
    }
}

impl AssetManagerTrait for AssetManager {
    fn open(&self, name: &str) -> AssetResult<AssetFileBox> {
        let buf = Arc::clone(self.content.get(&name.to_lowercase()).ok_or(AssetError::NotFound)?);
        Ok(Box::new(AssetFile::new(buf)))
    }
}

struct AssetFile {
    buf: BufRc,
}

impl AssetFile {
    fn new(buf: BufRc) -> Self {
        Self {
            buf,
        }
    }
}

impl AssetFileTrait for AssetFile {
    fn read(&self) -> AssetResult<Box<dyn Read + Send + Sync>> {
        Ok(Box::new(Cursor::new(Arc::clone(&self.buf))))
    }

    fn read_str(&self) -> AssetResult<String> {
        Ok(String::from(str::from_utf8(&self.buf).map_err(|_| AssetError::Decode)?))
    }
}

#[allow(unused)]
#[derive(Debug)]
pub enum SongZipError {
    Fetch(reqwest_Error),
    Decode(ZipError),
}

impl From<reqwest_Error> for SongZipError {
    fn from(value: reqwest_Error) -> Self {
        SongZipError::Fetch(value)
    }
}

impl From<ZipError> for SongZipError {
    fn from(value: ZipError) -> Self {
        SongZipError::Decode(value)
    }
}
