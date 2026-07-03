use std::path::{Component, Path};
use std::process::Command as ShellCommand;

use crate::agent::AgentMode;
use crate::command::{self, Command, JumpTarget};
use crate::context::{self, ContextLedger};
use crate::conversation::tree::{ConversationTree, LearningMetadata};
use crate::edit::{self, EditOp, EditStrategy};
use crate::vim::{self, VimCommand};

#[derive(Copy, Clone, PartialEq)]
pub enum Panel {
    Chat,
    Tree,
}

pub struct Status {
    pub message: String,
    pub is_error: bool,
}

pub struct ShellEntry {
    pub command: String,
    pub output: String,
    pub success: bool,
}

pub struct App {
    pub active_panel: Panel,
    pub input: String,
    pub tree: ConversationTree,
    pub agent_mode: AgentMode,
    pub edit_strategy: EditStrategy,
    /// Transient feedback from the last command. Cleared on next message submission.
    pub status: Option<Status>,
    /// Cursor index into `tree.display_entries()`. Used for tree panel navigation.
    pub tree_cursor: usize,
    /// Shell command output log. Persists for the session regardless of active branch.
    pub shell_log: Vec<ShellEntry>,
    /// Structured context gathered through read-only harness commands.
    pub context_ledger: ContextLedger,
    /// File selected by compact edit commands such as `:e`.
    pub open_file: Option<String>,
    /// Last 10 inputs, oldest first.
    pub history: Vec<String>,
    /// Index into history while browsing: Some(0) = newest, Some(1) = second-newest, etc.
    history_pos: Option<usize>,
    /// Saved input from before the user started browsing history.
    history_draft: String,
    pub should_quit: bool,
}

impl App {
    pub fn new() -> Self {
        Self {
            active_panel: Panel::Chat,
            input: String::new(),
            tree: ConversationTree::new(),
            agent_mode: AgentMode::Normal,
            edit_strategy: EditStrategy::Patch,
            status: None,
            tree_cursor: 0,
            shell_log: Vec::new(),
            context_ledger: ContextLedger::new(),
            open_file: None,
            history: Vec::new(),
            history_pos: None,
            history_draft: String::new(),
            should_quit: false,
        }
    }

    pub fn toggle_panel(&mut self) {
        self.active_panel = match self.active_panel {
            Panel::Chat => Panel::Tree,
            Panel::Tree => Panel::Chat,
        };
    }

    pub fn process_input(&mut self) {
        let input: String = self.input.drain(..).collect();
        self.history_pos = None;
        self.history_draft = String::new();
        if input.is_empty() {
            return;
        }
        self.push_history(input.clone());
        if input.starts_with('!') {
            self.run_shell(input[1..].trim());
        } else if input.starts_with(':') {
            self.handle_vim_command(&input);
        } else if input.starts_with('/') {
            self.handle_command(&input);
        } else {
            self.status = None;
            self.submit_message(input);
        }
    }

    /// Navigate backwards through history (older entries).
    pub fn history_up(&mut self) {
        let len = self.history.len();
        if len == 0 {
            return;
        }
        let next_pos = match self.history_pos {
            None => {
                self.history_draft = self.input.clone();
                0
            }
            Some(n) if n + 1 < len => n + 1,
            Some(n) => n, // already at oldest
        };
        self.history_pos = Some(next_pos);
        self.input = self.history[len - 1 - next_pos].clone();
    }

    /// Navigate forwards through history (newer entries, back to draft).
    pub fn history_down(&mut self) {
        match self.history_pos {
            None => {}
            Some(0) => {
                self.history_pos = None;
                self.input = self.history_draft.clone();
            }
            Some(n) => {
                let next = n - 1;
                self.history_pos = Some(next);
                self.input = self.history[self.history.len() - 1 - next].clone();
            }
        }
    }

    /// Move tree cursor up one entry.
    pub fn tree_cursor_up(&mut self) {
        self.tree_cursor = self.tree_cursor.saturating_sub(1);
    }

    /// Move tree cursor down one entry.
    pub fn tree_cursor_down(&mut self) {
        let max = self.tree.display_entries().len().saturating_sub(1);
        if self.tree_cursor < max {
            self.tree_cursor += 1;
        }
    }

    /// Jump active to whichever node the tree cursor is pointing at.
    pub fn tree_cursor_jump(&mut self) {
        let entries = self.tree.display_entries();
        if let Some(&(node_id, _)) = entries.get(self.tree_cursor) {
            self.tree.active = node_id;
            let hash = self.tree.nodes[&node_id].hash.clone();
            self.set_status(format!("jumped to {hash}"), false);
        }
    }

    fn run_shell(&mut self, cmd: &str) {
        let result = ShellCommand::new("sh").arg("-c").arg(cmd).output();
        match result {
            Ok(out) => {
                let mut output = String::from_utf8_lossy(&out.stdout).into_owned();
                let stderr = String::from_utf8_lossy(&out.stderr);
                if !stderr.is_empty() {
                    output.push_str(&stderr);
                }
                let output = output.trim_end().to_string();
                self.shell_log.push(ShellEntry {
                    command: cmd.to_string(),
                    output,
                    success: out.status.success(),
                });
            }
            Err(e) => {
                self.shell_log.push(ShellEntry {
                    command: cmd.to_string(),
                    output: format!("error: {e}"),
                    success: false,
                });
            }
        }
    }

    fn push_history(&mut self, input: String) {
        if self.history.last().map(String::as_str) == Some(&input) {
            return; // don't duplicate consecutive identical entries
        }
        self.history.push(input);
        if self.history.len() > 10 {
            self.history.remove(0);
        }
    }

    fn submit_message(&mut self, content: String) {
        let label = truncate_label(&content, 20);
        let parent = self.tree.active;
        let id = self.tree.add_child(parent, label, Some(content));
        self.tree.active = id;
        if self.agent_mode == AgentMode::Learning {
            self.seed_learning_response(id);
        }
        self.sync_cursor();
    }

    fn handle_command(&mut self, input: &str) {
        match command::parse(input) {
            Command::Jump(target) => {
                let result = match target {
                    JumpTarget::Steps(n) if n < 0 => self.tree.go_back((-n) as usize),
                    JumpTarget::Steps(n) if n > 0 => self.tree.go_forward(n as usize),
                    JumpTarget::Steps(_) => Ok(()),
                    JumpTarget::Hash(h) => match self.tree.find_by_hash(&h) {
                        Some(id) => {
                            self.tree.active = id;
                            Ok(())
                        }
                        None => Err(format!("no node matching '{h}'")),
                    },
                };
                self.set_status_from(result, |hash| format!("jumped to {hash}"));
                self.sync_cursor();
            }
            Command::Branch(name) => {
                let label = name.unwrap_or_else(|| "branch".to_string());
                let parent = self.tree.active;
                let id = self.tree.add_child(parent, label.clone(), None);
                self.tree.active = id;
                let hash = self.tree.nodes[&id].hash.clone();
                self.set_status(format!("branched to {hash} ({label})"), false);
                self.sync_cursor();
            }
            Command::Merge(hash_prefix) => match self.tree.find_by_hash(&hash_prefix) {
                Some(from_id) if from_id == self.tree.active => {
                    self.set_status("cannot merge a node with itself", true);
                }
                Some(from_id) => {
                    let from_hash = self.tree.nodes[&from_id].hash.clone();
                    let parent = self.tree.active;
                    let label = format!("merge {from_hash}");
                    let id = self.tree.add_merge_child(parent, from_id, label);
                    self.tree.active = id;
                    let new_hash = self.tree.nodes[&id].hash.clone();
                    self.set_status(format!("merged {from_hash} → {new_hash}"), false);
                    self.sync_cursor();
                }
                None => self.set_status(format!("no node matching '{hash_prefix}'"), true),
            },
            Command::Mode(mode) => {
                self.agent_mode = mode;
                self.set_status(format!("agent mode: {mode}"), false);
            }
            Command::Strategy(strategy) => {
                self.edit_strategy = strategy;
                self.set_status(format!("edit strategy: {strategy}"), false);
            }
            Command::Context(context_command) => {
                match context::execute(&context_command, &mut self.context_ledger) {
                    Ok(message) => self.set_status(message, false),
                    Err(error) => self.set_status(error, true),
                }
            }
            Command::Hint => self.reveal_learning_part(LearningPart::Hint),
            Command::Check => self.reveal_learning_part(LearningPart::Check),
            Command::Reveal => self.reveal_learning_part(LearningPart::Reveal),
            Command::Quit => self.should_quit = true,
            Command::Unknown(msg) => self.set_status(msg, true),
        }
    }

    fn handle_vim_command(&mut self, input: &str) {
        match vim::parse(input) {
            Ok(VimCommand::Edit(EditOp::OpenFile { path })) => {
                self.open_file = Some(path.clone());
                self.set_status(format!("open file: {path}"), false);
            }
            Ok(VimCommand::Edit(op)) if self.edit_strategy == EditStrategy::Macro => {
                match self.validate_macro_edit(&op) {
                    Ok(message) => self.set_status(message, false),
                    Err(error) => self.set_status(error, true),
                }
            }
            Ok(VimCommand::Edit(_)) => {
                self.set_status(
                    "vim edit parsed; switch to /strategy macro to validate replay",
                    false,
                );
            }
            Ok(VimCommand::Search {
                pattern,
                context_after,
            }) => {
                let message = if context_after == 0 {
                    format!("vim search parsed for '{pattern}'")
                } else {
                    format!("vim search parsed for '{pattern}' with +{context_after} context")
                };
                self.set_status(message, false);
            }
            Err(error) => self.set_status(error, true),
        }
    }

    fn validate_macro_edit(&self, op: &EditOp) -> Result<String, String> {
        let Some(path) = &self.open_file else {
            return Err("macro edit requires an open file; use :e <path> first".to_string());
        };
        ensure_workspace_relative(path)?;
        let content =
            std::fs::read_to_string(path).map_err(|e| format!("failed to read {path}: {e}"))?;
        edit::apply_to_string(&content, op)?;
        Ok(format!("macro edit validated against {path}"))
    }

    fn seed_learning_response(&mut self, id: usize) {
        let user_content = self.tree.nodes[&id]
            .user_content
            .clone()
            .unwrap_or_default();
        let metadata = LearningMetadata {
            objective: user_content.clone(),
            hint: "Start by identifying the smallest observable behavior and write that down before editing.".to_string(),
            expected_direction: "A good solution should preserve the existing shape of the code, make the smallest targeted change, and verify the behavior with a focused test or command.".to_string(),
        };
        let response = format!(
            "Learning mode scaffold\nObjective: {}\n\nTODO for you:\n1. Name the behavior you want to change.\n2. Identify the file or command that gives the first signal.\n3. Draft the smallest implementation step.\n\nUse /hint for a nudge, /check for a review prompt, or /reveal for the expected direction.",
            metadata.objective
        );
        self.tree.set_assistant_content(id, response);
        self.tree.set_learning_metadata(id, metadata);
    }

    fn reveal_learning_part(&mut self, part: LearningPart) {
        let node_id = self.tree.active;
        let Some(node) = self.tree.nodes.get(&node_id) else {
            self.set_status("no active node", true);
            return;
        };
        let Some(metadata) = &node.learning_metadata else {
            self.set_status("active node has no learning metadata", true);
            return;
        };

        let addition = match part {
            LearningPart::Hint => format!("Hint: {}", metadata.hint),
            LearningPart::Check => {
                "Check: explain what you changed, what behavior proves it, and what edge case still worries you.".to_string()
            }
            LearningPart::Reveal => format!("Expected direction: {}", metadata.expected_direction),
        };

        let existing = node.assistant_content.clone().unwrap_or_default();
        self.tree
            .set_assistant_content(node_id, format!("{existing}\n\n{addition}"));
        self.set_status("updated learning guidance", false);
    }

    fn sync_cursor(&mut self) {
        let entries = self.tree.display_entries();
        if let Some(pos) = entries.iter().position(|&(id, _)| id == self.tree.active) {
            self.tree_cursor = pos;
        }
    }

    fn set_status_from(&mut self, result: Result<(), String>, ok_msg: impl Fn(&str) -> String) {
        match result {
            Ok(()) => {
                let hash = self.tree.nodes[&self.tree.active].hash.clone();
                self.set_status(ok_msg(&hash), false);
            }
            Err(e) => self.set_status(e, true),
        }
    }

    fn set_status(&mut self, message: impl Into<String>, is_error: bool) {
        self.status = Some(Status {
            message: message.into(),
            is_error,
        });
    }
}

enum LearningPart {
    Hint,
    Check,
    Reveal,
}

fn truncate_label(s: &str, max_chars: usize) -> String {
    let mut chars = s.chars();
    let prefix: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{prefix}…")
    } else {
        prefix
    }
}

fn ensure_workspace_relative(path: &str) -> Result<(), String> {
    let path = Path::new(path);
    if path.is_absolute() {
        return Err("macro edit paths must be relative to the workspace".to_string());
    }
    if path
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return Err("macro edit paths cannot contain '..'".to_string());
    }
    Ok(())
}
