use std::collections::HashMap;
use std::hash::Hash;

pub struct IndexMap<T> {
    vec: Vec<T>,
    map: HashMap<T, usize>,
}

impl<T: Eq + Hash + Clone> IndexMap<T> {
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

    pub fn len(&self) -> usize {
        self.vec.len()
    }

    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.vec.iter()
    }
}
