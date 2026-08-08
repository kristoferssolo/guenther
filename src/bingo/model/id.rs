use crate::bingo::error::BingoError;
use std::{fmt, num::ParseIntError, str::FromStr};

macro_rules! define_id {
    ($name:ident) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, sqlx::Type)]
        #[sqlx(transparent)]
        pub struct $name(i64);

        impl $name {
            #[must_use]
            pub const fn get(self) -> i64 {
                self.0
            }
        }

        impl From<i64> for $name {
            fn from(value: i64) -> Self {
                Self(value)
            }
        }

        impl From<$name> for i64 {
            fn from(value: $name) -> Self {
                value.0
            }
        }

        impl FromStr for $name {
            type Err = ParseIntError;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                value.parse().map(Self)
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(formatter)
            }
        }
    };
}

define_id!(CardId);
define_id!(EntryId);
define_id!(GameId);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EntryNumber(i64);

impl EntryNumber {
    #[must_use]
    pub const fn get(self) -> i64 {
        self.0
    }
}

impl TryFrom<i64> for EntryNumber {
    type Error = BingoError;

    fn try_from(value: i64) -> Result<Self, Self::Error> {
        if value.is_positive() {
            Ok(Self(value))
        } else {
            Err(BingoError::InvalidStoredEntryNumber(value))
        }
    }
}

impl FromStr for EntryNumber {
    type Err = BingoError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let number = value.parse::<i64>().map_err(|_| {
            BingoError::InvalidCommand("entry numbers must be positive integers".to_owned())
        })?;
        if number.is_positive() {
            Ok(Self(number))
        } else {
            Err(BingoError::InvalidCommand(
                "entry numbers must be positive integers".to_owned(),
            ))
        }
    }
}

impl fmt::Display for EntryNumber {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use claims::{assert_err, assert_ok, assert_ok_eq};

    #[test]
    fn identifiers_parse_and_display() {
        assert_ok_eq!("42".parse::<CardId>(), CardId::from(42));
        assert_ok_eq!("42".parse::<EntryId>(), EntryId::from(42));
        let entry_number = assert_ok!(EntryNumber::try_from(42));
        assert_ok_eq!("42".parse::<EntryNumber>(), entry_number);
        assert_err!("0".parse::<EntryNumber>());
        assert_err!("-1".parse::<EntryNumber>());
        assert_ok_eq!("42".parse::<GameId>(), GameId::from(42));
        assert_eq!(CardId::from(42).to_string(), "42");
    }
}
