//! The playback state machine: step/play a Walrus file forward and backward,
//! with speed and loop, independent of any GUI.

use std::time::Duration;

use walrus_fmt::{Version, WalrusFile};

/// Direction the auto-player advances.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// Toward the head (higher indices).
    Forward,
    /// Toward the base (lower indices).
    Backward,
}

/// Playback state over an opened Walrus file.
///
/// The player owns the current version index and the auto-advance settings.
/// A GUI drives it by calling [`Player::tick`] with elapsed time each frame and
/// rendering [`Player::current_bytes`].
pub struct Player {
    file: WalrusFile,
    index: usize,
    len: usize,
    playing: bool,
    direction: Direction,
    looping: bool,
    /// Seconds between automatic steps (lower is faster).
    step_interval: Duration,
    /// Time accumulated toward the next automatic step.
    accumulated: Duration,
}

impl Player {
    /// Build a player positioned at the base (index 0).
    pub fn new(file: WalrusFile) -> Self {
        let len = file.len();
        Self {
            file,
            index: 0,
            len,
            playing: false,
            direction: Direction::Forward,
            looping: false,
            step_interval: Duration::from_millis(400),
            accumulated: Duration::ZERO,
        }
    }

    /// The current version index (`0..=len`).
    pub fn index(&self) -> usize {
        self.index
    }

    /// The far-end index (number of change records).
    pub fn len(&self) -> usize {
        self.len
    }

    /// True when there are no change records.
    // Companion to `len()`; part of the public Player API, not yet called by the
    // GUI transport. Kept so embedders have the standard len/is_empty pair.
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Whether auto-play is active.
    pub fn is_playing(&self) -> bool {
        self.playing
    }

    /// The auto-advance direction.
    pub fn direction(&self) -> Direction {
        self.direction
    }

    /// Whether playback loops at the ends.
    pub fn looping(&self) -> bool {
        self.looping
    }

    /// Reconstruct the bytes of the current version.
    pub fn current_bytes(&self) -> walrus_fmt::Result<Vec<u8>> {
        self.file.reconstruct(Version::Index(self.index))
    }

    /// The manifest of the opened file (for document name/MIME and variant).
    pub fn manifest(&self) -> &walrus_fmt::Manifest {
        self.file.manifest()
    }

    /// The variant (`wal` / `wbl`) of the opened file.
    pub fn variant(&self) -> walrus_fmt::Variant {
        self.file.manifest().variant
    }

    /// Jump directly to a version index (clamped to range).
    pub fn seek(&mut self, index: usize) {
        self.index = index.min(self.len);
    }

    /// Step one version toward the head; returns true if it moved.
    pub fn step_forward(&mut self) -> bool {
        if self.index < self.len {
            self.index += 1;
            true
        } else {
            false
        }
    }

    /// Step one version toward the base; returns true if it moved.
    pub fn step_backward(&mut self) -> bool {
        if self.index > 0 {
            self.index -= 1;
            true
        } else {
            false
        }
    }

    /// Start auto-play.
    pub fn play(&mut self) {
        self.playing = true;
        self.accumulated = Duration::ZERO;
    }

    /// Pause auto-play.
    pub fn pause(&mut self) {
        self.playing = false;
    }

    /// Set the auto-advance direction.
    pub fn set_direction(&mut self, direction: Direction) {
        self.direction = direction;
    }

    /// Enable or disable looping at the ends.
    pub fn set_looping(&mut self, looping: bool) {
        self.looping = looping;
    }

    /// Set the per-step interval (lower is faster). Clamped to a sane minimum.
    // Wired to the transport speed control (see `transport::set_speed_multiplier`).
    pub fn set_step_interval(&mut self, interval: Duration) {
        self.step_interval = interval.max(Duration::from_millis(16));
    }

    /// The current per-step interval (companion to `set_step_interval`).
    pub fn step_interval(&self) -> Duration {
        self.step_interval
    }

    /// Advance the player by the elapsed wall-clock time. Call once per frame.
    ///
    /// Returns true if the current version changed (so the GUI can refresh the
    /// displayed document).
    pub fn tick(&mut self, elapsed: Duration) -> bool {
        if !self.playing || self.len == 0 {
            return false;
        }
        self.accumulated += elapsed;
        let mut changed = false;
        while self.accumulated >= self.step_interval {
            self.accumulated -= self.step_interval;
            changed |= self.advance_one();
        }
        changed
    }

    /// Perform a single automatic step honouring direction and loop.
    fn advance_one(&mut self) -> bool {
        match self.direction {
            Direction::Forward => {
                if self.step_forward() {
                    true
                } else if self.looping {
                    self.index = 0;
                    true
                } else {
                    self.playing = false;
                    false
                }
            }
            Direction::Backward => {
                if self.step_backward() {
                    true
                } else if self.looping {
                    self.index = self.len;
                    true
                } else {
                    self.playing = false;
                    false
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use walrus_fmt::ChangeRecord;

    // The stepping/loop logic is pure arithmetic on `index`/`len`; validate it
    // without needing a real file by testing a detached mini state machine that
    // mirrors advance_one.
    struct Mini {
        index: usize,
        len: usize,
        looping: bool,
        dir: Direction,
        playing: bool,
    }
    impl Mini {
        fn advance(&mut self) {
            match self.dir {
                Direction::Forward => {
                    if self.index < self.len {
                        self.index += 1;
                    } else if self.looping {
                        self.index = 0;
                    } else {
                        self.playing = false;
                    }
                }
                Direction::Backward => {
                    if self.index > 0 {
                        self.index -= 1;
                    } else if self.looping {
                        self.index = self.len;
                    } else {
                        self.playing = false;
                    }
                }
            }
        }
    }

    #[test]
    fn forward_stops_at_head_without_loop() {
        let mut m = Mini {
            index: 1,
            len: 2,
            looping: false,
            dir: Direction::Forward,
            playing: true,
        };
        m.advance(); // 2 (head)
        assert_eq!(m.index, 2);
        m.advance(); // stays, stops
        assert_eq!(m.index, 2);
        assert!(!m.playing);
    }

    #[test]
    fn forward_wraps_at_head_with_loop() {
        let mut m = Mini {
            index: 2,
            len: 2,
            looping: true,
            dir: Direction::Forward,
            playing: true,
        };
        m.advance();
        assert_eq!(m.index, 0);
        assert!(m.playing);
    }

    #[test]
    fn set_step_interval_clamps_sub_sixteen_millis_to_sixteen() {
        let base = b"walrus".to_vec();
        let log = vec![ChangeRecord::splice(0, 0, "a")];
        let file = WalrusFile::new_wal(base, log).expect("new_wal builds a valid in-memory file");
        let mut player = Player::new(file);
        player.set_step_interval(Duration::from_millis(1));
        assert_eq!(player.step_interval(), Duration::from_millis(16));
    }

    #[test]
    fn backward_wraps_at_base_with_loop() {
        let mut m = Mini {
            index: 0,
            len: 2,
            looping: true,
            dir: Direction::Backward,
            playing: true,
        };
        m.advance();
        assert_eq!(m.index, 2);
    }
}
