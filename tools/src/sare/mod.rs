pub mod extract;
pub mod fuse;
pub mod retype;
pub mod serve;
pub mod viz;
pub mod viz_html;

use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct Ordered<V> {
    order: Vec<String>,
    map: HashMap<String, V>,
}

impl<V> Default for Ordered<V> {
    fn default() -> Self {
        Ordered { order: Vec::new(), map: HashMap::new() }
    }
}

impl<V> Ordered<V> {
    pub fn new() -> Self {
        Ordered::default()
    }

    pub fn insert(&mut self, key: &str, value: V) {
        if !self.map.contains_key(key) {
            self.order.push(key.to_string());
        }
        self.map.insert(key.to_string(), value);
    }

    pub fn entry_or(&mut self, key: &str, value: V) -> &mut V {
        if !self.map.contains_key(key) {
            self.order.push(key.to_string());
            self.map.insert(key.to_string(), value);
        }
        self.map.get_mut(key).unwrap()
    }

    pub fn get(&self, key: &str) -> Option<&V> {
        self.map.get(key)
    }

    pub fn get_mut(&mut self, key: &str) -> Option<&mut V> {
        self.map.get_mut(key)
    }

    pub fn contains(&self, key: &str) -> bool {
        self.map.contains_key(key)
    }

    pub fn remove(&mut self, key: &str) -> Option<V> {
        if self.map.contains_key(key) {
            self.order.retain(|k| k != key);
        }
        self.map.remove(key)
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    pub fn keys(&self) -> impl Iterator<Item = &String> {
        self.order.iter()
    }

    pub fn values(&self) -> impl Iterator<Item = &V> {
        self.order.iter().filter_map(|k| self.map.get(k))
    }

    pub fn iter(&self) -> impl Iterator<Item = (&String, &V)> {
        self.order.iter().filter_map(|k| self.map.get(k).map(|v| (k, v)))
    }
}

#[derive(Debug, Clone, Default)]
pub struct Counter {
    inner: Ordered<usize>,
}

impl Counter {
    pub fn new() -> Counter {
        Counter::default()
    }

    pub fn add(&mut self, key: &str) {
        *self.inner.entry_or(key, 0) += 1;
    }

    pub fn get(&self, key: &str) -> usize {
        self.inner.get(key).copied().unwrap_or(0)
    }

    pub fn total(&self) -> usize {
        self.inner.values().sum()
    }

    pub fn distinct(&self) -> usize {
        self.inner.len()
    }

    pub fn most_common(&self) -> Vec<(String, usize)> {
        let mut rows: Vec<(String, usize)> =
            self.inner.iter().map(|(k, v)| (k.clone(), *v)).collect();
        rows.sort_by_key(|a| std::cmp::Reverse(a.1));
        rows
    }
}

impl Counter {
    pub fn most_common_insertion_order(&self) -> Vec<(String, usize)> {
        self.inner.iter().map(|(k, v)| (k.clone(), *v)).collect()
    }
}
