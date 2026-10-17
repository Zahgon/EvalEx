// Copyright 2012-2022 Udo Klimaschewski (EvalEx)
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! The iteration order of `java.util.HashMap<String, V>`.

use super::chars::hash_code;
use super::JString;

const INITIAL_CAPACITY: usize = 16;

/// A map with the observable behaviour of a default `HashMap` keyed by strings.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HashMap<V> {
    entries: Vec<(JString, V)>,
}

impl<V> HashMap<V> {
    /// An empty map.
    pub fn new() -> HashMap<V> {
        HashMap { entries: Vec::new() }
    }

    /// `put`: a key that exists keeps its place.
    pub fn put(&mut self, key: JString, value: V) {
        match self.entries.iter_mut().find(|(existing, _)| *existing == key) {
            Some(entry) => entry.1 = value,
            None => self.entries.push((key, value)),
        }
    }

    /// `get`.
    pub fn get(&self, key: &[u16]) -> Option<&V> {
        self.entries.iter().find(|(existing, _)| existing == key).map(|(_, value)| value)
    }

    /// The number of entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the map is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The entries in iteration order: by bucket, and in insertion order inside a bucket.
    pub fn iter(&self) -> Vec<&(JString, V)> {
        let mut capacity = INITIAL_CAPACITY;
        while self.entries.len() * 4 > capacity * 3 {
            capacity *= 2;
        }
        let mut ordered: Vec<_> = self.entries.iter().collect();
        ordered.sort_by_key(|(key, _)| bucket(key, capacity));
        ordered
    }
}

fn bucket(key: &[u16], capacity: usize) -> usize {
    let hash = hash_code(key) as u32;
    (hash ^ (hash >> 16)) as usize & (capacity - 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::java::js;

    #[test]
    fn iterates_by_bucket() {
        let mut map = HashMap::new();
        for key in ["x", "y", "zz"] {
            map.put(js(key), ());
        }
        let keys: Vec<_> = map.iter().iter().map(|entry| entry.0.clone()).collect();
        assert_eq!(keys, vec![js("zz"), js("x"), js("y")]);
    }

    #[test]
    fn put_replaces_and_get_finds() {
        let mut map = HashMap::new();
        map.put(js("a"), 1);
        map.put(js("a"), 2);
        assert_eq!(map.get(&js("a")), Some(&2));
        assert_eq!(map.get(&js("b")), None);
        assert_eq!(map.len(), 1);
    }
}
