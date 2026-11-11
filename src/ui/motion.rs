use eframe::egui;

/// A cursor movement through a list, shared by the track table and the sidebar.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Motion {
    Down,
    Up,
    HalfPageDown,
    HalfPageUp,
    First,
    Last,
}

/// How many rows Ctrl+D / Ctrl+U move, given the height left for the list.
pub fn half_page(ui: &egui::Ui, row_height: f32) -> usize {
    ((ui.available_height() / row_height) as usize / 2).max(1)
}

/// Without a visible cursor, every motion but `Last` starts at the top.
pub fn target_row(
    motion: Motion,
    cursor_row: Option<usize>,
    row_count: usize,
    half_page: usize,
) -> Option<usize> {
    let last = row_count.checked_sub(1)?;
    let Some(current) = cursor_row else {
        return Some(if motion == Motion::Last { last } else { 0 });
    };
    let target = match motion {
        Motion::Down => current + 1,
        Motion::Up => current.saturating_sub(1),
        Motion::HalfPageDown => current + half_page,
        Motion::HalfPageUp => current.saturating_sub(half_page),
        Motion::First => 0,
        Motion::Last => last,
    };
    Some(target.min(last))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn motions_stay_within_the_rows() {
        assert_eq!(target_row(Motion::Down, Some(4), 5, 10), Some(4));
        assert_eq!(target_row(Motion::Up, Some(0), 5, 10), Some(0));
        assert_eq!(target_row(Motion::HalfPageDown, Some(1), 5, 10), Some(4));
        assert_eq!(target_row(Motion::Down, None, 5, 10), Some(0));
        assert_eq!(target_row(Motion::Last, None, 5, 10), Some(4));
        assert_eq!(target_row(Motion::Down, None, 0, 10), None);
    }
}
