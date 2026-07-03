#[derive(Clone, Debug)]
pub struct DiffEntry {
    pub path: String,
    pub lines: Vec<DiffLine>,
}

#[derive(Clone, Debug)]
pub enum DiffLine {
    Header(String),
    Context(String),
    Added(String),
    Removed(String),
}

pub fn unified(path: &str, before: &str, after: &str) -> DiffEntry {
    let mut lines = vec![
        DiffLine::Header(format!("--- {path}")),
        DiffLine::Header(format!("+++ {path}")),
        DiffLine::Header("@@ harness edit @@".to_string()),
    ];

    let before_lines: Vec<&str> = before.lines().collect();
    let after_lines: Vec<&str> = after.lines().collect();
    let max = before_lines.len().max(after_lines.len());

    for index in 0..max {
        match (before_lines.get(index), after_lines.get(index)) {
            (Some(old), Some(new)) if old == new => {
                lines.push(DiffLine::Context(format!(" {old}")))
            }
            (Some(old), Some(new)) => {
                lines.push(DiffLine::Removed(format!("-{old}")));
                lines.push(DiffLine::Added(format!("+{new}")));
            }
            (Some(old), None) => lines.push(DiffLine::Removed(format!("-{old}"))),
            (None, Some(new)) => lines.push(DiffLine::Added(format!("+{new}"))),
            (None, None) => {}
        }
    }

    DiffEntry {
        path: path.to_string(),
        lines,
    }
}
