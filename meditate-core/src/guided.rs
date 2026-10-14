//! Which guided file Setup shows after the library changed.

/// What Setup does with its guided file after a rename, a delete, an
/// Undo or a sync.
#[derive(Debug, PartialEq, Eq)]
pub enum Follow {
    /// Nothing to do, or the library can't be read right now.
    Keep,
    /// The selected file is still there: re-read its name and length.
    Refresh,
    /// The selected file is gone: clear the selection and remember it.
    Clear,
    /// The remembered file is back (Undo): select it again.
    Reselect(String),
}

/// `selected`: the library file selected, if any. `other_pick`: an
/// Open File or Local Folder file is the current pick. `cleared`: the
/// file a delete cleared. `exists` looks a file up; `None` when the
/// library can't be read, which never clears anything.
pub fn follow_selection(
    selected: Option<&str>,
    other_pick: bool,
    cleared: Option<&str>,
    exists: impl Fn(&str) -> Option<bool>,
) -> Follow {
    match selected {
        Some(uuid) => match exists(uuid) {
            Some(true) => Follow::Refresh,
            Some(false) => Follow::Clear,
            None => Follow::Keep,
        },
        None if other_pick => Follow::Keep,
        None => match cleared {
            Some(uuid) if exists(uuid) == Some(true) => Follow::Reselect(uuid.to_string()),
            _ => Follow::Keep,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_selected_file_follows_renames_and_deletes() {
        assert_eq!(follow_selection(Some("a"), false, None, |_| Some(true)), Follow::Refresh);
        assert_eq!(follow_selection(Some("a"), false, None, |_| Some(false)), Follow::Clear);
        // A busy or missing database is not a delete.
        assert_eq!(follow_selection(Some("a"), false, None, |_| None), Follow::Keep);
    }

    #[test]
    fn undo_of_a_delete_selects_the_file_again() {
        assert_eq!(follow_selection(None, false, Some("a"), |_| Some(true)), Follow::Reselect("a".into()));
        // Still deleted, or the user picked something else meanwhile.
        assert_eq!(follow_selection(None, false, Some("a"), |_| Some(false)), Follow::Keep);
        assert_eq!(follow_selection(None, true, Some("a"), |_| Some(true)), Follow::Keep);
        assert_eq!(follow_selection(None, false, None, |_| Some(true)), Follow::Keep);
    }
}
