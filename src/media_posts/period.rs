use chrono::{DateTime, Months, TimeDelta, Utc};
use strum::EnumString;

/// Time window for `/stats`, counted back from the moment of the request.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, EnumString)]
#[strum(serialize_all = "lowercase", ascii_case_insensitive)]
pub enum StatsPeriod {
    Week,
    Month,
    Year,
    #[default]
    All,
}

impl StatsPeriod {
    /// Parse a `/stats` argument; an empty argument selects all time.
    ///
    /// # Errors
    ///
    /// Returns an error for anything other than `week`, `month`, `year`, or `all`.
    pub fn from_argument(input: &str) -> Result<Self, strum::ParseError> {
        let input = input.trim();
        if input.is_empty() {
            return Ok(Self::default());
        }
        input.parse()
    }

    /// Earliest share time included in the period, or `None` for all time.
    pub fn since(self, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
        match self {
            Self::Week => now.checked_sub_signed(TimeDelta::weeks(1)),
            Self::Month => now.checked_sub_months(Months::new(1)),
            Self::Year => now.checked_sub_months(Months::new(12)),
            Self::All => None,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Week => "past week",
            Self::Month => "past month",
            Self::Year => "past year",
            Self::All => "all time",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use claims::{assert_err, assert_ok_eq, assert_some, assert_some_eq};
    use rstest::rstest;

    fn utc(year: i32, month: u32, day: u32) -> DateTime<Utc> {
        assert_some!(Utc.with_ymd_and_hms(year, month, day, 12, 0, 0).single())
    }

    #[rstest]
    #[case("", StatsPeriod::All)]
    #[case("  ", StatsPeriod::All)]
    #[case("all", StatsPeriod::All)]
    #[case("week", StatsPeriod::Week)]
    #[case(" Month ", StatsPeriod::Month)]
    #[case("YEAR", StatsPeriod::Year)]
    fn parses_arguments(#[case] input: &str, #[case] expected: StatsPeriod) {
        assert_ok_eq!(StatsPeriod::from_argument(input), expected);
    }

    #[test]
    fn rejects_unknown_arguments() {
        assert_err!(StatsPeriod::from_argument("day"));
    }

    #[test]
    fn month_and_year_follow_the_calendar() {
        let now = utc(2026, 3, 31);
        assert_some_eq!(StatsPeriod::Month.since(now), utc(2026, 2, 28));
        assert_some_eq!(StatsPeriod::Year.since(now), utc(2025, 3, 31));
        assert_some_eq!(StatsPeriod::Week.since(now), utc(2026, 3, 24));
    }
}
