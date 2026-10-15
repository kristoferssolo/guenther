use std::fmt;
use teloxide::types::{User, UserId};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnownUser {
    pub user_id: UserId,
    pub username: Option<String>,
    pub display_name: String,
}

impl From<&User> for KnownUser {
    fn from(user: &User) -> Self {
        let display_name = user.last_name.as_ref().map_or_else(
            || user.first_name.clone(),
            |last_name| format!("{} {last_name}", user.first_name),
        );
        Self {
            user_id: user.id,
            username: user.username.clone(),
            display_name,
        }
    }
}

impl fmt::Display for KnownUser {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.username {
            Some(name) => write!(formatter, "@{name}"),
            None => formatter.write_str(&self.display_name),
        }
    }
}
