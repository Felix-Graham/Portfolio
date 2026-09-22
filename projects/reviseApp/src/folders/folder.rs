/// A Notes folder (feature request: "Notes Folders") — a name plus a
/// colour, stored as a hex string the same way `theme::Palette`
/// represents its own colours, so a folder's colour swatch can reuse
/// the same rendering path as everything else that draws a colour
/// from a hex string.
///
/// `parent_id` (feature request: "Built-In Flashcard Editor") makes a
/// folder a subfolder of another — one level only, same precedent as
/// `Task::parent_task_id` (Sub Tasks). `None` (every folder before
/// this request, and every ordinary top-level folder since) means
/// "top-level."
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Folder {
    pub id: i64,
    pub name: String,
    pub color: String,
    pub parent_id: Option<i64>,
    pub created_at: String,
}
