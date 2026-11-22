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
    /// The project name is being asked (Android, untitled): go ahead once it is saved.
    WaitForName,
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

/// After the save started by [`Step::Save`]. `clean`: nothing left to save; `naming`: the save
/// waits for a name (Android, untitled). A failed or cancelled save stays.
pub fn after_save(clean: bool, naming: bool) -> Step {
    if clean {
        Step::Proceed
    } else if naming {
        Step::WaitForName
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
        // Android, untitled: the name dialog answers later.
        assert_eq!(after_save(false, true), Step::WaitForName);
    }
}
