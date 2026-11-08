use std::collections::HashSet;

use chrono::NaiveDate;
use rusqlite::{params, Connection, Result};

use crate::events::{Frequency, RecurrenceRule};

fn row_to_rule(row: &rusqlite::Row) -> rusqlite::Result<RecurrenceRule> {
    let frequency_raw: String = row.get(1)?;
    let frequency = Frequency::parse(&frequency_raw).unwrap_or(Frequency::Weekly);
    let weekdays_raw: Option<String> = row.get(3)?;
    let end_date_raw: Option<String> = row.get(4)?;

    Ok(RecurrenceRule {
        id: Some(row.get(0)?),
        frequency,
        interval: row.get::<_, i64>(2)? as u32,
        weekdays: RecurrenceRule::weekdays_from_storage(weekdays_raw.as_deref()),
        end_date: end_date_raw.and_then(|s| NaiveDate::parse_from_str(&s, "%Y-%m-%d").ok()),
        occurrence_limit: row.get::<_, Option<i64>>(5)?.map(|n| n as u32),
    })
}

const SELECT_COLUMNS: &str = "id, frequency, interval, weekdays, end_date, occurrence_limit";

pub fn create_rule(conn: &Connection, rule: &RecurrenceRule) -> Result<i64> {
    conn.execute(
        "INSERT INTO recurrence_rules (frequency, interval, weekdays, end_date, occurrence_limit)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            rule.frequency.as_str(),
            rule.interval,
            rule.weekdays_to_storage(),
            rule.end_date.map(|d| d.format("%Y-%m-%d").to_string()),
            rule.occurrence_limit,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn update_rule(conn: &Connection, id: i64, rule: &RecurrenceRule) -> Result<()> {
    conn.execute(
        "UPDATE recurrence_rules SET frequency = ?1, interval = ?2, weekdays = ?3, end_date = ?4, occurrence_limit = ?5
         WHERE id = ?6",
        params![
            rule.frequency.as_str(),
            rule.interval,
            rule.weekdays_to_storage(),
            rule.end_date.map(|d| d.format("%Y-%m-%d").to_string()),
            rule.occurrence_limit,
            id,
        ],
    )?;
    Ok(())
}

pub fn get_rule(conn: &Connection, id: i64) -> Result<Option<RecurrenceRule>> {
    conn.query_row(&format!("SELECT {SELECT_COLUMNS} FROM recurrence_rules WHERE id = ?1"), params![id], row_to_rule)
        .map(Some)
        .or_else(|err| if matches!(err, rusqlite::Error::QueryReturnedNoRows) { Ok(None) } else { Err(err) })
}

pub fn delete_rule(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM recurrence_rules WHERE id = ?1", params![id])?;
    Ok(())
}

/// Marks one occurrence date as skipped without touching the rule or
/// any other occurrence — Sprint 6's "individual occurrences can be
/// handled without corrupting the parent recurrence".
pub fn add_exception(conn: &Connection, event_id: i64, date: NaiveDate) -> Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO event_exceptions (event_id, occurrence_date) VALUES (?1, ?2)",
        params![event_id, date.format("%Y-%m-%d").to_string()],
    )?;
    Ok(())
}

pub fn list_exceptions(conn: &Connection, event_id: i64) -> Result<HashSet<NaiveDate>> {
    let mut stmt = conn.prepare("SELECT occurrence_date FROM event_exceptions WHERE event_id = ?1")?;
    let dates = stmt
        .query_map(params![event_id], |row| row.get::<_, String>(0))?
        .filter_map(|r| r.ok())
        .filter_map(|s| NaiveDate::parse_from_str(&s, "%Y-%m-%d").ok())
        .collect();
    Ok(dates)
}

pub fn delete_exceptions_for_event(conn: &Connection, event_id: i64) -> Result<()> {
    conn.execute("DELETE FROM event_exceptions WHERE event_id = ?1", params![event_id])?;
    Ok(())
}
