/// Moves a list selection index by `delta` (`+1` for Down, `-1` for
/// Up), clamped to `[0, len-1]` rather than wrapping. No selection
/// (`None`) plus any arrow press lands on the first item — pressing
/// Up with nothing selected doesn't jump to the last item, which
/// would be surprising the first time it happened.
///
/// Pure and UI-independent, shared by the To Do and Notes list
/// views, so this one rule can't drift between the two.
pub fn move_selection(current: Option<usize>, delta: i32, len: usize) -> Option<usize> {
    if len == 0 {
        return None;
    }
    let current = match current {
        Some(i) => i as i32,
        None => -1,
    };
    Some((current + delta).clamp(0, len as i32 - 1) as usize)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_selection_plus_down_selects_the_first_item() {
        assert_eq!(move_selection(None, 1, 5), Some(0));
    }

    #[test]
    fn no_selection_plus_up_also_selects_the_first_item_not_the_last() {
        assert_eq!(move_selection(None, -1, 5), Some(0));
    }

    #[test]
    fn moves_down_and_up_normally() {
        assert_eq!(move_selection(Some(2), 1, 5), Some(3));
        assert_eq!(move_selection(Some(2), -1, 5), Some(1));
    }

    #[test]
    fn clamps_at_the_top_and_bottom_rather_than_wrapping() {
        assert_eq!(move_selection(Some(0), -1, 5), Some(0));
        assert_eq!(move_selection(Some(4), 1, 5), Some(4));
    }

    #[test]
    fn empty_list_never_selects_anything() {
        assert_eq!(move_selection(None, 1, 0), None);
        assert_eq!(move_selection(Some(0), 1, 0), None);
    }
}
