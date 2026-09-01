use crate::bingo::error::{BingoError, Result};
use std::str::FromStr;

pub fn parse_slug_and_target(
    input: &str,
    expected: &str,
) -> Result<(Option<String>, Option<String>)> {
    let mut words = input.split_whitespace();
    match (words.next(), words.next(), words.next()) {
        (None, None, None) => Ok((None, None)),
        (Some(target), None, None) if is_target_token(target) => {
            Ok((None, Some(target.to_owned())))
        }
        (Some(slug), None, None) => Ok((Some(slug.to_owned()), None)),
        (Some(slug), Some(target), None) if is_target_token(target) => {
            Ok((Some(slug.to_owned()), Some(target.to_owned())))
        }
        _ => Err(usage(expected)),
    }
}

pub fn parse_required_slug_and_optional_target(input: &str) -> Result<(String, Option<String>)> {
    const USAGE: &str = "import <slug> [@user] followed by five grid rows";

    let (slug, rest) = split_once_whitespace(input);
    if slug.is_empty() {
        return Err(usage(USAGE));
    }
    if rest.is_empty() {
        return Ok((slug.to_owned(), None));
    }
    let target = required_word(rest, USAGE)?;
    if !is_target_token(target) {
        return Err(usage(USAGE));
    }
    Ok((slug.to_owned(), Some(target.to_owned())))
}

pub fn required_pair<'a>(input: &'a str, expected: &str) -> Result<(&'a str, &'a str)> {
    let (first, rest) = split_once_whitespace(input);
    if first.is_empty() || rest.is_empty() {
        return Err(usage(expected));
    }
    Ok((first, rest))
}

pub fn required_word<'a>(input: &'a str, expected: &str) -> Result<&'a str> {
    let word = input.trim();
    if word.is_empty() || word.contains(char::is_whitespace) {
        return Err(usage(expected));
    }
    Ok(word)
}

pub fn optional_word(input: &str) -> Option<&str> {
    let value = input.trim();
    (!value.is_empty()).then_some(value)
}

pub fn parse_id<T>(raw: &str, label: &str) -> Result<T>
where
    T: FromStr,
{
    raw.parse()
        .map_err(|_| BingoError::InvalidCommand(format!("Invalid {label} `{raw}`")))
}

pub fn is_target_token(word: &str) -> bool {
    word.starts_with('@') || (!word.is_empty() && word.chars().all(|value| value.is_ascii_digit()))
}

pub fn usage(expected: &str) -> BingoError {
    BingoError::InvalidCommand(format!("Usage: /bingo {expected}"))
}

pub fn split_once_whitespace(input: &str) -> (&str, &str) {
    input
        .trim()
        .split_once(char::is_whitespace)
        .map_or_else(|| (input.trim(), ""), |(head, rest)| (head, rest.trim()))
}
