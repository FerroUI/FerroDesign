//! Viewer navigation is separate from authored Back actions. Unconnected boards
//! follow canvas order. A flow can advance through an outgoing navigation link.
use super::*;

#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct FrameOrder {
    pages: BTreeMap<NodeId, Vec<NodeId>>,
    destinations: BTreeMap<NodeId, Vec<(NodeId, NodeId)>>,
    pub(super) keys: BTreeMap<String, Vec<NodeId>>,
}

impl FrameOrder {
    pub(super) fn new(document: &Document) -> Self {
        let mut order = Self::default();
        let mut connected = BTreeSet::new();
        let mut overlay_frames = BTreeSet::new();
        for page in &document.pages {
            let mut frames = Vec::new();
            for &frame in &page.children {
                let Some(node) = document.nodes.get(&frame) else { continue };
                let is_frame = node.kind == NodeKind::Frame;
                if is_frame { frames.push(frame); }
                for id in document.descendants(frame) {
                    for rule in &document.nodes[&id].interactions {
                        if rule.trigger == Trigger::Key {
                            if let Some(key) = rule.key.as_deref().and_then(normalize_prototype_key) {
                                let sources = order.keys.entry(key).or_default();
                                if sources.last() != Some(&id) { sources.push(id); }
                            }
                        }
                        if !is_frame { continue }
                        match rule.action {
                            PrototypeAction::NavigateTo { target } if target != frame => {
                                order.destinations.entry(frame).or_default().push((id, target));
                                connected.extend([frame, target]);
                            }
                            PrototypeAction::OpenOverlay { target, .. } => {
                                overlay_frames.insert(target);
                            }
                            _ => {}
                        }
                    }
                }
            }
            frames.sort_by(|first, second| {
                let a = &document.nodes[first];
                let b = &document.nodes[second];
                a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)).then(first.cmp(second))
            });
            order.pages.insert(page.id, frames);
        }
        for frames in order.pages.values_mut() {
            let linked = frames.iter().any(|id| connected.contains(id));
            frames.retain(|id| connected.contains(id) || (!linked && !overlay_frames.contains(id)));
        }
        order
    }

    pub(super) fn adjacent(&self, document: &Document, player: &PrototypePlayer, forward: bool) -> Option<NodeId> {
        let current = player.current?;
        let page = document.page(document.page_of(current)?)?;
        if page.prototype_start.is_some() {
            if !forward { return None }
            return self.destinations.get(&current)?.iter().find_map(|&(source, target)| {
                (!player.history.contains(&target) && document.effective_visible(source)
                    && document.effective_visible(target)).then_some(target)
            });
        }
        let frames = self.pages.get(&page.id)?;
        let index = frames.iter().position(|id| *id == current)?;
        if forward {
            frames[index + 1..].iter().copied().find(|id| document.effective_visible(*id))
        } else {
            frames[..index].iter().rev().copied().find(|id| document.effective_visible(*id))
        }
    }
}
