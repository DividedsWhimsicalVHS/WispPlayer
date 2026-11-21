//! The three web services behind the track editor: MusicBrainz (what the song is), the Cover Art
//! Archive (its album's cover) and LRCLIB (its lyrics).
//!
//! MusicBrainz allows one request per second and asks every application to identify itself with
//! a way to contact its operator; anonymous clients are refused at busy times. The contact
//! therefore comes from the user's settings and nothing else is ever sent. Every request here is
//! made because the user searched for or picked something: nothing runs in the background.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::Value;
use ureq::tls::{RootCerts, TlsConfig, TlsProvider};

/// Spacing between MusicBrainz requests (their limit is one per second), shared by every thread.
const MB_SPACING: Duration = Duration::from_millis(1100);
static MB_LAST: Mutex<Option<Instant>> = Mutex::new(None);

#[derive(Debug, PartialEq)]
pub enum NetError {
    /// The service asked us to slow down or refused an anonymous client (HTTP 503/429/403).
    Busy,
    /// No connection, a timeout, or a response that could not be read.
    Offline(String),
}

pub struct Net {
    agent: ureq::Agent,
}

/// `Wisp/0.1.0 ( contact )`, the form MusicBrainz and LRCLIB ask for.
pub fn user_agent(contact: &str) -> String {
    let version = env!("CARGO_PKG_VERSION");
    let contact: String = contact.chars().filter(|c| !c.is_control() && *c != '(' && *c != ')').collect();
    match contact.trim() {
        "" => format!("Wisp/{version}"),
        c => format!("Wisp/{version} ( {c} )"),
    }
}

/// MusicBrainz ids are UUIDs; anything else must never reach a URL.
pub fn is_mbid(id: &str) -> bool {
    id.len() == 36 && id.chars().all(|c| c.is_ascii_hexdigit() || c == '-')
}

fn offline(e: ureq::Error) -> NetError {
    NetError::Offline(e.to_string())
}

/// Waits for this thread's turn to ask MusicBrainz something.
fn pace_musicbrainz() {
    let wait = {
        let mut last = MB_LAST.lock().unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();
        let turn = last.map_or(now, |l| (l + MB_SPACING).max(now));
        *last = Some(turn);
        turn.saturating_duration_since(now)
    };
    std::thread::sleep(wait);
}

impl Net {
    pub fn new(contact: &str) -> Net {
        let tls = TlsConfig::builder().provider(TlsProvider::NativeTls).root_certs(RootCerts::PlatformVerifier).build();
        let agent = ureq::Agent::config_builder()
            .tls_config(tls)
            .user_agent(user_agent(contact))
            .timeout_global(Some(Duration::from_secs(20)))
            .http_status_as_error(false)
            .build()
            .new_agent();
        Net { agent }
    }

    fn json(&self, mut resp: ureq::http::Response<ureq::Body>) -> Result<Value, NetError> {
        match resp.status().as_u16() {
            200 => {
                let text = resp.body_mut().with_config().limit(4 << 20).read_to_string().map_err(offline)?;
                serde_json::from_str(&text).map_err(|e| NetError::Offline(format!("unreadable answer: {e}")))
            }
            403 | 429 | 503 => Err(NetError::Busy),
            s => Err(NetError::Offline(format!("the service answered HTTP {s}"))),
        }
    }

    /// One `/ws/2/recording` search (at most 25 recordings).
    pub fn search(&mut self, query: &str) -> Result<Value, NetError> {
        pace_musicbrainz();
        let resp = self
            .agent
            .get("https://musicbrainz.org/ws/2/recording")
            .query("query", query)
            .query("fmt", "json")
            .query("limit", "25")
            .header("Accept", "application/json")
            .call()
            .map_err(offline)?;
        self.json(resp)
    }

    /// The front cover of an album (its release group), or of the specific release when the album
    /// has none, `size` pixels wide (the archive has 250, 500 and 1200). `None` when there is none.
    pub fn cover(&mut self, group_id: &str, release_id: &str, size: u32) -> Result<Option<Vec<u8>>, NetError> {
        let urls = [("release-group", group_id), ("release", release_id)]
            .into_iter()
            .filter(|(_, id)| is_mbid(id))
            .map(|(kind, id)| format!("https://coverartarchive.org/{kind}/{id}/front-{size}"));
        for url in urls {
            let mut resp = self.agent.get(&url).call().map_err(offline)?;
            match resp.status().as_u16() {
                200 => return resp.body_mut().with_config().limit(8 << 20).read_to_vec().map(Some).map_err(offline),
                400 | 404 => continue,
                403 | 429 | 503 => return Err(NetError::Busy),
                s => return Err(NetError::Offline(format!("Cover Art Archive answered HTTP {s}"))),
            }
        }
        Ok(None)
    }

    /// LRCLIB's records for a song (up to 20).
    pub fn lyrics(&mut self, title: &str, artist: &str) -> Result<Value, NetError> {
        let resp = self
            .agent
            .get("https://lrclib.net/api/search")
            .query("track_name", title)
            .query("artist_name", artist)
            .header("Accept", "application/json")
            .call()
            .map_err(offline)?;
        self.json(resp)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_user_agent_carries_only_the_configured_contact() {
        let v = env!("CARGO_PKG_VERSION");
        assert_eq!(user_agent(""), format!("Wisp/{v}"));
        assert_eq!(user_agent("  me@example.org "), format!("Wisp/{v} ( me@example.org )"));
        assert_eq!(user_agent("a)\r\nX-Evil: 1"), format!("Wisp/{v} ( aX-Evil: 1 )"), "no header injection");
    }

    #[test]
    fn only_real_ids_become_urls() {
        assert!(is_mbid("48117b82-8a77-3a5c-8c7c-34d4af8f3f8b"));
        assert!(!is_mbid("../../etc/passwd"));
        assert!(!is_mbid(""));
    }

    /// `cargo test -- --ignored`: downloads a real album cover through the same TLS stack and
    /// redirects (coverartarchive.org -> archive.org) the app uses, at both sizes the editor asks for.
    #[test]
    #[ignore = "needs the internet"]
    fn the_cover_art_archive_serves_album_covers() {
        let mut net = Net::new("");
        let homework = "00054665-89fa-33d5-a8f0-1728ea8c32c3"; // Daft Punk, "Homework"
        for size in [250, 500] {
            let bytes = net.cover(homework, "", size).expect("reachable").expect("the album has a cover");
            let (w, h) = crate::art::image_size(&bytes).expect("a JPEG or PNG");
            assert!(w >= 200 && (w as f32 / h as f32 - 1.0).abs() < 0.2, "{w}x{h}");
        }
        assert_eq!(net.cover("00000000-0000-0000-0000-000000000000", "", 250).unwrap(), None, "no cover is not an error");
    }

    /// `cargo test live_search -- --ignored --nocapture`: the editor's whole search against the
    /// real services, for the song that was once filed under the wrong artist.
    #[test]
    #[ignore = "needs the internet"]
    fn live_search() {
        use super::super::{lrclib, musicbrainz as mb};
        use crate::library::filename::hypotheses;
        let mut net = Net::new("");
        let mut ask = |q: &str| loop {
            match net.search(q) {
                Ok(v) => break mb::parse_recordings(&v),
                Err(NetError::Busy) => std::thread::sleep(Duration::from_secs(4)),
                Err(e) => panic!("{e:?}"),
            }
        };
        let readings = hypotheses("Flashing Lights");
        let plan = mb::plan(&readings, 237_490);
        let mut all = ask(&plan.official);
        all.extend(ask(&plan.by_length.unwrap()));
        let rows = mb::rank(&readings, &all, 237_490);
        for r in rows.iter().take(6) {
            println!("{} | {} | {} | {} | #{} | length match: {}", r.title, r.artist, r.album, r.year, r.track_no, r.length_match);
        }
        assert!(rows.iter().any(|r| r.album == "Graduation"), "the studio album is on the list");
        let lyrics = lrclib::parse(&net.lyrics("Flashing Lights", "Kanye West").expect("reachable"));
        let found = lrclib::best(&lyrics, "Flashing Lights", "Kanye West feat. Dwele", "Graduation", Some(237_440));
        println!("lyrics: {:?}", found.map(|l| (l.synced.len(), l.plain.len())));
    }
}
