//! Pure time-block planner (Stage B of the two-stage plan in
//! `docs/planning/time-blocking.md` §6).
//!
//! Stage A (`tasks::auto_assign::plan`) decides *which day* a task is
//! on; this module decides *what time of day*, by placing each day's
//! tasks into the free gaps between calendar events.
//!
//! Same discipline as `tasks::auto_assign` and `ui::pager`: **no
//! database, no egui, no clock** — "now" is passed in, so every
//! function here is deterministic and unit-testable.
//!
//! # Time representation and DST
//!
//! Everything inside the planner is `NaiveDateTime` (wall-clock time),
//! so the algorithm never has to reason about ambiguous or nonexistent
//! local times. The *boundary* code that converts `DateTime<Local>`
//! to and from naive values owns the DST rule: a nonexistent local
//! time (spring-forward gap) is shifted forward, an ambiguous one
//! (fall-back overlap) takes the earlier offset — chrono's
//! `earliest()`. See [`naive_to_local`].
//!
//! # Granularity
//!
//! Every block starts and ends on a multiple of
//! [`BlockConfig::granularity`] minutes since midnight, and a task's
//! length is rounded **up** to the granularity (a 20-minute task on a
//! 15-minute grid gets a 30-minute block). Free slots are snapped
//! *inward* (start up, end down) so a snapped block can never overlap
//! the busy interval that bounded it.

use chrono::{DateTime, Duration, Local, NaiveDate, NaiveDateTime, NaiveTime, TimeZone};

/// Defaults for the time-blocking settings (`docs/planning/time-blocking.md`
/// §5.4), kept next to the planner like `auto_assign::DEFAULT_*`.
pub const DEFAULT_WORK_START_MINUTES: i64 = 9 * 60;
pub const DEFAULT_WORK_END_MINUTES: i64 = 21 * 60;
pub const DEFAULT_GRANULARITY_MINUTES: i64 = 15;
pub const DEFAULT_MIN_BLOCK_MINUTES: i64 = 25;
pub const DEFAULT_BUFFER_MINUTES: i64 = 0;
/// Most pieces one task may be split into, across all its days. The
/// person's answer to Q2 was "split as necessary, pieces may span days"
/// (e.g. a maths assignment as four 40-minute sessions), so the default
/// is generous; a task is still only split when it does not fit whole.
/// `1` in Settings turns splitting off.
pub const DEFAULT_MAX_CHUNKS: usize = 6;
/// Feature request: a task estimated at more than this many minutes is
/// planned as several blocks rather than one long one (so a 90-minute task
/// becomes two 45-minute sessions with something between them). Settings →
/// "Split tasks longer than".
pub const DEFAULT_SPLIT_OVER_MINUTES: i64 = 30;
/// Feature request: the longest block the planner makes when it splits a
/// task (Settings → "Longest block"). The shortest is [`DEFAULT_MIN_BLOCK_MINUTES`].
pub const DEFAULT_MAX_BLOCK_MINUTES: i64 = 60;

/// Planner tuning. See `docs/planning/time-blocking.md` §5.4 / D5.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockConfig {
    /// Blocks snap to this many minutes since midnight (5 or 15).
    pub granularity: i64,
    /// Smallest block/slot worth using, in minutes. Rounded up to the
    /// granularity before use (see [`BlockConfig::effective_min_block`]).
    pub min_block: i64,
    /// Gap kept clear around every busy interval, in minutes.
    pub buffer: i64,
    /// Most chunks one task may be split into. `1` = never split
    /// (the Phase 1 behaviour).
    pub max_chunks: usize,
    /// Feature request: a task estimated at more than this many minutes is
    /// planned as several blocks (see [`piece_sizes`]). Never applies when
    /// `max_chunks` is `1`.
    pub split_over: i64,
    /// Feature request: the longest block made when a task is split. Rounded
    /// up to the granularity and never below the shortest block (see
    /// [`BlockConfig::effective_max_block`]).
    pub max_block: i64,
}

impl Default for BlockConfig {
    fn default() -> Self {
        Self {
            granularity: DEFAULT_GRANULARITY_MINUTES,
            min_block: DEFAULT_MIN_BLOCK_MINUTES,
            buffer: DEFAULT_BUFFER_MINUTES,
            max_chunks: DEFAULT_MAX_CHUNKS,
            split_over: DEFAULT_SPLIT_OVER_MINUTES,
            max_block: DEFAULT_MAX_BLOCK_MINUTES,
        }
    }
}

impl BlockConfig {
    /// Granularity clamped to at least one minute, so a bad setting can
    /// never cause a divide-by-zero or an infinite loop.
    fn grid(&self) -> i64 {
        self.granularity.max(1)
    }

    /// `min_block` rounded up to the granularity (and at least one
    /// grid step), so chunk lengths stay on the grid.
    pub fn effective_min_block(&self) -> i64 {
        round_up_minutes(self.min_block.max(1), self.grid())
    }

    /// `max_block` rounded up to the granularity and never below the
    /// effective minimum, so the two settings can't contradict each other.
    pub fn effective_max_block(&self) -> i64 {
        round_up_minutes(self.max_block.max(1), self.grid()).max(self.effective_min_block())
    }
}

/// Feature request: how a task of `minutes` is cut into blocks.
///
/// * Up to `cfg.split_over` minutes — or when splitting is off
///   (`max_chunks` leaves room for one piece only) — it is one block, its
///   length rounded up to the grid as always.
/// * Longer than that it is cut into the fewest pieces that are each at most
///   the longest block, and at least two, shared out as evenly as the grid
///   allows (90 → 45 + 45; 105 on a 15-minute grid → 60 + 45).
/// * No piece is made shorter than the shortest block: if two pieces would be,
///   the task stays whole (a 45-minute task with a 25-minute minimum).
/// * At most `max_chunks − locked_chunks` pieces, so a very long task gets
///   longer pieces rather than becoming unplaceable.
///
/// The sizes sum to the task's length rounded up to the grid.
pub fn piece_sizes(minutes: i64, locked_chunks: usize, cfg: &BlockConfig) -> Vec<i64> {
    if minutes <= 0 {
        return Vec::new();
    }
    let grid = cfg.grid();
    let need = round_up_minutes(minutes, grid);
    let max_pieces = cfg.max_chunks.saturating_sub(locked_chunks).max(1) as i64;
    if minutes <= cfg.split_over || cfg.max_chunks <= 1 || max_pieces <= 1 {
        return vec![need];
    }
    let min = cfg.effective_min_block();
    let max = cfg.effective_max_block();
    let units = need / grid;
    let mut n = ((need + max - 1) / max).max(2).min(max_pieces);
    while n > 1 && (units / n) * grid < min {
        n -= 1;
    }
    let (base, extra) = (units / n, units % n);
    (0..n).map(|i| (base + i64::from(i < extra)) * grid).collect()
}

/// A half-open `[start, end)` span of wall-clock time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Interval {
    pub start: NaiveDateTime,
    pub end: NaiveDateTime,
}

impl Interval {
    pub fn new(start: NaiveDateTime, end: NaiveDateTime) -> Self {
        Self { start, end }
    }

    /// Length in whole minutes; `0` for an empty or inverted interval.
    pub fn minutes(&self) -> i64 {
        (self.end - self.start).num_minutes().max(0)
    }

    pub fn is_empty(&self) -> bool {
        self.end <= self.start
    }

    /// Half-open overlap: touching intervals do **not** overlap.
    pub fn overlaps(&self, other: &Interval) -> bool {
        self.start < other.end && other.start < self.end
    }
}

/// The span of one day in which blocks may be placed (the working
/// window).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DayWindow {
    pub day: NaiveDate,
    pub start: NaiveDateTime,
    pub end: NaiveDateTime,
}

impl DayWindow {
    /// A window on `day` from `start_minutes` to `end_minutes` after
    /// midnight (the shape the `work_start_minutes` /
    /// `work_end_minutes` settings use).
    pub fn from_minutes(day: NaiveDate, start_minutes: i64, end_minutes: i64) -> Self {
        let midnight = day.and_time(NaiveTime::MIN);
        Self {
            day,
            start: midnight + Duration::minutes(start_minutes),
            end: midnight + Duration::minutes(end_minutes),
        }
    }

    pub fn interval(&self) -> Interval {
        Interval::new(self.start, self.end)
    }
}

/// What a particular day allows (feature request: per-weekday working
/// hours, Q1): working hours, or a day off on which nothing is placed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkDay {
    /// Work from `start` to `end`, minutes after midnight.
    Hours(i64, i64),
    /// Nothing is planned and Auto-assign gives the day no work.
    Off,
}

impl WorkDay {
    /// The planner window for `day`. A day off is an **empty** window on
    /// that day rather than a missing one, so the run of days stays
    /// contiguous: the planner sees "no room here" and, where splitting is
    /// allowed, carries a task on to the next working day.
    pub fn window(self, day: NaiveDate) -> DayWindow {
        match self {
            WorkDay::Hours(start, end) => DayWindow::from_minutes(day, start, end),
            WorkDay::Off => DayWindow::from_minutes(day, 0, 0),
        }
    }
}

/// A task to be placed on a day.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockTask {
    pub id: i64,
    /// Estimated length in minutes (already defaulted by the caller).
    pub minutes: i64,
    pub due: Option<NaiveDate>,
    /// The To Do list's manual position.
    pub position: i64,
    /// Extension hook (D4 / §6.4): a time-of-day preference. Unused in
    /// v1 so adding it later doesn't change any signature.
    pub preferred_start: Option<NaiveTime>,
}

/// One planned block (or chunk of a split task).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NewBlock {
    pub task_id: i64,
    pub start: NaiveDateTime,
    pub end: NaiveDateTime,
    pub chunk_index: usize,
}

impl NewBlock {
    pub fn interval(&self) -> Interval {
        Interval::new(self.start, self.end)
    }
}

/// What [`place_day`] produced for one day.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DayResult {
    pub blocks: Vec<NewBlock>,
    /// Tasks that did not fit. A task is **never** partially placed:
    /// it is either fully in `blocks` or listed here.
    pub unplaced: Vec<i64>,
}

// ---------------------------------------------------------------------
// Boundary conversion
// ---------------------------------------------------------------------

/// Converts a planner wall-clock time back to `DateTime<Local>`.
///
/// DST rule (documented here because the planner itself is naive): a
/// time that does not exist (spring-forward gap) is shifted forward
/// by an hour into the first valid time; an ambiguous time (fall-back
/// overlap) takes the earlier offset.
pub fn naive_to_local(naive: NaiveDateTime) -> DateTime<Local> {
    match Local.from_local_datetime(&naive).earliest() {
        Some(dt) => dt,
        // Nonexistent local time: nudge forward past the gap.
        None => Local
            .from_local_datetime(&(naive + Duration::hours(1)))
            .earliest()
            .unwrap_or_else(|| Local.from_utc_datetime(&naive)),
    }
}

// ---------------------------------------------------------------------
// Grid helpers
// ---------------------------------------------------------------------

fn round_up_minutes(minutes: i64, grid: i64) -> i64 {
    let grid = grid.max(1);
    (minutes + grid - 1).div_euclid(grid) * grid
}

fn seconds_since_midnight(dt: NaiveDateTime) -> i64 {
    use chrono::Timelike;
    i64::from(dt.time().num_seconds_from_midnight())
}

/// Rounds `dt` **up** to the next multiple of `granularity` minutes
/// since midnight (unchanged if already aligned).
pub fn snap_up(dt: NaiveDateTime, granularity: i64) -> NaiveDateTime {
    use chrono::Timelike;
    let step = granularity.max(1) * 60;
    let mut secs = seconds_since_midnight(dt);
    if dt.nanosecond() > 0 {
        secs += 1;
    }
    let snapped = (secs + step - 1).div_euclid(step) * step;
    dt.date().and_time(NaiveTime::MIN) + Duration::seconds(snapped)
}

/// Rounds `dt` **down** to a multiple of `granularity` minutes since
/// midnight (unchanged if already aligned).
pub fn snap_down(dt: NaiveDateTime, granularity: i64) -> NaiveDateTime {
    let step = granularity.max(1) * 60;
    let snapped = seconds_since_midnight(dt).div_euclid(step) * step;
    dt.date().and_time(NaiveTime::MIN) + Duration::seconds(snapped)
}

// ---------------------------------------------------------------------
// Interval math (§6.2 functions 1–4, 6, 7)
// ---------------------------------------------------------------------

/// Sorts and coalesces overlapping **and adjacent** intervals; empty
/// or inverted ones are dropped.
pub fn merge_intervals(mut intervals: Vec<Interval>) -> Vec<Interval> {
    intervals.retain(|i| !i.is_empty());
    intervals.sort_by_key(|i| (i.start, i.end));

    let mut merged: Vec<Interval> = Vec::with_capacity(intervals.len());
    for next in intervals {
        match merged.last_mut() {
            Some(last) if next.start <= last.end => {
                if next.end > last.end {
                    last.end = next.end;
                }
            }
            _ => merged.push(next),
        }
    }
    merged
}

/// Expands `busy` by `buffer` minutes on each side, clamped to
/// `window`. May return an empty/inverted interval if `busy` lies
/// wholly outside the window; [`merge_intervals`] drops those.
pub fn inflate(busy: Interval, buffer: i64, window: &Interval) -> Interval {
    let pad = Duration::minutes(buffer.max(0));
    Interval::new((busy.start - pad).max(window.start), (busy.end + pad).min(window.end))
}

/// The earliest a block may start on a day: the window start, or
/// `now` rounded up to the grid if that is later. For a future day
/// `now` precedes the window, so this is just the window start; for a
/// day already over it lands past the window end (no slots).
pub fn earliest_start(now: NaiveDateTime, window: &DayWindow, cfg: &BlockConfig) -> NaiveDateTime {
    window.start.max(snap_up(now, cfg.grid()))
}

/// The window minus the (buffered) busy intervals, with each slot's
/// start snapped **up** and end snapped **down** to the grid, and
/// slots shorter than the effective minimum block dropped.
pub fn free_slots(window: &Interval, busy: &[Interval], cfg: &BlockConfig) -> Vec<Interval> {
    if window.is_empty() {
        return Vec::new();
    }
    let inflated: Vec<Interval> = busy.iter().map(|b| inflate(*b, cfg.buffer, window)).collect();
    let merged = merge_intervals(inflated);

    let mut raw: Vec<Interval> = Vec::new();
    let mut cursor = window.start;
    for b in &merged {
        if b.start > cursor {
            raw.push(Interval::new(cursor, b.start));
        }
        cursor = cursor.max(b.end);
    }
    if cursor < window.end {
        raw.push(Interval::new(cursor, window.end));
    }

    let grid = cfg.grid();
    let min_block = cfg.effective_min_block();
    raw.into_iter()
        .map(|s| Interval::new(snap_up(s.start, grid), snap_down(s.end, grid)))
        .filter(|s| !s.is_empty() && s.minutes() >= min_block)
        .collect()
}

/// Total free minutes in the window — what Stage A's per-day capacity
/// is built from (D2).
pub fn day_capacity(window: &Interval, busy: &[Interval], cfg: &BlockConfig) -> i64 {
    free_slots(window, busy, cfg).iter().map(Interval::minutes).sum()
}

/// Ids of the blocks that overlap any busy interval (used to flag
/// locked blocks that an event has since landed on). Takes
/// `(block id, interval)` pairs so it works for stored and planned
/// blocks alike; no buffer is applied — a conflict is a real overlap.
pub fn find_conflicts(blocks: &[(i64, Interval)], busy: &[Interval]) -> Vec<i64> {
    let merged = merge_intervals(busy.to_vec());
    blocks
        .iter()
        .filter(|(_, interval)| merged.iter().any(|b| b.overlaps(interval)))
        .map(|(id, _)| *id)
        .collect()
}

// ---------------------------------------------------------------------
// Placement (§6.2 function 5)
// ---------------------------------------------------------------------

/// Deadline soonest first, then the To Do list's manual position, then
/// id — the same key `auto_assign::plan` uses. A single function so a
/// future "hard tasks first / mornings" preference is a one-line
/// change.
pub fn order_key(task: &BlockTask) -> (bool, Option<NaiveDate>, i64, i64) {
    (task.due.is_none(), task.due, task.position, task.id)
}

/// Places one day's tasks into its free slots.
///
/// Deterministic: tasks are sorted by [`order_key`] inside, so the
/// result does not depend on input order. Each task goes in the
/// **earliest slot that holds it whole** (first fit, forward in time).
/// If none does and `cfg.max_chunks > 1`, it is split greedily across
/// slots — every chunk at least the effective minimum block, at most
/// `max_chunks` chunks, all-or-nothing. Otherwise it is `unplaced`.
///
/// `now` clamps the window start (see [`earliest_start`]) so nothing
/// is ever planned in the past.
pub fn place_day(
    window: &DayWindow,
    busy: &[Interval],
    tasks: &[BlockTask],
    cfg: &BlockConfig,
    now: NaiveDateTime,
) -> DayResult {
    let clamped = Interval::new(earliest_start(now, window, cfg), window.end);
    let mut slots = free_slots(&clamped, busy, cfg);

    let grid = cfg.grid();
    let min_block = cfg.effective_min_block();

    let mut ordered: Vec<BlockTask> = tasks.to_vec();
    ordered.sort_by_key(order_key);

    let mut result = DayResult::default();
    for task in ordered {
        if task.minutes <= 0 {
            result.unplaced.push(task.id);
            continue;
        }
        let need = round_up_minutes(task.minutes, grid);

        // 1. Whole, in the earliest slot that holds it.
        if let Some(i) = slots.iter().position(|s| s.minutes() >= need) {
            let start = slots[i].start;
            let end = start + Duration::minutes(need);
            result.blocks.push(NewBlock { task_id: task.id, start, end, chunk_index: 0 });
            consume(&mut slots, i, end);
            continue;
        }

        // 2. Split across slots, all-or-nothing.
        if cfg.max_chunks > 1 {
            if let Some(chunks) = split_across(&slots, need, min_block, grid, cfg.max_chunks, false) {
                for (n, (_, start, end)) in chunks.iter().enumerate() {
                    result.blocks.push(NewBlock {
                        task_id: task.id,
                        start: *start,
                        end: *end,
                        chunk_index: n,
                    });
                }
                // Consume in reverse so earlier slot indices stay valid
                // when a fully used slot is removed.
                for (slot_index, _, end) in chunks.iter().rev() {
                    consume(&mut slots, *slot_index, *end);
                }
                continue;
            }
        }

        result.unplaced.push(task.id);
    }

    result.blocks.sort_by_key(|b| (b.start, b.task_id, b.chunk_index));
    result
}

/// Shrinks `slots[i]` so it starts at `new_start` (the end of the
/// block just placed in it), dropping it if nothing is left.
fn consume(slots: &mut Vec<Interval>, i: usize, new_start: NaiveDateTime) {
    slots[i].start = new_start;
    if slots[i].is_empty() {
        slots.remove(i);
    }
}

/// Greedy split of `need` minutes over `slots` (in time order).
/// Returns `(slot index, start, end)` per chunk. With `allow_partial`
/// false it is all-or-nothing: `None` unless the whole `need` fits in
/// `max_chunks` chunks of at least `min_block`. With it true, whatever
/// chunks fit are returned (at least one) and the caller carries the
/// remainder — every chunk but the last still leaves at least
/// `min_block` behind, so the remainder is itself placeable.
fn split_across(
    slots: &[Interval],
    need: i64,
    min_block: i64,
    grid: i64,
    max_chunks: usize,
    allow_partial: bool,
) -> Option<Vec<(usize, NaiveDateTime, NaiveDateTime)>> {
    let mut remaining = need;
    let mut chunks: Vec<(usize, NaiveDateTime, NaiveDateTime)> = Vec::new();

    for (i, slot) in slots.iter().enumerate() {
        if remaining == 0 {
            break;
        }
        if chunks.len() >= max_chunks {
            // Out of pieces. A partial placement keeps what it has (the
            // caller carries the rest to the next day); otherwise it fails.
            return (allow_partial && !chunks.is_empty()).then_some(chunks);
        }
        let mut take = slot.minutes().min(remaining);
        // Don't strand a final chunk smaller than the minimum.
        if take < remaining && remaining - take < min_block {
            take = remaining - min_block;
        }
        take = take.div_euclid(grid) * grid;
        if take < min_block {
            continue;
        }
        let start = slot.start;
        let end = start + Duration::minutes(take);
        chunks.push((i, start, end));
        remaining -= take;
    }

    if chunks.is_empty() {
        return None;
    }
    (remaining == 0 || allow_partial).then_some(chunks)
}

// ---------------------------------------------------------------------
// Multi-day placement (a task may be split, and its pieces may span days)
// ---------------------------------------------------------------------

/// A task to place somewhere in a run of days.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HorizonTask {
    pub id: i64,
    /// Minutes still to place (the estimate minus anything already
    /// pinned), before rounding to the grid.
    pub minutes: i64,
    /// The day Auto-assign (or the person) gave it — the earliest day
    /// it may be placed on.
    pub assigned: NaiveDate,
    /// Its effective deadline: nothing is placed after this day.
    pub due: Option<NaiveDate>,
    pub position: i64,
    /// Pieces already pinned, which count against `max_chunks`.
    pub locked_chunks: usize,
}

/// What [`plan_horizon`] produced.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HorizonResult {
    /// Pieces in time order. `chunk_index` numbers each task's pieces
    /// in time order across all days.
    pub blocks: Vec<NewBlock>,
    /// `(task id, the day it was assigned)` for each task that could
    /// not be placed in full. Such a task gets **no** blocks at all.
    pub unplaced: Vec<(i64, NaiveDate)>,
}

fn horizon_order_key(task: &HorizonTask) -> (bool, Option<NaiveDate>, i64, i64) {
    (task.due.is_none(), task.due, task.position, task.id)
}

struct Progress {
    task: HorizonTask,
    remaining: i64,
    /// Feature request: the blocks still to place, in minutes, front first —
    /// one for a short task, several for one over `split_over` (see [`piece_sizes`]).
    queue: std::collections::VecDeque<i64>,
    pieces: Vec<NewBlock>,
    failed: bool,
}

/// Places tasks across consecutive days.
///
/// Days are filled in order. On a task's assigned day it goes whole in
/// the earliest gap that holds it; failing that, split across that
/// day's gaps; failing that (and when `cfg.max_chunks` allows), as much
/// as fits goes on that day and the rest **carries to the next day**,
/// and so on up to its deadline or the last day. A task is split only
/// as far as necessary. With `max_chunks == 1` nothing is split and
/// nothing carries: a task that does not fit its own day is unplaced.
///
/// All-or-nothing per task: if it cannot be placed in full (out of
/// days, past its deadline, or out of pieces), none of its pieces are
/// kept. Everything is deterministic and independent of input order.
pub fn plan_horizon(
    windows: &[DayWindow],
    busy: &[Interval],
    tasks: &[HorizonTask],
    cfg: &BlockConfig,
    now: NaiveDateTime,
) -> HorizonResult {
    let grid = cfg.grid();
    let min_block = cfg.effective_min_block();

    let mut windows: Vec<DayWindow> = windows.to_vec();
    windows.sort_by_key(|w| w.day);

    let mut ordered: Vec<HorizonTask> = tasks.to_vec();
    ordered.sort_by_key(horizon_order_key);
    let mut progress: Vec<Progress> = ordered
        .into_iter()
        .map(|task| {
            let remaining = if task.minutes > 0 { round_up_minutes(task.minutes, grid) } else { 0 };
            let queue = piece_sizes(task.minutes, task.locked_chunks, cfg).into();
            Progress { task, remaining, queue, pieces: Vec::new(), failed: task.minutes <= 0 }
        })
        .collect();

    for (window_index, window) in windows.iter().enumerate() {
        let is_last_day = window_index + 1 == windows.len();
        let clamped = Interval::new(earliest_start(now, window, cfg), window.end);
        let mut slots = free_slots(&clamped, busy, cfg);

        for p in progress.iter_mut() {
            if p.failed || p.remaining <= 0 || p.task.assigned > window.day {
                continue;
            }
            // Past its deadline, or carrying while splitting is off: give up.
            if p.task.due.is_some_and(|due| window.day > due) || (p.task.assigned < window.day && cfg.max_chunks <= 1)
            {
                p.failed = true;
                continue;
            }
            // This task's pieces for the day, one after another. `last_end` is where
            // its latest piece today finished: the next starts a step later, so two
            // pieces of one task are never butted together into one long block.
            let mut last_end: Option<NaiveDateTime> = None;
            while let Some(&need) = p.queue.front() {
                let budget = cfg.max_chunks.saturating_sub(p.task.locked_chunks + p.pieces.len());
                if budget == 0 {
                    p.failed = true;
                    break;
                }
                // Chunks this piece may be split over: what is left once every
                // piece still waiting has its own (all of them, for a short task).
                let spare = budget.saturating_sub(p.queue.len()) + 1;

                let whole = slots.iter().enumerate().find_map(|(i, slot)| {
                    let start = match last_end {
                        Some(end) if slot.start <= end => snap_up(end + Duration::minutes(grid), grid),
                        _ => slot.start,
                    };
                    (start + Duration::minutes(need) <= slot.end).then_some((i, start))
                });
                let placed: Option<Vec<(usize, NaiveDateTime, NaiveDateTime)>> =
                    // 1. Whole, in the earliest gap that holds it.
                    if let Some((i, start)) = whole {
                        Some(vec![(i, start, start + Duration::minutes(need))])
                    // 2. Split across today's gaps, finishing today.
                    } else if let Some(chunks) =
                        (spare > 1).then(|| split_across(&slots, need, min_block, grid, spare, false)).flatten()
                    {
                        Some(chunks)
                    // 3. As much as fits today, the rest carries on — only
                    //    worthwhile with a day left, a deadline not yet
                    //    reached, and a piece spare for the remainder.
                    } else if spare > 1 && !is_last_day && p.task.due.is_none_or(|due| window.day < due) {
                        split_across(&slots, need, min_block, grid, spare - 1, true)
                    } else {
                        None
                    };

                let Some(chunks) = placed else { break };
                let mut taken = 0;
                for (_, start, end) in &chunks {
                    p.pieces.push(NewBlock { task_id: p.task.id, start: *start, end: *end, chunk_index: 0 });
                    taken += (*end - *start).num_minutes();
                }
                last_end = chunks.last().map(|(_, _, end)| *end);
                for (slot_index, _, end) in chunks.iter().rev() {
                    consume(&mut slots, *slot_index, *end);
                }
                p.remaining -= taken;
                if taken >= need {
                    p.queue.pop_front();
                } else {
                    // Only part of this piece fit today: the rest carries to the next day.
                    if let Some(front) = p.queue.front_mut() {
                        *front = need - taken;
                    }
                    break;
                }
            }
        }
    }

    let mut result = HorizonResult::default();
    for p in &mut progress {
        if p.failed || p.remaining > 0 {
            result.unplaced.push((p.task.id, p.task.assigned));
        } else {
            p.pieces.sort_by_key(|b| b.start);
            for (n, piece) in p.pieces.iter_mut().enumerate() {
                piece.chunk_index = n;
            }
            result.blocks.extend(p.pieces.iter().copied());
        }
    }
    // Stable: tasks were walked in priority order, so ties keep it.
    result.unplaced.sort_by_key(|(_, assigned)| *assigned);
    result.blocks.sort_by_key(|b| (b.start, b.task_id, b.chunk_index));
    result
}

// ---------------------------------------------------------------------
// Hand placement: a task dropped onto the Day timeline
// ---------------------------------------------------------------------

/// Furthest, in minutes, a drop may be nudged to reach free time. A drop
/// on an event moves to the nearest free edge, but not across the day:
/// "I dropped it here" should never land hours away.
pub const DROP_NUDGE_LIMIT_MINUTES: i64 = 60;

/// Where a task dropped onto the timeline ends up, and how much of what
/// it needs the space holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DropPlacement {
    pub start: NaiveDateTime,
    pub end: NaiveDateTime,
    /// The time the task needs (its remaining estimate, rounded up to
    /// the grid). `end - start` is this, or less when the space ran out.
    pub wanted: i64,
}

impl DropPlacement {
    pub fn minutes(&self) -> i64 {
        (self.end - self.start).num_minutes().max(0)
    }

    /// The space held less than the task needs, so it is assumed not to
    /// be finished here and its remainder is left for another slot.
    pub fn is_short(&self) -> bool {
        self.minutes() < self.wanted
    }
}

/// Feature request: dragging a task from "Tasks for this day" onto the
/// timeline. The task takes the time it needs, starting where it was
/// dropped (snapped to the grid):
///
/// * it fits: the block is its full length;
/// * the next event or block (or the end of the window) is in the way:
///   it takes the free time up to that, *assuming it will not be
///   completed in this slot*; the caller carries the rest forward;
/// * the drop is on top of something, or too little room is left after
///   it: it moves to the nearest free time (at most
///   [`DROP_NUDGE_LIMIT_MINUTES`] away), filling back from the end of
///   the gap when only a sliver follows the drop point;
/// * no free time that is at least a minimum block (or, for a task
///   shorter than that, the task itself) is near enough: `None`.
///
/// `cfg.buffer` is ignored: the person chose this spot, so the planner's
/// breathing room doesn't override them. Everything is on the grid.
pub fn place_dropped(
    drop_at: NaiveDateTime,
    wanted_minutes: i64,
    window: &Interval,
    busy: &[Interval],
    cfg: &BlockConfig,
) -> Option<DropPlacement> {
    if wanted_minutes <= 0 || window.is_empty() {
        return None;
    }
    let grid = cfg.grid();
    let wanted = round_up_minutes(wanted_minutes, grid);
    let hand = BlockConfig { buffer: 0, min_block: cfg.effective_min_block().min(wanted), ..*cfg };
    let need_min = hand.effective_min_block();
    let slots = free_slots(window, busy, &hand);

    // The nearest grid line to where the pointer was.
    let at = snap_down(drop_at + Duration::minutes(grid / 2), grid);
    let distance = |s: &Interval| {
        if at < s.start {
            s.start - at
        } else if at >= s.end {
            at - s.end
        } else {
            Duration::zero()
        }
    };
    let slot = slots.iter().min_by_key(|s| (distance(s), s.start))?;
    if distance(slot) > Duration::minutes(DROP_NUDGE_LIMIT_MINUTES) {
        return None;
    }

    let mut start = at.clamp(slot.start, slot.end);
    if (slot.end - start).num_minutes() < need_min {
        // Only a sliver follows the drop point: use the gap's end instead.
        start = (slot.end - Duration::minutes(wanted)).max(slot.start);
    }
    let room = (slot.end - start).num_minutes();
    Some(DropPlacement { start, end: start + Duration::minutes(room.min(wanted)), wanted })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 10, 14).unwrap()
    }

    /// `h:m` on the test day.
    fn at(h: u32, m: u32) -> NaiveDateTime {
        day().and_hms_opt(h, m, 0).unwrap()
    }

    fn iv(h1: u32, m1: u32, h2: u32, m2: u32) -> Interval {
        Interval::new(at(h1, m1), at(h2, m2))
    }

    /// 09:00–21:00, the default working window.
    fn window() -> DayWindow {
        DayWindow::from_minutes(day(), 540, 1260)
    }

    /// A "now" safely before the window, i.e. planning a future day.
    fn early() -> NaiveDateTime {
        day().pred_opt().unwrap().and_hms_opt(12, 0, 0).unwrap()
    }

    fn task(id: i64, minutes: i64) -> BlockTask {
        BlockTask { id, minutes, due: None, position: id, preferred_start: None }
    }

    /// The single-day planner's tests are about placement, not splitting,
    /// so they run with splitting off regardless of the Settings default.
    fn cfg() -> BlockConfig {
        BlockConfig { max_chunks: 1, ..BlockConfig::default() }
    }

    fn split_cfg() -> BlockConfig {
        BlockConfig { max_chunks: 3, ..BlockConfig::default() }
    }

    // --- grid helpers -------------------------------------------------

    #[test]
    fn snap_up_and_down_round_to_the_grid() {
        assert_eq!(snap_up(at(9, 1), 15), at(9, 15));
        assert_eq!(snap_up(at(9, 15), 15), at(9, 15));
        assert_eq!(snap_down(at(9, 14), 15), at(9, 0));
        assert_eq!(snap_down(at(9, 15), 15), at(9, 15));
        assert_eq!(snap_up(at(9, 1), 5), at(9, 5));
    }

    #[test]
    fn snap_up_treats_a_sub_second_remainder_as_past_the_boundary() {
        let dt = day().and_hms_nano_opt(9, 15, 0, 1).unwrap();
        assert_eq!(snap_up(dt, 15), at(9, 30));
    }

    #[test]
    fn a_zero_or_negative_granularity_is_treated_as_one_minute() {
        assert_eq!(snap_up(at(9, 1), 0), at(9, 1));
        assert_eq!(snap_down(at(9, 1), -5), at(9, 1));
        assert_eq!(BlockConfig { granularity: 0, ..cfg() }.effective_min_block(), 25);
    }

    #[test]
    fn effective_min_block_rounds_up_to_the_grid() {
        assert_eq!(cfg().effective_min_block(), 30); // 25 on a 15 grid
        assert_eq!(BlockConfig { min_block: 30, ..cfg() }.effective_min_block(), 30);
    }

    // --- merge_intervals ----------------------------------------------

    #[test]
    fn merge_sorts_and_coalesces_overlapping_and_adjacent_intervals() {
        let merged = merge_intervals(vec![iv(12, 0, 13, 0), iv(9, 0, 10, 0), iv(9, 30, 11, 0), iv(11, 0, 11, 30)]);
        assert_eq!(merged, vec![iv(9, 0, 11, 30), iv(12, 0, 13, 0)]);
    }

    #[test]
    fn merge_drops_empty_and_inverted_intervals_and_keeps_a_contained_one_inside() {
        let merged = merge_intervals(vec![iv(10, 0, 10, 0), iv(11, 0, 10, 0), iv(9, 0, 12, 0), iv(10, 0, 11, 0)]);
        assert_eq!(merged, vec![iv(9, 0, 12, 0)]);
    }

    #[test]
    fn merge_of_nothing_is_nothing() {
        assert!(merge_intervals(Vec::new()).is_empty());
    }

    // --- inflate ------------------------------------------------------

    #[test]
    fn inflate_pads_both_sides_and_clamps_to_the_window() {
        let w = window().interval();
        assert_eq!(inflate(iv(12, 0, 13, 0), 10, &w), iv(11, 50, 13, 10));
        assert_eq!(inflate(iv(9, 0, 10, 0), 30, &w), iv(9, 0, 10, 30));
        assert_eq!(inflate(iv(20, 30, 21, 0), 30, &w), iv(20, 0, 21, 0));
    }

    // --- free_slots ---------------------------------------------------

    #[test]
    fn free_slots_of_an_empty_day_is_the_whole_window() {
        assert_eq!(free_slots(&window().interval(), &[], &cfg()), vec![iv(9, 0, 21, 0)]);
    }

    #[test]
    fn free_slots_removes_events_and_snaps_inward() {
        // Event 10:07–11:50 leaves 09:00–10:07 (-> 09:00–10:00) and 11:50–21:00 (-> 12:00–21:00).
        let slots = free_slots(&window().interval(), &[iv(10, 7, 11, 50)], &cfg());
        assert_eq!(slots, vec![iv(9, 0, 10, 0), iv(12, 0, 21, 0)]);
    }

    #[test]
    fn free_slots_drops_gaps_shorter_than_the_minimum_block() {
        // 20-minute gap between two events is below the 30-minute effective minimum.
        let slots = free_slots(&window().interval(), &[iv(9, 0, 10, 0), iv(10, 20, 11, 0)], &cfg());
        assert_eq!(slots, vec![iv(11, 0, 21, 0)]);
    }

    #[test]
    fn free_slots_applies_the_buffer_around_events() {
        let c = BlockConfig { buffer: 15, ..cfg() };
        let slots = free_slots(&window().interval(), &[iv(12, 0, 13, 0)], &c);
        assert_eq!(slots, vec![iv(9, 0, 11, 45), iv(13, 15, 21, 0)]);
    }

    #[test]
    fn free_slots_ignores_events_outside_the_window() {
        let slots = free_slots(&window().interval(), &[iv(6, 0, 7, 0), iv(22, 0, 23, 0)], &cfg());
        assert_eq!(slots, vec![iv(9, 0, 21, 0)]);
    }

    #[test]
    fn free_slots_handles_an_event_straddling_the_window_edges() {
        // A midnight-crossing overflow from the previous night, and one running past closing.
        let slots = free_slots(&window().interval(), &[iv(0, 0, 9, 30), iv(20, 0, 23, 0)], &cfg());
        assert_eq!(slots, vec![iv(9, 30, 20, 0)]);
    }

    #[test]
    fn free_slots_of_a_fully_booked_or_empty_window_is_empty() {
        assert!(free_slots(&window().interval(), &[iv(8, 0, 22, 0)], &cfg()).is_empty());
        assert!(free_slots(&Interval::new(at(10, 0), at(10, 0)), &[], &cfg()).is_empty());
    }

    #[test]
    fn day_capacity_sums_the_free_minutes() {
        assert_eq!(day_capacity(&window().interval(), &[], &cfg()), 12 * 60);
        assert_eq!(day_capacity(&window().interval(), &[iv(12, 0, 13, 0)], &cfg()), 11 * 60);
    }

    // --- earliest_start -----------------------------------------------

    #[test]
    fn earliest_start_is_the_window_start_for_a_future_day() {
        assert_eq!(earliest_start(early(), &window(), &cfg()), at(9, 0));
    }

    #[test]
    fn earliest_start_rounds_now_up_when_it_is_inside_the_window() {
        assert_eq!(earliest_start(at(10, 1), &window(), &cfg()), at(10, 15));
        assert_eq!(earliest_start(at(10, 15), &window(), &cfg()), at(10, 15));
    }

    #[test]
    fn earliest_start_is_past_the_window_end_once_the_day_is_over() {
        assert!(earliest_start(at(22, 0), &window(), &cfg()) > window().end);
    }

    // --- place_day ----------------------------------------------------

    #[test]
    fn place_day_puts_tasks_back_to_back_from_the_window_start() {
        let r = place_day(&window(), &[], &[task(1, 60), task(2, 30)], &cfg(), early());
        assert_eq!(
            r.blocks,
            vec![
                NewBlock { task_id: 1, start: at(9, 0), end: at(10, 0), chunk_index: 0 },
                NewBlock { task_id: 2, start: at(10, 0), end: at(10, 30), chunk_index: 0 },
            ]
        );
        assert!(r.unplaced.is_empty());
    }

    #[test]
    fn place_day_routes_around_events() {
        let r = place_day(&window(), &[iv(9, 0, 11, 0)], &[task(1, 60)], &cfg(), early());
        assert_eq!(r.blocks[0].start, at(11, 0));
    }

    #[test]
    fn place_day_rounds_a_task_up_to_the_grid() {
        let r = place_day(&window(), &[], &[task(1, 20)], &cfg(), early());
        assert_eq!(r.blocks[0].end, at(9, 30));
    }

    #[test]
    fn place_day_orders_by_deadline_then_position_then_id() {
        let d = |n: u32| NaiveDate::from_ymd_opt(2026, 10, n);
        let tasks = [
            BlockTask { id: 1, minutes: 30, due: None, position: 1, preferred_start: None },
            BlockTask { id: 2, minutes: 30, due: d(20), position: 5, preferred_start: None },
            BlockTask { id: 3, minutes: 30, due: d(16), position: 9, preferred_start: None },
            BlockTask { id: 4, minutes: 30, due: d(16), position: 2, preferred_start: None },
        ];
        let r = place_day(&window(), &[], &tasks, &cfg(), early());
        let order: Vec<i64> = r.blocks.iter().map(|b| b.task_id).collect();
        assert_eq!(order, vec![4, 3, 2, 1]);
    }

    #[test]
    fn place_day_reports_a_task_that_does_not_fit_and_never_places_it_partially() {
        let r = place_day(&window(), &[iv(9, 0, 20, 0)], &[task(1, 120)], &cfg(), early());
        assert!(r.blocks.is_empty());
        assert_eq!(r.unplaced, vec![1]);
    }

    #[test]
    fn place_day_skips_a_slot_too_small_and_uses_a_later_one_first_fit() {
        // Gaps: 09:00–10:00 (60) and 12:00–21:00. A 90-minute task skips the first.
        let r = place_day(&window(), &[iv(10, 0, 12, 0)], &[task(1, 90)], &cfg(), early());
        assert_eq!(r.blocks[0].start, at(12, 0));
    }

    #[test]
    fn place_day_treats_a_non_positive_estimate_as_unplaceable() {
        let r = place_day(&window(), &[], &[task(1, 0), task(2, -5)], &cfg(), early());
        assert!(r.blocks.is_empty());
        assert_eq!(r.unplaced, vec![1, 2]);
    }

    #[test]
    fn place_day_never_plans_in_the_past_today() {
        let r = place_day(&window(), &[], &[task(1, 60)], &cfg(), at(13, 1));
        assert_eq!(r.blocks[0].start, at(13, 15));
    }

    #[test]
    fn place_day_places_nothing_once_the_working_day_is_over() {
        let r = place_day(&window(), &[], &[task(1, 30)], &cfg(), at(21, 30));
        assert!(r.blocks.is_empty());
        assert_eq!(r.unplaced, vec![1]);
    }

    #[test]
    fn place_day_with_no_tasks_is_empty() {
        assert_eq!(place_day(&window(), &[], &[], &cfg(), early()), DayResult::default());
    }

    // --- splitting (max_chunks > 1) -------------------------------------

    #[test]
    fn a_max_chunks_of_one_never_splits() {
        // 09:00–10:00 and 11:00–12:00 free, nothing else.
        let busy = [iv(10, 0, 11, 0), iv(12, 0, 21, 0)];
        let r = place_day(&window(), &busy, &[task(1, 120)], &cfg(), early());
        assert_eq!(r.unplaced, vec![1]);
    }

    #[test]
    fn a_task_is_split_across_slots_when_allowed_and_chunks_are_numbered_in_time_order() {
        let busy = [iv(10, 0, 11, 0), iv(12, 0, 21, 0)];
        let r = place_day(&window(), &busy, &[task(1, 120)], &split_cfg(), early());
        assert!(r.unplaced.is_empty());
        assert_eq!(
            r.blocks,
            vec![
                NewBlock { task_id: 1, start: at(9, 0), end: at(10, 0), chunk_index: 0 },
                NewBlock { task_id: 1, start: at(11, 0), end: at(12, 0), chunk_index: 1 },
            ]
        );
    }

    #[test]
    fn a_whole_fit_is_preferred_over_a_split() {
        // A 60-minute task fits whole in the later, larger slot; no split even though allowed.
        let busy = [iv(9, 30, 10, 0)];
        let r = place_day(&window(), &busy, &[task(1, 60)], &split_cfg(), early());
        assert_eq!(r.blocks.len(), 1);
        assert_eq!(r.blocks[0].start, at(10, 0));
    }

    #[test]
    fn a_split_is_all_or_nothing_and_rolls_back_when_it_cannot_finish() {
        // Only 90 free minutes in total; a 120-minute task must not be partially placed.
        let busy = [iv(10, 0, 11, 0), iv(12, 0, 21, 0)];
        let r = place_day(&window(), &busy, &[task(1, 150)], &split_cfg(), early());
        assert!(r.blocks.is_empty());
        assert_eq!(r.unplaced, vec![1]);
    }

    #[test]
    fn a_split_respects_max_chunks() {
        // Three 60-minute gaps, a 180-minute task, but only 2 chunks allowed.
        let busy = [iv(10, 0, 11, 0), iv(12, 0, 13, 0), iv(14, 0, 21, 0)];
        let two = BlockConfig { max_chunks: 2, ..cfg() };
        assert_eq!(place_day(&window(), &busy, &[task(1, 180)], &two, early()).unplaced, vec![1]);
        let three = BlockConfig { max_chunks: 3, ..cfg() };
        assert!(place_day(&window(), &busy, &[task(1, 180)], &three, early()).unplaced.is_empty());
    }

    #[test]
    fn a_split_never_leaves_a_final_chunk_below_the_minimum() {
        // Slots of 60 and 60 and a 75-minute task: greedily taking 60 would
        // leave a 15-minute final chunk, below the 30-minute minimum, so the
        // first chunk is trimmed to 45 and the second is 30.
        let busy = [iv(10, 0, 11, 0), iv(12, 0, 21, 0)];
        let r = place_day(&window(), &busy, &[task(1, 75)], &split_cfg(), early());
        assert!(r.unplaced.is_empty());
        let lens: Vec<i64> = r.blocks.iter().map(|b| b.interval().minutes()).collect();
        assert_eq!(lens, vec![45, 30]);
        assert!(lens.iter().all(|&m| m >= cfg().effective_min_block()));
    }

    // --- find_conflicts -----------------------------------------------

    #[test]
    fn find_conflicts_flags_only_blocks_that_really_overlap_an_event() {
        let blocks = [(1, iv(9, 0, 10, 0)), (2, iv(10, 0, 11, 0)), (3, iv(10, 30, 11, 30))];
        // Event 10:30–11:00 overlaps blocks 2 and 3; block 1 ends before it starts.
        let conflicts = find_conflicts(&blocks, &[iv(10, 30, 11, 0)]);
        assert_eq!(conflicts, vec![2, 3]);
        assert!(find_conflicts(&blocks, &[iv(10, 0, 10, 0)]).is_empty());
        assert!(find_conflicts(&blocks, &[]).is_empty());
    }

    #[test]
    fn touching_intervals_do_not_overlap() {
        assert!(!iv(9, 0, 10, 0).overlaps(&iv(10, 0, 11, 0)));
        assert!(iv(9, 0, 10, 1).overlaps(&iv(10, 0, 11, 0)));
    }

    // --- DST boundary -------------------------------------------------

    #[test]
    fn naive_to_local_round_trips_an_ordinary_time() {
        use chrono::Timelike;
        let local = naive_to_local(at(14, 30));
        assert_eq!(local.hour(), 14);
        assert_eq!(local.minute(), 30);
        assert_eq!(local.date_naive(), day());
    }

    // --- invariants (hand-rolled property loops, like `pager`) ----------

    /// Tiny deterministic generator so the property loops need no crate.
    struct Lcg(u64);
    impl Lcg {
        fn next(&mut self) -> u64 {
            self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            self.0 >> 33
        }
        fn range(&mut self, lo: u64, hi: u64) -> u64 {
            lo + self.next() % (hi - lo + 1)
        }
    }

    fn random_case(rng: &mut Lcg) -> (Vec<Interval>, Vec<BlockTask>, BlockConfig, NaiveDateTime) {
        let grid = [5, 15][rng.range(0, 1) as usize];
        let cfg = BlockConfig {
            granularity: grid,
            min_block: [15, 25, 30][rng.range(0, 2) as usize],
            buffer: [0, 0, 5, 10, 15][rng.range(0, 4) as usize],
            max_chunks: rng.range(1, 3) as usize,
            ..BlockConfig::default()
        };
        let mut busy = Vec::new();
        for _ in 0..rng.range(0, 6) {
            // Anywhere in the day, odd minutes, sometimes crossing the window edges.
            let start = rng.range(0, 23 * 60) as i64;
            let len = rng.range(1, 240) as i64;
            busy.push(Interval::new(
                day().and_time(NaiveTime::MIN) + Duration::minutes(start),
                day().and_time(NaiveTime::MIN) + Duration::minutes(start + len),
            ));
        }
        let tasks = (0..rng.range(0, 8))
            .map(|i| BlockTask {
                id: i as i64 + 1,
                minutes: rng.range(5, 200) as i64,
                due: if rng.range(0, 1) == 0 { None } else { NaiveDate::from_ymd_opt(2026, 10, rng.range(14, 28) as u32) },
                position: rng.range(0, 20) as i64,
                preferred_start: None,
            })
            .collect();
        let now = if rng.range(0, 2) == 0 { early() } else { at(rng.range(6, 22) as u32, rng.range(0, 59) as u32) };
        (busy, tasks, cfg, now)
    }

    #[test]
    fn invariants_hold_over_many_random_days() {
        let mut rng = Lcg(0x5EED);
        for case in 0..500 {
            let (busy, tasks, cfg, now) = random_case(&mut rng);
            let w = window();
            let r = place_day(&w, &busy, &tasks, &cfg, now);
            let ctx = format!("case {case}: cfg={cfg:?} now={now} busy={busy:?} tasks={tasks:?} result={r:?}");

            let floor = earliest_start(now, &w, &cfg);
            for b in &r.blocks {
                // Inside the window, on the grid, not in the past.
                assert!(b.start >= w.start && b.end <= w.end, "outside window — {ctx}");
                assert!(b.start >= floor, "starts before earliest start — {ctx}");
                assert_eq!(snap_up(b.start, cfg.granularity), b.start, "start off-grid — {ctx}");
                assert_eq!(snap_down(b.end, cfg.granularity), b.end, "end off-grid — {ctx}");
                // Never overlaps an event, even buffered.
                for e in &busy {
                    let padded = Interval::new(e.start - Duration::minutes(cfg.buffer), e.end + Duration::minutes(cfg.buffer));
                    assert!(!b.interval().overlaps(&padded), "overlaps busy — {ctx}");
                }
            }
            // Never overlaps another block.
            for (i, a) in r.blocks.iter().enumerate() {
                for c in &r.blocks[i + 1..] {
                    assert!(!a.interval().overlaps(&c.interval()), "blocks overlap — {ctx}");
                }
            }
            // All-or-nothing per task, with the rounded-up length.
            for t in &tasks {
                let placed: i64 = r.blocks.iter().filter(|b| b.task_id == t.id).map(|b| b.interval().minutes()).sum();
                let unplaced = r.unplaced.contains(&t.id);
                if unplaced {
                    assert_eq!(placed, 0, "partially placed — {ctx}");
                } else {
                    assert_eq!(placed, round_up_minutes(t.minutes, cfg.granularity), "wrong total — {ctx}");
                }
                let chunks = r.blocks.iter().filter(|b| b.task_id == t.id).count();
                assert!(chunks <= cfg.max_chunks, "too many chunks — {ctx}");
            }
            // Every task is accounted for exactly once.
            assert_eq!(
                r.unplaced.len() + tasks.iter().filter(|t| r.blocks.iter().any(|b| b.task_id == t.id)).count(),
                tasks.len(),
                "task lost or duplicated — {ctx}"
            );
        }
    }

    #[test]
    fn output_is_independent_of_input_order() {
        let mut rng = Lcg(0xBEEF);
        for case in 0..200 {
            let (busy, tasks, cfg, now) = random_case(&mut rng);
            let forward = place_day(&window(), &busy, &tasks, &cfg, now);

            let mut reversed_tasks = tasks.clone();
            reversed_tasks.reverse();
            let mut reversed_busy = busy.clone();
            reversed_busy.reverse();
            let backward = place_day(&window(), &reversed_busy, &reversed_tasks, &cfg, now);

            assert_eq!(forward, backward, "case {case}: order changed the plan");
        }
    }

    #[test]
    fn re_planning_with_the_result_as_busy_places_nothing_new() {
        let mut rng = Lcg(0xFACE);
        for case in 0..200 {
            let (busy, tasks, cfg, now) = random_case(&mut rng);
            let first = place_day(&window(), &busy, &tasks, &cfg, now);

            let mut busy_with_blocks = busy.clone();
            busy_with_blocks.extend(first.blocks.iter().map(NewBlock::interval));
            let placed_ids: Vec<i64> = first.blocks.iter().map(|b| b.task_id).collect();
            let remaining: Vec<BlockTask> = tasks.iter().copied().filter(|t| !placed_ids.contains(&t.id)).collect();

            let second = place_day(&window(), &busy_with_blocks, &remaining, &cfg, now);
            // Whatever did not fit the first time still does not fit around the placed blocks.
            assert!(second.blocks.is_empty(), "case {case}: second pass placed {:?}", second.blocks);
        }
    }

    // --- plan_horizon (multi-day, split as necessary) ------------------------

    /// `n` consecutive working windows starting on the test day.
    fn windows(n: i64) -> Vec<DayWindow> {
        (0..n).map(|i| DayWindow::from_minutes(day() + Duration::days(i), 540, 1260)).collect()
    }

    /// `h:m` on day `offset` after the test day.
    fn on(offset: i64, h: u32, m: u32) -> NaiveDateTime {
        (day() + Duration::days(offset)).and_hms_opt(h, m, 0).unwrap()
    }

    fn ivd(offset: i64, h1: u32, m1: u32, h2: u32, m2: u32) -> Interval {
        Interval::new(on(offset, h1, m1), on(offset, h2, m2))
    }

    fn htask(id: i64, minutes: i64, assigned: i64, due: Option<i64>) -> HorizonTask {
        HorizonTask {
            id,
            minutes,
            assigned: day() + Duration::days(assigned),
            due: due.map(|d| day() + Duration::days(d)),
            position: id,
            locked_chunks: 0,
        }
    }

    fn splitting(max_chunks: usize) -> BlockConfig {
        // These tests are about where pieces *fit*; the split-over-a-threshold
        // behaviour has its own tests (`piece_sizes`, "long tasks").
        BlockConfig { max_chunks, split_over: 100_000, ..BlockConfig::default() }
    }

    #[test]
    fn horizon_places_a_task_whole_on_its_assigned_day() {
        let r = plan_horizon(&windows(3), &[], &[htask(1, 60, 1, None)], &splitting(4), early());
        assert_eq!(r.blocks, vec![NewBlock { task_id: 1, start: on(1, 9, 0), end: on(1, 10, 0), chunk_index: 0 }]);
        assert!(r.unplaced.is_empty());
    }

    #[test]
    fn horizon_never_places_a_task_before_its_assigned_day() {
        let r = plan_horizon(&windows(3), &[], &[htask(1, 60, 2, None)], &splitting(4), early());
        assert_eq!(r.blocks[0].start, on(2, 9, 0));
    }

    #[test]
    fn horizon_carries_the_rest_of_a_task_to_the_next_day() {
        // Day 0 has only a 30-minute and a 60-minute gap; the 120-minute task
        // takes both and finishes the last 30 minutes on day 1.
        let busy = [ivd(0, 9, 30, 20, 0)];
        let r = plan_horizon(&windows(3), &busy, &[htask(1, 120, 0, None)], &splitting(4), early());
        assert!(r.unplaced.is_empty());
        assert_eq!(
            r.blocks,
            vec![
                NewBlock { task_id: 1, start: on(0, 9, 0), end: on(0, 9, 30), chunk_index: 0 },
                NewBlock { task_id: 1, start: on(0, 20, 0), end: on(0, 21, 0), chunk_index: 1 },
                NewBlock { task_id: 1, start: on(1, 9, 0), end: on(1, 9, 30), chunk_index: 2 },
            ]
        );
    }

    #[test]
    fn a_big_task_becomes_several_sessions_across_days() {
        // The "maths assignment as four 40-minute sessions" shape: each day
        // has one 45-minute gap, so a 160-minute task needs four days.
        let busy: Vec<Interval> = (0..5).map(|d| ivd(d, 9, 45, 21, 0)).collect();
        let r = plan_horizon(&windows(5), &busy, &[htask(1, 160, 0, None)], &splitting(6), early());
        assert!(r.unplaced.is_empty(), "{r:?}");
        let per_day: Vec<i64> =
            (0..4).map(|d| r.blocks.iter().filter(|b| b.start.date() == on(d, 0, 0).date()).map(|b| b.interval().minutes()).sum()).collect();
        // 160 minutes rounds up to 165 on the 15-minute grid: 45 + 45 + 45 + 30.
        assert_eq!(per_day.iter().sum::<i64>(), 165);
        assert!(per_day.iter().all(|&m| m > 0 && m <= 45), "{per_day:?}");
    }

    #[test]
    fn horizon_stops_carrying_at_the_deadline_and_drops_every_piece() {
        let busy = [ivd(0, 9, 30, 20, 0), ivd(1, 9, 0, 21, 0)];
        // Due on day 1, which is fully booked: the 30 minutes that would carry cannot be placed.
        let r = plan_horizon(&windows(3), &busy, &[htask(1, 120, 0, Some(1))], &splitting(4), early());
        assert!(r.blocks.is_empty(), "all-or-nothing: no half-planned task");
        assert_eq!(r.unplaced, vec![(1, day())]);
    }

    #[test]
    fn horizon_gives_up_when_the_last_day_cannot_finish_the_task() {
        let busy = [ivd(0, 9, 30, 20, 0)];
        let r = plan_horizon(&windows(1), &busy, &[htask(1, 120, 0, None)], &splitting(4), early());
        assert!(r.blocks.is_empty());
        assert_eq!(r.unplaced.len(), 1);
    }

    #[test]
    fn with_splitting_off_a_task_that_does_not_fit_its_day_is_unplaced_not_moved() {
        let busy = [ivd(0, 9, 0, 20, 0)];
        let r = plan_horizon(&windows(3), &busy, &[htask(1, 120, 0, None)], &splitting(1), early());
        assert!(r.blocks.is_empty());
        assert_eq!(r.unplaced, vec![(1, day())]);
    }

    #[test]
    fn pinned_pieces_count_against_the_piece_budget() {
        // Budget 4 with 3 already pinned leaves one piece: it must go whole.
        let busy = [ivd(0, 9, 30, 20, 0)];
        let mut t = htask(1, 90, 0, None);
        t.locked_chunks = 3;
        let r = plan_horizon(&windows(3), &busy, &[t], &splitting(4), early());
        assert_eq!(r.blocks.len(), 1, "{r:?}");
        assert_eq!(r.blocks[0].start, on(1, 9, 0), "no gap on day 0 holds 90 whole, so it moves on");
    }

    #[test]
    fn a_task_with_nothing_left_to_place_is_unplaced_not_an_empty_success() {
        let r = plan_horizon(&windows(1), &[], &[htask(1, 0, 0, None)], &splitting(4), early());
        assert!(r.blocks.is_empty());
        assert_eq!(r.unplaced.len(), 1);
    }

    #[test]
    fn urgent_tasks_get_the_earlier_gaps() {
        let relaxed = htask(1, 60, 0, None);
        let urgent = htask(2, 60, 0, Some(0));
        let r = plan_horizon(&windows(2), &[], &[relaxed, urgent], &splitting(4), early());
        let start_of = |id: i64| r.blocks.iter().find(|b| b.task_id == id).unwrap().start;
        assert!(start_of(2) < start_of(1));
    }

    fn horizon_case(rng: &mut Lcg) -> (Vec<Interval>, Vec<HorizonTask>, BlockConfig, NaiveDateTime) {
        let cfg = BlockConfig {
            granularity: [5, 15][rng.range(0, 1) as usize],
            min_block: [15, 25, 30][rng.range(0, 2) as usize],
            buffer: [0, 0, 10][rng.range(0, 2) as usize],
            max_chunks: rng.range(1, 6) as usize,
            split_over: [10, 30, 60, 100_000][rng.range(0, 3) as usize],
            max_block: [30, 60, 120][rng.range(0, 2) as usize],
        };
        let mut busy = Vec::new();
        for _ in 0..rng.range(0, 8) {
            let d = rng.range(0, 3) as i64;
            let start = rng.range(8 * 60, 21 * 60) as i64;
            let len = rng.range(10, 300) as i64;
            let base = on(d, 0, 0);
            busy.push(Interval::new(base + Duration::minutes(start), base + Duration::minutes(start + len)));
        }
        let tasks = (0..rng.range(0, 7))
            .map(|i| {
                let assigned = rng.range(0, 2) as i64;
                HorizonTask {
                    id: i as i64 + 1,
                    minutes: rng.range(10, 400) as i64,
                    assigned: day() + Duration::days(assigned),
                    due: if rng.range(0, 1) == 0 { None } else { Some(day() + Duration::days(rng.range(0, 4) as i64)) },
                    position: rng.range(0, 20) as i64,
                    locked_chunks: rng.range(0, 1) as usize,
                }
            })
            .collect();
        (busy, tasks, cfg, early())
    }

    #[test]
    fn horizon_invariants_hold_over_many_random_plans() {
        let mut rng = Lcg(0xA11CE);
        for case in 0..400 {
            let (busy, tasks, cfg, now) = horizon_case(&mut rng);
            let ws = windows(4);
            let r = plan_horizon(&ws, &busy, &tasks, &cfg, now);
            let ctx = format!("case {case}: cfg={cfg:?} busy={busy:?} tasks={tasks:?} result={r:?}");

            for b in &r.blocks {
                let window = ws.iter().find(|w| w.day == b.start.date()).unwrap_or_else(|| panic!("no window — {ctx}"));
                assert!(b.start >= window.start && b.end <= window.end, "outside window — {ctx}");
                assert_eq!(snap_up(b.start, cfg.granularity), b.start, "start off-grid — {ctx}");
                assert_eq!(snap_down(b.end, cfg.granularity), b.end, "end off-grid — {ctx}");
                for e in &busy {
                    let padded = Interval::new(e.start - Duration::minutes(cfg.buffer), e.end + Duration::minutes(cfg.buffer));
                    assert!(!b.interval().overlaps(&padded), "overlaps busy — {ctx}");
                }
            }
            for (i, a) in r.blocks.iter().enumerate() {
                for c in &r.blocks[i + 1..] {
                    assert!(!a.interval().overlaps(&c.interval()), "blocks overlap — {ctx}");
                }
            }
            for t in &tasks {
                let mine: Vec<&NewBlock> = r.blocks.iter().filter(|b| b.task_id == t.id).collect();
                let total: i64 = mine.iter().map(|b| b.interval().minutes()).sum();
                if r.unplaced.iter().any(|(id, _)| *id == t.id) {
                    assert!(mine.is_empty(), "unplaced task kept pieces — {ctx}");
                } else {
                    assert_eq!(total, round_up_minutes(t.minutes, cfg.granularity), "wrong total — {ctx}");
                    assert!(mine.len() + t.locked_chunks <= cfg.max_chunks, "over the piece budget — {ctx}");
                    for b in &mine {
                        assert!(b.start.date() >= t.assigned, "before its assigned day — {ctx}");
                        if let Some(due) = t.due {
                            assert!(b.start.date() <= due, "after its deadline — {ctx}");
                        }
                    }
                    let mut indices: Vec<usize> = mine.iter().map(|b| b.chunk_index).collect();
                    indices.sort_unstable();
                    assert_eq!(indices, (0..mine.len()).collect::<Vec<_>>(), "chunk numbering — {ctx}");
                }
            }
            assert_eq!(
                r.unplaced.len() + tasks.iter().filter(|t| r.blocks.iter().any(|b| b.task_id == t.id)).count(),
                tasks.len(),
                "task lost or duplicated — {ctx}"
            );
        }
    }

    #[test]
    fn horizon_output_is_independent_of_input_order() {
        let mut rng = Lcg(0xD00D);
        for case in 0..200 {
            let (busy, tasks, cfg, now) = horizon_case(&mut rng);
            let forward = plan_horizon(&windows(4), &busy, &tasks, &cfg, now);
            let mut rt = tasks.clone();
            rt.reverse();
            let mut rb = busy.clone();
            rb.reverse();
            let mut rw = windows(4);
            rw.reverse();
            assert_eq!(forward, plan_horizon(&rw, &rb, &rt, &cfg, now), "case {case}");
        }
    }

    #[test]
    fn with_splitting_off_the_horizon_planner_matches_planning_each_day_on_its_own() {
        // The equivalence that keeps the old behaviour intact: max_chunks = 1
        // means no carrying, so days are independent and each is exactly `place_day`.
        let mut rng = Lcg(0xC0FFEE);
        for case in 0..300 {
            let (busy, tasks, mut cfg, now) = horizon_case(&mut rng);
            cfg.max_chunks = 1;
            // place_day has no deadline cut-off or pinned-piece budget, so compare like with like.
            let tasks: Vec<HorizonTask> =
                tasks.into_iter().map(|t| HorizonTask { due: None, locked_chunks: 0, ..t }).collect();
            let ws = windows(4);
            let horizon = plan_horizon(&ws, &busy, &tasks, &cfg, now);

            let mut expected_blocks = Vec::new();
            let mut expected_unplaced = Vec::new();
            for w in &ws {
                let day_tasks: Vec<BlockTask> = tasks
                    .iter()
                    .filter(|t| t.assigned == w.day)
                    .map(|t| BlockTask { id: t.id, minutes: t.minutes, due: None, position: t.position, preferred_start: None })
                    .collect();
                let r = place_day(w, &busy, &day_tasks, &cfg, now);
                expected_blocks.extend(r.blocks);
                expected_unplaced.extend(r.unplaced.into_iter().map(|id| (id, w.day)));
            }
            expected_blocks.sort_by_key(|b| (b.start, b.task_id, b.chunk_index));

            assert_eq!(horizon.blocks, expected_blocks, "case {case}: blocks differ");
            let mut got: Vec<_> = horizon.unplaced.clone();
            got.sort();
            expected_unplaced.sort();
            assert_eq!(got, expected_unplaced, "case {case}: unplaced differ");
        }
    }

    // --- WorkDay -----------------------------------------------------------------

    #[test]
    fn a_working_day_is_its_hours_and_a_day_off_is_an_empty_window_on_that_day() {
        let w = WorkDay::Hours(540, 1260).window(day());
        assert_eq!((w.start, w.end), (at(9, 0), at(21, 0)));

        let off = WorkDay::Off.window(day());
        assert_eq!(off.day, day());
        assert!(off.interval().is_empty());
        assert!(free_slots(&off.interval(), &[], &cfg()).is_empty(), "no room on a day off");
    }

    #[test]
    fn a_day_off_in_the_middle_makes_a_task_carry_over_it_when_splitting_is_on() {
        // Day 0 working, day 1 off, day 2 working. A 120-minute task with only 60 free on day 0.
        let ws = vec![WorkDay::Hours(540, 1260).window(day()), WorkDay::Off.window(day() + Duration::days(1)),
                      WorkDay::Hours(540, 1260).window(day() + Duration::days(2))];
        let busy = [ivd(0, 10, 0, 21, 0)];
        let r = plan_horizon(&ws, &busy, &[htask(1, 120, 0, None)], &splitting(4), early());
        assert!(r.unplaced.is_empty(), "{r:?}");
        let days: Vec<_> = r.blocks.iter().map(|b| b.start.date()).collect();
        assert!(!days.contains(&(day() + Duration::days(1))), "nothing on the day off: {days:?}");
        assert!(days.contains(&(day() + Duration::days(2))), "the rest lands on the next working day");
    }

    #[test]
    fn a_task_assigned_to_a_day_off_is_unplaced_when_splitting_is_off() {
        let ws = vec![WorkDay::Off.window(day()), WorkDay::Hours(540, 1260).window(day() + Duration::days(1))];
        let r = plan_horizon(&ws, &[], &[htask(1, 60, 0, None)], &splitting(1), early());
        assert!(r.blocks.is_empty());
        assert_eq!(r.unplaced, vec![(1, day())], "no work on a day off, and it isn't moved");
    }

    // --- place_dropped ------------------------------------------------

    fn day_window_interval() -> Interval {
        iv(9, 0, 21, 0)
    }

    fn drop(h: u32, m: u32, minutes: i64, busy: &[Interval]) -> Option<DropPlacement> {
        place_dropped(at(h, m), minutes, &day_window_interval(), busy, &BlockConfig::default())
    }

    #[test]
    fn a_drop_with_room_takes_the_tasks_full_length_from_where_it_landed() {
        let p = drop(14, 0, 60, &[]).expect("room");
        assert_eq!((p.start, p.end), (at(14, 0), at(15, 0)));
        assert!(!p.is_short());
    }

    #[test]
    fn a_drop_is_snapped_to_the_nearest_grid_line() {
        // On a 15-minute grid 14:08 is nearer 14:15, and 14:07 nearer 14:00.
        assert_eq!(drop(14, 8, 30, &[]).unwrap().start, at(14, 15));
        assert_eq!(drop(14, 7, 30, &[]).unwrap().start, at(14, 0));
    }

    #[test]
    fn a_length_off_the_grid_is_rounded_up_like_the_planner_does() {
        let p = drop(10, 0, 50, &[]).unwrap();
        assert_eq!(p.minutes(), 60);
        assert_eq!(p.wanted, 60);
    }

    #[test]
    fn the_next_event_cuts_the_block_short_and_marks_it_unfinished() {
        let busy = [iv(10, 30, 11, 30)];
        let p = drop(10, 0, 90, &busy).expect("room");
        assert_eq!((p.start, p.end), (at(10, 0), at(10, 30)));
        assert_eq!(p.wanted, 90);
        assert!(p.is_short());
    }

    #[test]
    fn the_end_of_the_window_also_cuts_a_block_short() {
        let p = drop(20, 0, 120, &[]).unwrap();
        assert_eq!((p.start, p.end), (at(20, 0), at(21, 0)));
        assert!(p.is_short());
    }

    #[test]
    fn a_drop_on_an_event_moves_to_the_nearest_free_edge() {
        let busy = [iv(10, 0, 11, 0)];
        // Near the event's start the nearest free time is before it: the block
        // ends where the event begins (filled back from the gap's end).
        let before = drop(10, 15, 30, &busy).unwrap();
        assert_eq!((before.start, before.end), (at(9, 30), at(10, 0)));
        // Near its end the nearest free time is after it.
        assert_eq!(drop(10, 45, 30, &busy).unwrap().start, at(11, 0));
    }

    #[test]
    fn a_drop_too_far_inside_something_is_refused() {
        let busy = [iv(9, 0, 13, 0)];
        // The nearest free time is two hours away from 11:00.
        assert_eq!(drop(11, 0, 30, &busy), None);
    }

    #[test]
    fn only_a_sliver_after_the_drop_fills_back_from_the_end_of_the_gap() {
        let busy = [iv(11, 0, 12, 0)];
        // 10:45 leaves 15 min before the event (< the 25-minute minimum), so the
        // task takes the end of the gap instead: 60 min back from 11:00.
        let p = drop(10, 45, 60, &busy).unwrap();
        assert_eq!((p.start, p.end), (at(10, 0), at(11, 0)));
        assert!(!p.is_short());
    }

    #[test]
    fn a_gap_below_the_minimum_block_is_not_used() {
        let busy = [iv(9, 0, 10, 0), iv(10, 15, 21, 0)];
        assert_eq!(drop(10, 0, 60, &busy), None);
    }

    #[test]
    fn a_task_shorter_than_the_minimum_can_use_a_gap_of_its_own_length() {
        let busy = [iv(9, 0, 10, 0), iv(10, 15, 21, 0)];
        let p = drop(10, 0, 15, &busy).unwrap();
        assert_eq!((p.start, p.end), (at(10, 0), at(10, 15)));
    }

    #[test]
    fn the_buffer_setting_does_not_stop_a_hand_placed_block() {
        let cfg = BlockConfig { buffer: 15, ..BlockConfig::default() };
        let busy = [iv(10, 0, 11, 0)];
        let p = place_dropped(at(11, 0), 30, &day_window_interval(), &busy, &cfg).unwrap();
        assert_eq!(p.start, at(11, 0));
    }

    #[test]
    fn nothing_is_placed_for_no_time_or_no_window() {
        assert_eq!(drop(10, 0, 0, &[]), None);
        let empty = Interval::new(at(9, 0), at(9, 0));
        assert_eq!(place_dropped(at(10, 0), 30, &empty, &[], &BlockConfig::default()), None);
    }

    // --- long tasks become several blocks (split over a threshold) -------

    fn defaults() -> BlockConfig {
        BlockConfig::default()
    }

    #[test]
    fn a_task_up_to_the_threshold_is_one_block_rounded_up_to_the_grid() {
        assert_eq!(piece_sizes(30, 0, &defaults()), vec![30]);
        assert_eq!(piece_sizes(20, 0, &defaults()), vec![30]);
        assert_eq!(piece_sizes(0, 0, &defaults()), Vec::<i64>::new());
    }

    #[test]
    fn a_task_over_the_threshold_is_cut_into_the_fewest_pieces_within_the_longest_block() {
        assert_eq!(piece_sizes(60, 0, &defaults()), vec![30, 30], "at least two");
        assert_eq!(piece_sizes(90, 0, &defaults()), vec![45, 45]);
        assert_eq!(piece_sizes(120, 0, &defaults()), vec![60, 60]);
        assert_eq!(piece_sizes(150, 0, &defaults()), vec![60, 45, 45], "three pieces, on the grid");
    }

    #[test]
    fn pieces_are_shared_out_as_evenly_as_the_grid_allows_and_add_up() {
        let sizes = piece_sizes(105, 0, &defaults());
        assert_eq!(sizes, vec![60, 45]);
        for minutes in [31, 45, 50, 61, 75, 100, 133, 200, 300] {
            let sizes = piece_sizes(minutes, 0, &defaults());
            assert_eq!(sizes.iter().sum::<i64>(), minutes.div_euclid(15) * 15 + if minutes % 15 > 0 { 15 } else { 0 }, "{minutes}");
            assert!(sizes.iter().all(|m| m % 15 == 0), "{sizes:?}");
        }
    }

    #[test]
    fn no_piece_is_shorter_than_the_shortest_block_so_a_short_task_stays_whole() {
        // 45 minutes in two would be 30 + 15, and 15 is under the 25-minute (30 on the grid) minimum.
        assert_eq!(piece_sizes(45, 0, &defaults()), vec![45]);
        let sizes = piece_sizes(100, 0, &BlockConfig { min_block: 15, ..defaults() });
        assert!(sizes.iter().all(|m| *m >= 15));
    }

    #[test]
    fn the_longest_block_setting_changes_the_size_of_the_pieces() {
        let long = BlockConfig { max_block: 90, ..defaults() };
        assert_eq!(piece_sizes(180, 0, &long), vec![90, 90]);
        let short = BlockConfig { max_block: 30, ..defaults() };
        assert_eq!(piece_sizes(90, 0, &short), vec![30, 30, 30]);
    }

    #[test]
    fn a_longest_block_below_the_shortest_is_raised_to_it() {
        let cfg = BlockConfig { min_block: 40, max_block: 20, ..defaults() };
        assert_eq!(cfg.effective_max_block(), cfg.effective_min_block());
    }

    #[test]
    fn the_threshold_setting_decides_which_tasks_are_split() {
        let high = BlockConfig { split_over: 90, ..defaults() };
        assert_eq!(piece_sizes(90, 0, &high), vec![90], "not over 90");
        assert_eq!(piece_sizes(105, 0, &high), vec![60, 45]);
    }

    #[test]
    fn splitting_off_or_no_spare_pieces_leaves_the_task_whole() {
        assert_eq!(piece_sizes(120, 0, &BlockConfig { max_chunks: 1, ..defaults() }), vec![120]);
        // Five of the six allowed pieces are already pinned elsewhere: one left, so one piece.
        assert_eq!(piece_sizes(120, 5, &defaults()), vec![120]);
    }

    #[test]
    fn a_very_long_task_gets_longer_pieces_rather_than_too_many() {
        let sizes = piece_sizes(600, 0, &defaults());
        assert_eq!(sizes.len(), 6, "capped at max_chunks");
        assert_eq!(sizes.iter().sum::<i64>(), 600);
    }

    fn pieces_of(r: &HorizonResult, task: i64) -> Vec<(NaiveDateTime, NaiveDateTime)> {
        r.blocks.iter().filter(|b| b.task_id == task).map(|b| (b.start, b.end)).collect()
    }

    #[test]
    fn the_planner_makes_a_long_task_several_blocks_with_a_gap_between() {
        let r = plan_horizon(&windows(1), &[], &[htask(1, 90, 0, None)], &defaults(), early());
        assert_eq!(pieces_of(&r, 1), vec![(on(0, 9, 0), on(0, 9, 45)), (on(0, 10, 0), on(0, 10, 45))]);
        assert_eq!(r.blocks.iter().map(|b| b.chunk_index).collect::<Vec<_>>(), vec![0, 1]);
    }

    #[test]
    fn a_task_at_the_threshold_is_still_one_block() {
        let r = plan_horizon(&windows(1), &[], &[htask(1, 30, 0, None)], &defaults(), early());
        assert_eq!(pieces_of(&r, 1), vec![(on(0, 9, 0), on(0, 9, 30))]);
    }

    #[test]
    fn pieces_go_back_to_back_when_the_day_has_no_room_for_a_gap() {
        // Exactly 90 minutes free: no room for a break between the two 45s.
        let busy = [ivd(0, 10, 30, 21, 0)];
        let r = plan_horizon(&windows(1), &busy, &[htask(1, 90, 0, None)], &defaults(), early());
        assert!(r.unplaced.is_empty(), "{r:?}");
        assert_eq!(pieces_of(&r, 1), vec![(on(0, 9, 0), on(0, 9, 45)), (on(0, 9, 45), on(0, 10, 30))]);
    }

    #[test]
    fn a_split_task_and_another_task_on_the_same_day_never_overlap() {
        let r = plan_horizon(&windows(1), &[], &[htask(1, 90, 0, None), htask(2, 15, 0, None)], &defaults(), early());
        assert!(r.unplaced.is_empty(), "{r:?}");
        let mut all: Vec<_> = r.blocks.iter().map(|b| (b.start, b.end)).collect();
        all.sort();
        assert_eq!(all.len(), 3);
        assert!(all.windows(2).all(|w| w[0].1 <= w[1].0), "{all:?}");
    }

    #[test]
    fn splitting_pieces_still_carry_to_the_next_day_when_today_is_short() {
        let busy = [ivd(0, 9, 45, 21, 0)];
        let r = plan_horizon(&windows(2), &busy, &[htask(1, 90, 0, None)], &defaults(), early());
        assert!(r.unplaced.is_empty(), "{r:?}");
        let total: i64 = pieces_of(&r, 1).iter().map(|(s, e)| (*e - *s).num_minutes()).sum();
        assert_eq!(total, 90);
        assert!(pieces_of(&r, 1).iter().any(|(s, _)| s.date() == on(1, 0, 0).date()), "some of it is on day 1");
    }
}

