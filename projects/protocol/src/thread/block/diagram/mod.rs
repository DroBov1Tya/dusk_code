//! A diagram the model shows: a flow, an architecture, a sequence, a timeline, a tree or a
//! state machine, given as nodes and the edges between them (`spec`). It is data only: no
//! drawing, colour or markup comes from the model. Every client draws it from the same
//! layout (`layout`), which says where each part stands, so the pictures agree; the
//! terminal, which has no pixels, draws its own text form.

pub mod layout;
mod spec;
mod text;

pub use spec::{
    DIAGRAM_TITLE_CHARS, Diagram, DiagramEdge, DiagramGroup, DiagramId, DiagramKind, DiagramNode,
    DiagramTone, Direction, EDGE_LABEL_CHARS, EdgeStyle, GROUP_LABEL_CHARS, ID_CHARS, LegendEntry,
    MAX_EDGES, MAX_EVENTS, MAX_GROUPS, MAX_LEGEND, MAX_MESSAGES, MAX_NODES, MAX_PARTICIPANTS,
    MAX_TONED, MIN_EVENTS, NODE_LABEL_CHARS, NODE_NOTE_CHARS, Shape, WHEN_CHARS,
};

impl Diagram {
    pub(super) fn write(&self, text: &mut String) {
        text::write(self, text);
    }
}

#[cfg(test)]
mod tests;
