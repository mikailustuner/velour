//! Play queue: order, shuffle, repeat. Pure logic so it can be unit-tested.

use rand::seq::SliceRandom;
use serde::{Deserialize, Serialize};
use velour_ytm::Track;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Repeat {
    #[default]
    Off,
    All,
    One,
}

impl Repeat {
    pub fn cycle(self) -> Self {
        match self {
            Repeat::Off => Repeat::All,
            Repeat::All => Repeat::One,
            Repeat::One => Repeat::Off,
        }
    }
}

/// Where more tracks come from when the queue runs out (endless radio).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RadioSource {
    pub token: String,
    pub playlist_id: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PlayQueue {
    /// In play order.
    tracks: Vec<Track>,
    pos: usize,
    shuffle: bool,
    /// Order before shuffling, to restore it.
    unshuffled: Option<Vec<Track>>,
    pub repeat: Repeat,
    pub radio: Option<RadioSource>,
}

impl PlayQueue {
    pub fn set(&mut self, tracks: Vec<Track>, start: usize) {
        self.tracks = tracks;
        self.pos = start.min(self.tracks.len().saturating_sub(1));
        self.unshuffled = None;
        self.radio = None;
        if self.shuffle {
            self.shuffle_rest();
        }
        self.skip_unplayable_forward();
    }

    pub fn tracks(&self) -> &[Track] {
        &self.tracks
    }

    pub fn position(&self) -> usize {
        self.pos
    }

    pub fn current(&self) -> Option<&Track> {
        self.tracks.get(self.pos)
    }

    pub fn is_empty(&self) -> bool {
        self.tracks.is_empty()
    }

    pub fn shuffle(&self) -> bool {
        self.shuffle
    }

    /// Index of the track that plays after the current one, honoring repeat.
    pub fn next_index(&self) -> Option<usize> {
        if self.tracks.is_empty() {
            return None;
        }
        if self.repeat == Repeat::One {
            return Some(self.pos);
        }
        let n = self.tracks.len();
        (1..=n)
            .map(|k| self.pos + k)
            .filter(|&i| i < n || self.repeat == Repeat::All)
            .map(|i| i % n)
            .find(|&i| self.tracks[i].playable && (i != self.pos || self.repeat == Repeat::All))
    }

    pub fn next(&self) -> Option<&Track> {
        self.next_index().map(|i| &self.tracks[i])
    }

    /// Moves to the next track. Returns false at the end of the queue.
    pub fn advance(&mut self) -> bool {
        match self.next_index() {
            Some(i) => {
                self.pos = i;
                true
            }
            None => false,
        }
    }

    /// Moves to the previous playable track (stays put at the start).
    pub fn back(&mut self) -> bool {
        let prev = (0..self.pos).rev().find(|&i| self.tracks[i].playable);
        match prev {
            Some(i) => {
                self.pos = i;
                true
            }
            None if self.repeat == Repeat::All && !self.tracks.is_empty() => {
                self.pos = self.tracks.len() - 1;
                true
            }
            None => false,
        }
    }

    pub fn jump(&mut self, index: usize) -> bool {
        if self.tracks.get(index).is_some_and(|t| t.playable) {
            self.pos = index;
            true
        } else {
            false
        }
    }

    /// Edits every copy of a track (e.g. like status), including the unshuffled order.
    pub fn update(&mut self, video_id: &str, f: impl Fn(&mut Track)) {
        let lists = std::iter::once(&mut self.tracks).chain(self.unshuffled.as_mut());
        for list in lists {
            list.iter_mut().filter(|t| t.video_id == video_id).for_each(&f);
        }
    }

    pub fn insert_next(&mut self, track: Track) {
        if self.tracks.is_empty() {
            self.tracks.push(track);
            self.pos = 0;
        } else {
            self.tracks.insert(self.pos + 1, track);
        }
    }

    pub fn append(&mut self, tracks: impl IntoIterator<Item = Track>) {
        self.tracks.extend(tracks);
    }

    /// Removes the entry at `index`. Removing the current track keeps `pos` pointing at
    /// whatever slid into its place. Returns true if the current track was removed.
    pub fn remove(&mut self, index: usize) -> bool {
        if index >= self.tracks.len() {
            return false;
        }
        self.tracks.remove(index);
        let was_current = index == self.pos;
        if index < self.pos {
            self.pos -= 1;
        }
        if self.pos >= self.tracks.len() {
            self.pos = self.tracks.len().saturating_sub(1);
        }
        was_current
    }

    pub fn move_item(&mut self, from: usize, to: usize) {
        if from >= self.tracks.len() || to >= self.tracks.len() || from == to {
            return;
        }
        let t = self.tracks.remove(from);
        self.tracks.insert(to, t);
        self.pos = match self.pos {
            p if p == from => to,
            p if from < p && to >= p => p - 1,
            p if from > p && to <= p => p + 1,
            p => p,
        };
    }

    pub fn set_shuffle(&mut self, on: bool) {
        if on == self.shuffle {
            return;
        }
        self.shuffle = on;
        if on {
            self.unshuffled = Some(self.tracks.clone());
            self.shuffle_rest();
        } else if let Some(orig) = self.unshuffled.take() {
            let current = self.current().map(|t| t.video_id.clone());
            // Keep tracks added while shuffled.
            let mut restored: Vec<Track> =
                orig.into_iter().filter(|t| self.tracks.iter().any(|x| x.video_id == t.video_id)).collect();
            for t in &self.tracks {
                if !restored.iter().any(|x| x.video_id == t.video_id) {
                    restored.push(t.clone());
                }
            }
            self.tracks = restored;
            self.pos = current.and_then(|id| self.tracks.iter().position(|t| t.video_id == id)).unwrap_or(0);
        }
    }

    /// Tracks left after the current one (for deciding when to fetch more radio).
    pub fn remaining(&self) -> usize {
        self.tracks.len().saturating_sub(self.pos + 1)
    }

    fn shuffle_rest(&mut self) {
        if self.pos + 1 < self.tracks.len() {
            self.tracks[self.pos + 1..].shuffle(&mut rand::rng());
        }
    }

    fn skip_unplayable_forward(&mut self) {
        if self.current().is_some_and(|t| !t.playable)
            && let Some(i) = (self.pos..self.tracks.len()).find(|&i| self.tracks[i].playable)
        {
            self.pos = i;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(id: &str) -> Track {
        Track {
            video_id: id.into(),
            title: id.into(),
            artists: vec![],
            album: None,
            duration: Some(100),
            thumbnail: None,
            set_video_id: None,
            playable: true,
            like: None,
            detail: None,
        }
    }

    fn ids(q: &PlayQueue) -> Vec<&str> {
        q.tracks().iter().map(|t| t.video_id.as_str()).collect()
    }

    #[test]
    fn plays_through_and_stops() {
        let mut q = PlayQueue::default();
        q.set(vec![t("a"), t("b"), t("c")], 0);
        assert!(q.advance());
        assert!(q.advance());
        assert_eq!(q.current().unwrap().video_id, "c");
        assert!(!q.advance(), "end of queue without repeat");
        assert_eq!(q.next_index(), None);
    }

    #[test]
    fn repeat_all_wraps_and_one_stays() {
        let mut q = PlayQueue::default();
        q.set(vec![t("a"), t("b")], 1);
        q.repeat = Repeat::All;
        assert_eq!(q.next_index(), Some(0));
        q.repeat = Repeat::One;
        assert_eq!(q.next_index(), Some(1));
    }

    #[test]
    fn skips_unplayable() {
        let mut bad = t("b");
        bad.playable = false;
        let mut q = PlayQueue::default();
        q.set(vec![t("a"), bad.clone(), t("c")], 0);
        assert_eq!(q.next().unwrap().video_id, "c");
        q.set(vec![bad, t("x")], 0);
        assert_eq!(q.current().unwrap().video_id, "x", "start skips a dead track");
        assert!(!q.jump(0));
    }

    #[test]
    fn back_goes_to_previous() {
        let mut q = PlayQueue::default();
        q.set(vec![t("a"), t("b")], 1);
        assert!(q.back());
        assert_eq!(q.position(), 0);
        assert!(!q.back());
    }

    #[test]
    fn insert_next_and_remove_keep_position() {
        let mut q = PlayQueue::default();
        q.set(vec![t("a"), t("b"), t("c")], 1);
        q.insert_next(t("n"));
        assert_eq!(ids(&q), ["a", "b", "n", "c"]);
        assert_eq!(q.next().unwrap().video_id, "n");
        assert!(!q.remove(0));
        assert_eq!(q.current().unwrap().video_id, "b");
        assert!(q.remove(q.position()));
        assert_eq!(q.current().unwrap().video_id, "n");
    }

    #[test]
    fn move_item_tracks_current() {
        let mut q = PlayQueue::default();
        q.set(vec![t("a"), t("b"), t("c"), t("d")], 1); // current b
        q.move_item(3, 0); // d a b c
        assert_eq!(q.current().unwrap().video_id, "b");
        q.move_item(2, 3); // d a c b
        assert_eq!(q.current().unwrap().video_id, "b");
        assert_eq!(ids(&q), ["d", "a", "c", "b"]);
    }

    #[test]
    fn shuffle_keeps_current_and_restores_order() {
        let mut q = PlayQueue::default();
        let list: Vec<Track> = (0..30).map(|i| t(&i.to_string())).collect();
        q.set(list, 5);
        q.set_shuffle(true);
        assert_eq!(q.current().unwrap().video_id, "5");
        assert_eq!(q.tracks().len(), 30);
        q.set_shuffle(false);
        assert_eq!(ids(&q), (0..30).map(|i| i.to_string()).collect::<Vec<_>>());
        assert_eq!(q.current().unwrap().video_id, "5");
    }
}
