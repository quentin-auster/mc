use std::collections::HashMap;

pub type NodeId = usize;

pub struct LearningMetadata {
    pub objective: String,
    pub hint: String,
    pub expected_direction: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ActivityKind {
    System,
    Shell,
    File,
    Diff,
    Provider,
}

#[derive(Clone, Debug)]
pub struct ActivityAction {
    pub kind: ActivityKind,
    pub title: String,
    pub detail: String,
    pub expanded: bool,
}

pub struct Node {
    pub id: NodeId,
    pub hash: String,
    pub parent: Option<NodeId>,
    /// Set on nodes created by /merge — points to the secondary branch being merged in.
    pub merged_from: Option<NodeId>,
    pub children: Vec<NodeId>,
    pub label: String,
    /// The user message that created this node. None for root, /branch, and /merge nodes.
    pub user_content: Option<String>,
    /// Filled when the assistant responds (API not yet wired).
    pub assistant_content: Option<String>,
    /// Actions, artifacts, and command results associated with this turn.
    pub actions: Vec<ActivityAction>,
    pub prompt_expanded: bool,
    pub response_expanded: bool,
    /// Present for learning-mode turns so follow-up commands can reveal scaffolded guidance.
    pub learning_metadata: Option<LearningMetadata>,
}

impl Node {
    pub fn is_branch(&self) -> bool {
        self.user_content.is_none() && self.parent.is_some() && self.merged_from.is_none()
    }

    pub fn is_merge(&self) -> bool {
        self.merged_from.is_some()
    }
}

pub struct ConversationTree {
    pub nodes: HashMap<NodeId, Node>,
    pub root: NodeId,
    pub active: NodeId,
    next_id: NodeId,
}

impl ConversationTree {
    pub fn new() -> Self {
        let root = Node {
            id: 0,
            hash: node_hash(0),
            parent: None,
            merged_from: None,
            children: Vec::new(),
            label: "root".to_string(),
            user_content: None,
            assistant_content: None,
            actions: Vec::new(),
            prompt_expanded: false,
            response_expanded: false,
            learning_metadata: None,
        };
        let mut nodes = HashMap::new();
        nodes.insert(0, root);
        Self {
            nodes,
            root: 0,
            active: 0,
            next_id: 1,
        }
    }

    pub fn add_child(
        &mut self,
        parent: NodeId,
        label: String,
        user_content: Option<String>,
    ) -> NodeId {
        let id = self.next_id;
        self.next_id += 1;
        let node = Node {
            id,
            hash: node_hash(id),
            parent: Some(parent),
            merged_from: None,
            children: Vec::new(),
            label,
            user_content,
            assistant_content: None,
            actions: Vec::new(),
            prompt_expanded: false,
            response_expanded: false,
            learning_metadata: None,
        };
        self.nodes.insert(id, node);
        if let Some(p) = self.nodes.get_mut(&parent) {
            p.children.push(id);
        }
        id
    }

    /// Creates a merge node as a child of `parent`, referencing `merged_from` as the secondary branch.
    pub fn add_merge_child(
        &mut self,
        parent: NodeId,
        merged_from: NodeId,
        label: String,
    ) -> NodeId {
        let id = self.next_id;
        self.next_id += 1;
        let node = Node {
            id,
            hash: node_hash(id),
            parent: Some(parent),
            merged_from: Some(merged_from),
            children: Vec::new(),
            label,
            user_content: None,
            assistant_content: None,
            actions: Vec::new(),
            prompt_expanded: false,
            response_expanded: false,
            learning_metadata: None,
        };
        self.nodes.insert(id, node);
        if let Some(p) = self.nodes.get_mut(&parent) {
            p.children.push(id);
        }
        id
    }

    pub fn set_assistant_content(&mut self, id: NodeId, content: String) {
        if let Some(node) = self.nodes.get_mut(&id) {
            node.assistant_content = Some(content);
        }
    }

    pub fn add_action(&mut self, id: NodeId, action: ActivityAction) {
        if let Some(node) = self.nodes.get_mut(&id) {
            node.actions.push(action);
        }
    }

    pub fn set_prompt_expanded(&mut self, id: NodeId, expanded: bool) {
        if let Some(node) = self.nodes.get_mut(&id) {
            node.prompt_expanded = expanded;
        }
    }

    pub fn set_response_expanded(&mut self, id: NodeId, expanded: bool) {
        if let Some(node) = self.nodes.get_mut(&id) {
            node.response_expanded = expanded;
        }
    }

    pub fn set_action_expanded(&mut self, id: NodeId, index: usize, expanded: bool) -> bool {
        let Some(node) = self.nodes.get_mut(&id) else {
            return false;
        };
        let Some(action) = node.actions.get_mut(index) else {
            return false;
        };
        action.expanded = expanded;
        true
    }

    pub fn set_learning_metadata(&mut self, id: NodeId, metadata: LearningMetadata) {
        if let Some(node) = self.nodes.get_mut(&id) {
            node.learning_metadata = Some(metadata);
        }
    }

    /// Returns node IDs from root to `active`, inclusive.
    pub fn active_path(&self) -> Vec<NodeId> {
        let mut path = vec![];
        let mut current = self.active;
        loop {
            path.push(current);
            match self.nodes.get(&current).and_then(|n| n.parent) {
                Some(parent) => current = parent,
                None => break,
            }
        }
        path.reverse();
        path
    }

    /// Returns all nodes in DFS display order, paired with their display depth.
    /// Depth only increases when a /branch node's children are encountered.
    pub fn display_entries(&self) -> Vec<(NodeId, usize)> {
        let mut out = vec![];
        self.dfs_entries(self.root, 0, &mut out);
        out
    }

    fn dfs_entries(&self, id: NodeId, depth: usize, out: &mut Vec<(NodeId, usize)>) {
        if let Some(node) = self.nodes.get(&id) {
            out.push((id, depth));
            let child_depth = if node.is_branch() { depth + 1 } else { depth };
            for &child in &node.children {
                self.dfs_entries(child, child_depth, out);
            }
        }
    }

    /// Find a node whose hash starts with `prefix`.
    pub fn find_by_hash(&self, prefix: &str) -> Option<NodeId> {
        self.nodes
            .values()
            .find(|n| n.hash.starts_with(prefix))
            .map(|n| n.id)
    }

    /// Walk up the parent chain `n` steps from `active`.
    pub fn go_back(&mut self, n: usize) -> Result<(), String> {
        let mut current = self.active;
        for step in 0..n {
            match self.nodes.get(&current).and_then(|node| node.parent) {
                Some(parent) => current = parent,
                None => return Err(format!("only {step} step(s) available going back")),
            }
        }
        self.active = current;
        Ok(())
    }

    /// Walk down the child chain `n` steps. Errors if a node has multiple children (ambiguous).
    pub fn go_forward(&mut self, n: usize) -> Result<(), String> {
        let mut current = self.active;
        for step in 0..n {
            let children = self.nodes[&current].children.clone();
            match children.as_slice() {
                [] => return Err(format!("already at head after {step} step(s)")),
                [only] => current = *only,
                _ => {
                    return Err(format!(
                        "multiple branches at step {step} — use `/jump <hash>` to specify"
                    ));
                }
            }
        }
        self.active = current;
        Ok(())
    }
}

/// Fibonacci hashing — spreads sequential IDs into a visually hash-like 7-char hex string.
fn node_hash(id: NodeId) -> String {
    let h = (id as u64).wrapping_mul(0x9e3779b97f4a7c15);
    format!("{:07x}", h >> 36)
}
