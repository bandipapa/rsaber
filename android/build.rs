use std::fs;
use std::io::{Cursor, Read};
use std::path::PathBuf;

use reqwest::blocking;
use sha2::{Digest, Sha256};
use zip::ZipArchive;

const URL: &str = "https://repo1.maven.org/maven2/org/khronos/openxr/openxr_loader_for_android/1.1.54/openxr_loader_for_android-1.1.54.aar";
const DIGEST: &str = "66702fc24475d7a5362688e77444670124c2fa119745d14c2524bcf3eb584834";
const ARCH: &str = "arm64-v8a";
const LIB: &str = "libopenxr_loader.so";

fn main() {
    // Fetch OpenXR loader.

    let buf = blocking::get(URL).unwrap_or_else(|e| panic!("Unable to fetch {}: {}", URL, e)).bytes().unwrap();
    let calc_digest = Sha256::digest(&buf);
    let exp_digest: Box<[u8]> = (0..DIGEST.len()).step_by(2).map(|i| u8::from_str_radix(&DIGEST[i..i + 2], 16).unwrap()).collect();
    if *calc_digest != *exp_digest {
        panic!("Digest mismatch");
    }

    // Open zip and unpack file.

    let reader = Cursor::new(buf);
    let mut zip = ZipArchive::new(reader).unwrap();
    let mut file = zip.by_path(format!("jni/{}/{}", ARCH, LIB)).unwrap();

    let mut buf = Vec::new();
    file.read_to_end(&mut buf).unwrap();

    // Create dir.
    // TODO: It would be nice if we can set package.metadata.android->runtime_libs dynamically, e.g. OUT_DIR/android_runtime_libs.

    let mut path = PathBuf::from("lib");
    path.push(ARCH);
    fs::create_dir_all(&path).unwrap();

    // Save file.

    path.push(LIB);
    fs::write(path, buf).unwrap();
}
