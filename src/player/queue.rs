//! The play queue: ordering, shuffle and repeat. Pure data, no audio.
//!
//! Every entry has a `uid` that is unique for the lifetime of the queue, so the same library
//! track can appear twice and "what is playing" is never ambiguous. Entries only store the
//! 4-byte library track id; paths and tags are looked up on demand, which keeps a queue holding
//! an entire 100k-track library under a megabyte.

/// One queue entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Item {
    /// Unique, never reused while the queue lives. `0` is reserved for "nothing".
    pub uid: u32,
    /// Library track id.
    pub track: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Repeat {
    #[default]
    Off,
    All,
    One,
}

impl Repeat {
    pub fn from_index(i: i32) -> Repeat {
        match i {
            1 => Repeat::All,
            2 => Repeat::One,
            _ => Repeat::Off,
        }
    }
    pub fn index(self) -> i32 {
        match self {
            Repeat::Off => 0,
            Repeat::All => 1,
            Repeat::One => 2,
        }
    }
}

#[derive(Default)]
pub struct Queue {
    /// Play order.
    items: Vec<Item>,
    /// Original order. Only populated while shuffle is on, so un-shuffling restores it.
    natural: Vec<Item>,
    shuffle: bool,
    repeat: Repeat,
    next_uid: u32,
    /// The most recently removed entry and the index it occupied, so that "next" still works
    /// when the user removes the track that is playing.
    removed: Option<(u32, usize)>,
}

impl Queue {
    pub fn new() -> Queue {
        Queue { next_uid: 1, ..Default::default() }
    }

    #[cfg(test)]
    pub fn items(&self) -> &[Item] {
        &self.items
    }
    pub fn len(&self) -> usize {
        self.items.len()
    }
    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
    pub fn shuffle(&self) -> bool {
        self.shuffle
    }
    pub fn repeat(&self) -> Repeat {
        self.repeat
    }
    pub fn set_repeat(&mut self, repeat: Repeat) {
        self.repeat = repeat;
    }

    fn fresh_uid(&mut self) -> u32 {
        if self.next_uid == 0 {
            self.next_uid = 1;
        }
        let uid = self.next_uid;
        self.next_uid = self.next_uid.wrapping_add(1);
        uid
    }

    pub fn index_of(&self, uid: u32) -> Option<usize> {
        self.items.iter().position(|i| i.uid == uid)
    }
    pub fn get(&self, uid: u32) -> Option<Item> {
        self.index_of(uid).map(|i| self.items[i])
    }
    pub fn at(&self, index: usize) -> Option<Item> {
        self.items.get(index).copied()
    }

    /// Replaces the queue with `tracks`. Returns the uid of `tracks[start]`, which is the entry
    /// to start playing. With shuffle on it is placed first and everything else is shuffled.
    pub fn set(&mut self, tracks: &[u32], start: usize) -> Option<u32> {
        self.items.clear();
        self.natural.clear();
        self.removed = None;
        if tracks.is_empty() {
            return None;
        }
        self.items.reserve_exact(tracks.len());
        for &track in tracks {
            let uid = self.fresh_uid();
            self.items.push(Item { uid, track });
        }
        let start = start.min(tracks.len() - 1);
        let start_uid = self.items[start].uid;
        if self.shuffle {
            self.natural = self.items.clone();
            self.items.swap(0, start);
            fastrand::shuffle(&mut self.items[1..]);
        }
        Some(start_uid)
    }

    /// Turning shuffle on shuffles everything after the current entry (history stays put);
    /// turning it off restores the original order.
    pub fn set_shuffle(&mut self, on: bool, current: Option<u32>) {
        if on == self.shuffle {
            return;
        }
        self.shuffle = on;
        if on {
            self.natural = self.items.clone();
            let from = current.and_then(|u| self.index_of(u)).map_or(0, |i| i + 1);
            fastrand::shuffle(&mut self.items[from..]);
        } else {
            self.items = std::mem::take(&mut self.natural);
        }
    }

    pub fn append(&mut self, track: u32) -> u32 {
        let uid = self.fresh_uid();
        let item = Item { uid, track };
        self.items.push(item);
        if self.shuffle {
            self.natural.push(item);
        }
        uid
    }

    /// Inserts right after `after` (or at the front when there is no current entry).
    pub fn play_next(&mut self, after: Option<u32>, track: u32) -> u32 {
        let uid = self.fresh_uid();
        let item = Item { uid, track };
        let pos = after.and_then(|u| self.index_of(u)).map_or(0, |i| i + 1);
        self.items.insert(pos, item);
        if self.shuffle {
            let npos = after
                .and_then(|u| self.natural.iter().position(|i| i.uid == u))
                .map_or(0, |i| i + 1);
            self.natural.insert(npos, item);
        }
        uid
    }

    pub fn remove(&mut self, uid: u32) {
        if let Some(i) = self.index_of(uid) {
            self.items.remove(i);
            self.removed = Some((uid, i));
        }
        if self.shuffle {
            self.natural.retain(|i| i.uid != uid);
        }
    }

    /// Drops everything except `keep` (the entry that is playing right now).
    pub fn clear_except(&mut self, keep: Option<u32>) {
        self.items.retain(|i| Some(i.uid) == keep);
        self.natural.retain(|i| Some(i.uid) == keep);
        self.removed = None;
    }

    /// The entry that follows `uid` when a track ends by itself.
    pub fn after_end(&self, uid: u32) -> Option<Item> {
        if self.repeat == Repeat::One {
            if let Some(item) = self.get(uid) {
                return Some(item);
            }
        }
        self.step(uid, 1)
    }

    /// The entry for the "next" button (repeat-one does not apply to an explicit skip).
    pub fn next(&self, uid: u32) -> Option<Item> {
        self.step(uid, 1)
    }

    pub fn prev(&self, uid: u32) -> Option<Item> {
        self.step(uid, -1)
    }

    fn step(&self, uid: u32, dir: isize) -> Option<Item> {
        let n = self.items.len() as isize;
        if n == 0 {
            return None;
        }
        let cur = match self.index_of(uid) {
            Some(i) => i as isize,
            None => match self.removed {
                // The removed entry's successor now sits at its old index.
                Some((u, i)) if u == uid => {
                    if dir > 0 {
                        i as isize - 1
                    } else {
                        i as isize
                    }
                }
                _ => return self.items.first().copied(),
            },
        };
        let mut next = cur + dir;
        if next < 0 || next >= n {
            if self.repeat == Repeat::All {
                next = next.rem_euclid(n);
            } else {
                return None;
            }
        }
        Some(self.items[next as usize])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tracks(q: &Queue) -> Vec<u32> {
        q.items().iter().map(|i| i.track).collect()
    }

    #[test]
    fn set_and_walk() {
        let mut q = Queue::new();
        let start = q.set(&[10, 11, 12], 1).unwrap();
        assert_eq!(q.get(start).unwrap().track, 11);
        assert_eq!(q.next(start).unwrap().track, 12);
        assert_eq!(q.prev(start).unwrap().track, 10);
        let last = q.next(start).unwrap().uid;
        assert!(q.next(last).is_none());
        assert!(q.after_end(last).is_none());
    }

    #[test]
    fn repeat_modes() {
        let mut q = Queue::new();
        q.set(&[1, 2, 3], 0);
        let first = q.at(0).unwrap().uid;
        let last = q.at(2).unwrap().uid;
        q.set_repeat(Repeat::All);
        assert_eq!(q.next(last).unwrap().track, 1);
        assert_eq!(q.prev(first).unwrap().track, 3);
        q.set_repeat(Repeat::One);
        assert_eq!(q.after_end(last).unwrap().uid, last, "repeat-one replays the entry");
        assert!(q.next(last).is_none(), "an explicit skip ignores repeat-one");
    }

    #[test]
    fn shuffle_keeps_current_first_and_restores_order() {
        let mut q = Queue::new();
        q.set_shuffle(true, None);
        let ids: Vec<u32> = (0..200).collect();
        let start = q.set(&ids, 50).unwrap();
        assert_eq!(q.at(0).unwrap().uid, start, "chosen track plays first");
        let mut sorted = tracks(&q);
        sorted.sort_unstable();
        assert_eq!(sorted, ids, "shuffle is a permutation");
        assert_ne!(tracks(&q), ids, "and it actually shuffles");
        q.set_shuffle(false, Some(start));
        assert_eq!(tracks(&q), ids, "un-shuffle restores the original order");
    }

    #[test]
    fn shuffle_toggle_midway_only_touches_the_future() {
        let mut q = Queue::new();
        let ids: Vec<u32> = (0..100).collect();
        q.set(&ids, 0);
        let cur = q.at(10).unwrap().uid;
        q.set_shuffle(true, Some(cur));
        assert_eq!(&tracks(&q)[..=10], &ids[..=10], "history and current untouched");
        q.set_shuffle(false, Some(cur));
        assert_eq!(tracks(&q), ids);
    }

    #[test]
    fn edits_while_shuffled_survive_unshuffle() {
        let mut q = Queue::new();
        q.set(&[1, 2, 3, 4, 5], 0);
        let cur = q.at(0).unwrap().uid;
        q.set_shuffle(true, Some(cur));
        let added = q.append(9);
        let inserted = q.play_next(Some(cur), 7);
        assert_eq!(q.at(1).unwrap().uid, inserted, "play-next goes right after the current");
        let victim = q.at(3).unwrap().uid;
        q.remove(victim);
        q.set_shuffle(false, Some(cur));
        let mut got = tracks(&q);
        assert!(got.contains(&9) && got.contains(&7));
        assert_eq!(got.len(), 6);
        got.sort_unstable();
        assert!(q.get(added).is_some());
        assert!(q.get(victim).is_none());
    }

    #[test]
    fn removing_the_playing_entry_keeps_navigation_sane() {
        let mut q = Queue::new();
        q.set(&[1, 2, 3, 4], 1);
        let playing = q.at(1).unwrap().uid;
        q.remove(playing);
        assert_eq!(q.next(playing).unwrap().track, 3, "successor takes over");
        assert_eq!(q.prev(playing).unwrap().track, 1);
        assert_eq!(q.after_end(playing).unwrap().track, 3);
    }

    #[test]
    fn unknown_uid_starts_from_the_top_and_empty_queue_is_none() {
        let mut q = Queue::new();
        assert!(q.next(5).is_none());
        q.set(&[7, 8], 0);
        assert_eq!(q.next(999).unwrap().track, 7);
        q.clear_except(None);
        assert!(q.is_empty());
    }

    #[test]
    fn uids_are_unique_and_nonzero() {
        let mut q = Queue::new();
        q.set(&[1, 1, 1], 0);
        let a = q.append(1);
        let b = q.append(1);
        let mut uids: Vec<u32> = q.items().iter().map(|i| i.uid).collect();
        uids.sort_unstable();
        uids.dedup();
        assert_eq!(uids.len(), 5);
        assert!(a != 0 && b != 0);
    }
}
