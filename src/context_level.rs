use crate::git::{DEFAULT_CONTEXT_LINES, FULL_FILE_CONTEXT_LINES};

/// How much unchanged code is shown around each change, as a rung on a fixed
/// ladder rather than a free-form number: the user steps through it with a
/// single key, so the choices have to be few and predictable.
const LADDER: [u32; 4] = [
    DEFAULT_CONTEXT_LINES,
    10,
    25,
    FULL_FILE_CONTEXT_LINES,
];

/// The currently displayed context width, as an index into [`LADDER`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContextLevel(usize);

impl ContextLevel {
    /// The tightest view: what git shows without being asked for more.
    pub const DEFAULT: Self = Self(0);

    /// Lines of context to ask libgit2 for.
    pub fn lines(self) -> u32 {
        LADDER[self.0]
    }

    /// The next rung up, saturating at the whole file.
    pub fn expanded(self) -> Self {
        Self((self.0 + 1).min(LADDER.len() - 1))
    }

    /// The next rung down, saturating at [`Self::DEFAULT`].
    pub fn collapsed(self) -> Self {
        Self(self.0.saturating_sub(1))
    }

    pub fn is_whole_file(self) -> bool {
        self.0 == LADDER.len() - 1
    }

    /// How this rung is described in the file header, so stepping past the end
    /// of the ladder is visibly a no-op rather than a dead key.
    ///
    /// `covers_whole_file` reports whether the diff on screen already reaches
    /// both ends of the file, which a short file does long before the top rung.
    /// Naming the rung in that case would invite a step that changes nothing.
    pub fn label(self, covers_whole_file: bool) -> String {
        if covers_whole_file || self.is_whole_file() {
            "whole file".to_string()
        } else {
            format!("±{} lines", self.lines())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expanding_steps_up_and_saturates_at_the_whole_file() {
        let mut level = ContextLevel::DEFAULT;
        assert_eq!(level.lines(), 3);

        level = level.expanded();
        assert_eq!(level.lines(), 10);

        level = level.expanded();
        assert_eq!(level.lines(), 25);

        level = level.expanded();
        assert!(level.is_whole_file());

        // Already showing everything: pressing expand again is a no-op, not a
        // wrap back to the tightest view.
        assert!(level.expanded().is_whole_file());
    }

    #[test]
    fn collapsing_steps_back_down_and_saturates_at_the_default() {
        let mut level = ContextLevel::DEFAULT.expanded().expanded();
        assert_eq!(level.lines(), 25);

        level = level.collapsed();
        assert_eq!(level.lines(), 10);

        level = level.collapsed();
        assert_eq!(level, ContextLevel::DEFAULT);

        // Collapsing past the default would hide context git normally shows.
        assert_eq!(level.collapsed(), ContextLevel::DEFAULT);
    }

    #[test]
    fn label_names_the_whole_file_rung_instead_of_its_line_count() {
        assert_eq!(ContextLevel::DEFAULT.label(false), "±3 lines");
        assert_eq!(ContextLevel::DEFAULT.expanded().label(false), "±10 lines");

        // The top rung's line count is an implementation detail standing in for
        // "everything" — showing it would read as nonsense.
        let full = ContextLevel::DEFAULT.expanded().expanded().expanded();
        assert_eq!(full.label(false), "whole file");
    }

    #[test]
    fn a_rung_that_happens_to_reach_both_ends_is_named_for_that() {
        // A file short enough to fit inside ±3 is showing everything, and the
        // label has to say so: it is the only thing explaining why the expand
        // control alongside it is inert.
        assert_eq!(ContextLevel::DEFAULT.label(true), "whole file");
        assert_eq!(ContextLevel::DEFAULT.expanded().label(true), "whole file");
    }
}
