//! The enlargement shown in the viewer: with 2× or 4× chosen, the area in
//! view is enlarged with the export's own tiles (the very pixels the file
//! will hold) and drawn on the "after" side, at the photo's place.
//!
//! Only what is looked at is enlarged, and only once the view has stood
//! still for [`SETTLE`]: moving or zooming asks again for what is missing.
//! A view wider than [`MAX_AREA_PX`] input pixels asks the user to zoom in
//! instead (at that zoom the enlargement could not be seen anyway). The
//! enlarged tiles are kept, up to [`MAX_KEPT_PX`] output pixels, so going
//! back costs nothing; a change of photo, sliders or factor drops them.
//!
//! The state is generic in the texture, so it is tested without a window.

use std::time::{Duration, Instant};

use denoise_core::upscale::Upscale;
use denoise_core::{Params, PixelRect};

/// How long the view must stand still before its area is enlarged.
pub const SETTLE: Duration = Duration::from_millis(300);
/// The widest area (input pixels) enlarged for the view.
pub const MAX_AREA_PX: usize = 3_000_000;
/// Enlarged pixels kept as textures (4 bytes each: 256 MB).
pub const MAX_KEPT_PX: usize = 64_000_000;

/// What the enlarged tiles depend on: when it changes they are dropped.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Key {
    pub doc_id: u64,
    pub upscale: Upscale,
    pub params: Params,
}

/// What the application does after [`EnlargedView::frame`].
#[derive(Debug, PartialEq)]
pub enum Step {
    Nothing,
    /// Stop the request in flight: what it enlarges is no longer wanted.
    Cancel,
    /// Look again after this long (the view is settling).
    Wait(Duration),
    /// Stop the request in flight, if any, and ask for the tiles over
    /// `area` other than `have`.
    Request {
        request: u64,
        area: PixelRect,
        have: Vec<usize>,
    },
}

/// What the viewer says about the enlargement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// No enlargement wanted, or not now.
    Off,
    /// Tiles of the view are being enlarged.
    Working,
    /// The view is too wide: zoom in to see the enlargement.
    ZoomIn,
    /// What is in view is enlarged.
    Ready,
    /// The last request failed; it is not asked again until something changes.
    Failed,
}

struct Shown<T> {
    index: usize,
    rect: PixelRect,
    texture: T,
    /// Output pixels of the texture.
    pixels: usize,
    /// When it was last drawn (a frame counter).
    used: u64,
}

pub struct EnlargedView<T> {
    key: Option<Key>,
    /// Requests are numbered; tiles of requests before `first` belong to an
    /// older key.
    last: u64,
    first: u64,
    /// The request in flight.
    working: Option<u64>,
    /// The area of the last request, done or in flight.
    asked: Option<PixelRect>,
    failed: bool,
    paused: bool,
    wide: bool,
    /// The area in view and since when it has not changed.
    seen: Option<(PixelRect, Instant)>,
    tiles: Vec<Shown<T>>,
    clock: u64,
}

impl<T> Default for EnlargedView<T> {
    fn default() -> Self {
        Self {
            key: None,
            last: 0,
            first: 1,
            working: None,
            asked: None,
            failed: false,
            paused: false,
            wide: false,
            seen: None,
            tiles: Vec::new(),
            clock: 0,
        }
    }
}

impl<T> EnlargedView<T> {
    /// Called every frame with what should be enlarged (`None`: nothing),
    /// whether it must wait (`paused`: another job, or a view without the
    /// result), and the area in view at `now`.
    pub fn frame(&mut self, want: Option<Key>, paused: bool, area: PixelRect, now: Instant) -> Step {
        let mut cancel = false;
        if want != self.key {
            cancel = self.working.is_some();
            self.key = want;
            self.first = self.last + 1;
            self.working = None;
            self.asked = None;
            self.failed = false;
            self.tiles.clear();
            // a slider being dragged changes the key every frame: wait for
            // it to stand still too
            self.seen = None;
        }
        self.paused = paused;
        if self.key.is_none() || paused || self.failed {
            return if cancel { Step::Cancel } else { Step::Nothing };
        }
        let since = match self.seen {
            Some((seen, since)) if seen == area => since,
            _ => {
                self.seen = Some((area, now));
                now
            }
        };
        let still = now.saturating_duration_since(since);
        if still < SETTLE {
            return if cancel { Step::Cancel } else { Step::Wait(SETTLE - still) };
        }
        self.wide = area.area() > MAX_AREA_PX;
        if self.wide || area.is_empty() || self.asked == Some(area) {
            return if cancel { Step::Cancel } else { Step::Nothing };
        }
        self.last += 1;
        self.working = Some(self.last);
        self.asked = Some(area);
        Step::Request { request: self.last, area, have: self.tiles.iter().map(|t| t.index).collect() }
    }

    /// Whether a tile of `request` is still wanted (before making its texture).
    pub fn wants(&self, request: u64) -> bool {
        self.key.is_some() && request >= self.first && request <= self.last
    }

    /// Keeps the enlarged tile `index` over `rect` of the photo, `pixels`
    /// output pixels, if `request` is still current; drops the tiles drawn
    /// least recently beyond [`MAX_KEPT_PX`].
    pub fn accept(&mut self, request: u64, index: usize, rect: PixelRect, texture: T, pixels: usize) {
        if !self.wants(request) {
            return;
        }
        self.tiles.retain(|t| t.index != index);
        self.tiles.push(Shown { index, rect, texture, pixels, used: self.clock });
        let mut kept: usize = self.tiles.iter().map(|t| t.pixels).sum();
        while kept > MAX_KEPT_PX && self.tiles.len() > 1 {
            let Some(oldest) = self.tiles.iter().enumerate().min_by_key(|(_, t)| t.used).map(|(i, _)| i) else { break };
            kept -= self.tiles.swap_remove(oldest).pixels;
        }
    }

    /// The end of `request`: the message for the user if it failed.
    pub fn finished(&mut self, request: u64, result: Result<(), Option<String>>) -> Option<String> {
        if self.working != Some(request) {
            return None;
        }
        self.working = None;
        match result {
            Ok(()) => None,
            // stopped by a newer request or a change: asked again if needed
            Err(None) => {
                self.asked = None;
                None
            }
            Err(Some(why)) => {
                self.failed = true;
                Some(why)
            }
        }
    }

    /// The enlarged tiles over `area`, to draw; they count as just used.
    pub fn shown(&mut self, area: PixelRect) -> Vec<(PixelRect, &T)> {
        self.clock += 1;
        let clock = self.clock;
        self.tiles
            .iter_mut()
            .filter(|t| !t.rect.intersect(&area).is_empty())
            .map(|t| {
                t.used = clock;
                (t.rect, &t.texture)
            })
            .collect()
    }

    pub fn status(&self) -> Status {
        match () {
            () if self.key.is_none() || self.paused => Status::Off,
            () if self.failed => Status::Failed,
            () if self.wide => Status::ZoomIn,
            () if self.working.is_some() => Status::Working,
            () => Status::Ready,
        }
    }

    /// The factor being shown, for the label of the "after" side.
    pub fn upscale(&self) -> Option<Upscale> {
        self.key.filter(|_| !self.paused).map(|k| k.upscale)
    }
}

#[cfg(test)]
mod tests {
    use denoise_core::upscale::{Detail, Factor};

    use super::*;

    fn key(doc_id: u64) -> Key {
        Key { doc_id, upscale: Upscale { factor: Factor::X2, detail: Detail::Faithful }, params: Params::default() }
    }

    const AREA: PixelRect = PixelRect { x: 0, y: 0, width: 1000, height: 800 };

    /// Runs frames until the view has settled; returns the last step.
    fn settled(v: &mut EnlargedView<u32>, want: Option<Key>, area: PixelRect, t0: Instant) -> Step {
        assert!(matches!(v.frame(want, false, area, t0), Step::Wait(_) | Step::Cancel));
        v.frame(want, false, area, t0 + SETTLE)
    }

    #[test]
    fn asks_once_the_view_stands_still() {
        let mut v = EnlargedView::<u32>::default();
        let t0 = Instant::now();
        assert_eq!(v.frame(None, false, AREA, t0), Step::Nothing);
        assert_eq!(v.status(), Status::Off);
        assert_eq!(v.frame(Some(key(1)), false, AREA, t0), Step::Wait(SETTLE));
        assert_eq!(
            v.frame(Some(key(1)), false, AREA, t0 + SETTLE),
            Step::Request { request: 1, area: AREA, have: vec![] }
        );
        assert_eq!(v.status(), Status::Working);
        // the same view is not asked for again
        assert_eq!(v.frame(Some(key(1)), false, AREA, t0 + SETTLE * 2), Step::Nothing);
        v.accept(1, 3, PixelRect::new(0, 0, 512, 512), 30, 1024 * 1024);
        assert_eq!(v.finished(1, Ok(())), None);
        assert_eq!(v.status(), Status::Ready);
        assert_eq!(v.shown(AREA).len(), 1);
        assert!(v.shown(PixelRect::new(600, 600, 10, 10)).is_empty());
    }

    #[test]
    fn a_moved_view_asks_for_what_is_missing() {
        let mut v = EnlargedView::<u32>::default();
        let t0 = Instant::now();
        settled(&mut v, Some(key(1)), AREA, t0);
        v.accept(1, 3, PixelRect::new(0, 0, 512, 512), 30, 1);
        let moved = PixelRect::new(200, 0, 1000, 800);
        let t1 = t0 + SETTLE * 2;
        // while moving: wait
        assert!(matches!(v.frame(Some(key(1)), false, moved, t1), Step::Wait(_)));
        assert_eq!(
            v.frame(Some(key(1)), false, moved, t1 + SETTLE),
            Step::Request { request: 2, area: moved, have: vec![3] }
        );
        // the first request, stopped by the second, changes nothing
        assert_eq!(v.finished(1, Err(None)), None);
        assert_eq!(v.status(), Status::Working);
    }

    #[test]
    fn a_change_drops_the_tiles_and_stops_the_request() {
        let mut v = EnlargedView::<u32>::default();
        let t0 = Instant::now();
        settled(&mut v, Some(key(1)), AREA, t0);
        v.accept(1, 3, PixelRect::new(0, 0, 512, 512), 30, 1);
        let other =
            Key { params: Params { luma: denoise_core::Percent::clamped(10.0), ..Params::default() }, ..key(1) };
        assert_eq!(v.frame(Some(other), false, AREA, t0 + SETTLE), Step::Cancel);
        assert!(v.shown(AREA).is_empty());
        // while the sliders move, nothing is asked; once they stand still, it is
        let next = Key { params: Params { luma: denoise_core::Percent::clamped(20.0), ..Params::default() }, ..key(1) };
        assert!(matches!(v.frame(Some(next), false, AREA, t0 + SETTLE * 2), Step::Wait(_)));
        assert!(matches!(v.frame(Some(next), false, AREA, t0 + SETTLE * 3), Step::Request { .. }));
        assert_eq!(v.frame(Some(other), false, AREA, t0 + SETTLE * 3), Step::Cancel);
        // a late tile of the old request is not shown
        v.accept(1, 4, PixelRect::new(512, 0, 512, 512), 40, 1);
        assert!(v.shown(AREA).is_empty());
        assert!(!v.wants(1));
        // nothing wanted: nothing left
        assert_eq!(v.frame(None, false, AREA, t0 + SETTLE * 3), Step::Nothing);
        assert_eq!(v.status(), Status::Off);
    }

    #[test]
    fn a_wide_view_asks_to_zoom_in() {
        let mut v = EnlargedView::<u32>::default();
        let wide = PixelRect::new(0, 0, 6000, 4000);
        assert_eq!(settled(&mut v, Some(key(1)), wide, Instant::now()), Step::Nothing);
        assert_eq!(v.status(), Status::ZoomIn);
    }

    #[test]
    fn paused_keeps_the_tiles_and_a_failure_is_said_once() {
        let mut v = EnlargedView::<u32>::default();
        let t0 = Instant::now();
        settled(&mut v, Some(key(1)), AREA, t0);
        v.accept(1, 3, PixelRect::new(0, 0, 512, 512), 30, 1);
        assert_eq!(v.frame(Some(key(1)), true, AREA, t0 + SETTLE), Step::Nothing);
        assert_eq!(v.status(), Status::Off);
        assert_eq!(v.upscale(), None);
        assert_eq!(v.shown(AREA).len(), 1);
        assert_eq!(v.finished(1, Err(Some("no GPU memory".into()))), Some("no GPU memory".into()));
        assert_eq!(v.frame(Some(key(1)), false, PixelRect::new(5, 5, 10, 10), t0 + SETTLE * 9), Step::Nothing);
        assert_eq!(v.status(), Status::Failed);
        // another photo tries again
        assert!(matches!(v.frame(Some(key(2)), false, AREA, t0 + SETTLE * 10), Step::Wait(_)));
    }

    #[test]
    fn the_least_recently_drawn_tiles_go_first() {
        let mut v = EnlargedView::<u32>::default();
        settled(&mut v, Some(key(1)), AREA, Instant::now());
        let half = MAX_KEPT_PX / 2;
        v.accept(1, 1, PixelRect::new(0, 0, 10, 10), 1, half);
        v.shown(PixelRect::new(0, 0, 1, 1));
        v.accept(1, 2, PixelRect::new(100, 0, 10, 10), 2, half);
        v.shown(PixelRect::new(0, 0, 1, 1)); // tile 1 drawn again, tile 2 not
        v.accept(1, 3, PixelRect::new(200, 0, 10, 10), 3, half);
        let left: Vec<u32> = v.shown(AREA).iter().map(|(_, t)| **t).collect();
        assert_eq!(left.len(), 2);
        assert!(left.contains(&1) && left.contains(&3), "{left:?}");
    }
}
