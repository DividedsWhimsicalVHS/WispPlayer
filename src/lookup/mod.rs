//! Looking a track up, for the track editor: which song it might be, which album carries it,
//! the album's cover and the song's lyrics.
//!
//! Nothing here runs by itself. A search starts when the user presses Search (or opens a track in
//! the editor), on a short-lived thread that reports back as it learns things: first the ranked
//! list, then lyrics and cover thumbnails as they arrive. Nothing is ever applied to a track
//! until the user picks a row and presses Save; the only write to a music file is `id3::write`.

pub mod id3;
mod lrclib;
mod musicbrainz;
mod net;

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering::*};
use std::time::Duration;

pub use lrclib::Lyrics;
pub use musicbrainz::Row;
use net::{Net, NetError};

use crate::library::filename::hypotheses;

/// How many rows get a thumbnail and are looked up for lyrics (the rest still can be picked).
const DECORATED_ROWS: usize = 8;
/// Different songs looked up for lyrics at most (a request each).
const LYRICS_LOOKUPS: usize = 3;
/// Times a busy MusicBrainz is asked again before one query is given up on. At busy times it
/// refuses about half of the requests that carry no contact, each time asking to come back in a
/// second.
const BUSY_RETRIES: u32 = 6;
const BUSY_WAIT: Duration = Duration::from_secs(2);

/// What a lookup reports to the UI as it learns things. Every event comes with the number of the
/// search it belongs to, so a slow answer to an old search can be ignored.
pub enum Event {
    /// The ranked list.
    Results(Vec<Row>),
    /// Lyrics found for a row.
    Lyrics { row: usize, lyrics: Lyrics },
    /// A small cover for a row (JPEG or PNG bytes).
    Thumb { row: usize, bytes: Vec<u8> },
    /// The full-size cover of the row the user picked.
    Cover { row: usize, bytes: Vec<u8> },
    /// No full-size cover could be had for the picked row.
    NoCover { row: usize, why: String },
    /// MusicBrainz is busy; the search waits a moment and asks again.
    Busy,
    /// The search has nothing more to report.
    Done,
    /// Something went wrong that the user should know about.
    Problem(String),
}

pub struct Lookup {
    current: Arc<AtomicU32>,
}

impl Lookup {
    pub fn new() -> Lookup {
        Lookup { current: Arc::new(AtomicU32::new(0)) }
    }

    /// Stops reporting anything from the searches so far.
    pub fn cancel(&self) {
        self.current.fetch_add(1, AcqRel);
    }

    /// Searches for what `query` says (`Artist - Title`, `Title - Artist` or a title), among
    /// recordings about `ms` long when that is known. Replaces any search still running. Returns
    /// the number its events will carry.
    pub fn search(&self, query: &str, ms: u32, contact: &str, deliver: impl Fn(u32, Event) + Send + 'static) -> u32 {
        let number = self.current.fetch_add(1, AcqRel) + 1;
        let (current, query, contact) = (self.current.clone(), query.to_string(), contact.to_string());
        let spawned = std::thread::Builder::new().name("wisp-lookup".into()).spawn(move || {
            let live = || current.load(Acquire) == number;
            run(&query, ms, &contact, &live, &|event| {
                if live() {
                    deliver(number, event);
                }
            });
        });
        if spawned.is_err() {
            self.cancel();
        }
        number
    }

    /// Fetches the full-size cover of a picked row.
    pub fn fetch_cover(&self, row: usize, group_id: String, release_id: String, contact: &str, deliver: impl Fn(u32, Event) + Send + 'static) {
        let (current, contact) = (self.current.clone(), contact.to_string());
        let number = current.load(Acquire);
        let _ = std::thread::Builder::new().name("wisp-cover".into()).spawn(move || {
            let event = match Net::new(&contact).cover(&group_id, &release_id, 500) {
                Ok(Some(bytes)) => Event::Cover { row, bytes },
                Ok(None) => Event::NoCover { row, why: "The Cover Art Archive has no cover for this release.".into() },
                Err(NetError::Busy) => Event::NoCover { row, why: "The Cover Art Archive is busy; pick the song again in a moment.".into() },
                Err(NetError::Offline(why)) => Event::NoCover { row, why: format!("Can't reach the Cover Art Archive ({why}).") },
            };
            if current.load(Acquire) == number {
                deliver(number, event);
            }
        });
    }
}

fn describe(e: &NetError) -> String {
    match e {
        NetError::Busy => "MusicBrainz is busy or asks for a contact: add your email or website in Settings, then search again.".into(),
        NetError::Offline(why) => format!("Can't reach the lookup service ({why})."),
    }
}

/// Sleeps for `total`, in short steps so that a replaced search stops at once.
fn nap(total: Duration, live: &dyn Fn() -> bool) {
    let step = Duration::from_millis(200);
    let mut slept = Duration::ZERO;
    while slept < total && live() {
        std::thread::sleep(step);
        slept += step;
    }
}

/// One MusicBrainz search, asking again a few times when the service is busy.
fn ask(net: &mut Net, query: &str, live: &dyn Fn() -> bool, report: &dyn Fn(Event)) -> Result<Vec<musicbrainz::Candidate>, NetError> {
    for attempt in 0..=BUSY_RETRIES {
        if !live() {
            return Ok(Vec::new());
        }
        match net.search(query) {
            Ok(v) => return Ok(musicbrainz::parse_recordings(&v)),
            Err(NetError::Busy) if attempt < BUSY_RETRIES => {
                report(Event::Busy);
                nap(BUSY_WAIT, live);
            }
            Err(e) => return Err(e),
        }
    }
    Ok(Vec::new())
}

/// The first of several credited artists: "Kanye West feat. Dwele" is Kanye West's.
fn first_artist(credit: &str) -> &str {
    [" feat. ", " ft. ", " featuring ", ", ", " with "]
        .iter()
        .filter_map(|sep| credit.find(sep))
        .min()
        .map_or(credit, |at| &credit[..at])
}

fn run(query: &str, ms: u32, contact: &str, live: &dyn Fn() -> bool, report: &dyn Fn(Event)) {
    let readings = hypotheses(query);
    let plan = musicbrainz::plan(&readings, ms);
    let mut net = Net::new(contact);
    let mut candidates = Vec::new();
    let queries = [Some(plan.official.clone()), plan.by_length.clone()];
    let mut failure = None;
    for q in queries.into_iter().flatten() {
        match ask(&mut net, &q, live, report) {
            Ok(found) => candidates.extend(found),
            Err(e) => failure = Some(e),
        }
    }
    // One of the two searches failing is no reason to throw away what the other found.
    if let (true, Some(e)) = (candidates.is_empty(), &failure) {
        return report(Event::Problem(describe(e)));
    }
    let mut rows = musicbrainz::rank(&readings, &candidates, ms);
    if rows.len() < 3 && live() {
        // Little among official studio releases: the song may only exist live or unofficially.
        match ask(&mut net, &plan.anything, live, report) {
            Ok(found) => candidates.extend(found),
            Err(e) if rows.is_empty() => return report(Event::Problem(describe(&e))),
            Err(_) => {}
        }
        rows = musicbrainz::rank(&readings, &candidates, ms);
    }
    if !live() {
        return;
    }
    report(Event::Results(rows.clone()));
    let shown = &rows[..rows.len().min(DECORATED_ROWS)];

    // Lyrics: one request per distinct song, then each row takes the record that is its version.
    let mut songs: Vec<(String, String)> = Vec::new();
    for r in shown {
        let song = (r.title.clone(), first_artist(&r.artist).to_string());
        if !songs.contains(&song) && songs.len() < LYRICS_LOOKUPS {
            songs.push(song);
        }
    }
    for (title, artist) in songs {
        if !live() {
            return;
        }
        let Ok(v) = net.lyrics(&title, &artist) else { continue };
        let entries = lrclib::parse(&v);
        for (i, r) in shown.iter().enumerate().filter(|(_, r)| r.title == title && first_artist(&r.artist) == artist) {
            if let Some(lyrics) = lrclib::best(&entries, &r.title, &r.artist, &r.album, r.length_ms) {
                report(Event::Lyrics { row: i, lyrics });
            }
        }
    }

    // Thumbnails: one request per album, shared by every row that has it.
    let mut albums: Vec<(String, String)> = Vec::new();
    for r in shown {
        let key = (r.group_id.clone(), r.release_id.clone());
        if !albums.contains(&key) && !(key.0.is_empty() && key.1.is_empty()) {
            albums.push(key);
        }
    }
    for (group, release) in albums {
        if !live() {
            return;
        }
        let Ok(Some(bytes)) = net.cover(&group, &release, 250) else { continue };
        for (i, _) in shown.iter().enumerate().filter(|(_, r)| r.group_id == group && r.release_id == release) {
            report(Event::Thumb { row: i, bytes: bytes.clone() });
        }
    }
    report(Event::Done);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_of_several_credited_artists_is_the_one_to_look_up() {
        assert_eq!(first_artist("Kanye West feat. Dwele"), "Kanye West");
        assert_eq!(first_artist("Daft Punk"), "Daft Punk");
        assert_eq!(first_artist("Simon & Garfunkel"), "Simon & Garfunkel");
        assert_eq!(first_artist("A, B and C"), "A");
        assert_eq!(first_artist("Mark Ronson ft. Bruno Mars"), "Mark Ronson");
    }

    #[test]
    fn an_old_search_stops_reporting_when_a_new_one_starts() {
        let lookup = Lookup::new();
        let before = lookup.current.load(Acquire);
        lookup.cancel();
        assert_eq!(lookup.current.load(Acquire), before + 1);
    }
}
