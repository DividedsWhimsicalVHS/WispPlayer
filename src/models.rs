//! Slint models that read straight from the library.
//!
//! A list of 100 000 songs is a `Vec<u32>` of ids; a row's text is fetched (and cached briefly)
//! only when the list actually asks for it, which happens for the dozen rows on screen. That is
//! what keeps a huge library from costing megabytes of UI strings.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use slint::{Image, Model, ModelNotify, ModelRc, ModelTracker, SharedString};

use crate::library::{Album, Library, Track};
use crate::player::queue::Queue;
use crate::{AlbumData, QueueRow, TrackRow};

/// 3:07 or 1:02:09.
pub fn fmt_time(ms: u64) -> String {
    let s = ms / 1000;
    if s >= 3600 { format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60) } else { format!("{}:{:02}", s / 60, s % 60) }
}

/// 12431 -> "12,431".
pub fn fmt_count(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

const ROW_CACHE: usize = 256;

fn track_row(t: &Track, numbered: bool) -> TrackRow {
    TrackRow {
        id: t.id as i32,
        title: t.title.as_str().into(),
        artist: t.artist.as_str().into(),
        album: t.album.as_str().into(),
        duration: if t.duration_ms > 0 { fmt_time(t.duration_ms as u64).into() } else { SharedString::from("--:--") },
        number: if numbered && t.track_no > 0 { t.track_no.to_string().into() } else { SharedString::new() },
        flags: t.flags as i32,
    }
}

/// Songs (all, filtered, or one album's).
pub struct TrackList {
    lib: Arc<Library>,
    ids: RefCell<Rc<Vec<u32>>>,
    numbered: Cell<bool>,
    cache: RefCell<HashMap<u32, TrackRow>>,
    notify: ModelNotify,
}

impl TrackList {
    pub fn new(lib: Arc<Library>) -> Rc<TrackList> {
        Rc::new(TrackList {
            lib,
            ids: RefCell::new(Rc::new(Vec::new())),
            numbered: Cell::new(false),
            cache: RefCell::new(HashMap::new()),
            notify: ModelNotify::default(),
        })
    }

    pub fn set(&self, ids: Rc<Vec<u32>>, numbered: bool) {
        *self.ids.borrow_mut() = ids;
        self.numbered.set(numbered);
        self.cache.borrow_mut().clear();
        self.notify.reset();
    }

    pub fn ids(&self) -> Rc<Vec<u32>> {
        self.ids.borrow().clone()
    }

    pub fn id_at(&self, row: usize) -> Option<u32> {
        self.ids.borrow().get(row).copied()
    }

    /// Drops cached row text (after the library changed underneath).
    pub fn invalidate(&self) {
        self.cache.borrow_mut().clear();
        self.notify.reset();
    }

    pub fn model(self: &Rc<Self>) -> ModelRc<TrackRow> {
        ModelRc::from(self.clone())
    }
}

impl Model for TrackList {
    type Data = TrackRow;

    fn row_count(&self) -> usize {
        self.ids.borrow().len()
    }

    fn row_data(&self, row: usize) -> Option<TrackRow> {
        let id = *self.ids.borrow().get(row)?;
        if let Some(hit) = self.cache.borrow().get(&id) {
            return Some(hit.clone());
        }
        let t = self.lib.track(id)?;
        let data = track_row(&t, self.numbered.get());
        let mut cache = self.cache.borrow_mut();
        if cache.len() >= ROW_CACHE {
            cache.clear();
        }
        cache.insert(id, data.clone());
        Some(data)
    }

    fn model_tracker(&self) -> &dyn ModelTracker {
        &self.notify
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// The play queue as rows. `start` skips the entries before it (the "Up Next" view begins at the
/// track that is playing).
pub struct QueueList {
    lib: Arc<Library>,
    queue: Arc<Mutex<Queue>>,
    start: Cell<usize>,
    notify: ModelNotify,
}

impl QueueList {
    pub fn new(lib: Arc<Library>, queue: Arc<Mutex<Queue>>) -> Rc<QueueList> {
        Rc::new(QueueList { lib, queue, start: Cell::new(0), notify: ModelNotify::default() })
    }

    fn q(&self) -> std::sync::MutexGuard<'_, Queue> {
        self.queue.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn set_start(&self, start: usize) {
        if self.start.replace(start) != start {
            self.notify.reset();
        }
    }

    pub fn changed(&self) {
        self.notify.reset();
    }

    pub fn model(self: &Rc<Self>) -> ModelRc<QueueRow> {
        ModelRc::from(self.clone())
    }
}

impl Model for QueueList {
    type Data = QueueRow;

    fn row_count(&self) -> usize {
        self.q().len().saturating_sub(self.start.get())
    }

    fn row_data(&self, row: usize) -> Option<QueueRow> {
        let item = self.q().at(self.start.get() + row)?;
        let t = self.lib.track(item.track)?;
        Some(QueueRow {
            uid: item.uid as i32,
            track: item.track as i32,
            title: t.title.as_str().into(),
            artist: t.artist.as_str().into(),
            duration: if t.duration_ms > 0 { fmt_time(t.duration_ms as u64).into() } else { SharedString::from("--:--") },
        })
    }

    fn model_tracker(&self) -> &dyn ModelTracker {
        &self.notify
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Albums for the grid, with lazily loaded, bounded thumbnails.
pub struct AlbumList {
    lib: Arc<Library>,
    /// Album indexes shown, in order.
    ids: RefCell<Rc<Vec<u32>>>,
    albums: RefCell<HashMap<u32, Album>>,
    art: RefCell<HashMap<u32, Image>>,
    /// Thumbnails already requested (loaded or failed), so a row is asked for once.
    asked: RefCell<std::collections::HashSet<u32>>,
    /// Called with `(album index, representative track)` when a row first needs its cover.
    want_art: RefCell<Option<Box<dyn Fn(u32, u32)>>>,
    notify: ModelNotify,
}

const ART_CACHE: usize = 48;

impl AlbumList {
    pub fn new(lib: Arc<Library>) -> Rc<AlbumList> {
        Rc::new(AlbumList {
            lib,
            ids: RefCell::new(Rc::new(Vec::new())),
            albums: RefCell::new(HashMap::new()),
            art: RefCell::new(HashMap::new()),
            asked: RefCell::new(Default::default()),
            want_art: RefCell::new(None),
            notify: ModelNotify::default(),
        })
    }

    pub fn on_want_art(&self, f: impl Fn(u32, u32) + 'static) {
        *self.want_art.borrow_mut() = Some(Box::new(f));
    }

    pub fn set(&self, ids: Rc<Vec<u32>>) {
        *self.ids.borrow_mut() = ids;
        self.albums.borrow_mut().clear();
        self.notify.reset();
    }

    /// A thumbnail arrived (or failed: `None`).
    pub fn art_loaded(&self, index: u32, image: Option<Image>) {
        if let Some(image) = image {
            let mut art = self.art.borrow_mut();
            if art.len() >= ART_CACHE {
                art.clear();
                self.asked.borrow_mut().clear();
                self.asked.borrow_mut().insert(index);
            }
            art.insert(index, image);
        }
        let row = self.ids.borrow().iter().position(|&i| i == index);
        if let Some(row) = row {
            self.notify.row_changed(row);
        }
    }

    /// Drops thumbnails and album text (leaving the Albums page).
    pub fn release(&self) {
        self.art.borrow_mut().clear();
        self.asked.borrow_mut().clear();
        self.albums.borrow_mut().clear();
    }

    pub fn model(self: &Rc<Self>) -> ModelRc<AlbumData> {
        ModelRc::from(self.clone())
    }
}

impl Model for AlbumList {
    type Data = AlbumData;

    fn row_count(&self) -> usize {
        self.ids.borrow().len()
    }

    fn row_data(&self, row: usize) -> Option<AlbumData> {
        let index = *self.ids.borrow().get(row)?;
        let album = {
            let cached = self.albums.borrow().get(&index).cloned();
            match cached {
                Some(a) => a,
                None => {
                    let a = self.lib.album(index)?;
                    let mut albums = self.albums.borrow_mut();
                    if albums.len() >= 512 {
                        albums.clear();
                    }
                    albums.insert(index, a.clone());
                    a
                }
            }
        };
        let art = self.art.borrow().get(&index).cloned();
        if art.is_none() && self.asked.borrow_mut().insert(index) {
            if let Some(f) = self.want_art.borrow().as_ref() {
                f(index, album.rep_track);
            }
        }
        Some(AlbumData {
            index: index as i32,
            title: album.name.as_str().into(),
            artist: album.artist.as_str().into(),
            year: if album.year > 0 { album.year.to_string().into() } else { SharedString::new() },
            has_art: art.is_some(),
            art: art.unwrap_or_default(),
        })
    }

    fn model_tracker(&self) -> &dyn ModelTracker {
        &self.notify
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times_and_counts_format_for_display() {
        assert_eq!(fmt_time(0), "0:00");
        assert_eq!(fmt_time(187_000), "3:07");
        assert_eq!(fmt_time(3_729_000), "1:02:09");
        assert_eq!(fmt_count(7), "7");
        assert_eq!(fmt_count(1000), "1,000");
        assert_eq!(fmt_count(12_431), "12,431");
        assert_eq!(fmt_count(1_234_567), "1,234,567");
    }
}
