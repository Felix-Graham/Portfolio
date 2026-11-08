//! The stored time block: a task's slot on a particular day.
//!
//! See `docs/planning/time-blocking.md` §5.2. A block is *derived,
//! disposable* planning data — the task's `assigned_date` stays the
//! source of truth, and an auto-placed block (`locked == false`) can
//! be thrown away and recomputed at any time. A `locked` block is one
//! the person placed or moved, which re-planning leaves alone.

use std::collections::HashMap;

use chrono::{DateTime, Duration, Local, NaiveDate};

use super::time_blocking::Interval;

/// How close (in minutes) to a block's start a stopwatch or timer has to
/// begin to count as starting that block.
pub const DEFAULT_START_TOLERANCE_MINUTES: i64 = 15;

/// Where a block is in its life.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BlockStatus {
    /// Planned: the planner may still move or drop it.
    #[default]
    Planned,
    /// A stopwatch or timer is running against it.
    InProgress,
    /// Worked: its times are what actually happened.
    Done,
}

impl BlockStatus {
    pub fn to_db(self) -> i64 {
        match self {
            BlockStatus::Planned => 0,
            BlockStatus::InProgress => 1,
            BlockStatus::Done => 2,
        }
    }

    /// Anything unrecognised reads as planned rather than failing.
    pub fn from_db(value: i64) -> Self {
        match value {
            1 => BlockStatus::InProgress,
            2 => BlockStatus::Done,
            _ => BlockStatus::Planned,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimeBlock {
    pub id: i64,
    pub task_id: i64,
    pub start_time: DateTime<Local>,
    pub end_time: DateTime<Local>,
    pub locked: bool,
    /// `0..` — the position of this chunk when a task is split.
    pub chunk_index: i64,
    pub status: BlockStatus,
}

impl TimeBlock {
    /// In progress or done: history that re-planning keeps and plans around.
    pub fn is_settled(&self) -> bool {
        self.status != BlockStatus::Planned
    }

    pub fn duration(&self) -> Duration {
        self.end_time - self.start_time
    }

    /// The local day the block **starts** on. A block that runs past
    /// midnight belongs to its start day, like an event (see
    /// `events::occurrence`).
    pub fn day(&self) -> NaiveDate {
        self.start_time.date_naive()
    }

    /// The block as a planner interval (wall-clock time).
    pub fn interval(&self) -> Interval {
        Interval::new(self.start_time.naive_local(), self.end_time.naive_local())
    }

    /// Whether the block overlaps `other` (touching is not overlap).
    pub fn overlaps(&self, other: &Interval) -> bool {
        self.interval().overlaps(other)
    }
}

/// The times of a task's blocks as short text — `09:00–10:00`, or
/// `09:00–09:30, 20:00–21:00` for a task split across gaps — in time
/// order. Empty input gives an empty string.
pub fn format_block_times(blocks: &[&TimeBlock]) -> String {
    let mut ordered: Vec<&&TimeBlock> = blocks.iter().collect();
    ordered.sort_by_key(|b| b.start_time);
    ordered
        .iter()
        .map(|b| format!("{}–{}", b.start_time.format("%H:%M"), b.end_time.format("%H:%M")))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Groups blocks by the day they start on, each day's blocks keeping
/// the order they came in (callers pass them chronologically).
pub fn group_blocks_by_day(blocks: Vec<TimeBlock>) -> HashMap<NaiveDate, Vec<TimeBlock>> {
    let mut by_day: HashMap<NaiveDate, Vec<TimeBlock>> = HashMap::new();
    for block in blocks {
        by_day.entry(block.day()).or_default().push(block);
    }
    by_day
}

/// The earliest block start per task, for ordering a day's tasks by
/// when they are planned.
pub fn first_block_start_by_task(blocks: &[TimeBlock]) -> HashMap<i64, DateTime<Local>> {
    let mut first: HashMap<i64, DateTime<Local>> = HashMap::new();
    for block in blocks {
        first.entry(block.task_id).and_modify(|s| *s = (*s).min(block.start_time)).or_insert(block.start_time);
    }
    first
}

/// The note shown beside a task in a day's list: its times if it has
/// blocks on that day, `unscheduled` if it should have one but doesn't
/// (see `queries::time_blocking::is_blockable`), nothing otherwise —
/// and nothing at all while time blocking is off, so the lists look
/// exactly as they always did for anyone who hasn't opted in.
pub fn schedule_note(time_blocking_on: bool, blockable: bool, blocks_on_day: &[&TimeBlock]) -> Option<String> {
    if !time_blocking_on {
        return None;
    }
    if !blocks_on_day.is_empty() {
        return Some(format_block_times(blocks_on_day));
    }
    blockable.then(|| "unscheduled".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn block(start_h: u32, start_m: u32, end_h: u32, end_m: u32) -> TimeBlock {
        TimeBlock {
            id: 1,
            task_id: 1,
            start_time: Local.with_ymd_and_hms(2026, 10, 14, start_h, start_m, 0).unwrap(),
            end_time: Local.with_ymd_and_hms(2026, 10, 14, end_h, end_m, 0).unwrap(),
            locked: false,
            chunk_index: 0,
            status: BlockStatus::Planned,
        }
    }

    #[test]
    fn duration_is_end_minus_start() {
        assert_eq!(block(9, 0, 10, 30).duration(), Duration::minutes(90));
    }

    #[test]
    fn day_is_the_start_day_even_when_the_block_runs_past_midnight() {
        let mut b = block(23, 0, 23, 30);
        b.end_time = Local.with_ymd_and_hms(2026, 10, 15, 0, 30, 0).unwrap();
        assert_eq!(b.day(), NaiveDate::from_ymd_opt(2026, 10, 14).unwrap());
    }

    #[test]
    fn overlaps_is_half_open() {
        let b = block(9, 0, 10, 0);
        let day = NaiveDate::from_ymd_opt(2026, 10, 14).unwrap();
        let at = |h, m| day.and_hms_opt(h, m, 0).unwrap();
        assert!(b.overlaps(&Interval::new(at(9, 30), at(10, 30))));
        assert!(!b.overlaps(&Interval::new(at(10, 0), at(11, 0))), "touching is not overlapping");
        assert!(!b.overlaps(&Interval::new(at(8, 0), at(9, 0))));
    }

    #[test]
    fn block_times_read_in_time_order_and_join_split_pieces() {
        let late = block(20, 0, 21, 0);
        let early = block(9, 0, 9, 30);
        assert_eq!(format_block_times(&[&late, &early]), "09:00–09:30, 20:00–21:00");
        assert_eq!(format_block_times(&[&early]), "09:00–09:30");
        assert_eq!(format_block_times(&[]), "");
    }

    #[test]
    fn the_schedule_note_is_silent_while_time_blocking_is_off() {
        let b = block(9, 0, 10, 0);
        assert_eq!(schedule_note(false, true, &[]), None);
        assert_eq!(schedule_note(false, true, &[&b]), None);
    }

    #[test]
    fn the_schedule_note_shows_times_or_unscheduled_or_nothing() {
        let b = block(9, 0, 10, 0);
        assert_eq!(schedule_note(true, true, &[&b]).as_deref(), Some("09:00–10:00"));
        assert_eq!(schedule_note(true, true, &[]).as_deref(), Some("unscheduled"));
        assert_eq!(schedule_note(true, false, &[]), None, "a task that never gets a block isn't 'unscheduled'");
        assert_eq!(
            schedule_note(true, false, &[&b]).as_deref(),
            Some("09:00–10:00"),
            "a finished task's block is still its time"
        );
    }

    #[test]
    fn the_first_block_start_per_task_is_the_earliest_of_its_pieces() {
        let mut a_late = block(20, 0, 21, 0);
        a_late.task_id = 1;
        let mut a_early = block(9, 0, 9, 30);
        a_early.task_id = 1;
        let mut b = block(11, 0, 12, 0);
        b.task_id = 2;

        let first = first_block_start_by_task(&[a_late, a_early.clone(), b]);

        assert_eq!(first[&1], a_early.start_time);
        assert_eq!(first.len(), 2);
        assert!(first_block_start_by_task(&[]).is_empty());
    }

    #[test]
    fn blocks_group_by_their_start_day_in_the_order_given() {
        let mut next_day = block(9, 0, 10, 0);
        next_day.id = 3;
        next_day.start_time = Local.with_ymd_and_hms(2026, 10, 15, 9, 0, 0).unwrap();
        next_day.end_time = Local.with_ymd_and_hms(2026, 10, 15, 10, 0, 0).unwrap();
        let mut first = block(9, 0, 10, 0);
        first.id = 1;
        let mut second = block(13, 0, 14, 0);
        second.id = 2;

        let grouped = group_blocks_by_day(vec![first, second, next_day]);

        let day14 = NaiveDate::from_ymd_opt(2026, 10, 14).unwrap();
        let day15 = NaiveDate::from_ymd_opt(2026, 10, 15).unwrap();
        assert_eq!(grouped[&day14].iter().map(|b| b.id).collect::<Vec<_>>(), vec![1, 2]);
        assert_eq!(grouped[&day15].len(), 1);
        assert!(group_blocks_by_day(Vec::new()).is_empty());
    }

    #[test]
    fn status_round_trips_and_unknown_values_read_as_planned() {
        for status in [BlockStatus::Planned, BlockStatus::InProgress, BlockStatus::Done] {
            assert_eq!(BlockStatus::from_db(status.to_db()), status);
        }
        assert_eq!(BlockStatus::from_db(99), BlockStatus::Planned);
        assert_eq!(BlockStatus::from_db(-1), BlockStatus::Planned);
    }

    #[test]
    fn only_in_progress_and_done_blocks_are_settled() {
        let mut b = block(9, 0, 10, 0);
        assert!(!b.is_settled());
        b.status = BlockStatus::InProgress;
        assert!(b.is_settled());
        b.status = BlockStatus::Done;
        assert!(b.is_settled());
    }
}
