use crate::agent::AgentMode;
use crate::edit::EditStrategy;

pub enum JumpTarget {
    Steps(i32), // negative = back, positive = forward
    Hash(String),
}

#[derive(Debug, Eq, PartialEq)]
pub enum ContextAction {
    List,
    Pin(usize),
    Drop(usize),
    Clear,
}

#[derive(Debug, Eq, PartialEq)]
pub enum ContextCommand {
    Rg(String),
    Files(Option<String>),
    Head {
        path: String,
        lines: usize,
    },
    Tail {
        path: String,
        lines: usize,
    },
    Wc {
        path: String,
    },
    Sed {
        path: String,
        start: usize,
        end: usize,
    },
    Awk {
        path: String,
        pattern: String,
    },
    Ledger(ContextAction),
}

pub enum Command {
    Jump(JumpTarget),
    Branch(Option<String>),
    Merge(String), // hash of node to merge from
    Mode(AgentMode),
    Strategy(EditStrategy),
    Context(ContextCommand),
    Hint,
    Check,
    Reveal,
    Quit,
    Unknown(String),
}

pub fn parse(input: &str) -> Command {
    let trimmed = input.trim_start_matches('/');
    let mut parts = trimmed.splitn(2, ' ');
    let name = parts.next().unwrap_or("").to_lowercase();
    let arg = parts.next().unwrap_or("").trim();

    match name.as_str() {
        "jump" | "j" => {
            if arg.is_empty() {
                Command::Unknown("jump requires an argument: /jump <±steps | hash>".into())
            } else if let Ok(n) = arg.parse::<i32>() {
                Command::Jump(JumpTarget::Steps(n))
            } else {
                Command::Jump(JumpTarget::Hash(arg.to_string()))
            }
        }
        "branch" | "b" => Command::Branch(if arg.is_empty() {
            None
        } else {
            Some(arg.to_string())
        }),
        "merge" | "m" => {
            if arg.is_empty() {
                Command::Unknown("merge requires a hash: /merge <hash>".into())
            } else {
                Command::Merge(arg.to_string())
            }
        }
        "mode" => match AgentMode::parse(arg) {
            Some(mode) => Command::Mode(mode),
            None => Command::Unknown("mode requires one of: normal, learning".into()),
        },
        "strategy" => match EditStrategy::parse(arg) {
            Some(strategy) => Command::Strategy(strategy),
            None => Command::Unknown("strategy requires one of: patch, macro".into()),
        },
        "rg" => {
            if arg.is_empty() {
                Command::Unknown("rg requires a pattern: /rg <pattern>".into())
            } else {
                Command::Context(ContextCommand::Rg(arg.to_string()))
            }
        }
        "files" => Command::Context(ContextCommand::Files(if arg.is_empty() {
            None
        } else {
            Some(arg.to_string())
        })),
        "head" => parse_path_lines(arg, "head")
            .map(|(path, lines)| Command::Context(ContextCommand::Head { path, lines }))
            .unwrap_or_else(Command::Unknown),
        "tail" => parse_path_lines(arg, "tail")
            .map(|(path, lines)| Command::Context(ContextCommand::Tail { path, lines }))
            .unwrap_or_else(Command::Unknown),
        "wc" => {
            if arg.is_empty() {
                Command::Unknown("wc requires a path: /wc <path>".into())
            } else {
                Command::Context(ContextCommand::Wc {
                    path: arg.to_string(),
                })
            }
        }
        "sed" => parse_sed(arg)
            .map(|(path, start, end)| Command::Context(ContextCommand::Sed { path, start, end }))
            .unwrap_or_else(Command::Unknown),
        "awk" => parse_awk(arg)
            .map(|(path, pattern)| Command::Context(ContextCommand::Awk { path, pattern }))
            .unwrap_or_else(Command::Unknown),
        "context" | "ctx" => parse_context_action(arg)
            .map(|action| Command::Context(ContextCommand::Ledger(action)))
            .unwrap_or_else(Command::Unknown),
        "hint" => Command::Hint,
        "check" => Command::Check,
        "reveal" => Command::Reveal,
        "quit" | "q" | "exit" => Command::Quit,
        _ => Command::Unknown(name),
    }
}

fn parse_path_lines(arg: &str, command: &str) -> Result<(String, usize), String> {
    let mut parts = arg.split_whitespace();
    let path = parts
        .next()
        .ok_or_else(|| format!("{command} requires a path: /{command} <path> [lines]"))?;
    let lines = match parts.next() {
        Some(raw) => raw
            .parse::<usize>()
            .map_err(|_| format!("{command} line count must be a positive number"))?,
        None => 20,
    };
    if lines == 0 {
        return Err(format!("{command} line count must be a positive number"));
    }
    Ok((path.to_string(), lines))
}

fn parse_sed(arg: &str) -> Result<(String, usize, usize), String> {
    let mut parts = arg.split_whitespace();
    let path = parts
        .next()
        .ok_or_else(|| "sed requires: /sed <path> <start> <end>".to_string())?;
    let start = parts
        .next()
        .ok_or_else(|| "sed requires a start line".to_string())?
        .parse::<usize>()
        .map_err(|_| "sed start line must be a positive number".to_string())?;
    let end = parts
        .next()
        .ok_or_else(|| "sed requires an end line".to_string())?
        .parse::<usize>()
        .map_err(|_| "sed end line must be a positive number".to_string())?;
    if start == 0 || end < start {
        return Err("sed requires a valid 1-based line range".to_string());
    }
    Ok((path.to_string(), start, end))
}

fn parse_awk(arg: &str) -> Result<(String, String), String> {
    let mut parts = arg.splitn(2, char::is_whitespace);
    let path = parts.next().unwrap_or("").trim();
    let pattern = parts.next().unwrap_or("").trim();
    if path.is_empty() || pattern.is_empty() {
        return Err("awk requires: /awk <path> <pattern>".to_string());
    }
    Ok((path.to_string(), pattern.to_string()))
}

fn parse_context_action(arg: &str) -> Result<ContextAction, String> {
    let mut parts = arg.split_whitespace();
    match parts.next().unwrap_or("") {
        "list" | "" => Ok(ContextAction::List),
        "pin" => parse_context_id(parts.next(), "pin").map(ContextAction::Pin),
        "drop" => parse_context_id(parts.next(), "drop").map(ContextAction::Drop),
        "clear" => Ok(ContextAction::Clear),
        other => Err(format!("unknown context action '{other}'")),
    }
}

fn parse_context_id(raw: Option<&str>, action: &str) -> Result<usize, String> {
    raw.ok_or_else(|| format!("context {action} requires an id"))?
        .parse::<usize>()
        .map_err(|_| format!("context {action} id must be a positive number"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_mode_command() {
        match parse("/mode learning") {
            Command::Mode(AgentMode::Learning) => {}
            _ => panic!("expected learning mode"),
        }
    }

    #[test]
    fn parses_strategy_command() {
        match parse("/strategy macro") {
            Command::Strategy(EditStrategy::Macro) => {}
            _ => panic!("expected macro strategy"),
        }
    }

    #[test]
    fn parses_context_sed_command() {
        match parse("/sed src/main.rs 2 4") {
            Command::Context(ContextCommand::Sed { path, start, end }) => {
                assert_eq!(path, "src/main.rs");
                assert_eq!(start, 2);
                assert_eq!(end, 4);
            }
            _ => panic!("expected sed command"),
        }
    }

    #[test]
    fn parses_context_ledger_command() {
        match parse("/context pin 3") {
            Command::Context(ContextCommand::Ledger(ContextAction::Pin(3))) => {}
            _ => panic!("expected context pin command"),
        }
    }
}
