//! When each command can be used, and why not: the decisions of the menus,
//! the toolbar and the panels, taken from a few facts about the state of the
//! app. Pure functions without egui, so they are tested here; the panels
//! only draw what these return.

use crate::text::tr;

/// What the commands depend on, read once per frame from the app.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Facts {
    pub has_image: bool,
    /// The noise reduction is switched on.
    pub reduce_noise: bool,
    /// The export does something: noise reduction, enlargement or both.
    pub changes_the_photo: bool,
    /// Every tile of the open image is denoised.
    pub image_complete: bool,
    /// Tiles of the open image already denoised.
    pub tiles_done: usize,
    /// The open image is being denoised.
    pub denoising: bool,
    pub queue_running: bool,
    /// Files in the queue still to process (queued or failed).
    pub queue_pending: usize,
    pub exporting: bool,
    /// This build can use a GPU.
    pub gpu_build: bool,
}

/// `Ok` if the command can be used now, otherwise why not, worded for the
/// tooltip of the disabled control.
pub type Availability = Result<(), &'static str>;

impl Facts {
    fn busy(&self) -> bool {
        self.denoising || self.queue_running
    }

    /// Fit, 100 %, removed noise, close: anything that needs an image.
    pub fn view(&self) -> Availability {
        if self.has_image {
            Ok(())
        } else {
            Err(tr().toolbar.why.no_image)
        }
    }

    /// Export and Export as… (an incomplete image is denoised first).
    pub fn export(&self) -> Availability {
        if !self.has_image {
            Err(tr().toolbar.why.open_image_to_export)
        } else if !self.changes_the_photo {
            Err(tr().toolbar.why.nothing_to_do)
        } else if self.exporting {
            Err(tr().toolbar.why.already_exporting)
        } else {
            Ok(())
        }
    }

    /// Denoise the open image.
    pub fn denoise(&self) -> Availability {
        let why = &tr().toolbar.why;
        if !self.has_image {
            Err(why.open_image)
        } else if !self.reduce_noise {
            Err(why.noise_off)
        } else if self.denoising {
            Err(why.already_processing)
        } else if self.queue_running {
            Err(why.queue_running)
        } else {
            Ok(())
        }
    }

    /// Stop whatever is being processed.
    pub fn stop(&self) -> Availability {
        if self.busy() {
            Ok(())
        } else {
            Err(tr().toolbar.why.nothing_running)
        }
    }

    /// Process the files of the queue.
    pub fn process_queue(&self) -> Availability {
        let why = &tr().toolbar.why;
        if self.queue_running {
            Err(why.queue_running)
        } else if self.queue_pending == 0 {
            Err(why.no_files_to_process)
        } else if !self.changes_the_photo {
            Err(why.nothing_to_do)
        } else if self.denoising {
            Err(why.wait_for_denoising)
        } else {
            Ok(())
        }
    }

    /// Pause or resume the queue being processed.
    pub fn pause_queue(&self) -> Availability {
        if self.queue_running {
            Ok(())
        } else {
            Err(tr().toolbar.why.queue_not_running)
        }
    }

    /// Add files to the queue or remove them.
    pub fn edit_queue(&self) -> Availability {
        if self.queue_running {
            Err(tr().toolbar.why.wait_for_queue)
        } else {
            Ok(())
        }
    }

    /// Search the best tile size again.
    pub fn redetect_tiles(&self) -> Availability {
        let why = &tr().toolbar.why;
        if !self.gpu_build {
            Err(why.no_gpu_build)
        } else if self.busy() {
            Err(why.wait_for_processing)
        } else {
            Ok(())
        }
    }

    /// The big button of the right panel.
    pub fn primary(&self) -> Primary {
        if self.denoising {
            Primary::Progress
        } else if !self.has_image {
            Primary::Open
        } else if self.image_complete || !self.reduce_noise {
            Primary::Export
        } else {
            Primary::Denoise { resume: self.tiles_done > 0 }
        }
    }
}

/// What the big button of the right panel does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Primary {
    /// Progress of the open image, with Stop.
    Progress,
    Open,
    Export,
    /// Denoise; `resume` when some tiles are already done.
    Denoise {
        resume: bool,
    },
}

/// The panes F6 moves the keyboard focus between, in the order of the
/// window from left to right (the Windows convention for F6).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pane {
    Queue,
    Photo,
    Params,
}

impl Pane {
    pub const ALL: [Pane; 3] = [Pane::Queue, Pane::Photo, Pane::Params];

    /// Position in [`Pane::ALL`].
    pub const fn index(self) -> usize {
        self as usize
    }
}

/// The pane F6 moves the focus to from `current` (Shift+F6, `back`, goes the
/// other way), wrapping around and skipping the queue when it is hidden.
/// With no current pane, F6 starts from the first and Shift+F6 from the last.
pub fn next_pane(current: Option<Pane>, queue_shown: bool, back: bool) -> Pane {
    let shown: Vec<Pane> = Pane::ALL.into_iter().filter(|p| queue_shown || *p != Pane::Queue).collect();
    let n = shown.len();
    let at = current.and_then(|c| shown.iter().position(|p| *p == c));
    let i = match (at, back) {
        (None, false) => 0,
        (None, true) => n - 1,
        (Some(i), false) => (i + 1) % n,
        (Some(i), true) => (i + n - 1) % n,
    };
    shown[i]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f6_goes_round_the_panes() {
        use Pane::*;
        assert_eq!(next_pane(None, true, false), Queue);
        assert_eq!(next_pane(Some(Queue), true, false), Photo);
        assert_eq!(next_pane(Some(Photo), true, false), Params);
        assert_eq!(next_pane(Some(Params), true, false), Queue);
        // Shift+F6 goes back
        assert_eq!(next_pane(Some(Queue), true, true), Params);
        assert_eq!(next_pane(Some(Params), true, true), Photo);
        assert_eq!(next_pane(None, true, true), Params);
    }

    #[test]
    fn f6_skips_the_hidden_queue() {
        use Pane::*;
        assert_eq!(next_pane(None, false, false), Photo);
        assert_eq!(next_pane(Some(Params), false, false), Photo);
        assert_eq!(next_pane(Some(Photo), false, true), Params);
        // the focus was in the queue just hidden: on to the next one shown
        assert_eq!(next_pane(Some(Queue), false, false), Photo);
        assert_eq!(Pane::ALL.map(Pane::index), [0, 1, 2]);
    }

    fn image() -> Facts {
        Facts { has_image: true, reduce_noise: true, changes_the_photo: true, gpu_build: true, ..Facts::default() }
    }

    #[test]
    fn nothing_open() {
        let f = Facts::default();
        assert!(f.view().is_err() && f.export().is_err() && f.denoise().is_err() && f.stop().is_err());
        assert_eq!(f.primary(), Primary::Open);
        assert_eq!(f.edit_queue(), Ok(()));
    }

    #[test]
    fn an_image_open() {
        let f = image();
        assert_eq!((f.view(), f.export(), f.denoise()), (Ok(()), Ok(()), Ok(())));
        assert_eq!(f.primary(), Primary::Denoise { resume: false });
        assert_eq!(Facts { tiles_done: 3, ..f }.primary(), Primary::Denoise { resume: true });
        assert_eq!(Facts { image_complete: true, ..f }.primary(), Primary::Export);
        assert_eq!(Facts { exporting: true, ..f }.export(), Err(tr().toolbar.why.already_exporting));
    }

    #[test]
    fn without_noise_reduction() {
        let why = &tr().toolbar.why;
        // the photo is exported (enlarged) as it is, without denoising first
        let f = Facts { reduce_noise: false, queue_pending: 2, ..image() };
        assert_eq!(f.primary(), Primary::Export);
        assert_eq!(f.denoise(), Err(why.noise_off));
        assert_eq!((f.export(), f.process_queue()), (Ok(()), Ok(())));
        // neither step: nothing to export
        let idle = Facts { changes_the_photo: false, ..f };
        assert_eq!((idle.export(), idle.process_queue()), (Err(why.nothing_to_do), Err(why.nothing_to_do)));
    }

    #[test]
    fn while_denoising() {
        let f = Facts { denoising: true, queue_pending: 4, ..image() };
        assert_eq!(f.denoise(), Err(tr().toolbar.why.already_processing));
        assert_eq!(f.stop(), Ok(()));
        assert_eq!(f.primary(), Primary::Progress);
        assert_eq!(f.process_queue(), Err(tr().toolbar.why.wait_for_denoising));
        assert_eq!(f.redetect_tiles(), Err(tr().toolbar.why.wait_for_processing));
        // the queue can still be filled for later
        assert_eq!(f.edit_queue(), Ok(()));
    }

    #[test]
    fn the_queue() {
        let why = &tr().toolbar.why;
        let empty = image();
        assert_eq!(empty.process_queue(), Err(why.no_files_to_process));
        assert_eq!(empty.pause_queue(), Err(why.queue_not_running));
        let ready = Facts { queue_pending: 2, ..empty };
        assert_eq!(ready.process_queue(), Ok(()));
        let running = Facts { queue_running: true, ..ready };
        assert_eq!(running.process_queue(), Err(why.queue_running));
        assert_eq!(running.denoise(), Err(why.queue_running));
        assert_eq!(running.edit_queue(), Err(why.wait_for_queue));
        assert_eq!((running.pause_queue(), running.stop()), (Ok(()), Ok(())));
    }

    #[test]
    fn tile_search_needs_a_gpu_build() {
        assert_eq!(Facts { gpu_build: false, ..image() }.redetect_tiles(), Err(tr().toolbar.why.no_gpu_build));
        assert_eq!(image().redetect_tiles(), Ok(()));
    }
}
