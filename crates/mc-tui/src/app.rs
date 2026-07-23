use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Command as ShellCommand;
use std::time::Instant;

use ratatui::layout::Rect;

use crate::agent::AgentMode;
use crate::command::{self, Command, ExpandTarget, JumpTarget};
use crate::context::{self, ContextLedger};
use crate::conversation::tree::{
    ActivityAction, ActivityKind, ConversationTree, LearningMetadata, NodeId,
};
use crate::diff::{self, DiffEntry};
use crate::edit::{self, EditOp, EditStrategy};
use crate::prompt_plan;
use crate::provider::{Provider, ProviderConfig};
use crate::telemetry::{self, TelemetryRecorder};
use crate::vim::{self, VimCommand};

#[derive(Copy, Clone, PartialEq)]
pub enum Panel {
    Chat,
    Tree,
    Files,
}

#[derive(Copy, Clone, Eq, PartialEq)]
pub enum MainView {
    Activity,
    File,
}

pub struct Status {
    pub message: String,
    pub is_error: bool,
}

pub struct FileBuffer {
    pub path: String,
    pub content: String,
    pub original: String,
    pub cursor_line: usize,
}

impl FileBuffer {
    pub fn is_dirty(&self) -> bool {
        self.content != self.original
    }
}

pub struct FsEntry {
    pub path: String,
    pub name: String,
    pub is_dir: bool,
}

pub struct FsState {
    pub current_dir: String,
    pub cursor: usize,
    pub entries: Vec<FsEntry>,
}

pub enum GasStep {
    ChooseProvider,
    EnterKey(Provider),
}

impl FsState {
    fn new() -> Self {
        let mut state = Self {
            current_dir: String::new(),
            cursor: 0,
            entries: Vec::new(),
        };
        state.refresh();
        state
    }

    fn refresh(&mut self) {
        self.entries = list_workspace_dir(&self.current_dir).unwrap_or_default();
        self.cursor = self.cursor.min(self.entries.len().saturating_sub(1));
    }
}

pub struct App {
    pub active_panel: Panel,
    pub main_view: MainView,
    pub input: String,
    pub input_cursor: usize,
    pub tree: ConversationTree,
    pub agent_mode: AgentMode,
    pub edit_strategy: EditStrategy,
    pub provider: Provider,
    pub model: Option<String>,
    pub gas_step: Option<GasStep>,
    /// Transient feedback from the last command. Cleared on next message submission.
    pub status: Option<Status>,
    /// Cursor index into `tree.display_entries()`. Used for tree panel navigation.
    pub tree_cursor: usize,
    /// Structured context gathered through read-only harness commands.
    pub context_ledger: ContextLedger,
    /// File selected by compact edit commands such as `:e`.
    pub open_file: Option<String>,
    pub file_buffer: Option<FileBuffer>,
    pub fs: FsState,
    telemetry: TelemetryRecorder,
    command_count_since_turn: usize,
    edit_bytes_since_turn: usize,
    pub layout: LayoutState,
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
        let provider = Provider::default_from_config().unwrap_or(Provider::Anthropic);
        let model = Provider::default_model_from_config();
        Self {
            active_panel: Panel::Chat,
            main_view: MainView::Activity,
            input: String::new(),
            input_cursor: 0,
            tree: ConversationTree::new(),
            agent_mode: AgentMode::Normal,
            edit_strategy: EditStrategy::Patch,
            provider,
            model,
            gas_step: None,
            status: None,
            tree_cursor: 0,
            context_ledger: ContextLedger::new(),
            open_file: None,
            file_buffer: None,
            fs: FsState::new(),
            telemetry: TelemetryRecorder::default(),
            command_count_since_turn: 0,
            edit_bytes_since_turn: 0,
            layout: LayoutState::default(),
            history: Vec::new(),
            history_pos: None,
            history_draft: String::new(),
            should_quit: false,
        }
    }

    pub fn toggle_panel(&mut self) {
        self.active_panel = match self.active_panel {
            Panel::Chat => Panel::Tree,
            Panel::Tree => Panel::Files,
            Panel::Files => Panel::Chat,
        };
    }

    pub fn toggle_main_view(&mut self) {
        self.main_view = match self.main_view {
            MainView::Activity => MainView::File,
            MainView::File => MainView::Activity,
        };
    }

    pub fn is_secret_input(&self) -> bool {
        matches!(self.gas_step, Some(GasStep::EnterKey(_)))
    }

    pub fn process_input(&mut self) {
        let input: String = self.input.drain(..).collect();
        self.input_cursor = 0;
        self.history_pos = None;
        self.history_draft = String::new();
        if input.is_empty() {
            return;
        }
        if self.gas_step.is_some() {
            self.handle_gas_input(input);
            return;
        }
        self.push_history(input.clone());
        if input.starts_with('!') {
            self.command_count_since_turn += 1;
            self.run_shell(input[1..].trim());
        } else if input.starts_with(':') {
            self.command_count_since_turn += 1;
            self.handle_vim_command(&input);
        } else if input.starts_with('/') || input.trim() == "?" {
            self.command_count_since_turn += 1;
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
        self.input_cursor = self.input.len();
    }

    /// Navigate forwards through history (newer entries, back to draft).
    pub fn history_down(&mut self) {
        match self.history_pos {
            None => {}
            Some(0) => {
                self.history_pos = None;
                self.input = self.history_draft.clone();
                self.input_cursor = self.input.len();
            }
            Some(n) => {
                let next = n - 1;
                self.history_pos = Some(next);
                self.input = self.history[self.history.len() - 1 - next].clone();
                self.input_cursor = self.input.len();
            }
        }
    }

    pub fn insert_char(&mut self, c: char) {
        self.input.insert(self.input_cursor, c);
        self.input_cursor += c.len_utf8();
    }

    pub fn backspace(&mut self) {
        if self.input_cursor == 0 {
            return;
        }
        let prev = previous_boundary(&self.input, self.input_cursor);
        self.input.drain(prev..self.input_cursor);
        self.input_cursor = prev;
    }

    pub fn delete_forward(&mut self) {
        if self.input_cursor >= self.input.len() {
            return;
        }
        let next = next_boundary(&self.input, self.input_cursor);
        self.input.drain(self.input_cursor..next);
    }

    pub fn cursor_left(&mut self) {
        self.input_cursor = previous_boundary(&self.input, self.input_cursor);
    }

    pub fn cursor_right(&mut self) {
        self.input_cursor = next_boundary(&self.input, self.input_cursor);
    }

    pub fn cursor_home(&mut self) {
        self.input_cursor = 0;
    }

    pub fn cursor_end(&mut self) {
        self.input_cursor = self.input.len();
    }

    pub fn cursor_word_left(&mut self) {
        let prefix = &self.input[..self.input_cursor];
        let trimmed = prefix.trim_end_matches(char::is_whitespace);
        let mut target = trimmed.len();
        while target > 0 {
            let prev = previous_boundary(trimmed, target);
            let ch = trimmed[prev..target].chars().next().unwrap_or_default();
            if ch.is_whitespace() {
                break;
            }
            target = prev;
        }
        self.input_cursor = target;
    }

    pub fn cursor_word_right(&mut self) {
        let suffix = &self.input[self.input_cursor..];
        let mut offset = 0;
        let mut seen_word = false;
        for ch in suffix.chars() {
            if ch.is_whitespace() {
                if seen_word {
                    break;
                }
            } else {
                seen_word = true;
            }
            offset += ch.len_utf8();
        }
        self.input_cursor += offset;
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

    pub fn fs_cursor_up(&mut self) {
        self.fs.cursor = self.fs.cursor.saturating_sub(1);
    }

    pub fn fs_cursor_down(&mut self) {
        let max = self.fs.entries.len().saturating_sub(1);
        if self.fs.cursor < max {
            self.fs.cursor += 1;
        }
    }

    pub fn fs_cursor_open(&mut self) {
        let Some(entry) = self.fs.entries.get(self.fs.cursor) else {
            return;
        };
        if entry.is_dir {
            self.open_fs_dir(entry.path.clone());
        } else {
            self.handle_file_open(entry.path.clone());
        }
    }

    pub fn file_cursor_up(&mut self) {
        if let Some(buffer) = &mut self.file_buffer {
            buffer.cursor_line = buffer.cursor_line.saturating_sub(1);
        }
    }

    pub fn file_cursor_down(&mut self) {
        if let Some(buffer) = &mut self.file_buffer {
            let max = buffer.content.lines().count().saturating_sub(1);
            if buffer.cursor_line < max {
                buffer.cursor_line += 1;
            }
        }
    }

    pub fn focus_chat(&mut self) {
        self.active_panel = Panel::Chat;
    }

    pub fn focus_tree(&mut self) {
        self.active_panel = Panel::Tree;
    }

    pub fn focus_files(&mut self) {
        self.active_panel = Panel::Files;
    }

    pub fn click_tree_row(&mut self, row: u16) {
        self.focus_tree();
        if let Some(node_id) = self
            .layout
            .tree_rows
            .iter()
            .find_map(|(y, id)| (*y == row).then_some(*id))
        {
            let entries = self.tree.display_entries();
            if let Some(pos) = entries.iter().position(|&(id, _)| id == node_id) {
                self.tree_cursor = pos;
                self.tree.active = node_id;
                let hash = self.tree.nodes[&node_id].hash.clone();
                self.set_status(format!("selected {hash}"), false);
            }
        }
    }

    pub fn click_fs_row(&mut self, row: u16) {
        self.focus_files();
        if let Some(path) = self
            .layout
            .fs_rows
            .iter()
            .find_map(|(y, path)| (*y == row).then_some(path.clone()))
        {
            if let Some(pos) = self.fs.entries.iter().position(|entry| entry.path == path) {
                self.fs.cursor = pos;
                self.fs_cursor_open();
            }
        }
    }

    fn run_shell(&mut self, cmd: &str) {
        if std::env::var("MC_ENABLE_SHELL").as_deref() != Ok("1") {
            let message =
                "shell execution is disabled; use safe context commands or set MC_ENABLE_SHELL=1";
            self.add_active_action(ActivityAction {
                kind: ActivityKind::System,
                title: "blocked shell command".to_string(),
                detail: format!("!{cmd}\n{message}"),
                expanded: true,
            });
            self.set_status(message, true);
            return;
        }

        let result = ShellCommand::new("sh").arg("-c").arg(cmd).output();
        match result {
            Ok(out) => {
                let mut output = String::from_utf8_lossy(&out.stdout).into_owned();
                let stderr = String::from_utf8_lossy(&out.stderr);
                if !stderr.is_empty() {
                    output.push_str(&stderr);
                }
                let output = output.trim_end().to_string();
                self.add_active_action(ActivityAction {
                    kind: ActivityKind::Shell,
                    title: format!("$ {cmd}"),
                    detail: output,
                    expanded: false,
                });
            }
            Err(e) => {
                let output = format!("error: {e}");
                self.add_active_action(ActivityAction {
                    kind: ActivityKind::Shell,
                    title: format!("$ {cmd}"),
                    detail: output,
                    expanded: true,
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
        let started = Instant::now();
        let label = truncate_label(&content, 20);
        let parent = self.tree.active;
        let id = self.tree.add_child(parent, label, Some(content));
        self.tree.active = id;
        if self.agent_mode == AgentMode::Learning {
            self.seed_learning_response(id);
        }
        self.record_turn_telemetry(id, started.elapsed().as_millis());
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
                    Ok(message) => {
                        self.add_active_action(ActivityAction {
                            kind: ActivityKind::System,
                            title: "context".to_string(),
                            detail: message.clone(),
                            expanded: false,
                        });
                        self.set_status(message, false);
                    }
                    Err(error) => self.set_status(error, true),
                }
            }
            Command::Hint => self.reveal_learning_part(LearningPart::Hint),
            Command::Check => self.reveal_learning_part(LearningPart::Check),
            Command::Reveal => self.reveal_learning_part(LearningPart::Reveal),
            Command::Help => self.show_help(),
            Command::Gas => self.start_gas_setup(),
            Command::Provider(provider) => self.handle_provider(provider),
            Command::Model(model) => {
                self.model = Some(model.clone());
                self.set_status(format!("model: {model}"), false);
            }
            Command::FileOpen(path) => self.handle_file_open(path),
            Command::FileRead { path, start, end } => self.handle_file_read(path, start, end),
            Command::FileWrite { path, content } => self.handle_file_write(path, content),
            Command::BufferSet { line, content } => self.handle_buffer_set(line, content),
            Command::BufferInsert { line, content } => self.handle_buffer_insert(line, content),
            Command::BufferDelete { line } => self.handle_buffer_delete(line),
            Command::View(view) => self.handle_view(view),
            Command::Expand(target) => self.set_expansion(target, true),
            Command::Collapse(target) => self.set_expansion(target, false),
            Command::Save => self.handle_file_save(),
            Command::Quit => self.should_quit = true,
            Command::Unknown(msg) => self.set_status(msg, true),
        }
    }

    fn handle_vim_command(&mut self, input: &str) {
        match vim::parse(input) {
            Ok(VimCommand::Edit(EditOp::OpenFile { path })) => {
                self.handle_file_open(path);
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

    fn show_help(&mut self) {
        let help = help_text();
        self.add_active_action(ActivityAction {
            kind: ActivityKind::System,
            title: "help".to_string(),
            detail: help,
            expanded: true,
        });
        self.set_status("showing help", false);
    }

    fn handle_provider(&mut self, provider: Option<Provider>) {
        if let Some(provider) = provider {
            self.provider = provider;
        }
        let available = if self.provider.has_credentials() {
            self.provider
                .credential_source()
                .unwrap_or_else(|| "available".to_string())
        } else {
            "missing".to_string()
        };
        let message = format!(
            "provider: {} ({} {})",
            self.provider,
            self.provider.env_var(),
            available
        );
        self.add_active_action(ActivityAction {
            kind: ActivityKind::Provider,
            title: "provider".to_string(),
            detail: message.clone(),
            expanded: false,
        });
        self.set_status(message, !self.provider.has_credentials());
    }

    fn start_gas_setup(&mut self) {
        self.gas_step = Some(GasStep::ChooseProvider);
        let message = "Connect an API provider: type openai, anthropic, other, or cancel";
        self.add_active_action(ActivityAction {
            kind: ActivityKind::Provider,
            title: "provider setup".to_string(),
            detail: message.to_string(),
            expanded: true,
        });
        self.set_status(message, false);
    }

    fn handle_gas_input(&mut self, input: String) {
        let trimmed = input.trim();
        if trimmed.eq_ignore_ascii_case("cancel") {
            self.gas_step = None;
            self.set_status("provider setup cancelled", false);
            return;
        }

        match self.gas_step.take() {
            Some(GasStep::ChooseProvider) => match Provider::parse(trimmed) {
                Some(provider) => {
                    self.gas_step = Some(GasStep::EnterKey(provider));
                    self.provider = provider;
                    self.set_status(format!("paste {provider} API key, or type cancel"), false);
                }
                None if trimmed.eq_ignore_ascii_case("other") => {
                    self.add_active_action(ActivityAction {
                        kind: ActivityKind::Provider,
                        title: "provider setup".to_string(),
                        detail: "custom providers are not implemented yet; supported providers are openai and anthropic".to_string(),
                        expanded: true,
                    });
                    self.set_status("custom providers are not implemented yet", true);
                }
                None => {
                    self.gas_step = Some(GasStep::ChooseProvider);
                    self.set_status("type openai, anthropic, other, or cancel", true);
                }
            },
            Some(GasStep::EnterKey(provider)) => {
                if trimmed.is_empty() {
                    self.gas_step = Some(GasStep::EnterKey(provider));
                    self.set_status("API key cannot be empty; paste key or type cancel", true);
                    return;
                }
                match save_provider_key(provider, trimmed.to_string()) {
                    Ok(()) => {
                        self.provider = provider;
                        self.add_active_action(ActivityAction {
                            kind: ActivityKind::Provider,
                            title: "provider setup".to_string(),
                            detail: format!(
                                "{provider} credentials saved to .mc/config.json; key value hidden"
                            ),
                            expanded: false,
                        });
                        self.set_status(
                            format!("{provider} credentials saved to .mc/config.json"),
                            false,
                        );
                    }
                    Err(error) => self.set_status(error, true),
                }
            }
            None => {}
        }
    }

    fn handle_file_open(&mut self, path: String) {
        match read_workspace_file(&path) {
            Ok(content) => {
                self.open_file = Some(path.clone());
                self.file_buffer = Some(FileBuffer {
                    path: path.clone(),
                    content: content.clone(),
                    original: content.clone(),
                    cursor_line: 0,
                });
                self.main_view = MainView::File;
                self.add_active_action(ActivityAction {
                    kind: ActivityKind::File,
                    title: format!("opened {path}"),
                    detail: format!("{} bytes", content.len()),
                    expanded: false,
                });
                self.set_status(format!("open file: {path}"), false);
            }
            Err(error) => self.set_status(error, true),
        }
    }

    fn handle_file_read(&mut self, path: String, start: Option<usize>, end: Option<usize>) {
        match read_workspace_file(&path).and_then(|content| select_lines(&content, start, end)) {
            Ok(content) => {
                self.add_active_action(ActivityAction {
                    kind: ActivityKind::File,
                    title: format!("read {path}"),
                    detail: content,
                    expanded: false,
                });
                self.set_status(format!("read {path}"), false);
            }
            Err(error) => self.set_status(error, true),
        }
    }

    fn handle_file_write(&mut self, path: String, content: String) {
        match read_workspace_file(&path).and_then(|before| {
            ensure_workspace_relative(&path)?;
            fs::write(&path, &content).map_err(|e| format!("failed to write {path}: {e}"))?;
            Ok(before)
        }) {
            Ok(before) => {
                let diff = diff::unified(&path, &before, &content);
                self.edit_bytes_since_turn += before.len().abs_diff(content.len());
                self.add_active_action(ActivityAction {
                    kind: ActivityKind::Diff,
                    title: format!("wrote {path}"),
                    detail: diff_to_text(&diff),
                    expanded: false,
                });
                if let Some(buffer) = &mut self.file_buffer {
                    if buffer.path == path {
                        buffer.content = content.clone();
                        buffer.original = content;
                    }
                }
                self.set_status(format!("wrote {path}"), false);
            }
            Err(error) => self.set_status(error, true),
        }
    }

    fn handle_file_save(&mut self) {
        let Some(buffer) = &self.file_buffer else {
            self.set_status("no open file buffer to save", true);
            return;
        };
        let path = buffer.path.clone();
        let content = buffer.content.clone();
        match ensure_workspace_relative(&path).and_then(|()| {
            fs::write(&path, &content).map_err(|e| format!("failed to write {path}: {e}"))
        }) {
            Ok(()) => {
                let before = self
                    .file_buffer
                    .as_ref()
                    .map(|buffer| buffer.original.clone())
                    .unwrap_or_default();
                let diff = diff::unified(&path, &before, &content);
                self.edit_bytes_since_turn += before.len().abs_diff(content.len());
                self.add_active_action(ActivityAction {
                    kind: ActivityKind::Diff,
                    title: format!("saved {path}"),
                    detail: diff_to_text(&diff),
                    expanded: false,
                });
                if let Some(buffer) = &mut self.file_buffer {
                    buffer.original = buffer.content.clone();
                }
                self.set_status(format!("saved {path}"), false);
            }
            Err(error) => self.set_status(error, true),
        }
    }

    fn handle_buffer_set(&mut self, line: usize, content: String) {
        match self.edit_buffer(|lines| {
            if line > lines.len() {
                return Err(format!("line {line} is outside the buffer"));
            }
            lines[line - 1] = content;
            Ok(())
        }) {
            Ok(()) => self.set_status(format!("edited line {line}"), false),
            Err(error) => self.set_status(error, true),
        }
    }

    fn handle_buffer_insert(&mut self, line: usize, content: String) {
        match self.edit_buffer(|lines| {
            if line > lines.len() + 1 {
                return Err(format!("line {line} is outside the buffer"));
            }
            lines.insert(line - 1, content);
            Ok(())
        }) {
            Ok(()) => self.set_status(format!("inserted line {line}"), false),
            Err(error) => self.set_status(error, true),
        }
    }

    fn handle_buffer_delete(&mut self, line: usize) {
        match self.edit_buffer(|lines| {
            if line > lines.len() {
                return Err(format!("line {line} is outside the buffer"));
            }
            lines.remove(line - 1);
            Ok(())
        }) {
            Ok(()) => self.set_status(format!("deleted line {line}"), false),
            Err(error) => self.set_status(error, true),
        }
    }

    fn edit_buffer(
        &mut self,
        edit: impl FnOnce(&mut Vec<String>) -> Result<(), String>,
    ) -> Result<(), String> {
        let Some(buffer) = &mut self.file_buffer else {
            return Err("no open file buffer; use /open <path>".to_string());
        };
        let mut lines = buffer
            .content
            .lines()
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        if buffer.content.ends_with('\n') {
            lines.push(String::new());
        }
        edit(&mut lines)?;
        buffer.content = lines.join("\n");
        buffer.cursor_line = buffer.cursor_line.min(lines.len().saturating_sub(1));
        self.main_view = MainView::File;
        Ok(())
    }

    fn handle_view(&mut self, view: String) {
        match view.as_str() {
            "activity" | "chat" => {
                self.main_view = MainView::Activity;
                self.set_status("main view: activity", false);
            }
            "file" | "edit" => {
                self.main_view = MainView::File;
                self.set_status("main view: file", false);
            }
            _ => self.set_status("view requires one of: activity, file", true),
        }
    }

    fn set_expansion(&mut self, target: ExpandTarget, expanded: bool) {
        let node_id = self.tree.active;
        match target {
            ExpandTarget::Prompt => self.tree.set_prompt_expanded(node_id, expanded),
            ExpandTarget::Response => self.tree.set_response_expanded(node_id, expanded),
            ExpandTarget::Actions => {
                let count = self.tree.nodes[&node_id].actions.len();
                for index in 0..count {
                    self.tree.set_action_expanded(node_id, index, expanded);
                }
            }
            ExpandTarget::Action(index) => {
                if !self.tree.set_action_expanded(node_id, index, expanded) {
                    self.set_status("no action with that number on active turn", true);
                    return;
                }
            }
        }
        self.set_status(if expanded { "expanded" } else { "collapsed" }, false);
    }

    fn add_active_action(&mut self, action: ActivityAction) {
        self.tree.add_action(self.tree.active, action);
    }

    fn record_turn_telemetry(&mut self, node_id: NodeId, wall_time_ms: u128) {
        let node = &self.tree.nodes[&node_id];
        let prompt = node.user_content.as_deref().unwrap_or_default();
        let completion = node.assistant_content.as_deref();
        let prompt_plan = prompt_plan::build_prompt_plan(
            &self.tree,
            &self.context_ledger,
            node_id,
            prompt_plan::default_budget(self.provider, self.model.as_deref()),
        );
        let context_tokens = prompt_plan
            .estimated_tokens
            .saturating_sub(crate::token::estimate_text(prompt));
        let record = telemetry::turn_record(
            node.hash.clone(),
            self.edit_strategy,
            self.agent_mode,
            prompt,
            completion,
            context_tokens,
            wall_time_ms,
            self.command_count_since_turn,
            self.edit_bytes_since_turn,
        );
        match self.telemetry.append(&record) {
            Ok(()) => {
                self.command_count_since_turn = 0;
                self.edit_bytes_since_turn = 0;
            }
            Err(error) => self.set_status(error, true),
        }
    }

    fn open_fs_dir(&mut self, path: String) {
        if let Err(error) = ensure_workspace_relative(&path) {
            self.set_status(error, true);
            return;
        }
        self.fs.current_dir = path;
        self.fs.cursor = 0;
        self.fs.refresh();
        self.set_status("filesystem directory changed", false);
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

#[derive(Default)]
pub struct LayoutState {
    pub chat: Option<Rect>,
    pub tree: Option<Rect>,
    pub files: Option<Rect>,
    pub input: Option<Rect>,
    pub tree_rows: Vec<(u16, NodeId)>,
    pub fs_rows: Vec<(u16, String)>,
}

enum LearningPart {
    Hint,
    Check,
    Reveal,
}

fn help_text() -> String {
    [
        "Commands",
        "/help or ? - show this help",
        "/mode normal|learning - switch collaboration mode",
        "/strategy patch|macro - switch edit strategy",
        "/gas - guided API provider setup",
        "/provider [anthropic|openai] - inspect or select API provider",
        "/model <name> - select model name",
        "/jump <steps|hash>, /branch [name], /merge <hash>",
        "/hint, /check, /reveal - learning-mode guidance",
        "/rg <pattern>, /files [filter], /head <path> [n], /tail <path> [n]",
        "/wc <path>, /sed <path> <start> <end>, /awk <path> <pattern>",
        "/pin <path>, /unpin <path>, /context list|pin <id>|unpin <id>|drop <id>|clear",
        "/open <path>, /read <path> [start end], /write <path> <content>",
        "/view activity|file, /expand [prompt|response|actions|n], /collapse [...]",
        "/edit <line> <text>, /insert <line> <text>, /delete <line>, /save",
        ":e <path>, :/pattern/, :%s/foo/bar/g, :2,4d, :2,4c text",
        "!<shell command> - run a shell command only when MC_ENABLE_SHELL=1",
    ]
    .join("\n")
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

fn read_workspace_file(path: &str) -> Result<String, String> {
    ensure_workspace_relative(path)?;
    fs::read_to_string(path).map_err(|e| format!("failed to read {path}: {e}"))
}

fn save_provider_key(provider: Provider, key: String) -> Result<(), String> {
    let mut config = ProviderConfig::load()?;
    config.set_provider_key(provider, key);
    config.save()
}

fn list_workspace_dir(path: &str) -> Result<Vec<FsEntry>, String> {
    ensure_workspace_relative(path)?;
    let mut entries = Vec::new();
    if !path.is_empty() {
        let parent = Path::new(path)
            .parent()
            .map(path_to_workspace_string)
            .unwrap_or_default();
        entries.push(FsEntry {
            path: parent,
            name: "..".to_string(),
            is_dir: true,
        });
    }

    let read_dir = fs::read_dir(if path.is_empty() { "." } else { path })
        .map_err(|e| format!("failed to list {path}: {e}"))?;
    for entry in read_dir {
        let entry = entry.map_err(|e| format!("failed to read directory entry: {e}"))?;
        let file_name = entry.file_name().to_string_lossy().into_owned();
        if file_name == ".git" || file_name == "target" {
            continue;
        }
        let entry_path = if path.is_empty() {
            PathBuf::from(&file_name)
        } else {
            Path::new(path).join(&file_name)
        };
        let is_dir = entry
            .file_type()
            .map_err(|e| format!("failed to inspect {file_name}: {e}"))?
            .is_dir();
        entries.push(FsEntry {
            path: path_to_workspace_string(&entry_path),
            name: file_name,
            is_dir,
        });
    }
    entries.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then_with(|| a.name.cmp(&b.name)));
    Ok(entries)
}

fn path_to_workspace_string(path: impl AsRef<Path>) -> String {
    path.as_ref()
        .to_string_lossy()
        .trim_start_matches("./")
        .to_string()
}

fn diff_to_text(diff: &DiffEntry) -> String {
    let mut lines = vec![format!("path: {}", diff.path)];
    lines.extend(diff.lines.iter().map(|line| match line {
        diff::DiffLine::Header(text)
        | diff::DiffLine::Context(text)
        | diff::DiffLine::Added(text)
        | diff::DiffLine::Removed(text) => text.clone(),
    }));
    lines.join("\n")
}

fn select_lines(content: &str, start: Option<usize>, end: Option<usize>) -> Result<String, String> {
    match (start, end) {
        (None, None) => Ok(content.to_string()),
        (Some(start), Some(end)) => {
            let lines: Vec<&str> = content.lines().collect();
            if start == 0 || end < start || end > lines.len() {
                return Err("read range is outside the file".to_string());
            }
            Ok(lines[start - 1..end].join("\n"))
        }
        _ => Err("read ranges require both start and end".to_string()),
    }
}

fn previous_boundary(input: &str, index: usize) -> usize {
    input[..index]
        .char_indices()
        .last()
        .map(|(i, _)| i)
        .unwrap_or(0)
}

fn next_boundary(input: &str, index: usize) -> usize {
    input[index..]
        .char_indices()
        .nth(1)
        .map(|(i, _)| index + i)
        .unwrap_or(input.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inserts_and_backspaces_at_cursor() {
        let mut app = App::new();
        app.insert_char('a');
        app.insert_char('c');
        app.cursor_left();
        app.insert_char('b');

        assert_eq!(app.input, "abc");
        assert_eq!(app.input_cursor, 2);

        app.backspace();
        assert_eq!(app.input, "ac");
        assert_eq!(app.input_cursor, 1);
    }

    #[test]
    fn word_navigation_moves_between_words() {
        let mut app = App::new();
        app.input = "one two".to_string();
        app.input_cursor = app.input.len();

        app.cursor_word_left();
        assert_eq!(app.input_cursor, 4);
        app.cursor_word_right();
        assert_eq!(app.input_cursor, app.input.len());
    }
}
