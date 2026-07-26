//! In-house insertion-ordered map, replacing the `indexmap` dependency.
//!
//! C# `Dictionary` preserves insertion order in practice and the export/PATH
//! code depends on it. `OrderedMap` gives the same guarantee with the small
//! subset of the map API this codebase actually uses. Key lookups are exact
//! (case-sensitive), matching `IndexMap<String, V>` behavior; `insert` on an
//! existing key replaces the value in place (position preserved), and
//! `remove` preserves the order of the remaining entries (i.e. `shift_remove`
//! semantics — no call site relied on `swap_remove`).

use std::fmt;
use std::ops::Index;

use serde::de::{Deserialize, Deserializer, MapAccess, Visitor};

/// An insertion-ordered `String`-keyed map backed by a `Vec` of pairs.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct OrderedMap<V> {
    entries: Vec<(String, V)>,
}

impl<V> OrderedMap<V> {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    fn position(&self, key: &str) -> Option<usize> {
        self.entries.iter().position(|(k, _)| k == key)
    }

    /// Insert a key/value pair. If the key already exists, the value is
    /// replaced in place and the entry keeps its original position (the same
    /// behavior as `IndexMap::insert`). Returns the previous value, if any.
    pub fn insert(&mut self, key: String, value: V) -> Option<V> {
        match self.position(&key) {
            Some(i) => Some(std::mem::replace(&mut self.entries[i].1, value)),
            None => {
                self.entries.push((key, value));
                None
            }
        }
    }

    pub fn get(&self, key: &str) -> Option<&V> {
        self.position(key).map(|i| &self.entries[i].1)
    }

    pub fn get_mut(&mut self, key: &str) -> Option<&mut V> {
        self.position(key).map(|i| &mut self.entries[i].1)
    }

    pub fn contains_key(&self, key: &str) -> bool {
        self.position(key).is_some()
    }

    /// Remove a key, preserving the order of the remaining entries
    /// (`IndexMap::shift_remove` semantics). Returns the removed value.
    pub fn remove(&mut self, key: &str) -> Option<V> {
        self.position(key).map(|i| self.entries.remove(i).1)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&String, &V)> {
        self.entries.iter().map(|(k, v)| (k, v))
    }

    pub fn keys(&self) -> impl Iterator<Item = &String> {
        self.entries.iter().map(|(k, _)| k)
    }

    pub fn values(&self) -> impl Iterator<Item = &V> {
        self.entries.iter().map(|(_, v)| v)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl<V> Index<&str> for OrderedMap<V> {
    type Output = V;

    fn index(&self, key: &str) -> &V {
        self.get(key).expect("key not found in OrderedMap")
    }
}

impl<V> IntoIterator for OrderedMap<V> {
    type Item = (String, V);
    type IntoIter = std::vec::IntoIter<(String, V)>;

    fn into_iter(self) -> Self::IntoIter {
        self.entries.into_iter()
    }
}

impl<'a, V> IntoIterator for &'a OrderedMap<V> {
    type Item = (&'a String, &'a V);
    type IntoIter = std::iter::Map<
        std::slice::Iter<'a, (String, V)>,
        fn(&'a (String, V)) -> (&'a String, &'a V),
    >;

    fn into_iter(self) -> Self::IntoIter {
        self.entries.iter().map(|(k, v)| (k, v))
    }
}

impl<V> FromIterator<(String, V)> for OrderedMap<V> {
    fn from_iter<T: IntoIterator<Item = (String, V)>>(iter: T) -> Self {
        let mut map = OrderedMap::new();
        for (k, v) in iter {
            map.insert(k, v);
        }
        map
    }
}

impl<V> Extend<(String, V)> for OrderedMap<V> {
    fn extend<T: IntoIterator<Item = (String, V)>>(&mut self, iter: T) {
        for (k, v) in iter {
            self.insert(k, v);
        }
    }
}

impl<'de, V: Deserialize<'de>> Deserialize<'de> for OrderedMap<V> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct MapVisitor<V>(std::marker::PhantomData<V>);

        impl<'de, V: Deserialize<'de>> Visitor<'de> for MapVisitor<V> {
            type Value = OrderedMap<V>;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a map")
            }

            fn visit_map<A: MapAccess<'de>>(self, mut access: A) -> Result<Self::Value, A::Error> {
                let mut map = OrderedMap {
                    entries: Vec::with_capacity(access.size_hint().unwrap_or(0)),
                };
                while let Some((key, value)) = access.next_entry::<String, V>()? {
                    map.insert(key, value);
                }
                Ok(map)
            }
        }

        deserializer.deserialize_map(MapVisitor(std::marker::PhantomData))
    }
}

impl<V: serde::Serialize> serde::Serialize for OrderedMap<V> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(Some(self.entries.len()))?;
        for (k, v) in &self.entries {
            map.serialize_entry(k, v)?;
        }
        map.end()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> OrderedMap<i32> {
        let mut m = OrderedMap::new();
        m.insert("zebra".to_string(), 1);
        m.insert("Apple".to_string(), 2);
        m.insert("mango".to_string(), 3);
        m
    }

    #[test]
    fn preserves_insertion_order() {
        let m = sample();
        let keys: Vec<_> = m.keys().map(String::as_str).collect();
        assert_eq!(keys, vec!["zebra", "Apple", "mango"]);
        let values: Vec<_> = m.values().copied().collect();
        assert_eq!(values, vec![1, 2, 3]);
    }

    #[test]
    fn insert_replaces_in_place_preserving_position() {
        let mut m = sample();
        assert_eq!(m.insert("Apple".to_string(), 20), Some(2));
        let pairs: Vec<_> = m.iter().map(|(k, v)| (k.as_str(), *v)).collect();
        assert_eq!(pairs, vec![("zebra", 1), ("Apple", 20), ("mango", 3)]);
        assert_eq!(m.len(), 3);
    }

    #[test]
    fn lookups_are_case_sensitive() {
        let m = sample();
        assert_eq!(m.get("Apple"), Some(&2));
        assert_eq!(m.get("apple"), None);
        assert!(m.contains_key("Apple"));
        assert!(!m.contains_key("APPLE"));
    }

    #[test]
    fn remove_preserves_remaining_order() {
        let mut m = sample();
        assert_eq!(m.remove("zebra"), Some(1));
        let keys: Vec<_> = m.keys().map(String::as_str).collect();
        assert_eq!(keys, vec!["Apple", "mango"]);
        assert_eq!(m.remove("missing"), None);
    }

    #[test]
    fn indexing_and_get_mut() {
        let mut m = sample();
        assert_eq!(m["mango"], 3);
        *m.get_mut("mango").unwrap() = 30;
        assert_eq!(m["mango"], 30);
    }

    #[test]
    fn empty_len_and_iterators() {
        let m: OrderedMap<i32> = OrderedMap::new();
        assert!(m.is_empty());
        assert_eq!(m.len(), 0);

        let m = sample();
        assert!(!m.is_empty());
        let owned: Vec<_> = m.clone().into_iter().collect();
        assert_eq!(owned[0], ("zebra".to_string(), 1));
        let borrowed: Vec<_> = (&m).into_iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(borrowed, vec!["zebra", "Apple", "mango"]);
    }

    #[test]
    fn from_iterator_and_extend() {
        let m: OrderedMap<i32> = vec![
            ("b".to_string(), 1),
            ("a".to_string(), 2),
            ("b".to_string(), 3),
        ]
        .into_iter()
        .collect();
        let pairs: Vec<_> = m.iter().map(|(k, v)| (k.as_str(), *v)).collect();
        // Duplicate key keeps first position, last value.
        assert_eq!(pairs, vec![("b", 3), ("a", 2)]);

        let mut m = m;
        m.extend(vec![("c".to_string(), 4), ("a".to_string(), 5)]);
        let pairs: Vec<_> = m.iter().map(|(k, v)| (k.as_str(), *v)).collect();
        assert_eq!(pairs, vec![("b", 3), ("a", 5), ("c", 4)]);
    }

    #[test]
    fn deserialize_preserves_json_order() {
        let m: OrderedMap<String> =
            serde_json::from_str(r#"{"Z":"1","a":"2","M":"3","0":"4"}"#).unwrap();
        let keys: Vec<_> = m.keys().map(String::as_str).collect();
        assert_eq!(keys, vec!["Z", "a", "M", "0"]);
    }
}
