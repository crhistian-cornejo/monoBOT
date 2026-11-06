//! What the page reads again and again, kept in memory while the app runs: the last reading of the accounts, the messages
//! opened lately, the pictures on the web fetched for them. Nothing here reaches the disk (README), and it is bounded: a
//! limit in bytes, and the least recently used entries go first when a new one would pass it.

use std::collections::VecDeque;

/// A map of string keys to values, each with its size in bytes. The most recent entry is at the front.
#[derive(Debug)]
pub struct Cache<V> {
    entries: VecDeque<(String, V, usize)>,
    bytes: usize,
    limit: usize,
}

impl<V: Clone> Cache<V> {
    pub fn new(limit: usize) -> Self {
        Self {
            entries: VecDeque::new(),
            bytes: 0,
            limit,
        }
    }

    /// The value under `key`, which becomes the most recent one.
    pub fn get(&mut self, key: &str) -> Option<V> {
        let at = self.entries.iter().position(|(k, _, _)| k == key)?;
        let entry = self.entries.remove(at)?;
        let value = entry.1.clone();
        self.entries.push_front(entry);
        Some(value)
    }

    /// Keeps `value` under `key` as the most recent one, of `size` bytes. A value bigger than the whole limit is not kept.
    pub fn put(&mut self, key: String, value: V, size: usize) {
        self.forget(&key);
        if size > self.limit {
            return;
        }
        while self.bytes + size > self.limit {
            let Some((_, _, old)) = self.entries.pop_back() else {
                break;
            };
            self.bytes -= old;
        }
        self.bytes += size;
        self.entries.push_front((key, value, size));
    }

    /// Drops the entries whose key the predicate refuses (a removed account's messages, say).
    pub fn retain(&mut self, keep: impl Fn(&str) -> bool) {
        self.entries.retain(|(key, _, _)| keep(key));
        self.bytes = self.entries.iter().map(|(_, _, size)| size).sum();
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.bytes = 0;
    }

    fn forget(&mut self, key: &str) {
        if let Some(at) = self.entries.iter().position(|(k, _, _)| k == key) {
            if let Some((_, _, old)) = self.entries.remove(at) {
                self.bytes -= old;
            }
        }
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.entries.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_value_comes_back_until_the_limit_pushes_the_least_recent_one_out() {
        let mut cache = Cache::new(10);
        cache.put("a".into(), 1, 4);
        cache.put("b".into(), 2, 4);
        assert_eq!(
            cache.get("a"),
            Some(1),
            "reading a makes it the most recent"
        );
        cache.put("c".into(), 3, 4);
        assert_eq!(
            (cache.get("b"), cache.get("a"), cache.get("c")),
            (None, Some(1), Some(3))
        );
        assert_eq!(cache.bytes, 8);
    }

    #[test]
    fn a_value_bigger_than_the_whole_limit_is_not_kept_and_a_new_value_under_a_key_replaces_it() {
        let mut cache = Cache::new(10);
        cache.put("a".into(), 1, 4);
        cache.put("a".into(), 2, 6);
        assert_eq!((cache.get("a"), cache.bytes), (Some(2), 6));
        cache.put("grande".into(), 3, 11);
        assert_eq!((cache.get("grande"), cache.len()), (None, 1));
    }

    #[test]
    fn retain_and_clear_forget_what_they_are_told_and_count_the_bytes_again() {
        let mut cache = Cache::new(100);
        cache.put("cuenta1.inbox.1".into(), 1, 10);
        cache.put("cuenta2.inbox.1".into(), 2, 20);
        cache.retain(|key| key.starts_with("cuenta2"));
        assert_eq!((cache.len(), cache.bytes), (1, 20));
        cache.clear();
        assert_eq!((cache.len(), cache.bytes), (0, 0));
    }
}
