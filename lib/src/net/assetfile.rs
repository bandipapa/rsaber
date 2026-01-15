use std::io::{Cursor, Read};
use std::sync::Arc;

use reqwest::{Client, Error as reqwest_Error};
use url::Url;

use crate::asset::{AssetError, AssetFileBox, AssetFileTrait, AssetResult};
use crate::net::Request;

pub struct AssetFileRequest {
    url: Url,
}

impl AssetFileRequest {
    pub fn new(url: Url) -> Self {
        Self {
            url,
        }
    }
}

impl Request for AssetFileRequest {
    type Response = AssetFileBox;
    type Error = reqwest_Error;

    async fn exec(self, client: Client) -> Result<Self::Response, Self::Error> {
        let buf = client.get(self.url)
            .send()
            .await?
            .error_for_status()?
            .bytes()
            .await?;

        Ok(Box::new(AssetFile::new(buf.as_ref())))
    }
}

struct AssetFile {
    buf: Arc<[u8]>,
}

impl AssetFile {
    fn new(buf: &[u8]) -> Self {
        Self {
            buf: Arc::from(buf),
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
