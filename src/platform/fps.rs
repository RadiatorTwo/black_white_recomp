//! Frame pacing. Black & White's own pacing is not known yet: flips wait for the display's
//! refresh as in the original (rustschweine's pictures between steps are Hogs of War's).

/// The cap in pictures per second: None unlimited, Some(0) the original.
pub fn cap() -> Option<u32> {
    Some(0)
}

/// Whether pictures are drawn between the game's steps (never yet).
pub fn active() -> bool {
    false
}

/// Frame statistics (none yet).
pub fn stats() {}

/// The camera, for marks in input recordings (not known yet).
pub fn camera_text() -> Option<String> {
    None
}
