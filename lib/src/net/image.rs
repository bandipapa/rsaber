use std::io::Cursor;

use image::{ImageError as image_Error, ImageReader};
use image::error::{DecodingError, ImageFormatHint};
use reqwest::{Client, Error as reqwest_Error};
use url::Url;

use crate::net::Request;

pub struct ImageRequest {
    url: Url,
}

impl ImageRequest {
    pub fn new(url: Url) -> Self {
        Self {
            url,
        }
    }
}

impl Request for ImageRequest {
    type Response = ImageResponse;
    type Error = ImageError;

    async fn exec(self, client: Client) -> Result<Self::Response, Self::Error> {
        let buf = client.get(self.url)
            .send()
            .await?
            .error_for_status()?
            .bytes()
            .await?;

        // Decode image, so we are independent of the actual UI toolkit.

        let reader = Cursor::new(buf);
        let image = ImageReader::new(reader)
            .with_guessed_format().map_err(|_| image_Error::Decoding(DecodingError::from_format_hint(ImageFormatHint::Unknown)))? // TODO: Use Content-Type from response to avoid format guess?
            .decode()?
            .into_rgba8();

        Ok(ImageResponse::new(image.width(), image.height(), image.into_raw().into_boxed_slice()))
    }
}

pub struct ImageResponse {
    width: u32,
    height: u32,
    data: Box<[u8]>,
}

impl ImageResponse {
    fn new(width: u32, height: u32, data: Box<[u8]>) -> Self {
        Self {
            width,
            height,
            data,
        }
    }

    pub fn get_width(&self) -> u32 {
        self.width
    }

    pub fn get_height(&self) -> u32 {
        self.height
    }

    pub fn get_data(&self) -> &[u8] {
        &self.data
    }
}

#[allow(unused)]
pub enum ImageError {
    Fetch(reqwest_Error),
    Decode(image_Error),
}

impl From<reqwest_Error> for ImageError {
    fn from(value: reqwest_Error) -> Self {
        ImageError::Fetch(value)
    }
}

impl From<image_Error> for ImageError {
    fn from(value: image_Error) -> Self {
        ImageError::Decode(value)
    }
}
