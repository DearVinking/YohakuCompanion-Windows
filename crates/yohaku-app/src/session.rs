use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::time::Instant;
use yohaku_protocol::presence::MediaKind;

const TRACKER_CAPACITY: usize = 32;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaIdentity {
    pub kind: MediaKind,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub player: Option<String>,
    pub duration_ms: Option<i64>,
}

impl MediaIdentity {
    fn hash(&self) -> [u8; 32] {
        #[derive(serde::Serialize)]
        struct Wire<'a> {
            album: &'a Option<String>,
            artist: &'a Option<String>,
            duration_ms: Option<i64>,
            kind: &'static str,
            player: &'a Option<String>,
            title: &'a Option<String>,
        }
        let wire = Wire {
            kind: match self.kind {
                MediaKind::Music => "music",
                MediaKind::Podcast => "podcast",
                MediaKind::Video => "video",
                MediaKind::Unknown => "unknown",
            },
            title: &self.title,
            artist: &self.artist,
            album: &self.album,
            player: &self.player,
            duration_ms: self.duration_ms,
        };
        let json = serde_json::to_vec(&wire).expect("serializable");
        Sha256::digest(&json).into()
    }
}

pub struct MediaSessionTracker {
    sessions: HashMap<[u8; 32], (uuid::Uuid, Instant)>,
    capacity: usize,
}

impl Default for MediaSessionTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl MediaSessionTracker {
    pub fn new() -> Self {
        MediaSessionTracker {
            sessions: HashMap::new(),
            capacity: TRACKER_CAPACITY,
        }
    }

    pub fn session_id(&mut self, identity: &MediaIdentity) -> String {
        let hash = identity.hash();
        let now = Instant::now();
        match self.sessions.get_mut(&hash) {
            Some((uuid, last_used)) => {
                *last_used = now;
                uuid.to_string()
            }
            None => {
                if self.sessions.len() >= self.capacity
                    && let Some(oldest) = self
                        .sessions
                        .iter()
                        .min_by_key(|(_, (_, last_used))| *last_used)
                        .map(|(k, _)| *k)
                {
                    self.sessions.remove(&oldest);
                }
                let uuid = uuid::Uuid::new_v4();
                self.sessions.insert(hash, (uuid, now));
                uuid.to_string()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity(title: &str) -> MediaIdentity {
        MediaIdentity {
            kind: MediaKind::Music,
            title: Some(title.into()),
            artist: Some("a".into()),
            album: None,
            player: Some("p".into()),
            duration_ms: Some(100),
        }
    }

    #[test]
    fn stable_and_distinct() {
        let mut tracker = MediaSessionTracker::new();
        let a1 = tracker.session_id(&identity("song"));
        let a2 = tracker.session_id(&identity("song"));
        assert_eq!(a1, a2);
        let b = tracker.session_id(&identity("other"));
        assert_ne!(a1, b);
        let mut live = identity("song");
        live.duration_ms = Some(101);
        assert_ne!(a1, tracker.session_id(&live));
    }

    #[test]
    fn evicts_oldest_beyond_capacity() {
        let mut tracker = MediaSessionTracker::new();
        tracker.capacity = 2;
        let one = identity("one");
        let two = identity("two");
        let first = tracker.session_id(&one);
        let second = tracker.session_id(&two);
        let now = Instant::now();
        tracker.sessions.get_mut(&one.hash()).unwrap().1 = now - std::time::Duration::from_secs(2);
        tracker.sessions.get_mut(&two.hash()).unwrap().1 = now - std::time::Duration::from_secs(1);
        assert_eq!(first, tracker.session_id(&one));
        tracker.session_id(&identity("three"));
        assert!(tracker.sessions.contains_key(&one.hash()));
        assert!(!tracker.sessions.contains_key(&two.hash()));
        assert_ne!(second, tracker.session_id(&two));
        assert_eq!(tracker.sessions.len(), 2);
    }

    #[test]
    fn identity_hash_matches_canonical_wire_bytes() {
        let expected = br#"{"album":null,"artist":"a","duration_ms":100,"kind":"music","player":"p","title":"song"}"#;
        let expected_hash: [u8; 32] = Sha256::digest(expected).into();
        assert_eq!(identity("song").hash(), expected_hash);
    }
}
