use crate::edit::{EditOp, RangeSpec};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum VimCommand {
    Edit(EditOp),
    Search {
        pattern: String,
        context_after: usize,
    },
}

pub fn parse(input: &str) -> Result<VimCommand, String> {
    let input = input.trim();
    if !input.starts_with(':') {
        return Err("vim command must start with ':'".to_string());
    }
    let body = &input[1..];

    if let Some(path) = body.strip_prefix("e ") {
        let path = path.trim();
        if path.is_empty() {
            return Err(":e requires a path".to_string());
        }
        return Ok(VimCommand::Edit(EditOp::OpenFile {
            path: path.to_string(),
        }));
    }

    if let Some(rest) = body.strip_prefix('/') {
        return parse_search(rest);
    }

    if let Some(rest) = body.strip_prefix("%s") {
        let (pattern, replacement, global) = parse_substitute_tail(rest)?;
        return Ok(VimCommand::Edit(EditOp::Substitute {
            range: None,
            pattern,
            replacement,
            global,
        }));
    }

    if let Some(rest) = body.strip_prefix('s') {
        let (pattern, replacement, global) = parse_substitute_tail(rest)?;
        return Ok(VimCommand::Edit(EditOp::Substitute {
            range: None,
            pattern,
            replacement,
            global,
        }));
    }

    if let Some(rest) = body.strip_suffix('d') {
        let range = parse_line_range(rest)?;
        return Ok(VimCommand::Edit(EditOp::DeleteRange { range }));
    }

    if let Some((anchor, text)) = parse_anchor_insert(body, " i ") {
        return Ok(VimCommand::Edit(EditOp::InsertBefore { anchor, text }));
    }

    if let Some((anchor, text)) = parse_anchor_insert(body, " a ") {
        return Ok(VimCommand::Edit(EditOp::InsertAfter { anchor, text }));
    }

    Err(format!("unsupported vim command ':{body}'"))
}

fn parse_search(rest: &str) -> Result<VimCommand, String> {
    let (pattern, tail) = rest
        .split_once('/')
        .ok_or_else(|| "search command requires a closing '/'".to_string())?;
    if pattern.is_empty() {
        return Err("search pattern cannot be empty".to_string());
    }

    let context_after = if tail.is_empty() {
        0
    } else if let Some(raw) = tail.strip_prefix(",+") {
        raw.parse::<usize>()
            .map_err(|_| "search context count must be a number".to_string())?
    } else {
        return Err("search only supports :/pattern/ or :/pattern/,+N".to_string());
    };

    Ok(VimCommand::Search {
        pattern: pattern.to_string(),
        context_after,
    })
}

fn parse_substitute_tail(rest: &str) -> Result<(String, String, bool), String> {
    let delimiter = rest
        .chars()
        .next()
        .ok_or_else(|| "substitute requires a delimiter".to_string())?;
    let parts: Vec<&str> = rest[delimiter.len_utf8()..].split(delimiter).collect();
    if parts.len() < 3 {
        return Err("substitute requires pattern and replacement".to_string());
    }
    let pattern = parts[0];
    if pattern.is_empty() {
        return Err("substitute pattern cannot be empty".to_string());
    }
    let replacement = parts[1];
    let flags = parts[2];
    Ok((
        pattern.to_string(),
        replacement.to_string(),
        flags.contains('g'),
    ))
}

fn parse_line_range(input: &str) -> Result<RangeSpec, String> {
    let (start, end) = input
        .split_once(',')
        .ok_or_else(|| "delete requires a line range like :2,4d".to_string())?;
    let start = start
        .parse::<usize>()
        .map_err(|_| "range start must be a number".to_string())?;
    let end = end
        .parse::<usize>()
        .map_err(|_| "range end must be a number".to_string())?;
    if start == 0 || end < start {
        return Err("line range must be 1-based and increasing".to_string());
    }
    Ok(RangeSpec::Lines { start, end })
}

fn parse_anchor_insert(body: &str, operator: &str) -> Option<(String, String)> {
    if !body.starts_with('/') {
        return None;
    }
    let rest = &body[1..];
    let (anchor, tail) = rest.split_once('/')?;
    let text = tail.strip_prefix(operator)?;
    Some((anchor.to_string(), text.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_edit_file() {
        assert_eq!(
            parse(":e crates/mc-tui/src/main.rs").unwrap(),
            VimCommand::Edit(EditOp::OpenFile {
                path: "crates/mc-tui/src/main.rs".to_string()
            })
        );
    }

    #[test]
    fn parses_search_with_context() {
        assert_eq!(
            parse(":/fn main/,+3").unwrap(),
            VimCommand::Search {
                pattern: "fn main".to_string(),
                context_after: 3
            }
        );
    }

    #[test]
    fn parses_global_substitute() {
        assert_eq!(
            parse(":%s/foo/bar/g").unwrap(),
            VimCommand::Edit(EditOp::Substitute {
                range: None,
                pattern: "foo".to_string(),
                replacement: "bar".to_string(),
                global: true,
            })
        );
    }

    #[test]
    fn parses_delete_range() {
        assert_eq!(
            parse(":2,4d").unwrap(),
            VimCommand::Edit(EditOp::DeleteRange {
                range: RangeSpec::Lines { start: 2, end: 4 },
            })
        );
    }
}
