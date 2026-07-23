use crate::context::{ContextLedger, ContextPacket};
use crate::conversation::tree::{ConversationTree, NodeId};
use crate::token::{self, TokenBudget, TokenBudgetMode};

const RECENT_RAW_TURNS: usize = 4;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PromptItemKind {
    System,
    CurrentTurn,
    PinnedContext,
    RecentTurn,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PromptInclusionReason {
    Required,
    Pinned,
    Recent,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StaleState {
    Unknown,
}

#[derive(Clone, Debug)]
pub struct PromptPlanItem {
    pub kind: PromptItemKind,
    pub title: String,
    pub content: String,
    pub reason: PromptInclusionReason,
    pub estimated_tokens: usize,
    pub stale_state: StaleState,
}

#[derive(Clone, Debug)]
pub struct PromptPlan {
    pub items: Vec<PromptPlanItem>,
    pub budget: TokenBudget,
    pub estimated_tokens: usize,
    pub omitted_items: usize,
}

impl PromptPlan {
    pub fn render(&self) -> String {
        let mut sections = vec![format!(
            "Prompt plan: ~{} / {} tokens",
            self.estimated_tokens, self.budget.prompt_budget
        )];
        sections.extend(self.items.iter().map(|item| {
            let metadata = format!(
                "reason: {}; stale: {}",
                item.reason.label(),
                item.stale_state.label()
            );
            if item.title.is_empty() {
                format!("[{metadata}]\n{}", item.content)
            } else {
                format!("{} [{metadata}]\n{}", item.title, item.content)
            }
        }));
        sections.join("\n\n")
    }
}

pub fn build_prompt_plan(
    tree: &ConversationTree,
    ledger: &ContextLedger,
    node_id: NodeId,
    budget: TokenBudget,
) -> PromptPlan {
    let mut builder = PromptPlanBuilder::new(budget);
    builder.push_required(PromptPlanItem::new(
        PromptItemKind::System,
        "System".to_string(),
        "You are a coding assistant inside a terminal UI. Answer concisely and use only relevant repository context.".to_string(),
        PromptInclusionReason::Required,
    ));

    if let Some(node) = tree.nodes.get(&node_id) {
        if let Some(user) = &node.user_content {
            builder.push_required(PromptPlanItem::new(
                PromptItemKind::CurrentTurn,
                "Current user request".to_string(),
                user.clone(),
                PromptInclusionReason::Required,
            ));
        }
    }

    for packet in ledger.packets.iter().filter(|packet| packet.pinned) {
        builder.push_budgeted(context_item(packet));
    }

    for turn_id in recent_turns(tree, node_id) {
        if turn_id == node_id {
            continue;
        }
        if let Some(item) = recent_turn_item(tree, turn_id) {
            builder.push_budgeted(item);
        }
    }

    builder.finish()
}

fn context_item(packet: &ContextPacket) -> PromptPlanItem {
    let paths = if packet.paths.is_empty() {
        "no paths".to_string()
    } else {
        packet.paths.join(",")
    };
    PromptPlanItem {
        kind: PromptItemKind::PinnedContext,
        title: format!(
            "[pinned context #{} from {}; {}]",
            packet.id, packet.command, paths
        ),
        content: packet.output.clone(),
        reason: PromptInclusionReason::Pinned,
        estimated_tokens: packet.estimated_tokens,
        stale_state: StaleState::Unknown,
    }
}

fn recent_turn_item(tree: &ConversationTree, turn_id: NodeId) -> Option<PromptPlanItem> {
    let node = tree.nodes.get(&turn_id)?;
    let mut content = String::new();
    if let Some(user) = &node.user_content {
        content.push_str("User: ");
        content.push_str(user);
        content.push('\n');
    }
    if let Some(assistant) = &node.assistant_content {
        content.push_str("Assistant: ");
        content.push_str(assistant);
    }
    let content = content.trim_end().to_string();
    if content.is_empty() {
        return None;
    }
    Some(PromptPlanItem::new(
        PromptItemKind::RecentTurn,
        format!("[recent turn {}]", node.hash),
        content,
        PromptInclusionReason::Recent,
    ))
}

fn recent_turns(tree: &ConversationTree, node_id: NodeId) -> Vec<NodeId> {
    let mut path = tree.path_to(node_id);
    if path.len() > RECENT_RAW_TURNS {
        path = path[path.len() - RECENT_RAW_TURNS..].to_vec();
    }
    path
}

impl PromptPlanItem {
    fn new(
        kind: PromptItemKind,
        title: String,
        content: String,
        reason: PromptInclusionReason,
    ) -> Self {
        let estimated_tokens = token::estimate_text(&title) + token::estimate_text(&content);
        Self {
            kind,
            title,
            content,
            reason,
            estimated_tokens,
            stale_state: StaleState::Unknown,
        }
    }
}

struct PromptPlanBuilder {
    items: Vec<PromptPlanItem>,
    budget: TokenBudget,
    estimated_tokens: usize,
    omitted_items: usize,
}

impl PromptPlanBuilder {
    fn new(budget: TokenBudget) -> Self {
        Self {
            items: Vec::new(),
            budget,
            estimated_tokens: 0,
            omitted_items: 0,
        }
    }

    fn push_required(&mut self, item: PromptPlanItem) {
        self.estimated_tokens += item.estimated_tokens;
        self.items.push(item);
    }

    fn push_budgeted(&mut self, item: PromptPlanItem) {
        if self.estimated_tokens + item.estimated_tokens <= self.budget.prompt_budget {
            self.estimated_tokens += item.estimated_tokens;
            self.items.push(item);
        } else {
            self.omitted_items += 1;
        }
    }

    fn finish(self) -> PromptPlan {
        PromptPlan {
            items: self.items,
            budget: self.budget,
            estimated_tokens: self.estimated_tokens,
            omitted_items: self.omitted_items,
        }
    }
}

pub fn default_budget(provider: crate::provider::Provider, model: Option<&str>) -> TokenBudget {
    TokenBudget::for_model(provider, model, TokenBudgetMode::Balanced)
}

impl PromptInclusionReason {
    fn label(&self) -> &'static str {
        match self {
            Self::Required => "required",
            Self::Pinned => "pinned",
            Self::Recent => "recent",
        }
    }
}

impl StaleState {
    fn label(&self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::Provider;

    #[test]
    fn includes_required_current_turn_and_pinned_context() {
        let mut tree = ConversationTree::new();
        let node_id = tree.add_child(0, "request".to_string(), Some("fix bug".to_string()));
        let mut ledger = ContextLedger::new();
        let pinned = ledger.record(
            "/pin Cargo.toml".to_string(),
            vec!["Cargo.toml".to_string()],
            None,
            "[package]\nname = \"mc\"".to_string(),
        );
        ledger.pin(pinned).unwrap();
        ledger.record(
            "/sed ignored 1 2".to_string(),
            vec!["ignored".to_string()],
            None,
            "should not be included".to_string(),
        );

        let plan = build_prompt_plan(
            &tree,
            &ledger,
            node_id,
            TokenBudget::for_model(Provider::Anthropic, None, TokenBudgetMode::Cheap),
        );

        assert!(
            plan.items
                .iter()
                .any(|item| item.kind == PromptItemKind::System)
        );
        assert!(
            plan.items
                .iter()
                .any(|item| item.kind == PromptItemKind::CurrentTurn)
        );
        assert_eq!(
            plan.items
                .iter()
                .filter(|item| item.kind == PromptItemKind::PinnedContext)
                .count(),
            1
        );
        assert!(plan.render().contains("fix bug"));
        assert!(plan.render().contains("[package]"));
        assert!(!plan.render().contains("should not be included"));
    }

    #[test]
    fn omits_budgeted_items_that_exceed_budget() {
        let mut tree = ConversationTree::new();
        let node_id = tree.add_child(0, "request".to_string(), Some("fix bug".to_string()));
        let mut ledger = ContextLedger::new();
        let pinned = ledger.record(
            "/pin large.txt".to_string(),
            vec!["large.txt".to_string()],
            None,
            "x".repeat(500),
        );
        ledger.pin(pinned).unwrap();

        let plan = build_prompt_plan(
            &tree,
            &ledger,
            node_id,
            TokenBudget {
                context_window: 100,
                prompt_budget: 20,
                mode: TokenBudgetMode::Custom(20),
            },
        );

        assert_eq!(plan.omitted_items, 1);
        assert!(
            plan.items
                .iter()
                .all(|item| item.kind != PromptItemKind::PinnedContext)
        );
    }
}
