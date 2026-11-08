//! Feature request: "Startup Questions". A short multiple-choice quiz drawn at
//! random from the flashcards already stored in the app, shown full-screen when
//! Revise launches.
//!
//! This module is the pure part, per the project's convention of testing logic
//! apart from the `egui` code that draws it (`ui::startup_quiz`): turning a pool
//! of cards into questions, and the small state machine that records what was
//! answered. Nothing here touches the database or the clock; the caller supplies
//! the pool and a seed.

use std::collections::HashSet;

use super::shuffled_order;

/// How many questions a launch asks unless the setting says otherwise.
pub const DEFAULT_QUESTIONS: usize = 5;
/// The most a person can ask for. Generous, but stops a typo from turning
/// startup into an exam.
pub const MAX_QUESTIONS: usize = 50;
/// Answers shown per question: the right one plus up to three wrong ones.
pub const CHOICES: usize = 4;

/// One flashcard, flattened out of whichever set it came from. `set` only says
/// which cards share a set (an imported deck or a flashcard-set Note), so wrong
/// answers can be taken from the same set first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuizCard {
    pub set: usize,
    pub question: String,
    pub answer: String,
}

/// One multiple-choice question.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Question {
    pub prompt: String,
    /// The answers on offer, in the order shown.
    pub choices: Vec<String>,
    /// Index into `choices` of the right one.
    pub correct: usize,
}

fn normalize(text: &str) -> String {
    text.trim().to_lowercase()
}

/// A different seed for each use of the shuffle within one quiz, so the choice
/// order of one question doesn't mirror another's.
fn mix(seed: u64, salt: u64) -> u64 {
    seed ^ salt.wrapping_mul(0x9E37_79B9_7F4A_7C15)
}

/// Wrong answers for `pool[card]`: other cards' answers, from the same set
/// first (a French word's wrong answers should be other English words from the
/// same deck, not a chemistry formula), then from any set. Never the right
/// answer again, never two the same.
fn distractors(pool: &[QuizCard], card: usize, seed: u64) -> Vec<String> {
    let correct = normalize(&pool[card].answer);
    let (same_set, other_sets): (Vec<usize>, Vec<usize>) =
        (0..pool.len()).filter(|i| *i != card).partition(|i| pool[*i].set == pool[card].set);

    let mut taken = HashSet::from([correct]);
    let mut picked = Vec::new();
    for (group, salt) in [(same_set, 1), (other_sets, 2)] {
        for position in shuffled_order(group.len(), mix(seed, salt)) {
            let answer = pool[group[position]].answer.trim();
            if answer.is_empty() || !taken.insert(normalize(answer)) {
                continue;
            }
            picked.push(answer.to_owned());
            if picked.len() == CHOICES - 1 {
                return picked;
            }
        }
    }
    picked
}

/// Up to `count` questions from random cards in `pool`. Fewer come back when the
/// pool can't supply them: a card is only usable if it has at least one wrong
/// answer to sit beside, and the same question text is never asked twice. An
/// empty result means "no quiz".
pub fn build_questions(pool: &[QuizCard], count: usize, seed: u64) -> Vec<Question> {
    let mut asked = HashSet::new();
    let mut questions = Vec::new();
    for card in shuffled_order(pool.len(), seed) {
        if questions.len() >= count {
            break;
        }
        let QuizCard { question, answer, .. } = &pool[card];
        if question.trim().is_empty() || answer.trim().is_empty() || !asked.insert(normalize(question)) {
            continue;
        }
        let wrong = distractors(pool, card, mix(seed, card as u64 + 3));
        if wrong.is_empty() {
            continue;
        }
        let mut choices = vec![answer.trim().to_owned()];
        choices.extend(wrong);
        let order = shuffled_order(choices.len(), mix(seed, card as u64 + 1_000_003));
        let correct = order.iter().position(|original| *original == 0).expect("the right answer is in the order");
        let choices = order.into_iter().map(|original| choices[original].clone()).collect();
        questions.push(Question { prompt: question.trim().to_owned(), choices, correct });
    }
    questions
}

/// How one question ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Correct,
    Incorrect,
    Skipped,
}

/// A quiz in progress: the questions, what has happened to each, and where the
/// person is. The progress bar is drawn straight from `outcomes`.
#[derive(Debug, Clone)]
pub struct Quiz {
    questions: Vec<Question>,
    outcomes: Vec<Option<Outcome>>,
    index: usize,
    picked: Option<usize>,
}

impl Quiz {
    pub fn new(questions: Vec<Question>) -> Self {
        let outcomes = vec![None; questions.len()];
        Self { questions, outcomes, index: 0, picked: None }
    }

    pub fn total(&self) -> usize {
        self.questions.len()
    }

    /// Zero-based index of the question being shown.
    pub fn index(&self) -> usize {
        self.index
    }

    /// The question being shown, or `None` once the quiz is over.
    pub fn current(&self) -> Option<&Question> {
        self.questions.get(self.index)
    }

    /// The choice picked for the current question, once it has been answered.
    pub fn picked(&self) -> Option<usize> {
        self.picked
    }

    pub fn is_answered(&self) -> bool {
        self.picked.is_some()
    }

    pub fn is_finished(&self) -> bool {
        self.index >= self.questions.len()
    }

    /// One entry per question, in order: how it ended, or `None` if it hasn't yet.
    pub fn outcomes(&self) -> &[Option<Outcome>] {
        &self.outcomes
    }

    pub fn count(&self, outcome: Outcome) -> usize {
        self.outcomes.iter().filter(|o| **o == Some(outcome)).count()
    }

    /// Picks `choice` for the current question. Returns whether this recorded an
    /// answer: `false` for a choice that doesn't exist, a question already
    /// answered (the first pick stands), or a finished quiz.
    pub fn answer(&mut self, choice: usize) -> bool {
        let Some(question) = self.current() else { return false };
        if self.is_answered() || choice >= question.choices.len() {
            return false;
        }
        let outcome = if choice == question.correct { Outcome::Correct } else { Outcome::Incorrect };
        self.outcomes[self.index] = Some(outcome);
        self.picked = Some(choice);
        true
    }

    /// Skips the current question and moves straight on. Does nothing once the
    /// question is answered (that's what `advance` is for) or the quiz is over.
    pub fn skip(&mut self) -> bool {
        if self.is_finished() || self.is_answered() {
            return false;
        }
        self.outcomes[self.index] = Some(Outcome::Skipped);
        self.index += 1;
        true
    }

    /// Moves on from an answered question. Does nothing before it is answered.
    pub fn advance(&mut self) -> bool {
        if !self.is_answered() {
            return false;
        }
        self.index += 1;
        self.picked = None;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card(set: usize, question: &str, answer: &str) -> QuizCard {
        QuizCard { set, question: question.to_owned(), answer: answer.to_owned() }
    }

    fn french() -> Vec<QuizCard> {
        vec![
            card(0, "le chat", "the cat"),
            card(0, "le chien", "the dog"),
            card(0, "le cheval", "the horse"),
            card(0, "la vache", "the cow"),
            card(0, "le lapin", "the rabbit"),
            card(1, "H2O", "water"),
            card(1, "NaCl", "salt"),
        ]
    }

    #[test]
    fn it_asks_the_number_of_questions_requested() {
        assert_eq!(build_questions(&french(), 5, 1).len(), 5);
        assert_eq!(build_questions(&french(), 3, 1).len(), 3);
    }

    #[test]
    fn it_asks_fewer_when_the_pool_is_smaller() {
        assert_eq!(build_questions(&french(), 50, 1).len(), 7);
    }

    #[test]
    fn an_empty_pool_gives_no_quiz() {
        assert!(build_questions(&[], 5, 1).is_empty());
    }

    #[test]
    fn a_lone_card_cannot_be_asked_because_it_has_no_wrong_answer() {
        assert!(build_questions(&[card(0, "le chat", "the cat")], 5, 1).is_empty());
    }

    #[test]
    fn two_cards_with_the_same_answer_cannot_be_asked_either() {
        let pool = [card(0, "chat", "cat"), card(0, "Katze", "Cat ")];
        assert!(build_questions(&pool, 5, 1).is_empty(), "no distinct wrong answer exists");
    }

    #[test]
    fn every_question_holds_its_right_answer_exactly_once() {
        for seed in 0..40 {
            for question in build_questions(&french(), 7, seed) {
                let source = french().into_iter().find(|c| c.question == question.prompt).expect("a real card");
                assert_eq!(question.choices[question.correct], source.answer, "seed {seed}");
                let right = question.choices.iter().filter(|c| **c == source.answer).count();
                assert_eq!(right, 1, "the right answer appears once, seed {seed}");
            }
        }
    }

    #[test]
    fn choices_are_distinct_and_at_most_four() {
        for seed in 0..40 {
            for question in build_questions(&french(), 7, seed) {
                assert!(question.choices.len() >= 2 && question.choices.len() <= CHOICES);
                let unique: HashSet<_> = question.choices.iter().map(|c| normalize(c)).collect();
                assert_eq!(unique.len(), question.choices.len(), "{:?}", question.choices);
            }
        }
    }

    #[test]
    fn wrong_answers_come_from_the_same_set_when_it_has_enough() {
        // Set 0 has five cards, so a set-0 question's three wrong answers are all set-0 answers.
        let set_zero: HashSet<_> = french().into_iter().filter(|c| c.set == 0).map(|c| c.answer).collect();
        for seed in 0..40 {
            for question in build_questions(&french(), 7, seed) {
                if set_zero.contains(&question.choices[question.correct]) {
                    assert!(question.choices.iter().all(|c| set_zero.contains(c)), "{:?}", question.choices);
                }
            }
        }
    }

    #[test]
    fn a_small_set_borrows_wrong_answers_from_other_sets() {
        // "H2O" is alone with "NaCl" in its set; the other two wrong answers come from elsewhere.
        let question = (0..200)
            .flat_map(|seed| build_questions(&french(), 7, seed))
            .find(|q| q.prompt == "H2O")
            .expect("H2O is asked at some seed");
        assert_eq!(question.choices.len(), CHOICES);
        assert!(question.choices.contains(&"salt".to_owned()), "its own set's answer is preferred");
    }

    #[test]
    fn the_same_question_text_is_never_asked_twice() {
        let pool = [card(0, "le chat", "the cat"), card(1, "Le Chat", "the cat (fr.)"), card(0, "le chien", "the dog")];
        for seed in 0..20 {
            let prompts: Vec<_> = build_questions(&pool, 3, seed).into_iter().map(|q| normalize(&q.prompt)).collect();
            let unique: HashSet<_> = prompts.iter().collect();
            assert_eq!(unique.len(), prompts.len(), "{prompts:?}");
        }
    }

    #[test]
    fn blank_cards_are_ignored() {
        let pool = [card(0, "", "x"), card(0, "q", "  "), card(0, "a", "1"), card(0, "b", "2")];
        for question in build_questions(&pool, 5, 3) {
            assert!(question.choices.iter().all(|c| !c.trim().is_empty()));
            assert!(!question.prompt.trim().is_empty());
        }
    }

    #[test]
    fn the_same_seed_gives_the_same_quiz_and_other_seeds_usually_differ() {
        assert_eq!(build_questions(&french(), 5, 9), build_questions(&french(), 5, 9));
        assert!((0..10).any(|s| build_questions(&french(), 5, s) != build_questions(&french(), 5, 9)));
    }

    #[test]
    fn the_right_answer_is_not_always_in_the_same_position() {
        let positions: HashSet<_> =
            (0..60).flat_map(|seed| build_questions(&french(), 5, seed)).map(|q| q.correct).collect();
        assert!(positions.len() > 1, "{positions:?}");
    }

    // --- Quiz ---------------------------------------------------------------

    fn quiz_of(n: usize) -> Quiz {
        Quiz::new(
            (0..n)
                .map(|i| Question {
                    prompt: format!("q{i}"),
                    choices: vec!["right".into(), "wrong".into(), "also wrong".into()],
                    correct: 0,
                })
                .collect(),
        )
    }

    #[test]
    fn a_new_quiz_starts_at_the_first_question_with_nothing_recorded() {
        let quiz = quiz_of(10);
        assert_eq!(quiz.total(), 10);
        assert_eq!(quiz.index(), 0);
        assert_eq!(quiz.outcomes(), &[None; 10]);
        assert!(!quiz.is_answered() && !quiz.is_finished());
    }

    #[test]
    fn a_right_answer_is_recorded_correct_and_a_wrong_one_incorrect() {
        let mut quiz = quiz_of(3);
        assert!(quiz.answer(0));
        assert_eq!(quiz.outcomes()[0], Some(Outcome::Correct));
        assert_eq!(quiz.picked(), Some(0));
        quiz.advance();
        assert!(quiz.answer(2));
        assert_eq!(quiz.outcomes()[1], Some(Outcome::Incorrect));
        assert_eq!(quiz.picked(), Some(2));
    }

    #[test]
    fn answering_does_not_move_on_by_itself() {
        let mut quiz = quiz_of(3);
        quiz.answer(0);
        assert_eq!(quiz.index(), 0, "the person gets to see the result first");
        assert!(quiz.is_answered());
    }

    #[test]
    fn the_first_pick_stands() {
        let mut quiz = quiz_of(3);
        assert!(quiz.answer(1));
        assert!(!quiz.answer(0), "a second pick is ignored");
        assert_eq!(quiz.outcomes()[0], Some(Outcome::Incorrect));
        assert_eq!(quiz.picked(), Some(1));
    }

    #[test]
    fn a_choice_that_does_not_exist_is_ignored() {
        let mut quiz = quiz_of(3);
        assert!(!quiz.answer(3));
        assert!(!quiz.is_answered());
        assert_eq!(quiz.outcomes()[0], None);
    }

    #[test]
    fn skipping_records_skipped_and_moves_straight_on() {
        let mut quiz = quiz_of(3);
        assert!(quiz.skip());
        assert_eq!(quiz.outcomes()[0], Some(Outcome::Skipped));
        assert_eq!(quiz.index(), 1);
        assert!(!quiz.is_answered());
    }

    #[test]
    fn an_answered_question_cannot_then_be_skipped() {
        let mut quiz = quiz_of(3);
        quiz.answer(0);
        assert!(!quiz.skip());
        assert_eq!(quiz.outcomes()[0], Some(Outcome::Correct));
        assert_eq!(quiz.index(), 0);
    }

    #[test]
    fn advance_needs_an_answer_first() {
        let mut quiz = quiz_of(3);
        assert!(!quiz.advance());
        assert_eq!(quiz.index(), 0);
        quiz.answer(1);
        assert!(quiz.advance());
        assert_eq!(quiz.index(), 1);
        assert_eq!(quiz.picked(), None, "the next question starts unanswered");
    }

    #[test]
    fn the_quiz_finishes_after_the_last_question_is_dealt_with() {
        let mut quiz = quiz_of(2);
        quiz.answer(0);
        quiz.advance();
        assert!(!quiz.is_finished());
        quiz.skip();
        assert!(quiz.is_finished());
        assert!(quiz.current().is_none());
        assert!(!quiz.answer(0) && !quiz.skip() && !quiz.advance(), "nothing happens after the end");
    }

    #[test]
    fn counts_tally_each_outcome() {
        let mut quiz = quiz_of(4);
        quiz.answer(0);
        quiz.advance();
        quiz.answer(1);
        quiz.advance();
        quiz.skip();
        quiz.answer(0);
        assert_eq!(quiz.count(Outcome::Correct), 2);
        assert_eq!(quiz.count(Outcome::Incorrect), 1);
        assert_eq!(quiz.count(Outcome::Skipped), 1);
    }

    #[test]
    fn the_worked_example_ten_questions_with_the_third_correct() {
        // Feature request: with 10 questions and q3 answered correctly, three
        // units of the ten-unit bar are filled once that question is done.
        let mut quiz = quiz_of(10);
        quiz.answer(0);
        quiz.advance();
        quiz.answer(0);
        quiz.advance();
        quiz.answer(0);
        let filled = quiz.outcomes().iter().filter(|o| o.is_some()).count();
        assert_eq!(filled, 3);
        assert!(quiz.outcomes()[..3].iter().all(|o| *o == Some(Outcome::Correct)));
        assert!(quiz.outcomes()[3..].iter().all(|o| o.is_none()));
    }
}
