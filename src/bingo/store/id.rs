use crate::bingo::error::{BingoError, Result};
use teloxide::types::UserId;

pub(super) const fn db_user_id(user_id: UserId) -> [u8; 8] {
    user_id.0.to_be_bytes()
}

pub(super) fn user_id_from_db(user_id: &[u8]) -> Result<UserId> {
    let bytes = user_id
        .try_into()
        .map_err(|_| BingoError::InvalidStoredUserId)?;
    Ok(UserId(u64::from_be_bytes(bytes)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use claims::{assert_err, assert_ok_eq};

    #[test]
    fn user_ids_round_trip_across_the_full_range() {
        for user_id in [UserId(0), UserId(42), UserId(u64::MAX)] {
            assert_ok_eq!(user_id_from_db(&db_user_id(user_id)), user_id);
        }
    }

    #[test]
    fn rejects_invalid_database_values() {
        assert_err!(user_id_from_db(&[]));
        assert_err!(user_id_from_db(&[0; 7]));
        assert_err!(user_id_from_db(&[0; 9]));
    }
}
