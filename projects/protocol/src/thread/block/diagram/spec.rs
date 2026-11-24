use std::borrow::Cow;
use std::collections::HashMap;

use anyhow::{bail, ensure};
use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Deserializer, Serialize};

use super::super::Line;

pub const MAX_NODES: usize = 24;
pub const MAX_EDGES: usize = 40;
pub const MAX_GROUPS: usize = 4;
pub const MAX_LEGEND: usize = 6;
pub const MAX_PARTICIPANTS: usize = 6;
pub const MAX_MESSAGES: usize = 20;
pub const MIN_EVENTS: usize = 2;
pub const MAX_EVENTS: usize = 12;
/// Nodes with a tone besides the plain one: a diagram says one or two things loudly.
pub const MAX_TONED: usize = 3;
pub const ID_CHARS: usize = 24;
pub const NODE_LABEL_CHARS: usize = 40;
pub const NODE_NOTE_CHARS: usize = 60;
pub const EDGE_LABEL_CHARS: usize = 32;
pub const GROUP_LABEL_CHARS: usize = 32;
pub const DIAGRAM_TITLE_CHARS: usize = 80;
pub const WHEN_CHARS: usize = 20;

// Plain comments on the types here: a doc comment goes into the schema of `show` and so
// into every request, so only the few a model needs are doc comments.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DiagramKind {
    Flow,
    Architecture,
    Sequence,
    Timeline,
    Tree,
    State,
}

impl DiagramKind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Flow => "flow",
            Self::Architecture => "architecture",
            Self::Sequence => "sequence",
            Self::Timeline => "timeline",
            Self::Tree => "tree",
            Self::State => "state",
        }
    }

    /// Whether its nodes are laid out in layers along a direction.
    pub fn layered(self) -> bool {
        matches!(
            self,
            Self::Flow | Self::Architecture | Self::Tree | Self::State
        )
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    #[default]
    Down,
    Right,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Shape {
    #[default]
    Box,
    Pill,
    Diamond,
    Store,
    Start,
    End,
}

impl Shape {
    pub fn name(self) -> &'static str {
        match self {
            Self::Box => "box",
            Self::Pill => "pill",
            Self::Diamond => "diamond",
            Self::Store => "store",
            Self::Start => "start",
            Self::End => "end",
        }
    }

    /// The start and end markers of a state machine: a dot and a ringed dot, no text.
    pub fn is_marker(self) -> bool {
        matches!(self, Self::Start | Self::End)
    }
}

// What a node or a legend entry stands for; each client maps it to its theme, the model
// never names a colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DiagramTone {
    Accent,
    Info,
    Success,
    Warning,
    Danger,
    Muted,
}

impl DiagramTone {
    pub fn name(self) -> &'static str {
        match self {
            Self::Accent => "accent",
            Self::Info => "info",
            Self::Success => "success",
            Self::Warning => "warning",
            Self::Danger => "danger",
            Self::Muted => "muted",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EdgeStyle {
    #[default]
    Solid,
    Dashed,
    Strong,
}

impl EdgeStyle {
    pub fn name(self) -> &'static str {
        match self {
            Self::Solid => "solid",
            Self::Dashed => "dashed",
            Self::Strong => "strong",
        }
    }
}

/// The name a node or a group is known by in the diagram: letters, digits, `_`, `-`
/// and `.`, at most `ID_CHARS`. It is never drawn unless the node has no label.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct DiagramId(String);

impl DiagramId {
    pub fn new(text: &str) -> anyhow::Result<Self> {
        let quoted: String = text.chars().take(ID_CHARS + 1).collect();
        ensure!(
            !text.is_empty() && text.chars().count() <= ID_CHARS,
            "the id {quoted:?} must have 1 to {ID_CHARS} characters"
        );
        ensure!(
            text.chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.')),
            "the id {quoted:?} may only have letters, digits, _, - and ."
        );
        Ok(Self(text.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for DiagramId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::new(&text).map_err(serde::de::Error::custom)
    }
}

impl JsonSchema for DiagramId {
    fn inline_schema() -> bool {
        true
    }

    fn schema_name() -> Cow<'static, str> {
        "DiagramId".into()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({ "type": "string", "maxLength": ID_CHARS, "pattern": "^[A-Za-z0-9_.-]+$" })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DiagramNode {
    pub id: DiagramId,
    /// Defaults to the id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<Line<NODE_LABEL_CHARS>>,
    /// A second, quieter line.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<Line<NODE_NOTE_CHARS>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<DiagramId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tone: Option<DiagramTone>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shape: Option<Shape>,
    /// Timeline only: the date or step it happened at.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub when: Option<Line<WHEN_CHARS>>,
}

impl DiagramNode {
    /// What the node says: its label, or its id when it has none.
    pub fn label(&self) -> &str {
        self.label
            .as_ref()
            .map_or(self.id.as_str(), |label| label.as_str())
    }

    pub fn shape(&self) -> Shape {
        self.shape.unwrap_or_default()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DiagramEdge {
    pub from: DiagramId,
    pub to: DiagramId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<Line<EDGE_LABEL_CHARS>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<EdgeStyle>,
}

impl DiagramEdge {
    pub fn style(&self) -> EdgeStyle {
        self.style.unwrap_or_default()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DiagramGroup {
    pub id: DiagramId,
    pub label: Line<GROUP_LABEL_CHARS>,
}

// What a tone or a line style means; exactly one of the two.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LegendEntry {
    pub label: Line<GROUP_LABEL_CHARS>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tone: Option<DiagramTone>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<EdgeStyle>,
}

// A diagram of nodes and the edges between them, drawn by each client from this data
// alone. Built and read only through `new`, so one that exists is within its caps and
// every id it names is one of its own.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(try_from = "DiagramFields")]
pub struct Diagram {
    kind: DiagramKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    title: Option<Line<DIAGRAM_TITLE_CHARS>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    direction: Option<Direction>,
    nodes: Vec<DiagramNode>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    edges: Vec<DiagramEdge>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    groups: Vec<DiagramGroup>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    legend: Vec<LegendEntry>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct DiagramFields {
    kind: DiagramKind,
    #[serde(default)]
    title: Option<Line<DIAGRAM_TITLE_CHARS>>,
    /// Layered kinds only; down by default.
    #[serde(default)]
    direction: Option<Direction>,
    /// Sequence: the participants, left to right. Timeline: the events, in order.
    #[schemars(length(min = 1, max = MAX_NODES))]
    nodes: Vec<DiagramNode>,
    /// Sequence: the messages, top to bottom. Tree: parent to child.
    #[serde(default)]
    #[schemars(length(max = MAX_EDGES))]
    edges: Vec<DiagramEdge>,
    #[serde(default)]
    #[schemars(length(max = MAX_GROUPS))]
    groups: Vec<DiagramGroup>,
    #[serde(default)]
    #[schemars(length(max = MAX_LEGEND))]
    legend: Vec<LegendEntry>,
}

impl TryFrom<DiagramFields> for Diagram {
    type Error = anyhow::Error;

    fn try_from(fields: DiagramFields) -> anyhow::Result<Self> {
        Self::new(
            fields.kind,
            fields.title,
            fields.direction,
            fields.nodes,
            fields.edges,
            fields.groups,
            fields.legend,
        )
    }
}

impl Diagram {
    pub fn new(
        kind: DiagramKind,
        title: Option<Line<DIAGRAM_TITLE_CHARS>>,
        direction: Option<Direction>,
        nodes: Vec<DiagramNode>,
        edges: Vec<DiagramEdge>,
        groups: Vec<DiagramGroup>,
        legend: Vec<LegendEntry>,
    ) -> anyhow::Result<Self> {
        let name = kind.name();
        let (min_nodes, max_nodes, max_edges) = match kind {
            DiagramKind::Sequence => (2, MAX_PARTICIPANTS, MAX_MESSAGES),
            DiagramKind::Timeline => (MIN_EVENTS, MAX_EVENTS, 0),
            _ => (1, MAX_NODES, MAX_EDGES),
        };
        ensure!(
            (min_nodes..=max_nodes).contains(&nodes.len()),
            "a {name} diagram has {min_nodes} to {max_nodes} nodes"
        );
        ensure!(
            edges.len() <= max_edges,
            "a {name} diagram has at most {max_edges} edges"
        );
        ensure!(
            groups.len() <= MAX_GROUPS,
            "give at most {MAX_GROUPS} groups"
        );
        ensure!(
            legend.len() <= MAX_LEGEND,
            "give at most {MAX_LEGEND} legend entries"
        );
        ensure!(
            direction.is_none() || kind.layered(),
            "direction is for flow, architecture, tree and state"
        );
        ensure!(
            groups.is_empty()
                || matches!(
                    kind,
                    DiagramKind::Flow | DiagramKind::Architecture | DiagramKind::State
                ),
            "groups are for flow, architecture and state"
        );

        let mut at: HashMap<&str, usize> = HashMap::new();
        for (index, node) in nodes.iter().enumerate() {
            ensure!(
                at.insert(node.id.as_str(), index).is_none(),
                "the node id {:?} is used twice",
                node.id.as_str()
            );
        }
        let mut group_ids: HashMap<&str, usize> = HashMap::new();
        for group in &groups {
            ensure!(
                group_ids.insert(group.id.as_str(), 0).is_none(),
                "the group id {:?} is used twice",
                group.id.as_str()
            );
        }
        let mut toned = 0;
        for node in &nodes {
            let id = node.id.as_str();
            if let Some(group) = &node.group {
                let Some(count) = group_ids.get_mut(group.as_str()) else {
                    bail!(
                        "node {id:?} is in the group {:?}, which is not given",
                        group.as_str()
                    );
                };
                *count += 1;
            }
            if node.tone.is_some() {
                toned += 1;
            }
            let shape = node.shape();
            match kind {
                DiagramKind::Sequence | DiagramKind::Timeline => ensure!(
                    node.shape.is_none(),
                    "node {id:?}: shape is not for a {name} diagram"
                ),
                DiagramKind::State => {}
                _ => ensure!(
                    !shape.is_marker(),
                    "node {id:?}: the {} shape is for a state diagram",
                    shape.name()
                ),
            }
            ensure!(
                node.when.is_none() || kind == DiagramKind::Timeline,
                "node {id:?}: when is for a timeline"
            );
        }
        ensure!(
            toned <= MAX_TONED,
            "give a tone to at most {MAX_TONED} nodes, the ones to see first"
        );
        if let Some((id, _)) = group_ids.iter().find(|(_, count)| **count == 0) {
            bail!("the group {id:?} has no nodes");
        }

        let mut parent = vec![None; nodes.len()];
        for (index, edge) in edges.iter().enumerate() {
            let find = |id: &DiagramId| {
                at.get(id.as_str()).copied().ok_or_else(|| {
                    anyhow::anyhow!(
                        "edge {index} names the node {:?}, which is not given",
                        id.as_str()
                    )
                })
            };
            let (from, to) = (find(&edge.from)?, find(&edge.to)?);
            if from == to {
                ensure!(
                    matches!(kind, DiagramKind::State | DiagramKind::Sequence),
                    "edge {index} goes from a node to itself, which only state and sequence draw"
                );
            }
            if kind == DiagramKind::Tree {
                ensure!(
                    parent[to].replace(from).is_none(),
                    "node {:?} has two parents; a tree has one",
                    edge.to.as_str()
                );
            }
        }
        if kind == DiagramKind::Tree {
            // Every walk up from a node ends at a root within as many steps as there are
            // nodes, or the edges go round in a circle.
            for start in 0..nodes.len() {
                let mut at = start;
                let mut steps = 0;
                while let Some(up) = parent[at] {
                    at = up;
                    steps += 1;
                    ensure!(steps <= nodes.len(), "the tree's edges make a circle");
                }
            }
        }

        for entry in &legend {
            ensure!(
                entry.tone.is_some() != entry.style.is_some(),
                "legend entry {:?} needs exactly one of tone and style",
                entry.label.as_str()
            );
        }
        Ok(Self {
            kind,
            title,
            direction,
            nodes,
            edges,
            groups,
            legend,
        })
    }

    pub fn kind(&self) -> DiagramKind {
        self.kind
    }

    pub fn title(&self) -> Option<&str> {
        self.title.as_ref().map(Line::as_str)
    }

    pub fn direction(&self) -> Direction {
        self.direction.unwrap_or_default()
    }

    pub fn nodes(&self) -> &[DiagramNode] {
        &self.nodes
    }

    pub fn edges(&self) -> &[DiagramEdge] {
        &self.edges
    }

    pub fn groups(&self) -> &[DiagramGroup] {
        &self.groups
    }

    pub fn legend(&self) -> &[LegendEntry] {
        &self.legend
    }

    /// The index of the node with `id`. Every id an edge names is one, by `new`.
    pub fn node(&self, id: &DiagramId) -> usize {
        self.nodes
            .iter()
            .position(|node| node.id == *id)
            .unwrap_or_default()
    }

    /// The index of the group a node is in.
    pub fn group_of(&self, node: usize) -> Option<usize> {
        let group = self.nodes.get(node)?.group.as_ref()?;
        self.groups.iter().position(|g| g.id == *group)
    }

    /// Each edge as the indices of the nodes it joins.
    pub fn ends(&self) -> Vec<(usize, usize)> {
        self.edges
            .iter()
            .map(|edge| (self.node(&edge.from), self.node(&edge.to)))
            .collect()
    }
}
