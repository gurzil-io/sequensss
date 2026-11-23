//! What happens to unsaved changes when the user leaves the project: closing the window,
//! Android's back button, New, Open.

/// Answer to "Save changes?".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    Save,
    DontSave,
    Cancel,
}

/// Next step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// Go ahead (close, leave, open…).
    Proceed,
    /// Ask "Save changes?".
    Ask,
    /// Save, then see [`after_save`].
    Save,
    /// Go ahead without saving: the changes are dropped.
    Discard,
    /// Stay on the project.
    Stay,
    /// The system file picker is choosing where to save (Android: untitled, or the
    /// document could not be written): go ahead once it is saved.
    WaitForPicker,
}

/// The user wants to leave the project.
pub fn request(dirty: bool) -> Step {
    if dirty { Step::Ask } else { Step::Proceed }
}

pub fn answer(a: Answer) -> Step {
    match a {
        Answer::Save => Step::Save,
        Answer::DontSave => Step::Discard,
        Answer::Cancel => Step::Stay,
    }
}

/// After the save started by [`Step::Save`]. `clean`: nothing left to save; `picking`: the save
/// waits for the system file picker (Android). A failed or cancelled save stays.
pub fn after_save(clean: bool, picking: bool) -> Step {
    if clean {
        Step::Proceed
    } else if picking {
        Step::WaitForPicker
    } else {
        Step::Stay
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_to_save_leaves_at_once() {
        assert_eq!(request(false), Step::Proceed);
        assert_eq!(request(true), Step::Ask);
    }

    #[test]
    fn answers() {
        assert_eq!(answer(Answer::Save), Step::Save);
        assert_eq!(answer(Answer::DontSave), Step::Discard);
        assert_eq!(answer(Answer::Cancel), Step::Stay);
    }

    #[test]
    fn save_then_leave_only_once_saved() {
        // Saved to its file, or through Save as… (desktop: the file dialog returns at once).
        assert_eq!(after_save(true, false), Step::Proceed);
        // File dialog cancelled, or write error: the work stays open.
        assert_eq!(after_save(false, false), Step::Stay);
        // Android, untitled or document no longer writable: the file picker answers later.
        assert_eq!(after_save(false, true), Step::WaitForPicker);
    }
}
