//! Feature request: "Startup Questions". The full-screen quiz shown when Revise
//! launches: one multiple-choice question at a time, a segmented progress bar
//! along the top edge, and a Skip button in the bottom-right corner.
//!
//! The questions and the record of what was answered live in
//! `flashcards::quiz`; this file only draws them and turns clicks and keys into
//! calls on `Quiz`.

use eframe::egui::{self, Key, Margin, RichText, Sense, Stroke, Vec2};

use crate::flashcards::quiz::{Outcome, Quiz};
use crate::theme::{Border, Spacing, Theme};
use crate::ui::components::RButton;

/// Height of the progress bar along the top edge.
const BAR_HEIGHT: f32 = 10.0;
/// Widest the question and its answers grow, so lines stay readable on a big screen.
const CONTENT_MAX_WIDTH: f32 = 680.0;
/// Height of one answer button.
const CHOICE_HEIGHT: f32 = 52.0;

/// What a frame of the quiz did that the app needs to know about.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct QuizOutput {
    /// A question was answered this frame (not skipped) — its answer was shown,
    /// so the app counts it toward "Flashcards seen".
    pub answered_now: bool,
}

/// The slice of `bar` that question `index` of `total` owns: the bar divided into
/// `total` equal units, left to right. Filling units in this order is what makes
/// the bar fill proportionally — with 10 questions, each is a tenth of the bar.
fn segment_rect(bar: egui::Rect, index: usize, total: usize) -> egui::Rect {
    let unit = bar.width() / total.max(1) as f32;
    egui::Rect::from_min_max(
        egui::pos2(bar.left() + unit * index as f32, bar.top()),
        egui::pos2(bar.left() + unit * (index + 1) as f32, bar.bottom()),
    )
}

/// Draws the bar: a unit per question, green once answered right, red once
/// answered wrong, a plain grey once skipped, and empty while still to come.
fn draw_progress_bar(ui: &mut egui::Ui, theme: &Theme, quiz: &Quiz) {
    let p = &theme.palette;
    let (bar, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), BAR_HEIGHT), Sense::hover());
    ui.painter().rect_filled(bar, 0.0, p.surface);
    for (index, outcome) in quiz.outcomes().iter().enumerate() {
        let color = match outcome {
            Some(Outcome::Correct) => p.correct,
            Some(Outcome::Incorrect) => p.incorrect,
            Some(Outcome::Skipped) => p.border_strong,
            None => continue,
        };
        ui.painter().rect_filled(segment_rect(bar, index, quiz.total()), 0.0, color);
    }
    // Hairline dividers between units, so each question's share is readable.
    for index in 1..quiz.total() {
        let x = segment_rect(bar, index, quiz.total()).left();
        ui.painter().vline(x, bar.y_range(), Stroke::new(Border::HAIRLINE, p.background));
    }
}

/// Keys the quiz answers to: 1–4 pick an answer, Enter or Space continues once
/// answered, S skips.
fn read_keys(ctx: &egui::Context) -> (Option<usize>, bool, bool) {
    ctx.input(|i| {
        let pick = [Key::Num1, Key::Num2, Key::Num3, Key::Num4].iter().position(|key| i.key_pressed(*key));
        let next = i.key_pressed(Key::Enter) || i.key_pressed(Key::Space);
        (pick, next, i.key_pressed(Key::S))
    })
}

/// Shows the current question. Does nothing once the quiz is finished.
pub fn show(ui: &mut egui::Ui, theme: &Theme, quiz: &mut Quiz) -> QuizOutput {
    let mut output = QuizOutput::default();
    if quiz.is_finished() {
        return output;
    }

    let p = &theme.palette;
    let (key_pick, key_next, key_skip) = read_keys(ui.ctx());
    let mut pick = key_pick;
    let mut next = key_next;
    let mut skip = key_skip;

    // The footer is added first so the question area takes what's left above it.
    let last_question = quiz.index() + 1 == quiz.total();
    egui::Panel::bottom("startup_quiz_footer")
        .frame(egui::Frame::NONE.fill(p.background).inner_margin(Margin::same(Spacing::MD as i8)))
        .show_separator_line(false)
        .show(ui, |ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if quiz.is_answered() {
                    let label = if last_question { "Finish" } else { "Next" };
                    next |= RButton::new(label).primary().show(ui, theme).clicked();
                } else {
                    skip |= RButton::new("Skip").show(ui, theme).clicked();
                }
            });
        });

    egui::CentralPanel::default().frame(egui::Frame::NONE.fill(p.background)).show(ui, |ui| {
        draw_progress_bar(ui, theme, quiz);
        ui.add_space((ui.available_height() * 0.14).max(Spacing::LG));

        let Some(question) = quiz.current().cloned() else { return };
        let width = ui.available_width().min(CONTENT_MAX_WIDTH);
        ui.vertical_centered(|ui| {
            ui.set_max_width(width);
            ui.label(
                RichText::new(format!("QUESTION {} OF {}", quiz.index() + 1, quiz.total()))
                    .font(theme.small_font())
                    .color(p.text_faint),
            );
            ui.add_space(Spacing::MD);
            ui.label(RichText::new(&question.prompt).font(theme.heading_font()).color(p.text));
            ui.add_space(Spacing::LG);

            for (i, choice) in question.choices.iter().enumerate() {
                let (marker, stroke, color) = if quiz.is_answered() {
                    if i == question.correct {
                        ("✓", Stroke::new(2.0, p.correct), p.text)
                    } else if quiz.picked() == Some(i) {
                        ("✗", Stroke::new(2.0, p.incorrect), p.text)
                    } else {
                        ("", Stroke::new(Border::HAIRLINE, p.border), p.text_muted)
                    }
                } else {
                    ("", Stroke::new(Border::HAIRLINE, p.border), p.text)
                };
                let text = format!("{}   {choice}   {marker}", i + 1);
                let response = ui.add(
                    egui::Button::new(RichText::new(text).font(theme.label_font()).color(color))
                        .fill(p.surface)
                        .stroke(stroke)
                        .corner_radius(egui::CornerRadius::same(Border::RADIUS))
                        .min_size(Vec2::new(width, CHOICE_HEIGHT)),
                );
                if response.clicked() {
                    pick = Some(i);
                }
                ui.add_space(Spacing::SM);
            }

            ui.add_space(Spacing::SM);
            let hint = if quiz.is_answered() { "Enter to continue" } else { "1–4 to answer · S to skip" };
            ui.label(RichText::new(hint).font(theme.small_font()).color(p.text_faint));
        });
    });

    // Apply what was asked, in the order that makes sense: an answer first, then
    // moving on (so a pick and Enter in one frame can't skip past the result).
    if let Some(choice) = pick {
        output.answered_now = quiz.answer(choice);
    }
    if skip {
        quiz.skip();
    } else if next && !output.answered_now {
        quiz.advance();
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::flashcards::quiz::Question;
    use eframe::egui::{Context, Event, Modifiers, Pos2, RawInput, Rect};

    const SCREEN: Vec2 = Vec2::new(1000.0, 700.0);

    fn quiz_of(n: usize) -> Quiz {
        Quiz::new(
            (0..n)
                .map(|i| Question {
                    prompt: format!("question {i}"),
                    choices: vec!["right".into(), "wrong".into(), "also wrong".into(), "still wrong".into()],
                    correct: 0,
                })
                .collect(),
        )
    }

    fn key(key: Key) -> Event {
        Event::Key { key, physical_key: Some(key), pressed: true, repeat: false, modifiers: Modifiers::NONE }
    }

    /// One frame of the quiz with `events` delivered to it.
    fn frame(ctx: &Context, theme: &Theme, quiz: &mut Quiz, time: f64, events: Vec<Event>) -> QuizOutput {
        let input = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, SCREEN)),
            time: Some(time),
            events,
            ..Default::default()
        };
        let mut out = QuizOutput::default();
        let mut full = ctx.run_ui(input, |ui| out = show(ui, theme, quiz));
        full.textures_delta.clear();
        out
    }

    // --- The bar's geometry -------------------------------------------------

    #[test]
    fn units_divide_the_bar_into_equal_parts_left_to_right() {
        let bar = Rect::from_min_size(Pos2::new(0.0, 0.0), Vec2::new(1000.0, 10.0));
        assert_eq!(segment_rect(bar, 0, 10).left(), 0.0);
        assert_eq!(segment_rect(bar, 0, 10).width(), 100.0);
        assert_eq!(segment_rect(bar, 2, 10).left(), 200.0);
        assert_eq!(segment_rect(bar, 9, 10).right(), 1000.0);
        assert_eq!(segment_rect(bar, 0, 5).width(), 200.0, "five questions make fifths");
    }

    #[test]
    fn a_unit_is_the_full_height_of_the_bar() {
        let bar = Rect::from_min_size(Pos2::new(40.0, 7.0), Vec2::new(300.0, 10.0));
        let unit = segment_rect(bar, 1, 3);
        assert_eq!((unit.top(), unit.bottom()), (7.0, 17.0));
    }

    #[test]
    fn the_units_tile_the_bar_with_no_gap_or_overlap() {
        let bar = Rect::from_min_size(Pos2::ZERO, Vec2::new(777.0, 10.0));
        for total in 1..=12 {
            for index in 1..total {
                let gap = segment_rect(bar, index, total).left() - segment_rect(bar, index - 1, total).right();
                assert!(gap.abs() < 1e-3, "{total} questions, unit {index}: {gap}");
            }
            assert!((segment_rect(bar, total - 1, total).right() - bar.right()).abs() < 1e-3);
        }
    }

    // --- Through real layout ------------------------------------------------

    #[test]
    fn it_draws_a_question_without_trouble() {
        let (ctx, theme) = (Context::default(), Theme::new());
        let mut quiz = quiz_of(5);
        frame(&ctx, &theme, &mut quiz, 0.0, vec![]);
        frame(&ctx, &theme, &mut quiz, 0.05, vec![]);
        assert_eq!(quiz.index(), 0);
        assert!(!quiz.is_answered(), "drawing alone answers nothing");
    }

    #[test]
    fn pressing_a_number_answers_and_reports_it() {
        let (ctx, theme) = (Context::default(), Theme::new());
        let mut quiz = quiz_of(5);
        frame(&ctx, &theme, &mut quiz, 0.0, vec![]);
        let out = frame(&ctx, &theme, &mut quiz, 0.05, vec![key(Key::Num2)]);
        assert!(out.answered_now);
        assert_eq!(quiz.picked(), Some(1));
        assert_eq!(quiz.outcomes()[0], Some(Outcome::Incorrect));
        assert_eq!(quiz.index(), 0, "it stays to show the result");
    }

    #[test]
    fn a_number_beyond_the_choices_does_nothing() {
        let (ctx, theme) = (Context::default(), Theme::new());
        let mut quiz = Quiz::new(vec![Question {
            prompt: "q".into(),
            choices: vec!["a".into(), "b".into()],
            correct: 0,
        }]);
        frame(&ctx, &theme, &mut quiz, 0.0, vec![]);
        let out = frame(&ctx, &theme, &mut quiz, 0.05, vec![key(Key::Num4)]);
        assert!(!out.answered_now && !quiz.is_answered());
    }

    #[test]
    fn enter_continues_only_after_an_answer() {
        let (ctx, theme) = (Context::default(), Theme::new());
        let mut quiz = quiz_of(5);
        frame(&ctx, &theme, &mut quiz, 0.0, vec![]);
        frame(&ctx, &theme, &mut quiz, 0.05, vec![key(Key::Enter)]);
        assert_eq!(quiz.index(), 0, "Enter before answering does nothing");

        frame(&ctx, &theme, &mut quiz, 0.1, vec![key(Key::Num1)]);
        frame(&ctx, &theme, &mut quiz, 0.15, vec![key(Key::Enter)]);
        assert_eq!(quiz.index(), 1);
        assert_eq!(quiz.picked(), None);
    }

    #[test]
    fn a_pick_and_enter_in_the_same_frame_still_shows_the_result() {
        let (ctx, theme) = (Context::default(), Theme::new());
        let mut quiz = quiz_of(5);
        frame(&ctx, &theme, &mut quiz, 0.0, vec![]);
        frame(&ctx, &theme, &mut quiz, 0.05, vec![key(Key::Num1), key(Key::Enter)]);
        assert_eq!(quiz.index(), 0);
        assert!(quiz.is_answered());
    }

    #[test]
    fn s_skips_straight_to_the_next_question() {
        let (ctx, theme) = (Context::default(), Theme::new());
        let mut quiz = quiz_of(5);
        frame(&ctx, &theme, &mut quiz, 0.0, vec![]);
        let out = frame(&ctx, &theme, &mut quiz, 0.05, vec![key(Key::S)]);
        assert!(!out.answered_now, "a skip is not an answer");
        assert_eq!(quiz.index(), 1);
        assert_eq!(quiz.outcomes()[0], Some(Outcome::Skipped));
    }

    #[test]
    fn a_whole_quiz_can_be_played_through_the_keyboard() {
        let (ctx, theme) = (Context::default(), Theme::new());
        let mut quiz = quiz_of(4);
        let mut t = 0.0;
        let mut next_frame = |quiz: &mut Quiz, events| {
            t += 0.05;
            frame(&ctx, &theme, quiz, t, events)
        };
        next_frame(&mut quiz, vec![]);
        next_frame(&mut quiz, vec![key(Key::Num1)]); // q1 right
        next_frame(&mut quiz, vec![key(Key::Enter)]);
        next_frame(&mut quiz, vec![key(Key::Num3)]); // q2 wrong
        next_frame(&mut quiz, vec![key(Key::Space)]);
        next_frame(&mut quiz, vec![key(Key::S)]); // q3 skipped
        next_frame(&mut quiz, vec![key(Key::Num1)]); // q4 right
        assert!(!quiz.is_finished(), "the last question waits for Finish");
        next_frame(&mut quiz, vec![key(Key::Enter)]);
        assert!(quiz.is_finished());
        assert_eq!(
            quiz.outcomes(),
            &[Some(Outcome::Correct), Some(Outcome::Incorrect), Some(Outcome::Skipped), Some(Outcome::Correct)]
        );
        assert_eq!(next_frame(&mut quiz, vec![key(Key::Num1)]), QuizOutput::default(), "a finished quiz is inert");
    }
}
