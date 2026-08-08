use crate::bingo::error::{BingoError, Result};
use teloxide::types::UserId;

pub fn db_user_id(user_id: UserId) -> Result<i64> {
    i64::try_from(user_id.0).map_err(|_| BingoError::UserIdOutOfRange(user_id.0))
}

pub fn user_id_from_db(user_id: i64) -> Result<UserId> {
    u64::try_from(user_id)
        .map(UserId)
        .map_err(|_| BingoError::InvalidStoredUserId(user_id))
}

#[cfg(test)]
mod tests {
    use crate::bingo::store::id::{db_user_id, user_id_from_db};
    use claims::{assert_err, assert_ok_eq};
    use teloxide::types::UserId;

    #[test]
    fn converts_valid_user_ids() {
        assert_ok_eq!(db_user_id(UserId(42)), 42);
        assert_ok_eq!(user_id_from_db(42), UserId(42));
    }

    #[test]
    fn rejects_user_ids_outside_sqlite_integer_range() {
        assert_err!(db_user_id(UserId(u64::MAX)));
        assert_err!(user_id_from_db(-1));
    }
}
