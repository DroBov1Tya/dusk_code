//! Flow, architecture, tree and state, in layers. Edges that go back are turned round
//! so the graph has no circle (`structure`); each node goes in the layer after the
//! longest path to it, and an edge that skips layers gets a waypoint in each it skips;
//! the order in a layer is settled by sweeps of averages, keeping a group's nodes side by
//! side, and the order with fewest crossings is kept. Nodes then slide toward the middle
//! of their neighbours (`place`), groups are pushed clear of what is not theirs, and edges
//! run between layers in tracks of their own, their labels set where no line runs through.
//!
//! The work is done along two axes: `main`, the way the layers follow each other, and
//! `cross`, along a layer. Going down, main is y; going right, main is x.

use std::collections::BTreeSet;

use super::geometry::{Point, Rect, Seg, rounded};
use super::{
    Anchor, EdgePath, GroupBox, HEAD_LENGTH, Head, Layout, Measure, NodeBox, Owner, Sized, Text,
    TextStyle, edge_label, edge_texts, node_texts, size,
};
use crate::thread::block::diagram::spec::{Diagram, DiagramKind, Direction};

/// Space between two nodes of a layer, and next to an edge's waypoint.
const NODE_GAP: f32 = 28.;
const WAY_GAP: f32 = 12.;
/// Space inside a group's box around its nodes, and the room its name takes on top.
const GROUP_PAD: f32 = 14.;
const GROUP_HEAD: f32 = 18.;
/// Space between two tracks of edges, and the radius of a corner.
const TRACK: f32 = 8.;
const ROUND: f32 = 6.;
/// How far a loop from a state back to itself stands out.
const LOOP: f32 = 26.;
/// How many sweeps order the layers.
const SWEEPS: usize = 24;
/// How many rounds slide the nodes toward their neighbours.
const ROUNDS: usize = 8;

/// The graph in layers, before anything has a size.
struct Structure {
    /// Items of each layer in order: a node, or the waypoint of an edge.
    ranks: Vec<Vec<usize>>,
    items: Vec<Item>,
    /// Each edge as the items it runs through, or `None` for a loop to itself.
    chains: Vec<Option<Chain>>,
    up: Vec<Vec<usize>>,
    down: Vec<Vec<usize>>,
    /// Each group's place in the one order of groups along every layer.
    group_rank: Vec<usize>,
}

#[derive(Clone, Copy)]
struct Item {
    node: Option<usize>,
    rank: usize,
    group: Option<usize>,
}

struct Chain {
    items: Vec<usize>,
    /// Drawn the other way round from how it was laid out: it closes a circle.
    reversed: bool,
}

/// The nodes in reading order: layer by layer, each in its place along the layer.
pub fn reading_order(diagram: &Diagram) -> Vec<usize> {
    if !diagram.kind().layered() {
        return (0..diagram.nodes().len()).collect();
    }
    let structure = structure(diagram);
    structure
        .ranks
        .iter()
        .flatten()
        .filter_map(|&item| structure.items[item].node)
        .collect()
}

fn structure(diagram: &Diagram) -> Structure {
    let count = diagram.nodes().len();
    let ends = diagram.ends();
    let reversed = back_edges(count, &ends);
    // Each edge the way it is laid out: from a layer to a later one.
    let laid: Vec<Option<(usize, usize)>> = ends
        .iter()
        .zip(&reversed)
        .map(|(&(from, to), &back)| match (from == to, back) {
            (true, _) => None,
            (false, false) => Some((from, to)),
            (false, true) => Some((to, from)),
        })
        .collect();
    let rank = ranks(count, &laid);

    let mut items: Vec<Item> = (0..count)
        .map(|node| Item {
            node: Some(node),
            rank: rank[node],
            group: diagram.group_of(node),
        })
        .collect();
    let mut chains = Vec::new();
    for (edge, laid) in laid.iter().enumerate() {
        let Some((from, to)) = *laid else {
            chains.push(None);
            continue;
        };
        let group = match (diagram.group_of(from), diagram.group_of(to)) {
            (Some(a), Some(b)) if a == b => Some(a),
            _ => None,
        };
        let mut chain = vec![from];
        for between in rank[from] + 1..rank[to] {
            chain.push(items.len());
            items.push(Item {
                node: None,
                rank: between,
                group,
            });
        }
        chain.push(to);
        chains.push(Some(Chain {
            items: chain,
            reversed: reversed[edge],
        }));
    }
    let (mut up, mut down) = (vec![Vec::new(); items.len()], vec![Vec::new(); items.len()]);
    for chain in chains.iter().flatten() {
        for pair in chain.items.windows(2) {
            down[pair[0]].push(pair[1]);
            up[pair[1]].push(pair[0]);
        }
    }
    let layers = rank.iter().copied().max().unwrap_or(0) + 1;
    let mut ranks = vec![Vec::new(); layers];
    for (at, item) in items.iter().enumerate() {
        ranks[item.rank].push(at);
    }
    let mut structure = Structure {
        ranks,
        items,
        chains,
        up,
        down,
        group_rank: Vec::new(),
    };
    structure.group_rank = order(&mut structure, diagram.groups().len());
    structure
}

/// The edges that close a circle, found by a walk in depth from the nodes nothing points
/// to, in the order they are given.
fn back_edges(count: usize, ends: &[(usize, usize)]) -> Vec<bool> {
    let mut out = vec![Vec::new(); count];
    let mut pointed = vec![false; count];
    for (edge, &(from, to)) in ends.iter().enumerate() {
        if from != to {
            out[from].push((edge, to));
            pointed[to] = true;
        }
    }
    let mut back = vec![false; ends.len()];
    // 0 not seen, 1 on the walk, 2 done.
    let mut state = vec![0u8; count];
    let starts = (0..count)
        .filter(|&node| !pointed[node])
        .chain(0..count)
        .collect::<Vec<_>>();
    for start in starts {
        if state[start] != 0 {
            continue;
        }
        state[start] = 1;
        let mut walk = vec![(start, 0usize)];
        while let Some(&(node, next)) = walk.last() {
            if let Some(&(edge, to)) = out[node].get(next) {
                if let Some(top) = walk.last_mut() {
                    top.1 += 1;
                }
                match state[to] {
                    0 => {
                        state[to] = 1;
                        walk.push((to, 0));
                    }
                    1 => back[edge] = true,
                    _ => {}
                }
            } else {
                state[node] = 2;
                walk.pop();
            }
        }
    }
    back
}

/// Each node's layer: one after the latest of the nodes before it. A node nothing points
/// to stays in the first layer, so the parts that start things line up there.
fn ranks(count: usize, laid: &[Option<(usize, usize)>]) -> Vec<usize> {
    let mut rank = vec![0usize; count];
    let mut waiting = vec![0usize; count];
    let mut out = vec![Vec::new(); count];
    for &(from, to) in laid.iter().flatten() {
        out[from].push(to);
        waiting[to] += 1;
    }
    let mut ready: BTreeSet<usize> = (0..count).filter(|&n| waiting[n] == 0).collect();
    while let Some(node) = ready.pop_first() {
        for &to in &out[node] {
            rank[to] = rank[to].max(rank[node] + 1);
            waiting[to] -= 1;
            if waiting[to] == 0 {
                ready.insert(to);
            }
        }
    }
    rank
}

/// Orders each layer: sweeps down and up, each item to the average place of its
/// neighbours in the layer before, a group's items kept together and the groups in one
/// order on every layer. The order with fewest crossings wins.
fn order(structure: &mut Structure, groups: usize) -> Vec<usize> {
    let mut place = vec![0usize; structure.items.len()];
    let set_places = |ranks: &[Vec<usize>], place: &mut [usize]| {
        for rank in ranks {
            for (at, &item) in rank.iter().enumerate() {
                place[item] = at;
            }
        }
    };
    set_places(&structure.ranks, &mut place);
    // Groups together from the start, so even the first order keeps them whole, and in
    // one order for good: were it to change, two layers could put two groups each side
    // of the other.
    let first_order = group_order(structure, &place, groups);
    for rank in 0..structure.ranks.len() {
        let keyed = structure.ranks[rank]
            .iter()
            .enumerate()
            .map(|(at, &item)| (at as f32, item))
            .collect();
        structure.ranks[rank] = arrange(keyed, &structure.items, &first_order);
    }
    set_places(&structure.ranks, &mut place);
    let mut best = structure.ranks.clone();
    let mut fewest = crossings(structure, &place);
    let layers = structure.ranks.len();
    for sweep in 0..SWEEPS {
        if fewest == 0 && sweep > 1 {
            break;
        }
        let group_order = &first_order;
        let downward = sweep % 2 == 0;
        let rank_order: Vec<usize> = if downward {
            (1..layers).collect()
        } else {
            (0..layers.saturating_sub(1)).rev().collect()
        };
        for rank in rank_order {
            let items = &structure.items;
            let neighbours = if downward {
                &structure.up
            } else {
                &structure.down
            };
            let keyed: Vec<(f32, usize)> = structure.ranks[rank]
                .iter()
                .enumerate()
                .map(|(at, &item)| {
                    let near = &neighbours[item];
                    let key = if near.is_empty() {
                        at as f32
                    } else {
                        near.iter().map(|&n| place[n] as f32).sum::<f32>() / near.len() as f32
                    };
                    (key, item)
                })
                .collect();
            structure.ranks[rank] = arrange(keyed, items, group_order);
            for (at, &item) in structure.ranks[rank].iter().enumerate() {
                place[item] = at;
            }
        }
        let count = crossings(structure, &place);
        if count < fewest {
            fewest = count;
            best = structure.ranks.clone();
        }
    }
    structure.ranks = best;
    first_order
}

/// The groups by where their nodes stand on average, left to right.
fn group_order(structure: &Structure, place: &[usize], groups: usize) -> Vec<usize> {
    let mut sums = vec![(0f32, 0usize); groups];
    for (at, item) in structure.items.iter().enumerate() {
        if let (Some(group), Some(_)) = (item.group, item.node) {
            let width = structure.ranks[item.rank].len().max(1) as f32;
            sums[group].0 += (place[at] as f32 + 0.5) / width;
            sums[group].1 += 1;
        }
    }
    let mut order: Vec<usize> = (0..groups).collect();
    let mean = |g: usize| sums[g].0 / sums[g].1.max(1) as f32;
    order.sort_by(|&a, &b| mean(a).total_cmp(&mean(b)).then(a.cmp(&b)));
    // The place of each group in that order.
    let mut rank = vec![0; groups];
    for (at, &group) in order.iter().enumerate() {
        rank[group] = at;
    }
    rank
}

/// One layer's items by their keys, a group's items side by side where its average puts
/// them, and the groups in their one order.
fn arrange(mut keyed: Vec<(f32, usize)>, items: &[Item], group_order: &[usize]) -> Vec<usize> {
    keyed.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    enum Unit {
        Item(usize),
        Group(usize),
    }
    let mut group_keys: Vec<(usize, f32, usize)> = Vec::new();
    for &(key, item) in &keyed {
        if let Some(group) = items[item].group {
            match group_keys.iter_mut().find(|(g, _, _)| *g == group) {
                Some(entry) => {
                    entry.1 += key;
                    entry.2 += 1;
                }
                None => group_keys.push((group, key, 1)),
            }
        }
    }
    let mut units: Vec<(f32, Unit)> = Vec::new();
    for &(key, item) in &keyed {
        match items[item].group {
            None => units.push((key, Unit::Item(item))),
            Some(group) => {
                if !units
                    .iter()
                    .any(|(_, unit)| matches!(unit, Unit::Group(g) if *g == group))
                {
                    let (_, sum, count) = group_keys
                        .iter()
                        .find(|(g, _, _)| *g == group)
                        .copied()
                        .unwrap_or((group, key, 1));
                    units.push((sum / count as f32, Unit::Group(group)));
                }
            }
        }
    }
    units.sort_by(|a, b| a.0.total_cmp(&b.0));
    // The groups take the places groups have, in the one order.
    let mut groups: Vec<usize> = units
        .iter()
        .filter_map(|(_, unit)| match unit {
            Unit::Group(g) => Some(*g),
            Unit::Item(_) => None,
        })
        .collect();
    groups.sort_by_key(|&g| group_order.get(g).copied().unwrap_or(g));
    let mut groups = groups.into_iter();
    let mut out = Vec::with_capacity(keyed.len());
    for (_, unit) in &units {
        match unit {
            Unit::Item(item) => out.push(*item),
            Unit::Group(_) => {
                if let Some(group) = groups.next() {
                    out.extend(
                        keyed
                            .iter()
                            .filter(|(_, item)| items[*item].group == Some(group))
                            .map(|(_, item)| *item),
                    );
                }
            }
        }
    }
    out
}

/// How many pairs of edges cross between neighbouring layers.
fn crossings(structure: &Structure, place: &[usize]) -> usize {
    let mut total = 0;
    for rank in &structure.ranks {
        let mut pairs: Vec<(usize, usize)> = Vec::new();
        for &item in rank {
            for &next in &structure.down[item] {
                pairs.push((place[item], place[next]));
            }
        }
        for (at, a) in pairs.iter().enumerate() {
            for b in &pairs[at + 1..] {
                if (a.0 < b.0 && a.1 > b.1) || (a.0 > b.0 && a.1 < b.1) {
                    total += 1;
                }
            }
        }
    }
    total
}

/// The sizes of the graph's parts along its two axes.
struct Extents {
    /// How far each item reaches along the main axis.
    main: Vec<f32>,
    /// How far each item reaches before and after its centre along its layer; after
    /// includes a loop to itself and the loop's label.
    before: Vec<f32>,
    after: Vec<f32>,
    /// Padding of a group's box: before and after along its layer, before and after
    /// along the main axis.
    group_cross: (f32, f32),
    group_main: (f32, f32),
}

impl Extents {
    fn sep(&self, items: &[Item], a: usize, b: usize) -> f32 {
        let gap = match (items[a].node, items[b].node) {
            (Some(_), Some(_)) => NODE_GAP,
            (None, None) => WAY_GAP,
            _ => (NODE_GAP + WAY_GAP) / 2.,
        };
        let (ga, gb) = (items[a].group, items[b].group);
        let pads = if ga == gb {
            0.
        } else {
            ga.map_or(0., |_| self.group_cross.1) + gb.map_or(0., |_| self.group_cross.0)
        };
        self.after[a] + self.before[b] + gap + pads
    }
}

/// `desired` places of a layer's items moved as little as they can, by weight, while
/// each stays `seps` from the one before: pooling neighbours that would overlap.
fn settle(desired: &[f32], seps: &[f32], weights: &[f32]) -> Vec<f32> {
    let count = desired.len();
    let mut offset = vec![0f32; count];
    for at in 1..count {
        offset[at] = offset[at - 1] + seps[at - 1];
    }
    // Blocks of (mean, weight, length) of the shifted places, kept non-decreasing.
    let mut blocks: Vec<(f32, f32, usize)> = Vec::new();
    for at in 0..count {
        let mut block = (desired[at] - offset[at], weights[at], 1);
        while let Some(&last) = blocks.last() {
            if last.0 <= block.0 {
                break;
            }
            blocks.pop();
            let weight = last.1 + block.1;
            block = (
                (last.0 * last.1 + block.0 * block.1) / weight,
                weight,
                last.2 + block.2,
            );
        }
        blocks.push(block);
    }
    let mut out = Vec::with_capacity(count);
    for (mean, _, length) in blocks {
        for _ in 0..length {
            out.push(mean + offset[out.len()]);
        }
    }
    out
}

/// Each item's place along its layer.
fn cross_places(structure: &Structure, extents: &Extents) -> Vec<f32> {
    let items = &structure.items;
    let mut cross = vec![0f32; items.len()];
    let weight = |item: usize| if items[item].node.is_some() { 1. } else { 2. };
    let settle_rank = |rank: &[usize], desired: &[f32]| {
        let seps: Vec<f32> = rank
            .windows(2)
            .map(|pair| extents.sep(items, pair[0], pair[1]))
            .collect();
        let weights: Vec<f32> = rank.iter().map(|&item| weight(item)).collect();
        settle(desired, &seps, &weights)
    };
    for rank in &structure.ranks {
        let placed = settle_rank(rank, &vec![0.; rank.len()]);
        for (&item, x) in rank.iter().zip(placed) {
            cross[item] = x;
        }
    }
    let mean_of = |near: &[usize], cross: &[f32], own: f32| {
        if near.is_empty() {
            own
        } else {
            near.iter().map(|&n| cross[n]).sum::<f32>() / near.len() as f32
        }
    };
    let layers = structure.ranks.len();
    for round in 0..=ROUNDS {
        let downward: Vec<usize> = (1..layers).collect();
        let upward: Vec<usize> = (0..layers.saturating_sub(1)).rev().collect();
        let passes: Vec<(&Vec<usize>, &Vec<Vec<usize>>)> = if round == ROUNDS {
            vec![(&downward, &structure.up)]
        } else {
            vec![(&downward, &structure.up), (&upward, &structure.down)]
        };
        for (order, near) in passes {
            for &rank in order {
                let members = &structure.ranks[rank];
                let desired: Vec<f32> = members
                    .iter()
                    .map(|&item| mean_of(&near[item], &cross, cross[item]))
                    .collect();
                for (&item, x) in members.iter().zip(settle_rank(members, &desired)) {
                    cross[item] = x;
                }
            }
        }
    }
    cross
}

/// Moves items and groups right, never left, until no item stands in a group that is not
/// its own and no two groups on the same layers overlap; gives each group's span along
/// the layers, padding included. It is a longest path over "this is at least that far
/// right of that": each layer's order, each group around its items, every other item
/// of the group's layers on the side the order puts it.
fn clear_groups(
    structure: &Structure,
    extents: &Extents,
    cross: &mut [f32],
    least: &[f32],
) -> Vec<(f32, f32)> {
    let items = &structure.items;
    let count = items.len();
    let groups = least.len();
    // Variables: the items, then each group's start and end.
    let start = |g: usize| count + 2 * g;
    let end = |g: usize| count + 2 * g + 1;
    let mut value: Vec<f32> = cross.to_vec();
    value.resize(count + 2 * groups, f32::MIN);
    let mut rules: Vec<(usize, usize, f32)> = Vec::new();
    for rank in &structure.ranks {
        for pair in rank.windows(2) {
            rules.push((pair[0], pair[1], extents.sep(items, pair[0], pair[1])));
        }
    }
    let span: Vec<Option<(usize, usize)>> = (0..groups).map(|g| group_ranks(items, g)).collect();
    let (pad_before, pad_after) = extents.group_cross;
    for g in 0..groups {
        rules.push((start(g), end(g), least[g]));
        for (at, item) in items.iter().enumerate() {
            if item.group == Some(g) {
                rules.push((start(g), at, extents.before[at] + pad_before));
                rules.push((at, end(g), extents.after[at] + pad_after));
                // The group starts where its leftmost item asks, and only moves right.
                let left = cross[at] - extents.before[at] - pad_before;
                value[start(g)] = if value[start(g)] == f32::MIN {
                    left
                } else {
                    value[start(g)].min(left)
                };
            }
        }
        value[end(g)] = value[start(g)];
    }
    let middle: Vec<f32> = (0..groups)
        .map(|g| {
            let members: Vec<f32> = (0..count)
                .filter(|&at| items[at].group == Some(g))
                .map(|at| cross[at])
                .collect();
            members.iter().sum::<f32>() / members.len().max(1) as f32
        })
        .collect();
    let mut in_order: Vec<usize> = (0..groups).collect();
    in_order.sort_by_key(|&g| structure.group_rank[g]);
    for (at, &g) in in_order.iter().enumerate() {
        for &h in &in_order[at + 1..] {
            let overlap = match (span[g], span[h]) {
                (Some((a, b)), Some((c, d))) => a <= d && c <= b,
                _ => false,
            };
            if overlap {
                rules.push((end(g), start(h), NODE_GAP));
            }
        }
    }
    for (rank_at, rank) in structure.ranks.iter().enumerate() {
        // Where each group stands in this layer, as the items before and after it: its
        // own items, or a place between two items that keeps the groups in their order.
        let mut floor = 0;
        for (at, &g) in in_order.iter().enumerate() {
            let Some((first, last)) = span[g] else {
                continue;
            };
            if rank_at < first || rank_at > last {
                continue;
            }
            let own: Vec<usize> = (0..rank.len())
                .filter(|&i| items[rank[i]].group == Some(g))
                .collect();
            let (slot_start, slot_end) = match (own.first(), own.last()) {
                (Some(&a), Some(&b)) => (a, b + 1),
                _ => {
                    let ceiling = (0..rank.len())
                        .find(|&i| {
                            items[rank[i]]
                                .group
                                .is_some_and(|h| in_order[at + 1..].contains(&h))
                        })
                        .unwrap_or(rank.len());
                    let by_middle = (0..rank.len())
                        .filter(|&i| cross[rank[i]] < middle[g])
                        .count();
                    let k = by_middle.clamp(floor, ceiling.max(floor));
                    (k, k)
                }
            };
            floor = floor.max(slot_end);
            for (i, &item) in rank.iter().enumerate() {
                if items[item].group == Some(g) {
                    continue;
                }
                let gap = if items[item].node.is_some() {
                    NODE_GAP
                } else {
                    WAY_GAP
                };
                let own = items[item].group.is_some();
                if i < slot_start {
                    let pad = if own { pad_after } else { 0. };
                    rules.push((item, start(g), extents.after[item] + pad + gap));
                } else if i >= slot_end {
                    let pad = if own { pad_before } else { 0. };
                    rules.push((end(g), item, extents.before[item] + pad + gap));
                }
            }
        }
    }
    // Rounds of pushing; the rules are in order without a circle, so as many rounds as
    // there are variables settle them. A circle would only stop at the cap.
    for _ in 0..=value.len() {
        let mut moved = false;
        for &(from, to, gap) in &rules {
            if value[to] < value[from] + gap - 0.001 {
                value[to] = value[from] + gap;
                moved = true;
            }
        }
        if !moved {
            break;
        }
    }
    cross.copy_from_slice(&value[..count]);
    (0..groups)
        .map(|g| (value[start(g)], value[end(g)]))
        .collect()
}

/// One edge's piece in the space between two layers.
struct Piece {
    edge: usize,
    /// The items it joins, so pieces from one point or into one point can share a track.
    ends: (usize, usize),
    /// Where it leaves the layer before and reaches the one after, along the layers.
    from: f32,
    to: f32,
    track: Option<usize>,
}

/// A label set in the space between two layers: its edge, its span along the layers and
/// its row.
struct Placed {
    edge: usize,
    start: f32,
    end: f32,
    row: usize,
}

pub(super) fn layout(
    diagram: &Diagram,
    measure: &dyn Measure,
    direction: Direction,
    wrap: f32,
) -> Layout {
    let structure = structure(diagram);
    let items = &structure.items;
    let down = direction == Direction::Down;
    let smooth = diagram.kind() == DiagramKind::State;
    let arrows = diagram.kind() != DiagramKind::Tree;
    let edges = diagram.edges();
    let ends = diagram.ends();

    let sized: Vec<Sized> = diagram
        .nodes()
        .iter()
        .map(|node| size(measure, node, wrap))
        .collect();
    // Along the main axis, then along the layer.
    let axes = |w: f32, h: f32| if down { (h, w) } else { (w, h) };
    let labels: Vec<Option<(Vec<String>, f32, f32)>> = edges
        .iter()
        .map(|edge| {
            edge.label
                .as_ref()
                .map(|label| edge_label(measure, label.as_str(), wrap.min(140.)))
        })
        .collect();

    // Loops of a node to itself, in the order given.
    let mut loops: Vec<Vec<usize>> = vec![Vec::new(); sized.len()];
    for (edge, &(from, to)) in ends.iter().enumerate() {
        if from == to {
            loops[from].push(edge);
        }
    }
    let mut extents = Extents {
        main: vec![0.; items.len()],
        before: vec![0.; items.len()],
        after: vec![0.; items.len()],
        group_cross: if down {
            (GROUP_PAD, GROUP_PAD)
        } else {
            (GROUP_PAD + GROUP_HEAD, GROUP_PAD)
        },
        group_main: if down {
            (GROUP_PAD + GROUP_HEAD, GROUP_PAD)
        } else {
            (GROUP_PAD, GROUP_PAD)
        },
    };
    for (at, item) in items.iter().enumerate() {
        if let Some(node) = item.node {
            let (main, across) = axes(sized[node].w, sized[node].h);
            // Labels of loops stack along the node, inside its layer.
            let labelled = loops[node].iter().filter(|&&e| labels[e].is_some()).count();
            let stacked = labelled as f32 * (loop_row(&loops[node], &labels, &axes) + 2.);
            extents.main[at] = main.max(stacked);
            extents.before[at] = across / 2.;
            extents.after[at] = across / 2. + loop_room(&loops[node], &labels, down);
        }
    }
    // An edge that skips layers carries its label on its middle waypoint, which takes
    // the label's room in its layer, so the label sits on the line and nothing else can
    // stand there.
    let mut label_on: Vec<Option<usize>> = vec![None; edges.len()];
    for (edge, chain) in structure.chains.iter().enumerate() {
        if let (Some(chain), Some((_, w, h))) = (chain, &labels[edge])
            && chain.items.len() > 2
        {
            let way = chain.items[chain.items.len() / 2];
            let (main, across) = axes(*w, *h);
            extents.main[way] = main;
            extents.before[way] = across / 2.;
            extents.after[way] = across / 2.;
            label_on[edge] = Some(way);
        }
    }
    let group_names: Vec<String> = diagram
        .groups()
        .iter()
        .map(|group| group.label.as_str().to_uppercase())
        .collect();
    let name_widths: Vec<f32> = group_names
        .iter()
        .map(|name| measure.width(name, TextStyle::Group))
        .collect();
    let least: Vec<f32> = name_widths
        .iter()
        .map(|w| if down { w + 2. * 12. } else { 0. })
        .collect();

    let mut cross = cross_places(&structure, &extents);
    let spans = clear_groups(&structure, &extents, &mut cross, &least);

    // Ports: where an edge leaves or reaches a node along its side, spread so edges do
    // not share one point, in the order of where they go.
    let mut out_port = vec![0f32; edges.len()];
    let mut in_port = vec![0f32; edges.len()];
    for node in 0..sized.len() {
        // A tree's children hang from one point of their parent, as from a bus.
        let spread = diagram.kind() != DiagramKind::Tree
            && !matches!(
                sized[node].figure,
                super::Figure::Diamond | super::Figure::Dot | super::Figure::Ring
            );
        let side = axes(sized[node].w, sized[node].h).1;
        let mut outs: Vec<(f32, usize)> = Vec::new();
        let mut ins: Vec<(f32, usize)> = Vec::new();
        for (edge, chain) in structure.chains.iter().enumerate() {
            let Some(chain) = chain else { continue };
            let length = chain.items.len();
            if chain.items[0] == node {
                outs.push((cross[chain.items[1]], edge));
            }
            if chain.items[length - 1] == node {
                ins.push((cross[chain.items[length - 2]], edge));
            }
        }
        for (list, ports) in [(&mut outs, &mut out_port), (&mut ins, &mut in_port)] {
            list.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
            let count = list.len();
            let step = if spread && count > 1 {
                ((side - 24.) / (count - 1) as f32).clamp(0., 14.)
            } else {
                0.
            };
            for (at, &(_, edge)) in list.iter().enumerate() {
                ports[edge] = cross[node] + (at as f32 - (count as f32 - 1.) / 2.) * step;
            }
        }
    }

    // The pieces of edges between each pair of layers, their tracks and their labels.
    let layers = structure.ranks.len();
    let mut pieces: Vec<Vec<Piece>> = (0..layers.saturating_sub(1)).map(|_| Vec::new()).collect();
    for (edge, chain) in structure.chains.iter().enumerate() {
        let Some(chain) = chain else { continue };
        let last = chain.items.len() - 1;
        for step in 0..last {
            let (a, b) = (chain.items[step], chain.items[step + 1]);
            pieces[items[a].rank].push(Piece {
                edge,
                ends: (a, b),
                from: if step == 0 { out_port[edge] } else { cross[a] },
                to: if step + 1 == last {
                    in_port[edge]
                } else {
                    cross[b]
                },
                track: None,
            });
        }
    }
    let mut tracks = vec![0usize; pieces.len()];
    if !smooth {
        for (gap, list) in pieces.iter_mut().enumerate() {
            let mut bent: Vec<usize> = (0..list.len())
                .filter(|&at| (list[at].from - list[at].to).abs() > 0.5)
                .collect();
            let span = |p: &Piece| (p.from.min(p.to), p.from.max(p.to));
            bent.sort_by(|&a, &b| {
                let (sa, sb) = (span(&list[a]), span(&list[b]));
                sa.0.total_cmp(&sb.0)
                    .then(sa.1.total_cmp(&sb.1))
                    .then(list[a].edge.cmp(&list[b].edge))
            });
            // Pieces that leave one point, or reach one, may run on top of each other:
            // they read as one line that forks or joins.
            let mut used: Vec<Vec<usize>> = Vec::new();
            for at in bent {
                let (a, b) = span(&list[at]);
                let shares = |other: &Piece| {
                    let mine = &list[at];
                    (other.ends.0 == mine.ends.0 && (other.from - mine.from).abs() < 0.5)
                        || (other.ends.1 == mine.ends.1 && (other.to - mine.to).abs() < 0.5)
                };
                let free = used.iter().position(|track| {
                    track.iter().all(|&other| {
                        let (c, d) = span(&list[other]);
                        !(a < d - 0.5 && c < b - 0.5) || shares(&list[other])
                    })
                });
                let track = free.unwrap_or_else(|| {
                    used.push(Vec::new());
                    used.len() - 1
                });
                used[track].push(at);
                list[at].track = Some(track);
            }
            tracks[gap] = used.len();
        }
    }
    let mut placed: Vec<Vec<Placed>> = (0..pieces.len()).map(|_| Vec::new()).collect();
    let mut rows: Vec<Vec<f32>> = (0..pieces.len()).map(|_| Vec::new()).collect();
    for (edge, chain) in structure.chains.iter().enumerate() {
        let (Some(chain), Some((_, w, h))) = (chain, &labels[edge]) else {
            continue;
        };
        if label_on[edge].is_some() {
            continue;
        }
        let gap = items[chain.items[chain.items.len() - 2]].rank;
        let (main, across) = axes(*w, *h);
        let at = in_port[edge];
        let lines: Vec<f32> = pieces[gap]
            .iter()
            .filter(|p| p.edge != edge)
            .map(|p| p.to)
            .collect();
        let candidates = [
            (at - across / 2., at + across / 2.),
            (at + 5., at + 5. + across),
            (at - 5. - across, at - 5.),
        ];
        let clear_of_lines = |(a, b): (f32, f32)| lines.iter().all(|&x| x <= a - 3. || x >= b + 3.);
        let mut choice = None;
        'rows: for row in 0..=rows[gap].len() {
            for &(a, b) in &candidates {
                let clear_of_labels = placed[gap]
                    .iter()
                    .filter(|p| p.row == row)
                    .all(|p| b + 6. <= p.start || a >= p.end + 6.);
                if clear_of_lines((a, b)) && clear_of_labels {
                    choice = Some((row, a, b));
                    break 'rows;
                }
            }
        }
        let (row, a, b) = choice.unwrap_or((rows[gap].len(), candidates[0].0, candidates[0].1));
        if row == rows[gap].len() {
            rows[gap].push(0.);
        }
        rows[gap][row] = rows[gap][row].max(main);
        placed[gap].push(Placed {
            edge,
            start: a,
            end: b,
            row,
        });
    }

    // Along the main axis: each layer's band, and the space after it.
    let mut band = vec![(0f32, 0f32); layers];
    let mut gap_layout: Vec<GapLayout> = Vec::new();
    let mut main = 0.;
    let groups_ending = |rank: usize| {
        (0..diagram.groups().len())
            .any(|g| group_ranks(items, g).is_some_and(|(_, last)| last == rank))
    };
    let groups_starting = |rank: usize| {
        (0..diagram.groups().len())
            .any(|g| group_ranks(items, g).is_some_and(|(first, _)| first == rank))
    };
    for rank in 0..layers {
        let size = structure.ranks[rank]
            .iter()
            .map(|&item| extents.main[item])
            .fold(0., f32::max);
        band[rank] = (main, size);
        main += size;
        if rank + 1 < layers {
            let end_pad = if groups_ending(rank) {
                extents.group_main.1
            } else {
                0.
            };
            let start_pad = if groups_starting(rank + 1) {
                extents.group_main.0
            } else {
                0.
            };
            let track_room = if tracks[rank] > 0 {
                tracks[rank] as f32 * TRACK + 4.
            } else {
                0.
            };
            let label_room: f32 = rows[rank].iter().map(|size| size + 4.).sum();
            let least = if smooth && label_room > 0. { 96. } else { 40. };
            let inner = (12. + track_room + label_room + HEAD_LENGTH + 7.).max(least);
            // Curves meet at their ends, so their labels stand in the middle of the space,
            // where the curves are apart; straight runs set theirs after the tracks.
            let labels = if smooth {
                main + end_pad + (inner - label_room) / 2.
            } else {
                main + end_pad + 12. + track_room
            };
            let layout = GapLayout {
                tracks: main + end_pad + 12.,
                labels,
            };
            gap_layout.push(layout);
            main += end_pad + inner + start_pad;
        }
    }
    let center = |item: usize| band[items[item].rank].0 + band[items[item].rank].1 / 2.;
    let near_side = |item: usize| center(item) - extents.main[item] / 2.;
    let far_side = |item: usize| center(item) + extents.main[item] / 2.;

    // From the two axes to the page.
    let page = |cross: f32, main: f32| {
        if down {
            Point::new(cross, main)
        } else {
            Point::new(main, cross)
        }
    };
    let page_rect = |cross: f32, main: f32, across: f32, along: f32| {
        if down {
            Rect::new(cross, main, across, along)
        } else {
            Rect::new(main, cross, along, across)
        }
    };

    let mut laid = Layout::empty();
    let mut node_rects = vec![Rect::default(); sized.len()];
    for (node, sized) in sized.iter().enumerate() {
        let (along, across) = axes(sized.w, sized.h);
        let rect = page_rect(
            cross[node] - across / 2.,
            center(node) - along / 2.,
            across,
            along,
        );
        node_rects[node] = rect;
        laid.nodes.push(NodeBox {
            node,
            rect,
            figure: sized.figure,
            tone: diagram.nodes()[node].tone,
        });
        node_texts(measure, node, sized, rect, &mut laid.texts);
    }

    for (edge, chain) in structure.chains.iter().enumerate() {
        let (from, to) = ends[edge];
        let style = edges[edge].style();
        let Some(chain) = chain else {
            let index = loops[from].iter().position(|&e| e == edge).unwrap_or(0);
            let labelled: Vec<usize> = loops[from]
                .iter()
                .copied()
                .filter(|&e| labels[e].is_some())
                .collect();
            let slot = (
                labelled.iter().position(|&e| e == edge).unwrap_or(0),
                labelled.len(),
                loop_row(&loops[from], &labels, &axes),
            );
            let (segs, head, label) = self_loop(
                &extents,
                from,
                cross[from],
                center(from),
                (index, loops[from].len()),
                slot,
                labels[edge].as_ref().map(|(_, w, h)| axes(*w, *h)),
                &page,
                &page_rect,
            );
            if let (Some(rect), Some((lines, _, _))) = (label, &labels[edge]) {
                laid.texts.extend(edge_texts(measure, edge, lines, rect));
            }
            laid.edges.push(EdgePath {
                edge,
                from,
                to,
                segs,
                head: Some(head),
                style,
                label,
            });
            continue;
        };
        let last = chain.items.len() - 1;
        let start = (out_port[edge], far_side(chain.items[0]));
        let mut segs: Vec<Seg> = Vec::new();
        let mut points = vec![page(start.0, start.1)];
        let mut current = start;
        for step in 0..last {
            let gap = items[chain.items[step]].rank;
            let next = chain.items[step + 1];
            let piece = pieces[gap].iter().find(|p| p.edge == edge);
            let target_cross = piece.map_or(cross[next], |p| p.to);
            let arrive = if step + 1 == last {
                near_side(next)
            } else {
                band[items[next].rank].0
            };
            if smooth {
                let middle = (current.1 + arrive) / 2.;
                segs.push(Seg::Cubic(
                    page(current.0, current.1),
                    page(current.0, middle),
                    page(target_cross, middle),
                    page(target_cross, arrive),
                ));
            } else if let Some(track) = piece.and_then(|p| p.track) {
                let level = gap_layout[gap].tracks + track as f32 * TRACK;
                points.push(page(current.0, level));
                points.push(page(target_cross, level));
            }
            if step + 1 == last {
                points.push(page(target_cross, arrive));
            } else {
                let (top, size) = band[items[next].rank];
                points.push(page(target_cross, top));
                points.push(page(target_cross, top + size));
                if smooth {
                    segs.push(Seg::Line(
                        page(target_cross, top),
                        page(target_cross, top + size),
                    ));
                }
            }
            current = (
                target_cross,
                band[items[next].rank].0 + band[items[next].rank].1,
            );
        }
        let (segs, head) = if smooth {
            finish_curve(segs, chain.reversed, arrows)
        } else {
            finish_lines(points, chain.reversed, arrows)
        };
        let on_way = label_on[edge].map(|way| {
            let (along, across) = (extents.main[way], extents.before[way] * 2.);
            page_rect(
                cross[way] - across / 2.,
                center(way) - along / 2.,
                across,
                along,
            )
        });
        let label = on_way.or_else(|| {
            placed
                .iter()
                .enumerate()
                .flat_map(|(gap, list)| list.iter().map(move |p| (gap, p)))
                .find(|(_, p)| p.edge == edge)
                .map(|(gap, p)| {
                    let row_start: f32 = gap_layout[gap].labels
                        + rows[gap][..p.row].iter().map(|size| size + 4.).sum::<f32>();
                    let along = rows[gap][p.row];
                    let own = labels[edge].as_ref().map_or(0., |(_, w, h)| axes(*w, *h).0);
                    page_rect(
                        p.start,
                        row_start + (along - own) / 2.,
                        p.end - p.start,
                        own,
                    )
                })
        });
        if let (Some(rect), Some((lines, _, _))) = (label, &labels[edge]) {
            laid.texts.extend(edge_texts(measure, edge, lines, rect));
        }
        laid.edges.push(EdgePath {
            edge,
            from,
            to,
            segs,
            head,
            style,
            label,
        });
    }
    laid.edges.sort_by_key(|path| path.edge);
    if smooth {
        follow_curves(&mut laid, &label_on, down);
    }

    for (group, name) in group_names.iter().enumerate() {
        let (start, end) = spans[group];
        let Some((first, last)) = group_ranks(items, group) else {
            continue;
        };
        let main_start = band[first].0 - extents.group_main.0;
        let main_end = band[last].0 + band[last].1 + extents.group_main.1;
        let mut rect = page_rect(start, main_start, end - start, main_end - main_start);
        if !down && rect.w < name_widths[group] + 24. {
            let grow = name_widths[group] + 24. - rect.w;
            rect = Rect::new(rect.x - grow / 2., rect.y, rect.w + grow, rect.h);
        }
        laid.groups.push(GroupBox { group, rect });
        laid.texts.push(group_name(
            name,
            name_widths[group],
            group,
            rect,
            &laid.edges,
        ));
    }
    laid.fit()
}

/// Moves each label set between two layers onto its curve, where the curve crosses the
/// label's middle, unless it would land on another label or a node there.
fn follow_curves(laid: &mut Layout, label_on: &[Option<usize>], down: bool) {
    for at in 0..laid.edges.len() {
        let edge = &laid.edges[at];
        let Some(rect) = edge.label else { continue };
        if label_on[edge.edge].is_some() {
            continue;
        }
        let middle = rect.center();
        let (along, across) = if down {
            (middle.y, middle.x)
        } else {
            (middle.x, middle.y)
        };
        let Some(on) = cross_at(&edge.segs, along, down) else {
            continue;
        };
        let moved = if down {
            rect.moved(on - across, 0.)
        } else {
            rect.moved(0., on - across)
        };
        let blocked = laid
            .edges
            .iter()
            .enumerate()
            .filter(|(other, _)| *other != at)
            .filter_map(|(_, other)| other.label)
            .chain(laid.nodes.iter().map(|node| node.rect))
            .any(|other| other.overlaps(&moved));
        if blocked {
            continue;
        }
        let (dx, dy) = (moved.x - rect.x, moved.y - rect.y);
        let id = laid.edges[at].edge;
        laid.edges[at].label = Some(moved);
        for text in &mut laid.texts {
            if text.owner == Owner::Edge(id) {
                text.x += dx;
                text.y += dy;
            }
        }
    }
}

/// Where along the layers a line of curves is when it is `along` down the main axis.
fn cross_at(segs: &[Seg], along: f32, down: bool) -> Option<f32> {
    let main = |p: Point| if down { p.y } else { p.x };
    let side = |p: Point| if down { p.x } else { p.y };
    for seg in segs {
        let (a, b) = (main(seg.start()), main(seg.end()));
        if along < a.min(b) || along > a.max(b) || (a - b).abs() < 0.01 {
            continue;
        }
        // The curves here only go one way along the main axis: halve until close.
        let (mut low, mut high) = (0f32, 1f32);
        for _ in 0..24 {
            let t = (low + high) / 2.;
            let here = main(seg.at(t));
            if (here < along) == (a < b) {
                low = t;
            } else {
                high = t;
            }
        }
        return Some(side(seg.at((low + high) / 2.)));
    }
    None
}

/// Where the spaces between two layers put their parts along the main axis: the first
/// track, and the first row of labels.
struct GapLayout {
    tracks: f32,
    labels: f32,
}

fn group_ranks(items: &[Item], group: usize) -> Option<(usize, usize)> {
    let ranks = items
        .iter()
        .filter(|item| item.group == Some(group))
        .map(|item| item.rank);
    let first = ranks.clone().min()?;
    Some((first, ranks.max()?))
}

/// The room a node's loops to itself take beside it: the loops and their labels.
fn loop_room(loops: &[usize], labels: &[Option<(Vec<String>, f32, f32)>], down: bool) -> f32 {
    if loops.is_empty() {
        return 0.;
    }
    let widest = loops
        .iter()
        .filter_map(|&edge| labels[edge].as_ref())
        .map(|(_, w, h)| if down { *w } else { *h })
        .fold(0., f32::max);
    LOOP + 10. * (loops.len() - 1) as f32 + 8. + widest
}

/// How far along the main axis the tallest label of a node's loops reaches.
fn loop_row(
    loops: &[usize],
    labels: &[Option<(Vec<String>, f32, f32)>],
    axes: &dyn Fn(f32, f32) -> (f32, f32),
) -> f32 {
    loops
        .iter()
        .filter_map(|&edge| labels[edge].as_ref())
        .map(|(_, w, h)| axes(*w, *h).0)
        .fold(0., f32::max)
}

/// A loop from a node back to itself, out of the side after it along its layer, and the
/// box of its label beyond the loop.
#[allow(clippy::too_many_arguments)]
fn self_loop(
    extents: &Extents,
    node: usize,
    cross: f32,
    center: f32,
    (index, count): (usize, usize),
    (slot, slots, row): (usize, usize, f32),
    label: Option<(f32, f32)>,
    page: &dyn Fn(f32, f32) -> Point,
    page_rect: &dyn Fn(f32, f32, f32, f32) -> Rect,
) -> (Vec<Seg>, Head, Option<Rect>) {
    let side = cross + extents.before[node];
    let reach = LOOP + 10. * index as f32;
    let rise = 16. + 4. * index as f32;
    let start = page(side, center - 7.);
    let end = page(side, center + 7.);
    let c1 = page(side + reach, center - rise);
    let c2 = page(side + reach, center + rise);
    let (segs, head) = finish_curve(vec![Seg::Cubic(start, c1, c2, end)], false, true);
    let rect = label.map(|(along, across)| {
        let offset = (slot as f32 - (slots as f32 - 1.) / 2.) * (row + 2.);
        let beyond = side + LOOP + 10. * (count - 1) as f32 + 6.;
        page_rect(beyond, center - along / 2. + offset, across, along)
    });
    (
        segs,
        head.unwrap_or(Head {
            tip: end,
            back: end,
        }),
        rect,
    )
}

/// An edge of straight runs: turned round if it was laid out backwards, cut short for
/// its arrowhead, its corners rounded.
fn finish_lines(mut points: Vec<Point>, reversed: bool, arrow: bool) -> (Vec<Seg>, Option<Head>) {
    if reversed {
        points.reverse();
    }
    // Points in a straight line with their neighbours are not corners.
    let mut kept: Vec<Point> = Vec::new();
    for point in points {
        if kept.last().is_some_and(|last: &Point| {
            (last.x - point.x).abs() < 0.01 && (last.y - point.y).abs() < 0.01
        }) {
            continue;
        }
        if kept.len() >= 2 {
            let (a, b) = (kept[kept.len() - 2], kept[kept.len() - 1]);
            let straight = ((a.x - b.x).abs() < 0.01 && (b.x - point.x).abs() < 0.01)
                || ((a.y - b.y).abs() < 0.01 && (b.y - point.y).abs() < 0.01);
            if straight {
                kept.pop();
            }
        }
        kept.push(point);
    }
    let head = if arrow && kept.len() >= 2 {
        let tip = kept[kept.len() - 1];
        let before = kept[kept.len() - 2];
        let back = toward(tip, before, super::HEAD_LENGTH);
        if let Some(last) = kept.last_mut() {
            *last = back;
        }
        Some(Head { tip, back })
    } else {
        None
    };
    (rounded(&kept, ROUND), head)
}

/// An edge of curves: turned round if it was laid out backwards, its last curve cut short
/// for its arrowhead.
fn finish_curve(mut segs: Vec<Seg>, reversed: bool, arrow: bool) -> (Vec<Seg>, Option<Head>) {
    if reversed {
        segs.reverse();
        for seg in &mut segs {
            *seg = match *seg {
                Seg::Line(a, b) => Seg::Line(b, a),
                Seg::Cubic(a, b, c, d) => Seg::Cubic(d, c, b, a),
            };
        }
    }
    if !arrow {
        return (segs, None);
    }
    let Some(last) = segs.last_mut() else {
        return (segs, None);
    };
    let (tip, back) = match *last {
        Seg::Line(a, b) => {
            let back = toward(b, a, HEAD_LENGTH);
            *last = Seg::Line(a, back);
            (b, back)
        }
        Seg::Cubic(a, b, c, d) => {
            let from = if (c.x - d.x).abs() + (c.y - d.y).abs() > 0.5 {
                c
            } else {
                b
            };
            let back = toward(d, from, HEAD_LENGTH);
            *last = Seg::Cubic(a, b, c, back);
            (d, back)
        }
    };
    (segs, Some(Head { tip, back }))
}

/// The point `length` from `from` toward `to`.
fn toward(from: Point, to: Point, length: f32) -> Point {
    let (dx, dy) = (to.x - from.x, to.y - from.y);
    let distance = (dx * dx + dy * dy).sqrt();
    if distance < 0.001 {
        return from;
    }
    let t = (length / distance).min(1.);
    Point::new(from.x + dx * t, from.y + dy * t)
}

/// A group's name at the top left of its box, or the top right when an edge runs
/// through the left.
fn group_name(name: &str, width: f32, group: usize, rect: Rect, edges: &[EdgePath]) -> Text {
    let line = TextStyle::Group.line_height();
    let top = rect.y + 8.;
    let crosses = |left: f32| {
        let area = Rect::new(left - 3., top, width + 6., line);
        edges.iter().any(|edge| {
            super::polyline(&edge.segs)
                .windows(2)
                .any(|pair| crosses_rect(pair[0], pair[1], &area))
        })
    };
    let left = rect.x + 12.;
    let right = rect.right() - 12. - width;
    let (x, anchor) = if crosses(left) && !crosses(right) {
        (rect.right() - 12., Anchor::End)
    } else {
        (left, Anchor::Start)
    };
    Text {
        text: name.to_owned(),
        x,
        y: top,
        width,
        anchor,
        style: TextStyle::Group,
        owner: Owner::Group(group),
    }
}

/// Whether a straight step from `a` to `b` passes through `rect`.
fn crosses_rect(a: Point, b: Point, rect: &Rect) -> bool {
    const STEPS: usize = 8;
    (0..=STEPS).any(|step| {
        let t = step as f32 / STEPS as f32;
        rect.contains(Point::new(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t))
    })
}

#[cfg(test)]
mod tests {
    use super::settle;

    #[test]
    fn settling_keeps_order_and_spacing_and_moves_little() {
        let placed = settle(&[0., 0., 100.], &[10., 10.], &[1., 1., 1.]);
        assert_eq!(placed, vec![-5., 5., 100.]);
        let placed = settle(&[50., 0.], &[20.], &[1., 1.]);
        assert_eq!(placed, vec![15., 35.]);
    }
}
