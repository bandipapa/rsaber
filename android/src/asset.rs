use std::ffi::{CStr, CString};
use std::io::{Read, Result as io_Result};
use std::iter;
use std::thread;

use android_activity::AndroidApp;

use rsaber_lib::AssetManagerTrait;
use rsaber_lib::circbuf;

const BUF_LEN: usize = 64 * 1024;
const READ_LEN: usize = 4 * 1024;

pub struct AssetManager {
    app: AndroidApp,
}

impl AssetManager {
    pub fn new(app: AndroidApp) -> Self {
        Self {
            app,
        }
    }

    fn open(app: &AndroidApp, name: &CStr) -> impl Read + 'static {
        app.asset_manager().open(name).expect("Unable to open asset")
    }
}

impl AssetManagerTrait for AssetManager {
    fn open_thr(&self, name: &str) -> Box<dyn Read + Send + Sync + 'static> { // TODO: this is too complex, just read the file into memory?
        let app = self.app.clone();
        let name = CString::new(name).unwrap();
        let (sender, receiver) = circbuf::circbuf::<u8>(BUF_LEN);

        thread::spawn(move || {
            let mut asset = Self::open(&app, &name);
            let mut buf = Box::from_iter(iter::repeat_n(0_u8, READ_LEN));

            loop {
                let r = asset.read(&mut buf).expect("I/O error");
                if r == 0 {
                    break;
                }

                if !sender.send(&buf[..r]) { // If the receiver is dropped, stop.
                    break;
                }
            }
        });

        Box::new(Reader::new(receiver))
    }

    fn open(&self, name: &str) -> Box<dyn Read> {
        let name: CString = CString::new(name).unwrap();
        Box::new(Self::open(&self.app, &name))
    }

    fn read_file(&self, name: &str) -> String {
        let name: CString = CString::new(name).unwrap();
        let mut asset = Self::open(&self.app, &name);

        let mut buf = String::new();
        asset.read_to_string(&mut buf).expect("I/O error");
        buf
    }
}

struct Reader {
    receiver: circbuf::Receiver<u8>,
}

impl Reader {
    fn new(receiver: circbuf::Receiver<u8>) -> Self {
        Self {
            receiver,
        }
    }
}

impl Read for Reader {
    fn read(&mut self, buf: &mut [u8]) -> io_Result<usize> {
        Ok(self.receiver.recv(buf))
    }
}
