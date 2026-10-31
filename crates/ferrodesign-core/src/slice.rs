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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Color, Editor, Node, Point};

    #[test]
    fn transformed_slice_crops_the_page_without_exporting_its_guides() {
        let mut doc = Document::new("Page");
        let page = doc.pages[0].id;
        let mut slice = Node::new(NodeKind::Slice, "Marker", 0.0, 0.0, 40.0, 24.0);
        slice.set_local_transform(Transform::translation(20.0, 40.0) * Transform::rotation(90.0));
        let id = doc.insert_node(page, slice).unwrap();
        let world = doc.world_transform(id).unwrap();
        let p = world.apply(Point::new(12.0, 10.0));
        for (name, x, y) in [("crossing", p.x, p.y), ("private distant object", 900.0, 900.0)] {
            doc.insert_node(page, Node::new(NodeKind::Rectangle, name, x, y, 20.0, 20.0).with_fill(Color::BLACK)).unwrap();
        }
        let region = doc.export_slice(page, &[id]).unwrap();
        assert!(region.world_to_local.apply(p).distance(Point::new(12.0, 10.0)) < 1e-7);
        let svg = doc.export_svg(page, &[id]);
        assert!(svg.contains(r#"width="40" height="24" viewBox="0 0 40 24""#), "{svg}");
        assert!(svg.contains("crossing"));
        assert!(!svg.contains("Marker") && !svg.contains("private distant object"));
        let restored = Document::from_json(&doc.to_json().unwrap()).unwrap();
        assert_eq!(restored.export_svg(page, &[id]), svg);
        assert_eq!(restored.nodes[&id].kind, NodeKind::Slice);
        let mut editor = Editor::new(restored);
        editor.selection = vec![id];
        editor.delete_selection();
        assert!(editor.document.export_slice(page, &[id]).is_none());
        editor.undo();
        assert_eq!(editor.document.export_svg(page, &[id]), svg);
    }
}
