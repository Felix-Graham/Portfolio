/// The five primary views (Section 3 of the plan, plus Flashcards
/// added later as a feature request). Sprint 1 wires up navigation
/// between shells; each view's real content arrives in later sprints.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Dashboard,
    Calendar,
    ToDo,
    Notes,
    Flashcards,
}

impl View {
    pub const ALL: [View; 5] = [View::Dashboard, View::Calendar, View::ToDo, View::Notes, View::Flashcards];

    pub fn label(&self) -> &'static str {
        match self {
            View::Dashboard => "Dashboard",
            View::Calendar => "Calendar",
            View::ToDo => "To Do",
            View::Notes => "Notes",
            View::Flashcards => "Flashcards",
        }
    }

    /// 1-based keyboard shortcut digit, per Sprint 1's "basic keyboard
    /// navigation" requirement.
    pub fn shortcut_digit(&self) -> u8 {
        match self {
            View::Dashboard => 1,
            View::Calendar => 2,
            View::ToDo => 3,
            View::Notes => 4,
            View::Flashcards => 5,
        }
    }

    /// The id of the field the vim-esque `i` key should focus when
    /// entering "insert mode" on this view (see
    /// `ReviseApp::handle_keyboard_navigation`). `None` for views with
    /// no primary text input.
    pub fn primary_input_id(&self) -> Option<&'static str> {
        match self {
            View::Dashboard => Some("dashboard_quick_task"),
            View::Calendar => None,
            View::ToDo => Some("todo_new_task_title"),
            View::Notes => Some("notes_search"),
            View::Flashcards => Some("flashcards_import_path"),
        }
    }
}
