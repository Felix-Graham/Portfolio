//! Direct request: "an API... callable from other applications and
//! through the terminal from anywhere... other revision applications
//! ... to be registered and accounted for." Real logic for the
//! `revise-cli` binary (`src/bin/revise-cli.rs`) lives here rather
//! than in that binary's `fn main()` directly — same reasoning
//! `revise-reminderd`'s own doc comment gives for splitting
//! `notifications::check_reminders` out of *that* binary: parsing and
//! database writes belong in the library crate, where `cargo test`
//! can actually exercise them.

use chrono::NaiveDate;

use crate::database::queries::pomodoro::FocusSource;
use crate::database::{queries, Database};
use crate::sources::Source;
use crate::subjects::Subject;

/// One thing `revise-cli` can be asked to do — already fully parsed
/// and shape-checked (right number of flags, `--minutes` is actually a
/// number, `--date` is actually a date), but not yet checked against
/// the database (e.g. whether `--source` names something registered).
/// That second kind of check happens in `run`, not here, because it
/// needs a `Database` and `parse_args` deliberately doesn't.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// `revise-cli register <name>`
    Register { name: String },
    /// `revise-cli log --source <name-or-id> --subject <name> --minutes <n> [--note <text>] [--date YYYY-MM-DD]`
    Log { source: String, subject: String, minutes: i64, note: Option<String>, date: Option<NaiveDate> },
    /// `revise-cli sources`
    ListSources,
    /// `revise-cli focus --kind <pomodoro|timer|stopwatch> --minutes <n> [--subject <name>] [--title <text>] [--no-notify]`
    ///
    /// Feature request: the Revise applet ("a smaller unit for the stopwatch
    /// and timer") logs through this. Unlike `Log`, which files time under a
    /// registered *source* and *requires* a subject, this records a run of the
    /// app's own focus tools exactly as the main app does: the minutes always
    /// count toward Focus time, and — only if a subject is given — also
    /// toward that subject's hours. The subject is optional because it is
    /// optional in the app's own Pomodoro, Timer and Stopwatch.
    Focus { kind: FocusSource, minutes: i64, subject: Option<String>, title: Option<String>, notify: bool },
    /// `revise-cli subjects` — the active subjects `--subject` accepts.
    ListSubjects,
}

fn usage() -> String {
    "Usage:\n\
     \x20 revise-cli register <name>\n\
     \x20 revise-cli log --source <name-or-id> --subject <name> --minutes <n> [--note <text>] [--date YYYY-MM-DD]\n\
     \x20 revise-cli sources\n\
     \x20 revise-cli focus --kind <pomodoro|timer|stopwatch> --minutes <n> [--subject <name>] [--title <text>] [--no-notify]\n\
     \x20 revise-cli subjects"
        .to_owned()
}

/// Parses `revise-cli`'s own argument list — i.e. `std::env::args()`
/// with `argv[0]` (the binary's own path) already stripped — into a
/// `Command`, or a human-readable error naming exactly what's wrong.
/// This is the only thing standing between a typo in a cron job or a
/// hand-written Python `subprocess` call and a cryptic panic, so every
/// rejection says specifically what was expected rather than just
/// "invalid arguments."
pub fn parse_args(args: &[String]) -> Result<Command, String> {
    let Some(subcommand) = args.first() else {
        return Err(usage());
    };

    match subcommand.as_str() {
        "register" => {
            let name = args.get(1).ok_or_else(|| format!("Missing <name>.\n\n{}", usage()))?;
            if name.trim().is_empty() {
                return Err("Source name can't be empty.".to_owned());
            }
            Ok(Command::Register { name: name.trim().to_owned() })
        }
        "log" => parse_log_args(&args[1..]),
        "sources" => Ok(Command::ListSources),
        "focus" => parse_focus_args(&args[1..]),
        "subjects" => Ok(Command::ListSubjects),
        "help" | "--help" | "-h" => Err(usage()),
        other => Err(format!("Unknown command '{other}'.\n\n{}", usage())),
    }
}

fn parse_log_args(args: &[String]) -> Result<Command, String> {
    let mut source = None;
    let mut subject = None;
    let mut minutes = None;
    let mut note = None;
    let mut date = None;

    let mut i = 0;
    while i < args.len() {
        let flag = args[i].as_str();
        let value = args.get(i + 1).ok_or_else(|| format!("'{flag}' needs a value."))?;
        match flag {
            "--source" => source = Some(value.clone()),
            "--subject" => subject = Some(value.clone()),
            "--minutes" => {
                minutes = Some(
                    value.parse::<i64>().map_err(|_| format!("'--minutes' must be a whole number, got '{value}'."))?,
                )
            }
            "--note" => note = Some(value.clone()),
            "--date" => {
                date = Some(
                    NaiveDate::parse_from_str(value, "%Y-%m-%d")
                        .map_err(|_| format!("'--date' must be in YYYY-MM-DD format, got '{value}'."))?,
                )
            }
            other => return Err(format!("Unknown flag '{other}'.\n\n{}", usage())),
        }
        i += 2;
    }

    let source = source.ok_or_else(|| format!("Missing required '--source <name-or-id>'.\n\n{}", usage()))?;
    let subject = subject.ok_or_else(|| format!("Missing required '--subject <name>'.\n\n{}", usage()))?;
    let minutes = minutes.ok_or_else(|| format!("Missing required '--minutes <n>'.\n\n{}", usage()))?;
    if minutes <= 0 {
        return Err("'--minutes' must be a positive number.".to_owned());
    }

    Ok(Command::Log { source, subject, minutes, note, date })
}

fn parse_focus_kind(value: &str) -> Result<FocusSource, String> {
    match value.to_ascii_lowercase().as_str() {
        "pomodoro" => Ok(FocusSource::Pomodoro),
        "timer" => Ok(FocusSource::Timer),
        "stopwatch" => Ok(FocusSource::Stopwatch),
        other => Err(format!("'--kind' must be pomodoro, timer or stopwatch, got '{other}'.")),
    }
}

fn parse_focus_args(args: &[String]) -> Result<Command, String> {
    let mut kind = None;
    let mut minutes = None;
    let mut subject = None;
    let mut title = None;
    let mut notify = true;

    let mut i = 0;
    while i < args.len() {
        let flag = args[i].as_str();
        // The one flag that takes no value.
        if flag == "--no-notify" {
            notify = false;
            i += 1;
            continue;
        }
        let value = args.get(i + 1).ok_or_else(|| format!("'{flag}' needs a value."))?;
        match flag {
            "--kind" => kind = Some(parse_focus_kind(value)?),
            "--minutes" => {
                minutes = Some(
                    value.parse::<i64>().map_err(|_| format!("'--minutes' must be a whole number, got '{value}'."))?,
                )
            }
            "--subject" => subject = Some(value.clone()),
            "--title" => title = Some(value.clone()),
            other => return Err(format!("Unknown flag '{other}'.\n\n{}", usage())),
        }
        i += 2;
    }

    let kind = kind.ok_or_else(|| format!("Missing required '--kind <pomodoro|timer|stopwatch>'.\n\n{}", usage()))?;
    let minutes = minutes.ok_or_else(|| format!("Missing required '--minutes <n>'.\n\n{}", usage()))?;
    if minutes <= 0 {
        return Err("'--minutes' must be a positive number.".to_owned());
    }
    Ok(Command::Focus { kind, minutes, subject, title, notify })
}

/// What a focus run is called on the Calendar when it has a subject, if the
/// caller didn't say — the same names the main app gives its own runs.
fn default_focus_title(kind: FocusSource) -> &'static str {
    match kind {
        FocusSource::Pomodoro => "Pomodoro focus session",
        FocusSource::Timer => "Timer focus session",
        FocusSource::Stopwatch => "Stopwatch focus session",
    }
}

/// `--source` accepts either the id `register` printed back or the
/// name itself — a script might reasonably hold on to either. `id`
/// is tried first only when the whole string parses as one, so a
/// source literally named e.g. "7" (unlikely, but not forbidden) isn't
/// silently unreachable by name.
fn resolve_source(db: &Database, name_or_id: &str) -> Result<Source, String> {
    if let Ok(id) = name_or_id.parse::<i64>() {
        if let Ok(Some(source)) = queries::sources::find_source(&db.conn, id) {
            return Ok(source);
        }
    }
    queries::sources::find_source_by_name(&db.conn, name_or_id)
        .map_err(|err| format!("Could not look up source: {err}"))?
        .ok_or_else(|| {
            format!("No registered source named '{name_or_id}'. Register it first: revise-cli register \"{name_or_id}\"")
        })
}

/// Name only, unlike `resolve_source` — a subject is something the
/// person already named and can see in "Manage Subjects"; there's no
/// equivalent of `register`'s printed-back id for a script to hold
/// onto instead, so there's nothing an id fallback would actually
/// help with here. Deliberately does *not* create a missing subject —
/// see the migration's own reasoning for `subject_id NOT NULL` on
/// `external_contributions`: a typo should surface as a clear error,
/// not silently create "Frnech".
fn resolve_subject(db: &Database, name: &str) -> Result<Subject, String> {
    queries::subjects::find_subject_by_name(&db.conn, name)
        .map_err(|err| format!("Could not look up subject: {err}"))?
        .ok_or_else(|| format!("No subject named '{name}'. Add it in Revise first (Manage Subjects)."))
}

/// Actually carries out an already-parsed `Command`, returning a
/// short, human-readable line for the CLI to print on success, or an
/// error naming what went wrong — most commonly an unresolvable
/// `--source`/`--subject`, the one kind of problem `parse_args` can't
/// catch on its own since it never touches the database.
pub fn run(db: &Database, command: Command) -> Result<String, String> {
    match command {
        Command::Register { name } => {
            let source =
                queries::sources::create_source(&db.conn, &name).map_err(|err| format!("Could not register '{name}': {err}"))?;
            Ok(format!("Registered '{}' as source id {}.", source.name, source.id))
        }

        Command::Log { source, subject, minutes, note, date } => {
            let source = resolve_source(db, &source)?;
            let subject = resolve_subject(db, &subject)?;
            let occurred_on = date.unwrap_or_else(|| chrono::Local::now().date_naive());

            let contribution =
                queries::contributions::create_contribution(&db.conn, source.id, subject.id, occurred_on, minutes, note.as_deref())
                    .map_err(|err| format!("Could not log contribution: {err}"))?;

            // Direct request context: the same free desktop-notification
            // feedback every other "something just happened" moment in
            // the app already gets (Pomodoro/Stopwatch/session start) —
            // this needs no connection to a running GUI, since
            // `notifications::send` talks to the desktop directly.
            crate::notifications::send(
                Some(&db.conn),
                "Time logged",
                &format!("{} min — {} via {}", contribution.minutes, subject.name, source.name),
            );

            Ok(format!(
                "Logged {} minutes for '{}' via '{}' on {}.",
                contribution.minutes, subject.name, source.name, contribution.occurred_on
            ))
        }

        Command::Focus { kind, minutes, subject, title, notify } => {
            // The subject is optional here, but a name that was given must exist: a
            // typo is an error, never a silently unattributed run.
            let subject = subject.as_deref().map(|name| resolve_subject(db, name)).transpose()?;
            let title = title.unwrap_or_else(|| default_focus_title(kind).to_owned());
            queries::pomodoro::record_focus_run(
                &db.conn,
                kind,
                &title,
                subject.as_ref().map(|s| s.id),
                minutes,
                minutes,
                chrono::Local::now(),
                false,
            )
            .map_err(|err| format!("Could not log focus time: {err}"))?;

            let whose = subject.as_ref().map(|s| format!(" for '{}'", s.name)).unwrap_or_default();
            if notify {
                crate::notifications::send(
                    Some(&db.conn),
                    "Time logged",
                    &format!("{minutes} min of {} focus{whose}", kind.as_str()),
                );
            }
            Ok(format!("Logged {minutes} minutes of {} focus time{whose}.", kind.as_str()))
        }

        Command::ListSubjects => {
            let subjects = queries::subjects::list_active_subjects(&db.conn)
                .map_err(|err| format!("Could not list subjects: {err}"))?;
            if subjects.is_empty() {
                return Ok("No subjects yet. Add one in Revise (Manage Subjects).".to_owned());
            }
            Ok(subjects.iter().map(|s| format!("{}\t{}", s.id, s.name)).collect::<Vec<_>>().join("\n"))
        }

        Command::ListSources => {
            let sources = queries::sources::list_all_sources(&db.conn).map_err(|err| format!("Could not list sources: {err}"))?;
            if sources.is_empty() {
                return Ok("No sources registered yet. Register one with: revise-cli register <name>".to_owned());
            }
            Ok(sources.iter().map(|s| format!("{}\t{}", s.id, s.name)).collect::<Vec<_>>().join("\n"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;

    fn args(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parse_args_with_nothing_returns_usage() {
        assert!(parse_args(&args(&[])).is_err());
    }

    #[test]
    fn parse_args_register() {
        assert_eq!(parse_args(&args(&["register", "French Quiz"])), Ok(Command::Register { name: "French Quiz".to_owned() }));
    }

    #[test]
    fn parse_args_register_trims_whitespace() {
        assert_eq!(
            parse_args(&args(&["register", "  French Quiz  "])),
            Ok(Command::Register { name: "French Quiz".to_owned() })
        );
    }

    #[test]
    fn parse_args_register_rejects_an_empty_name() {
        assert!(parse_args(&args(&["register", "   "])).is_err());
    }

    #[test]
    fn parse_args_register_without_a_name_is_an_error() {
        assert!(parse_args(&args(&["register"])).is_err());
    }

    #[test]
    fn parse_args_log_with_every_flag() {
        let command = parse_args(&args(&[
            "log", "--source", "French Quiz", "--subject", "French", "--minutes", "25", "--note", "Chapter 4",
            "--date", "2026-09-27",
        ]))
        .expect("should parse");
        assert_eq!(
            command,
            Command::Log {
                source: "French Quiz".to_owned(),
                subject: "French".to_owned(),
                minutes: 25,
                note: Some("Chapter 4".to_owned()),
                date: Some(NaiveDate::from_ymd_opt(2026, 9, 27).unwrap()),
            }
        );
    }

    #[test]
    fn parse_args_log_with_only_required_flags() {
        let command =
            parse_args(&args(&["log", "--source", "French Quiz", "--subject", "French", "--minutes", "25"]))
                .expect("should parse");
        assert_eq!(
            command,
            Command::Log { source: "French Quiz".to_owned(), subject: "French".to_owned(), minutes: 25, note: None, date: None }
        );
    }

    #[test]
    fn parse_args_log_missing_source_is_an_error() {
        assert!(parse_args(&args(&["log", "--subject", "French", "--minutes", "25"])).is_err());
    }

    #[test]
    fn parse_args_log_missing_subject_is_an_error() {
        assert!(parse_args(&args(&["log", "--source", "French Quiz", "--minutes", "25"])).is_err());
    }

    #[test]
    fn parse_args_log_missing_minutes_is_an_error() {
        assert!(parse_args(&args(&["log", "--source", "French Quiz", "--subject", "French"])).is_err());
    }

    #[test]
    fn parse_args_log_rejects_non_numeric_minutes() {
        assert!(
            parse_args(&args(&["log", "--source", "French Quiz", "--subject", "French", "--minutes", "soon"])).is_err()
        );
    }

    #[test]
    fn parse_args_log_rejects_zero_or_negative_minutes() {
        assert!(
            parse_args(&args(&["log", "--source", "French Quiz", "--subject", "French", "--minutes", "0"])).is_err()
        );
        assert!(
            parse_args(&args(&["log", "--source", "French Quiz", "--subject", "French", "--minutes", "-5"])).is_err()
        );
    }

    #[test]
    fn parse_args_log_rejects_a_malformed_date() {
        assert!(parse_args(&args(&[
            "log", "--source", "French Quiz", "--subject", "French", "--minutes", "25", "--date", "27/09/2026",
        ]))
        .is_err());
    }

    #[test]
    fn parse_args_log_rejects_an_unknown_flag() {
        assert!(parse_args(&args(&[
            "log", "--source", "French Quiz", "--subject", "French", "--minutes", "25", "--bogus", "x",
        ]))
        .is_err());
    }

    #[test]
    fn parse_args_sources() {
        assert_eq!(parse_args(&args(&["sources"])), Ok(Command::ListSources));
    }

    #[test]
    fn parse_args_rejects_an_unknown_subcommand() {
        assert!(parse_args(&args(&["frobnicate"])).is_err());
    }

    #[test]
    fn run_register_creates_a_source() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let result = run(&db, Command::Register { name: "French Quiz".to_owned() }).expect("should register");
        assert!(result.contains("French Quiz"));
        assert_eq!(queries::sources::list_all_sources(&db.conn).expect("list").len(), 1);
    }

    #[test]
    fn run_register_rejects_a_duplicate_name() {
        let db = Database::open_in_memory().expect("open in-memory db");
        run(&db, Command::Register { name: "French Quiz".to_owned() }).expect("first registers fine");
        assert!(run(&db, Command::Register { name: "French Quiz".to_owned() }).is_err());
    }

    #[test]
    fn run_log_creates_a_contribution_for_an_existing_source_and_subject() {
        let db = Database::open_in_memory().expect("open in-memory db");
        queries::subjects::create_subject(&db.conn, "French").expect("create subject");
        queries::sources::create_source(&db.conn, "French Quiz").expect("create source");

        let result = run(
            &db,
            Command::Log {
                source: "French Quiz".to_owned(),
                subject: "French".to_owned(),
                minutes: 25,
                note: None,
                date: None,
            },
        )
        .expect("should log");
        assert!(result.contains("25"));
        assert_eq!(queries::contributions::list_all_contributions(&db.conn).expect("list").len(), 1);
    }

    #[test]
    fn run_log_resolves_source_by_id_too() {
        let db = Database::open_in_memory().expect("open in-memory db");
        queries::subjects::create_subject(&db.conn, "French").expect("create subject");
        let source = queries::sources::create_source(&db.conn, "French Quiz").expect("create source");

        run(
            &db,
            Command::Log { source: source.id.to_string(), subject: "French".to_owned(), minutes: 25, note: None, date: None },
        )
        .expect("should log");
        assert_eq!(queries::contributions::list_all_contributions(&db.conn).expect("list").len(), 1);
    }

    #[test]
    fn run_log_is_case_insensitive_on_source_and_subject_names() {
        let db = Database::open_in_memory().expect("open in-memory db");
        queries::subjects::create_subject(&db.conn, "French").expect("create subject");
        queries::sources::create_source(&db.conn, "French Quiz").expect("create source");

        run(
            &db,
            Command::Log {
                source: "french quiz".to_owned(),
                subject: "FRENCH".to_owned(),
                minutes: 25,
                note: None,
                date: None,
            },
        )
        .expect("should log");
        assert_eq!(queries::contributions::list_all_contributions(&db.conn).expect("list").len(), 1);
    }

    #[test]
    fn run_log_fails_for_an_unregistered_source() {
        let db = Database::open_in_memory().expect("open in-memory db");
        queries::subjects::create_subject(&db.conn, "French").expect("create subject");

        let err = run(
            &db,
            Command::Log {
                source: "Nonexistent".to_owned(),
                subject: "French".to_owned(),
                minutes: 25,
                note: None,
                date: None,
            },
        )
        .unwrap_err();
        assert!(err.contains("Nonexistent"));
        assert!(queries::contributions::list_all_contributions(&db.conn).expect("list").is_empty());
    }

    #[test]
    fn run_log_fails_for_an_unknown_subject() {
        let db = Database::open_in_memory().expect("open in-memory db");
        queries::sources::create_source(&db.conn, "French Quiz").expect("create source");

        let err = run(
            &db,
            Command::Log {
                source: "French Quiz".to_owned(),
                subject: "Nonexistent".to_owned(),
                minutes: 25,
                note: None,
                date: None,
            },
        )
        .unwrap_err();
        assert!(err.contains("Nonexistent"));
        assert!(queries::contributions::list_all_contributions(&db.conn).expect("list").is_empty());
    }

    #[test]
    fn run_log_defaults_to_today_when_no_date_given() {
        let db = Database::open_in_memory().expect("open in-memory db");
        queries::subjects::create_subject(&db.conn, "French").expect("create subject");
        queries::sources::create_source(&db.conn, "French Quiz").expect("create source");

        run(
            &db,
            Command::Log { source: "French Quiz".to_owned(), subject: "French".to_owned(), minutes: 25, note: None, date: None },
        )
        .expect("should log");

        let contributions = queries::contributions::list_all_contributions(&db.conn).expect("list");
        assert_eq!(contributions[0].occurred_on, chrono::Local::now().date_naive());
    }

    #[test]
    fn run_sources_lists_every_registered_source() {
        let db = Database::open_in_memory().expect("open in-memory db");
        queries::sources::create_source(&db.conn, "French Quiz").expect("create");
        queries::sources::create_source(&db.conn, "Anki").expect("create");

        let result = run(&db, Command::ListSources).expect("should list");
        assert!(result.contains("French Quiz"));
        assert!(result.contains("Anki"));
    }

    #[test]
    fn run_sources_says_so_when_none_are_registered() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let result = run(&db, Command::ListSources).expect("should list");
        assert!(result.contains("No sources registered"));
    }

    // --- `focus` and `subjects` (the applet's way in) -----------------------

    #[test]
    fn parse_args_focus_needs_only_a_kind_and_minutes() {
        let command = parse_args(&args(&["focus", "--kind", "timer", "--minutes", "25"])).expect("should parse");
        assert_eq!(
            command,
            Command::Focus { kind: FocusSource::Timer, minutes: 25, subject: None, title: None, notify: true }
        );
    }

    #[test]
    fn parse_args_focus_with_every_flag() {
        let command = parse_args(&args(&[
            "focus", "--kind", "Pomodoro", "--minutes", "30", "--subject", "French", "--title", "Verbs", "--no-notify",
        ]))
        .expect("should parse");
        assert_eq!(
            command,
            Command::Focus {
                kind: FocusSource::Pomodoro,
                minutes: 30,
                subject: Some("French".to_owned()),
                title: Some("Verbs".to_owned()),
                notify: false,
            }
        );
    }

    #[test]
    fn parse_args_focus_rejects_bad_input_clearly() {
        assert!(parse_args(&args(&["focus", "--minutes", "25"])).unwrap_err().contains("--kind"));
        assert!(parse_args(&args(&["focus", "--kind", "timer"])).unwrap_err().contains("--minutes"));
        assert!(parse_args(&args(&["focus", "--kind", "nap", "--minutes", "5"])).unwrap_err().contains("pomodoro, timer or stopwatch"));
        assert!(parse_args(&args(&["focus", "--kind", "timer", "--minutes", "0"])).is_err());
        assert!(parse_args(&args(&["focus", "--kind", "timer", "--minutes", "x"])).is_err());
        assert!(parse_args(&args(&["focus", "--kind", "timer", "--minutes"])).unwrap_err().contains("needs a value"));
        assert!(parse_args(&args(&["focus", "--kind", "timer", "--minutes", "5", "--bogus", "1"])).is_err());
    }

    #[test]
    fn parse_args_subjects() {
        assert_eq!(parse_args(&args(&["subjects"])), Ok(Command::ListSubjects));
    }

    fn focus(kind: FocusSource, minutes: i64, subject: Option<&str>) -> Command {
        Command::Focus { kind, minutes, subject: subject.map(str::to_owned), title: None, notify: false }
    }

    #[test]
    fn a_focus_run_without_a_subject_counts_toward_focus_time_only() {
        let db = Database::open_in_memory().expect("open in-memory db");

        let message = run(&db, focus(FocusSource::Timer, 25, None)).expect("run");

        assert!(message.contains("25 minutes"), "{message}");
        assert_eq!(queries::pomodoro::total_minutes(&db.conn).unwrap(), 25);
        assert_eq!(queries::pomodoro::count_sessions(&db.conn).unwrap(), 0, "a timer is not a Pomodoro");
        assert!(queries::events::list_all_events(&db.conn).unwrap().is_empty(), "no subject, so no event");
    }

    #[test]
    fn a_focus_run_with_a_subject_also_counts_toward_that_subjects_hours() {
        let db = Database::open_in_memory().expect("open in-memory db");
        let french = queries::subjects::create_subject(&db.conn, "French").expect("subject");

        let message = run(&db, focus(FocusSource::Pomodoro, 25, Some("french"))).expect("run");

        assert!(message.contains("'French'"), "{message}");
        assert_eq!(queries::pomodoro::total_minutes(&db.conn).unwrap(), 25);
        assert_eq!(queries::pomodoro::count_sessions(&db.conn).unwrap(), 1);
        let events = queries::events::list_all_events(&db.conn).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].subject_id, Some(french.id));
        assert_eq!(events[0].title, "Pomodoro focus session");
        assert_eq!(queries::session_log::list_all_logs(&db.conn).unwrap().len(), 1, "logged as a completed session");
    }

    #[test]
    fn a_focus_title_can_be_given() {
        let db = Database::open_in_memory().expect("open in-memory db");
        queries::subjects::create_subject(&db.conn, "French").unwrap();
        let command = Command::Focus {
            kind: FocusSource::Stopwatch,
            minutes: 10,
            subject: Some("French".to_owned()),
            title: Some("Irregular verbs".to_owned()),
            notify: false,
        };
        run(&db, command).unwrap();
        assert_eq!(queries::events::list_all_events(&db.conn).unwrap()[0].title, "Irregular verbs");
    }

    #[test]
    fn a_focus_run_for_an_unknown_subject_is_an_error_and_logs_nothing() {
        let db = Database::open_in_memory().expect("open in-memory db");

        let err = run(&db, focus(FocusSource::Timer, 25, Some("Frnech"))).unwrap_err();

        assert!(err.contains("No subject named 'Frnech'"), "{err}");
        assert_eq!(queries::pomodoro::total_minutes(&db.conn).unwrap(), 0);
    }

    #[test]
    fn subjects_lists_the_active_ones_for_scripts() {
        let db = Database::open_in_memory().expect("open in-memory db");
        assert!(run(&db, Command::ListSubjects).unwrap().contains("No subjects yet"));
        let french = queries::subjects::create_subject(&db.conn, "French").unwrap();
        let listed = run(&db, Command::ListSubjects).unwrap();
        assert_eq!(listed, format!("{}\tFrench", french.id));
    }
}

