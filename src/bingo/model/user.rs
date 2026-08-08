use std::fmt;
use teloxide::types::UserId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnownUser {
    pub user_id: UserId,
    pub username: Option<String>,
    pub display_name: String,
}

impl fmt::Display for KnownUser {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.username {
            Some(name) => write!(formatter, "@{name}"),
            None => formatter.write_str(&self.display_name),
        }
    }
}
