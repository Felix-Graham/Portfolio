use chrono::Weekday;

/// How often a recurring event repeats. "Never repeat" isn't a
/// variant here — it's represented by the event simply having no
/// `RecurrenceRule` at all (`Event.recurrence_id: None`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Frequency {
    Daily,
    Weekly,
    Monthly,
}

impl Frequency {
    pub fn as_str(&self) -> &'static str {
        match self {
            Frequency::Daily => "daily",
            Frequency::Weekly => "weekly",
            Frequency::Monthly => "monthly",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "daily" => Some(Frequency::Daily),
            "weekly" => Some(Frequency::Weekly),
            "monthly" => Some(Frequency::Monthly),
            _ => None,
        }
    }
}

/// A repeat rule for a revision session: covers the plan's "Daily /
/// Every N days", "Weekly / Every N weeks / Selected weekdays",
/// "Monthly", "Optional end date", and "Optional occurrence limit" in
/// one structure. The event it's attached to still owns the anchor
/// date and time-of-day; this only describes the repetition pattern.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecurrenceRule {
    pub id: Option<i64>,
    pub frequency: Frequency,
    /// "Every N days/weeks/months". `1` means every day/week/month.
    pub interval: u32,
    /// Only meaningful for `Weekly`. Empty means "the same weekday as
    /// the anchor event" (plain weekly recurrence); non-empty means
    /// "selected weekdays" per the plan's example.
    pub weekdays: Vec<Weekday>,
    pub end_date: Option<chrono::NaiveDate>,
    pub occurrence_limit: Option<u32>,
}

impl RecurrenceRule {
    /// Serializes `weekdays` for storage: comma-separated
    /// days-from-Monday, e.g. selecting Tue+Thu is `"1,3"`.
    pub fn weekdays_to_storage(&self) -> Option<String> {
        if self.weekdays.is_empty() {
            None
        } else {
            Some(
                self.weekdays
                    .iter()
                    .map(|d| d.num_days_from_monday().to_string())
                    .collect::<Vec<_>>()
                    .join(","),
            )
        }
    }

    pub fn weekdays_from_storage(raw: Option<&str>) -> Vec<Weekday> {
        raw.map(|s| {
            s.split(',')
                .filter_map(|part| part.trim().parse::<u32>().ok())
                .filter_map(weekday_from_monday_index)
                .collect()
        })
        .unwrap_or_default()
    }

    /// A short, human-readable summary shown as a badge on recurring
    /// event blocks — e.g. "Every 2 weeks (Tue, Thu), 10x".
    pub fn describe(&self) -> String {
        let unit = match self.frequency {
            Frequency::Daily => "day",
            Frequency::Weekly => "week",
            Frequency::Monthly => "month",
        };
        let mut summary = if self.interval <= 1 {
            format!("Every {unit}")
        } else {
            format!("Every {} {unit}s", self.interval)
        };

        if self.frequency == Frequency::Weekly && !self.weekdays.is_empty() {
            let mut days = self.weekdays.clone();
            days.sort_by_key(|d| d.num_days_from_monday());
            let names: Vec<&str> = days.iter().map(|d| weekday_short_name(*d)).collect();
            summary.push_str(&format!(" ({})", names.join(", ")));
        }

        if let Some(limit) = self.occurrence_limit {
            summary.push_str(&format!(", {limit}x"));
        }
        if let Some(end) = self.end_date {
            summary.push_str(&format!(", until {}", end.format("%d %b")));
        }

        summary
    }
}

fn weekday_short_name(d: Weekday) -> &'static str {
    match d {
        Weekday::Mon => "Mon",
        Weekday::Tue => "Tue",
        Weekday::Wed => "Wed",
        Weekday::Thu => "Thu",
        Weekday::Fri => "Fri",
        Weekday::Sat => "Sat",
        Weekday::Sun => "Sun",
    }
}

fn weekday_from_monday_index(index: u32) -> Option<Weekday> {
    [
        Weekday::Mon,
        Weekday::Tue,
        Weekday::Wed,
        Weekday::Thu,
        Weekday::Fri,
        Weekday::Sat,
        Weekday::Sun,
    ]
    .get(index as usize)
    .copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weekday_storage_roundtrips() {
        let rule = RecurrenceRule {
            id: None,
            frequency: Frequency::Weekly,
            interval: 1,
            weekdays: vec![Weekday::Tue, Weekday::Thu],
            end_date: None,
            occurrence_limit: None,
        };
        let stored = rule.weekdays_to_storage().unwrap();
        let restored = RecurrenceRule::weekdays_from_storage(Some(&stored));
        assert_eq!(restored, vec![Weekday::Tue, Weekday::Thu]);
    }

    #[test]
    fn empty_weekdays_stores_as_none() {
        let rule = RecurrenceRule {
            id: None,
            frequency: Frequency::Daily,
            interval: 1,
            weekdays: Vec::new(),
            end_date: None,
            occurrence_limit: None,
        };
        assert_eq!(rule.weekdays_to_storage(), None);
    }

    #[test]
    fn describe_simple_daily() {
        let rule = RecurrenceRule {
            id: None,
            frequency: Frequency::Daily,
            interval: 1,
            weekdays: Vec::new(),
            end_date: None,
            occurrence_limit: None,
        };
        assert_eq!(rule.describe(), "Every day");
    }

    #[test]
    fn describe_every_n_weeks_with_selected_days_and_limit() {
        let rule = RecurrenceRule {
            id: None,
            frequency: Frequency::Weekly,
            interval: 2,
            weekdays: vec![Weekday::Thu, Weekday::Tue],
            end_date: None,
            occurrence_limit: Some(10),
        };
        assert_eq!(rule.describe(), "Every 2 weeks (Tue, Thu), 10x");
    }
}
