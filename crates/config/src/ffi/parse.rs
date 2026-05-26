use crate::error::ParseOutcome;

pub(crate) fn parse_from_toml_str(s: &str) -> Box<ParseOutcome> {
    ParseOutcome::from_result(crate::parse_from_toml_str(s))
}

pub(crate) fn parse_from_ini_str(s: &str) -> Box<ParseOutcome> {
    ParseOutcome::from_result(crate::parse_from_ini_str(s))
}

pub(crate) fn parse_from_file(path: &str) -> Box<ParseOutcome> {
    ParseOutcome::from_result(crate::parse_from_file(path))
}
