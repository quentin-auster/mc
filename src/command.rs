pub enum JumpTarget {
    Steps(i32), // negative = back, positive = forward
    Hash(String),
}

pub enum Command {
    Jump(JumpTarget),
    Branch(Option<String>),
    Merge(String), // hash of node to merge from
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
        "branch" | "b" => Command::Branch(if arg.is_empty() { None } else { Some(arg.to_string()) }),
        "merge" | "m" => {
            if arg.is_empty() {
                Command::Unknown("merge requires a hash: /merge <hash>".into())
            } else {
                Command::Merge(arg.to_string())
            }
        }
        "quit" | "q" | "exit" => Command::Quit,
        _ => Command::Unknown(name),
    }
}
