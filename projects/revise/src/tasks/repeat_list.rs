use std::fmt;

/// Feature request: "Tasks Repeating Through A List" — how the next
/// topic is chosen each time a `repeat_through_list` task resets.
/// `Smart` (varying frequency by a subtopic's flagged weakness/
/// strength) is the request's own "Future Extension," deliberately
/// not implemented here — it depends on the separate "Subject Sub
/// Topics" feature's weakness/strength flags existing first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RepeatListMode {
    #[default]
    Sequential,
    Shuffle,
    SpacedRepetition,
}

impl RepeatListMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            RepeatListMode::Sequential => "sequential",
            RepeatListMode::Shuffle => "shuffle",
            RepeatListMode::SpacedRepetition => "spaced",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "sequential" => Some(RepeatListMode::Sequential),
            "shuffle" => Some(RepeatListMode::Shuffle),
            "spaced" => Some(RepeatListMode::SpacedRepetition),
            _ => None,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            RepeatListMode::Sequential => "In order",
            RepeatListMode::Shuffle => "Shuffle",
            RepeatListMode::SpacedRepetition => "Spaced repetition",
        }
    }
}

impl fmt::Display for RepeatListMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.label())
    }
}

/// Parses a repeat-list's topics: one per line, same permissive shape
/// as `flashcards::import::parse_pairs` — blank lines ignored,
/// surrounding whitespace trimmed, never errors (this runs live
/// against a text field that might still be mid-edit).
pub fn parse_topics(text: &str) -> Vec<String> {
    text.lines().map(str::trim).filter(|line| !line.is_empty()).map(str::to_owned).collect()
}

/// Advances a repeat-list task to its next topic. `state` is
/// whatever `repeat_list_state` currently holds (empty string for "no
/// state yet," e.g. a brand new task) — its meaning depends entirely
/// on `mode`, and it's never meant to be inspected outside this
/// module. Returns the chosen topic and the new state to persist
/// alongside it.
///
/// `topics` is assumed non-empty; returns `None` if it isn't (nothing
/// to advance to — the caller should simply not set up repeat-through-
/// list in the first place with an empty list, but this guards
/// against it happening anyway rather than panicking or picking
/// something meaningless).
///
/// `seed` only matters for `Shuffle`; ignored otherwise. The caller
/// supplies a fresh one (e.g. from the system clock) each call so a
/// freshly-reshuffled cycle doesn't repeat the same order every time.
pub fn next_topic(mode: RepeatListMode, topics: &[String], state: &str, seed: u64) -> Option<(String, String)> {
    if topics.is_empty() {
        return None;
    }

    match mode {
        RepeatListMode::Sequential => Some(next_sequential(topics, state)),
        RepeatListMode::Shuffle => Some(next_shuffle(topics, state, seed)),
        RepeatListMode::SpacedRepetition => Some(next_spaced(topics, state)),
    }
}

/// Moves `steps` occurrences forward through the list and returns the
/// topic for the *last* of them plus the state to persist — what a
/// repeating task needs when it is re-dated by more than one cycle at
/// once (the app wasn't opened for a few days): the title must show the
/// topic that belongs to *today's* occurrence, not merely the one after
/// the last it showed. `steps` of 0 or 1 is a single `next_topic`.
/// `seed` varies per step so a `Shuffle` list reshuffles differently
/// each time it wraps. Capped at a few thousand steps so a corrupt date
/// can never spin.
pub fn advance_topic(
    mode: RepeatListMode,
    topics: &[String],
    state: &str,
    steps: i64,
    seed: u64,
) -> Option<(String, String)> {
    let steps = steps.clamp(1, 5000);
    let mut current = (String::new(), state.to_owned());
    for i in 0..steps {
        current = next_topic(mode, topics, &current.1, seed.wrapping_add(i as u64))?;
    }
    Some(current)
}

/// `state` is the index of the topic to show *next*, as plain
/// decimal — `""` or anything unparseable defaults to `0` (a fresh
/// start). Wraps back to the first topic after the last.
fn next_sequential(topics: &[String], state: &str) -> (String, String) {
    let index = state.trim().parse::<usize>().unwrap_or(0) % topics.len();
    let next_state = ((index + 1) % topics.len()).to_string();
    (topics[index].clone(), next_state)
}

/// `state` is a comma-separated queue of topic indices still left in
/// the current shuffled cycle, e.g. `"2,0,1"` — the front is popped
/// and returned; once the queue empties (or was never valid), a fresh
/// shuffle of every topic is generated for the *next* call, so a
/// cycle never repeats a topic before every other one has had a turn,
/// without ever showing the exact same order twice in a row.
fn next_shuffle(topics: &[String], state: &str, seed: u64) -> (String, String) {
    let mut queue: Vec<usize> = state
        .split(',')
        .filter(|s| !s.trim().is_empty())
        .filter_map(|s| s.trim().parse::<usize>().ok())
        .filter(|&i| i < topics.len())
        .collect();

    if queue.is_empty() {
        queue = crate::flashcards::shuffled_order(topics.len(), seed);
    }

    let chosen = queue.remove(0);
    let next_state = queue.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(",");
    (topics[chosen].clone(), next_state)
}

/// `state` is a comma-separated "times shown" count per topic, in
/// list order, e.g. `"3,1,2"` — the topic with the *lowest* count
/// goes next (ties broken by earliest position, so a brand new list
/// with an all-zero/empty state simply starts at the top), then that
/// one topic's count is incremented. Least-shown-first is a simple,
/// literal reading of "spaced repetition" that doesn't require any
/// explicit difficulty rating from the person — topics naturally
/// spread out evenly over time rather than favoring whichever the
/// cycle happens to land on next.
fn next_spaced(topics: &[String], state: &str) -> (String, String) {
    let mut counts: Vec<u32> = state.split(',').map(|s| s.trim().parse::<u32>().unwrap_or(0)).collect();
    counts.resize(topics.len(), 0);

    let chosen = counts
        .iter()
        .enumerate()
        .min_by_key(|(index, count)| (**count, *index))
        .map(|(index, _)| index)
        .unwrap_or(0);

    counts[chosen] += 1;
    let next_state = counts.iter().map(|c| c.to_string()).collect::<Vec<_>>().join(",");
    (topics[chosen].clone(), next_state)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn advancing_one_step_is_just_next_topic() {
        let topics = list(&["A", "B", "C"]);
        assert_eq!(advance_topic(RepeatListMode::Sequential, &topics, "", 1, 1), next_topic(RepeatListMode::Sequential, &topics, "", 1));
        // 0 steps still advances once (a reset always shows a new topic).
        assert_eq!(advance_topic(RepeatListMode::Sequential, &topics, "", 0, 1), advance_topic(RepeatListMode::Sequential, &topics, "", 1, 1));
    }

    #[test]
    fn advancing_several_steps_lands_on_that_days_topic_and_wraps() {
        let topics = list(&["A", "B", "C"]);
        // State "1" means B is next. Three steps: B, C, A.
        let (topic, state) = advance_topic(RepeatListMode::Sequential, &topics, "1", 3, 1).unwrap();
        assert_eq!(topic, "A");
        assert_eq!(state, "1");
        // Skipping a whole number of cycles lands where it started.
        let (topic, _) = advance_topic(RepeatListMode::Sequential, &topics, "", 7, 1).unwrap();
        assert_eq!(topic, "A", "7 steps from a fresh start: indices 0..=6, the last is 6 mod 3 = 0");
    }

    #[test]
    fn advancing_through_a_stateful_list_threads_the_state() {
        let topics = list(&["A", "B", "C"]);
        // Least-shown-first: three steps show each topic exactly once.
        let (_, state) = advance_topic(RepeatListMode::SpacedRepetition, &topics, "", 3, 1).unwrap();
        assert_eq!(state, "1,1,1");
        let (_, state) = advance_topic(RepeatListMode::Shuffle, &topics, "", 3, 7).unwrap();
        assert_eq!(state, "", "a shuffled cycle of 3 is used up after 3 steps");
    }

    #[test]
    fn advancing_an_empty_list_is_none() {
        assert_eq!(advance_topic(RepeatListMode::Sequential, &[], "", 3, 1), None);
    }

    #[test]
    fn parse_topics_trims_and_skips_blank_lines() {
        let topics = parse_topics("  Topic 1  \n\nTopic 2\n   \nTopic 3");
        assert_eq!(topics, vec!["Topic 1".to_owned(), "Topic 2".to_owned(), "Topic 3".to_owned()]);
    }

    #[test]
    fn parse_topics_of_empty_text_is_empty() {
        assert!(parse_topics("").is_empty());
        assert!(parse_topics("   \n  \n").is_empty());
    }

    #[test]
    fn next_topic_returns_none_for_an_empty_list() {
        assert_eq!(next_topic(RepeatListMode::Sequential, &[], "", 1), None);
    }

    fn topics() -> Vec<String> {
        vec!["Topic 1".to_owned(), "Topic 2".to_owned(), "Topic 3".to_owned()]
    }

    #[test]
    fn sequential_starts_at_the_first_topic_with_no_prior_state() {
        let (topic, state) = next_topic(RepeatListMode::Sequential, &topics(), "", 1).expect("some");
        assert_eq!(topic, "Topic 1");
        assert_eq!(state, "1");
    }

    #[test]
    fn sequential_advances_one_at_a_time_and_wraps_around() {
        let t = topics();
        let (topic, state) = next_topic(RepeatListMode::Sequential, &t, "1", 1).expect("some");
        assert_eq!(topic, "Topic 2");
        assert_eq!(state, "2");

        let (topic, state) = next_topic(RepeatListMode::Sequential, &t, "2", 1).expect("some");
        assert_eq!(topic, "Topic 3");
        assert_eq!(state, "0", "wraps back to the first topic after the last");

        let (topic, _) = next_topic(RepeatListMode::Sequential, &t, "0", 1).expect("some");
        assert_eq!(topic, "Topic 1");
    }

    #[test]
    fn sequential_treats_garbage_state_as_a_fresh_start() {
        let (topic, _) = next_topic(RepeatListMode::Sequential, &topics(), "not a number", 1).expect("some");
        assert_eq!(topic, "Topic 1");
    }

    #[test]
    fn shuffle_with_no_state_visits_every_topic_exactly_once_before_repeating() {
        let t = topics();
        let mut state = String::new();
        let mut seen = Vec::new();
        for _ in 0..t.len() {
            let (topic, next_state) = next_topic(RepeatListMode::Shuffle, &t, &state, 42).expect("some");
            seen.push(topic);
            state = next_state;
        }
        seen.sort();
        assert_eq!(seen, vec!["Topic 1".to_owned(), "Topic 2".to_owned(), "Topic 3".to_owned()]);
        assert_eq!(state, "", "queue is empty again exactly when the cycle completes");
    }

    #[test]
    fn shuffle_reshuffles_once_the_queue_is_exhausted() {
        let t = topics();
        // Empty state (as if the queue just ran out) should produce a
        // brand new full cycle, not an empty pick.
        let (topic, state) = next_topic(RepeatListMode::Shuffle, &t, "", 7).expect("some");
        assert!(t.contains(&topic));
        assert!(!state.is_empty() || t.len() == 1, "two more picks should remain queued");
    }

    #[test]
    fn spaced_repetition_starts_at_the_first_topic_with_no_prior_state() {
        let (topic, state) = next_topic(RepeatListMode::SpacedRepetition, &topics(), "", 1).expect("some");
        assert_eq!(topic, "Topic 1");
        assert_eq!(state, "1,0,0");
    }

    #[test]
    fn spaced_repetition_always_picks_the_least_shown_topic() {
        let t = topics();
        // Topic 1 already shown twice, Topic 2 once, Topic 3 never —
        // Topic 3 should go next.
        let (topic, state) = next_topic(RepeatListMode::SpacedRepetition, &t, "2,1,0", 1).expect("some");
        assert_eq!(topic, "Topic 3");
        assert_eq!(state, "2,1,1");
    }

    #[test]
    fn spaced_repetition_ties_break_toward_the_earliest_topic() {
        let (topic, state) = next_topic(RepeatListMode::SpacedRepetition, &topics(), "0,0,0", 1).expect("some");
        assert_eq!(topic, "Topic 1");
        assert_eq!(state, "1,0,0");
    }

    #[test]
    fn spaced_repetition_evens_out_over_several_cycles() {
        let t = topics();
        let mut state = String::new();
        let mut chosen = Vec::new();
        for _ in 0..6 {
            let (topic, next_state) = next_topic(RepeatListMode::SpacedRepetition, &t, &state, 1).expect("some");
            chosen.push(topic);
            state = next_state;
        }
        // Six picks across three topics, always taking the least-shown
        // one, must land exactly twice on each.
        assert_eq!(state, "2,2,2");
        for topic in &t {
            assert_eq!(chosen.iter().filter(|c| *c == topic).count(), 2);
        }
    }

    #[test]
    fn mode_as_str_and_parse_round_trip_every_variant() {
        for mode in [RepeatListMode::Sequential, RepeatListMode::Shuffle, RepeatListMode::SpacedRepetition] {
            assert_eq!(RepeatListMode::parse(mode.as_str()), Some(mode));
        }
    }

    #[test]
    fn mode_parse_rejects_unknown_strings() {
        assert_eq!(RepeatListMode::parse("smart"), None);
        assert_eq!(RepeatListMode::parse(""), None);
    }
}
