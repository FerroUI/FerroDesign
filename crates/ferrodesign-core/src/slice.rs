//! A slice describes a viewport in the document. Its transform maps the local
//! export rectangle into world space, independently of the camera and artwork.
use crate::{Document, NodeId, NodeKind, Rect, Transform};

#[derive(Clone, Copy, Debug)]
pub struct ExportSlice {
    pub local: Rect,
    pub world_bounds: Rect,
    pub world_to_local: Transform,
}

impl Document {
    pub fn export_slice(&self, page: NodeId, selection: &[NodeId]) -> Option<ExportSlice> {
        let &[id] = selection else { return None };
        let node = self.nodes.get(&id)?;
        if node.kind != NodeKind::Slice || self.page_of(id) != Some(page) {
            return None;
        }
        let local = node.local_bounds();
        let world = self.world_transform(id)?;
        let world_bounds = world.transform_rect(local);
        if !local.is_finite() || !world_bounds.is_finite() || local.width <= 0.0 || local.height <= 0.0 {
            return None;
        }
        Some(ExportSlice { local, world_bounds, world_to_local: world.inverse()? })
    }
}
