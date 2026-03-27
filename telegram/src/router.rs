use crate::commands::Command;
use teloxide::utils::command::BotCommands;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RouteAction {
    HandleCommand(Command),
    HandleMessage,
    Ignore,
}

pub fn decide_route(text: Option<&str>, bot_name: &str) -> RouteAction {
    let Some(text) = text else {
        return RouteAction::Ignore;
    };
    Command::parse(text, bot_name)
        .map_or_else(|_| RouteAction::HandleMessage, RouteAction::HandleCommand)
}
