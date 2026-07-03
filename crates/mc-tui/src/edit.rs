use std::fmt;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum EditStrategy {
    Patch,
    Macro,
}

impl EditStrategy {
    pub fn parse(input: &str) -> Option<Self> {
        match input.trim().to_lowercase().as_str() {
            "patch" => Some(Self::Patch),
            "macro" | "command" | "commands" => Some(Self::Macro),
            _ => None,
        }
    }
}

impl fmt::Display for EditStrategy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Patch => write!(f, "patch"),
            Self::Macro => write!(f, "macro"),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RangeSpec {
    Lines { start: usize, end: usize },
    Anchor { pattern: String },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EditOp {
    OpenFile {
        path: String,
    },
    Substitute {
        range: Option<RangeSpec>,
        pattern: String,
        replacement: String,
        global: bool,
    },
    InsertBefore {
        anchor: String,
        text: String,
    },
    InsertAfter {
        anchor: String,
        text: String,
    },
    DeleteRange {
        range: RangeSpec,
    },
    ReplaceRange {
        range: RangeSpec,
        text: String,
    },
}

pub fn apply_to_string(input: &str, op: &EditOp) -> Result<String, String> {
    match op {
        EditOp::OpenFile { .. } => Ok(input.to_string()),
        EditOp::Substitute {
            range,
            pattern,
            replacement,
            global,
        } => apply_substitute(input, range.as_ref(), pattern, replacement, *global),
        EditOp::InsertBefore { anchor, text } => insert_relative(input, anchor, text, false),
        EditOp::InsertAfter { anchor, text } => insert_relative(input, anchor, text, true),
        EditOp::DeleteRange { range } => replace_range(input, range, ""),
        EditOp::ReplaceRange { range, text } => replace_range(input, range, text),
    }
}

fn apply_substitute(
    input: &str,
    range: Option<&RangeSpec>,
    pattern: &str,
    replacement: &str,
    global: bool,
) -> Result<String, String> {
    if pattern.is_empty() {
        return Err("substitute pattern cannot be empty".to_string());
    }

    let mut lines: Vec<String> = input.lines().map(str::to_string).collect();
    let had_trailing_newline = input.ends_with('\n');
    let (start, end) = range_bounds(input, range)?;
    let mut replaced = false;

    for line in &mut lines[start..end] {
        if line.contains(pattern) {
            replaced = true;
            if global {
                *line = line.replace(pattern, replacement);
            } else {
                *line = line.replacen(pattern, replacement, 1);
            }
        }
    }

    if !replaced {
        return Err(format!("pattern '{pattern}' not found"));
    }

    Ok(join_lines(lines, had_trailing_newline))
}

fn insert_relative(input: &str, anchor: &str, text: &str, after: bool) -> Result<String, String> {
    if anchor.is_empty() {
        return Err("anchor cannot be empty".to_string());
    }

    let mut lines: Vec<String> = input.lines().map(str::to_string).collect();
    let had_trailing_newline = input.ends_with('\n');
    let index = lines
        .iter()
        .position(|line| line.contains(anchor))
        .ok_or_else(|| format!("anchor '{anchor}' not found"))?;
    let insert_at = if after { index + 1 } else { index };
    let new_lines: Vec<String> = text.lines().map(str::to_string).collect();
    lines.splice(insert_at..insert_at, new_lines);
    Ok(join_lines(lines, had_trailing_newline))
}

fn replace_range(input: &str, range: &RangeSpec, text: &str) -> Result<String, String> {
    let mut lines: Vec<String> = input.lines().map(str::to_string).collect();
    let had_trailing_newline = input.ends_with('\n');
    let (start, end) = range_bounds(input, Some(range))?;
    let replacement: Vec<String> = text.lines().map(str::to_string).collect();
    lines.splice(start..end, replacement);
    Ok(join_lines(lines, had_trailing_newline))
}

fn range_bounds(input: &str, range: Option<&RangeSpec>) -> Result<(usize, usize), String> {
    let lines: Vec<&str> = input.lines().collect();
    match range {
        None => Ok((0, lines.len())),
        Some(RangeSpec::Lines { start, end }) => {
            if *start == 0 || end < start || *end > lines.len() {
                return Err(format!("invalid line range {start}..{end}"));
            }
            Ok((start - 1, *end))
        }
        Some(RangeSpec::Anchor { pattern }) => {
            let index = lines
                .iter()
                .position(|line| line.contains(pattern))
                .ok_or_else(|| format!("anchor '{pattern}' not found"))?;
            Ok((index, index + 1))
        }
    }
}

fn join_lines(lines: Vec<String>, trailing_newline: bool) -> String {
    let mut out = lines.join("\n");
    if trailing_newline {
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn substitutes_within_line_range() {
        let input = "alpha\nbeta\nbeta\n";
        let op = EditOp::Substitute {
            range: Some(RangeSpec::Lines { start: 2, end: 2 }),
            pattern: "beta".to_string(),
            replacement: "gamma".to_string(),
            global: false,
        };

        assert_eq!(apply_to_string(input, &op).unwrap(), "alpha\ngamma\nbeta\n");
    }

    #[test]
    fn inserts_after_anchor() {
        let input = "one\ntwo\n";
        let op = EditOp::InsertAfter {
            anchor: "one".to_string(),
            text: "inserted".to_string(),
        };

        assert_eq!(apply_to_string(input, &op).unwrap(), "one\ninserted\ntwo\n");
    }

    #[test]
    fn deletes_line_range() {
        let input = "one\ntwo\nthree\n";
        let op = EditOp::DeleteRange {
            range: RangeSpec::Lines { start: 2, end: 2 },
        };

        assert_eq!(apply_to_string(input, &op).unwrap(), "one\nthree\n");
    }

    #[test]
    fn substitutes_anchor_line() {
        let input = "one\ntwo\nthree\n";
        let op = EditOp::Substitute {
            range: Some(RangeSpec::Anchor {
                pattern: "two".to_string(),
            }),
            pattern: "two".to_string(),
            replacement: "deux".to_string(),
            global: false,
        };

        assert_eq!(apply_to_string(input, &op).unwrap(), "one\ndeux\nthree\n");
    }

    #[test]
    fn replaces_line_range() {
        let input = "one\ntwo\nthree\n";
        let op = EditOp::ReplaceRange {
            range: RangeSpec::Lines { start: 2, end: 3 },
            text: "replacement".to_string(),
        };

        assert_eq!(apply_to_string(input, &op).unwrap(), "one\nreplacement\n");
    }
}
