use std::collections::HashMap;
use std::hash::Hash;
use std::sync::{Arc, Condvar, Mutex};

pub struct IndexMap<T> {
    vec: Vec<T>,
    map: HashMap<T, usize>,
}

impl<T: Eq + Hash + Clone> IndexMap<T> {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self {
            vec: Vec::new(),
            map: HashMap::new(),
        }
    }

    pub fn add(&mut self, value: T) -> usize {
        let index = self.map.entry(value.clone()).or_insert_with(|| {
            let index = self.vec.len();
            self.vec.push(value);
            index
        });

        *index
    }

    #[allow(clippy::len_without_is_empty)]
    pub fn len(&self) -> usize {
        self.vec.len()
    }

    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.vec.iter()
    }
}

pub struct MuCo<T> {
    pub mutex: Mutex<T>,
    pub cond: Condvar,
}

impl<T> MuCo<T> {
    pub fn new(data: T) -> Self {
        Self {
            mutex: Mutex::new(data),
            cond: Condvar::new(),
        }
    }
}

pub type StatsRc = Arc<Stats>;

pub struct Stats {
    // Although the render is happening on a single thread, we are already
    // prepared for multi-threading, that's the reason for Mutex.
    
    inner_mutex: Mutex<StatsInner>,
}

#[derive(Copy, Clone)]
pub struct StatsInner {
    pub comment: &'static str,
    pub fps: u32,
    pub frame_time: u32,
    pub draw_calls: u32,
    pub inst_num: u32,
    pub inst_buf: u32,
}

impl Stats {
    pub fn new(comment: &'static str) -> Self {
        let inner = StatsInner {
            comment,
            fps: 0,
            frame_time: 0,
            draw_calls: 0,
            inst_num: 0,
            inst_buf: 0,
        };

        Self {
            inner_mutex: Mutex::new(inner),
        }
    }

    pub fn get_inner(&self) -> StatsInner {
        *self.inner_mutex.lock().unwrap()
    }

    // TODO: simply return with mutexguard, so we don't have to lock everytime
    // when multiple values are updated?
    pub fn set_fps(&self, fps: u32) {
        self.inner_mutex.lock().unwrap().fps = fps;
    }

    pub fn set_frame_time(&self, frame_time: u32) {
        self.inner_mutex.lock().unwrap().frame_time = frame_time;
    }

    pub fn set_draw_calls(&self, draw_calls: u32) {
        self.inner_mutex.lock().unwrap().draw_calls = draw_calls;
    }

    pub fn set_inst_num(&self, inst_num: u32) {
        self.inner_mutex.lock().unwrap().inst_num = inst_num;
    }

    pub fn set_inst_buf(&self, inst_buf: u32) {
        self.inner_mutex.lock().unwrap().inst_buf = inst_buf;
    }
}
