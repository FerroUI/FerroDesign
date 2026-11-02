//! Ruler guide coordinates are measured along the owner's local X or Y axis.
//! Pages use world coordinates. Guides are editor metadata, never artwork.
use crate::{Document, DocumentError, Editor, Guide, NodeId, NodeKind, Point, Rect, Transform};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RulerGuideId {
    pub owner: NodeId,
    pub index: usize,
}

impl Document {
    pub fn ruler_guide(&self, id: RulerGuideId) -> Option<&Guide> {
        self.guides.get(&id.owner)?.get(id.index)
    }

    pub fn ruler_owner_transform(&self, page: NodeId, owner: NodeId) -> Option<Transform> {
        if owner == page {
            return self.page(page).map(|_| Transform::IDENTITY);
        }
        let node = self.nodes.get(&owner)?;
        (self.page_of(owner) == Some(page)
            && self.effective_visible(owner)
            && matches!(
                node.kind,
                NodeKind::Frame | NodeKind::Component | NodeKind::Instance | NodeKind::Section
            ))
        .then(|| self.world_transform(owner))
        .flatten()
    }

    /// Segment in world space. Clip in each ancestor's local axes so rendering,
    /// pointer picking and snapping see the same finite portion of a guide.
    pub fn ruler_guide_segment(
        &self,
        page: NodeId,
        owner: NodeId,
        guide: Guide,
        view: Rect,
    ) -> Option<(Point, Point)> {
        if !guide.position.is_finite() {
            return None;
        }
        let transform = self.ruler_owner_transform(page, owner)?;
        let bounds = if owner == page {
            view
        } else {
            self.nodes.get(&owner)?.local_bounds()
        };
        let p = guide.position;
        let (start, end) = if guide.horizontal {
            if p < bounds.y || p > bounds.bottom() {
                return None;
            }
            (Point::new(bounds.x, p), Point::new(bounds.right(), p))
        } else {
            if p < bounds.x || p > bounds.right() {
                return None;
            }
            (Point::new(p, bounds.y), Point::new(p, bounds.bottom()))
        };
        let start = transform.apply(start);
        let end = transform.apply(end);
        let (mut lo, mut hi) = (0.0_f64, 1.0_f64);
        if !clip_rect(view, start, end, &mut lo, &mut hi) {
            return None;
        }
        let mut current = owner;
        while let Some(node) = self.nodes.get(&current) {
            if current == owner || node.clip_content {
                let inverse = self.world_transform(current)?.inverse()?;
                let a = inverse.apply(start);
                let b = inverse.apply(end);
                if !clip_rect(node.local_bounds(), a, b, &mut lo, &mut hi) {
                    return None;
                }
                if node
                    .corner_radii
                    .unwrap_or([node.corner_radius; 4])
                    .iter()
                    .any(|r| *r > 0.0)
                {
                    let contours = crate::corner_path(node).fill_contours();
                    let contour = contours.first()?;
                    let sign = contour
                        .iter()
                        .zip(contour.iter().cycle().skip(1))
                        .map(|(u, v)| u.x * v.y - u.y * v.x)
                        .sum::<f64>()
                        .signum();
                    for (u, v) in contour.iter().zip(contour.iter().cycle().skip(1)) {
                        let side = |q: Point| {
                            sign * ((v.x - u.x) * (q.y - u.y) - (v.y - u.y) * (q.x - u.x))
                        };
                        if !clip_halfspace(side(a), side(b), &mut lo, &mut hi) {
                            return None;
                        }
                    }
                }
            }
            current = node.parent?;
        }
        Some((start.lerp(end, lo), start.lerp(end, hi)))
    }
}

fn clip_rect(r: Rect, a: Point, b: Point, lo: &mut f64, hi: &mut f64) -> bool {
    [
        (a.x - r.x, b.x - r.x),
        (r.right() - a.x, r.right() - b.x),
        (a.y - r.y, b.y - r.y),
        (r.bottom() - a.y, r.bottom() - b.y),
    ]
    .into_iter()
    .all(|(start, end)| clip_halfspace(start, end, lo, hi))
}

fn clip_halfspace(start: f64, end: f64, lo: &mut f64, hi: &mut f64) -> bool {
    if start < -crate::GEOMETRY_EPSILON {
        if end < -crate::GEOMETRY_EPSILON {
            return false;
        }
        *lo = lo.max(start / (start - end));
    } else if end < -crate::GEOMETRY_EPSILON {
        *hi = hi.min(start / (start - end));
    }
    *lo <= *hi
}

impl Editor {
    /// Apply one add, move, duplicate or removal to the authored guide list.
    pub fn edit_ruler_guide(
        &mut self,
        source: Option<RulerGuideId>,
        destination: Option<(NodeId, Guide)>,
        duplicate: bool,
    ) -> Result<Option<RulerGuideId>, DocumentError> {
        let invalid =
            || DocumentError::Invalid("Choose an editable guide on the active page.".into());
        let editable = |owner| {
            self.document
                .ruler_owner_transform(self.active_page, owner)
                .is_some()
                && !self.document.effective_locked(owner)
        };
        if self.has_active_transaction() {
            return Err(invalid());
        }
        if let Some(id) = source {
            if !editable(id.owner) || self.document.ruler_guide(id).is_none() {
                return Err(invalid());
            }
        }
        if let Some((owner, guide)) = destination {
            if !editable(owner) || !guide.position.is_finite() {
                return Err(invalid());
            }
            if let Some(node) = self.document.nodes.get(&owner) {
                let extent = if guide.horizontal {
                    node.height
                } else {
                    node.width
                };
                if !(0.0..=extent).contains(&guide.position) {
                    return Err(invalid());
                }
            }
            if let Some(id) = source.filter(|id| id.owner == owner && !duplicate) {
                if self.document.ruler_guide(id) != Some(&guide) {
                    self.transact("Move ruler guide", |d| {
                        d.guides.get_mut(&owner).unwrap()[id.index] = guide
                    });
                }
                return Ok(Some(id));
            }
        } else if source.is_none() || duplicate {
            return Ok(source);
        }
        let name = match (source, destination, duplicate) {
            (_, None, _) => "Remove ruler guide",
            (None, _, _) => "Add ruler guide",
            (_, _, true) => "Duplicate ruler guide",
            _ => "Move ruler guide",
        };
        Ok(self.transact(name, |d| {
            if let Some(id) = source.filter(|_| !duplicate) {
                let guides = d.guides.get_mut(&id.owner).unwrap();
                guides.remove(id.index);
                if guides.is_empty() {
                    d.guides.remove(&id.owner);
                }
            }
            destination.map(|(owner, guide)| {
                let guides = d.guides.entry(owner).or_default();
                let index = guides.len();
                guides.push(guide);
                RulerGuideId { owner, index }
            })
        }))
    }
}

/// Restrict the query to the dragged handles' vicinity. A constrained drag is
/// intersected with the guide along its allowed direction, preserving its axis.
pub fn snap_ruler_guides(
    editor: &Editor,
    points: &[Point],
    threshold: f64,
    excluded: &[NodeId],
    direction: Option<Point>,
) -> Option<crate::LayoutGuideSnap> {
    let first = *points.first()?;
    if threshold <= 0.0 || !threshold.is_finite() || editor.document.guides.is_empty() {
        return None;
    }
    let bounds = points
        .iter()
        .fold(Rect::new(first.x, first.y, 0.0, 0.0), |r, p| {
            r.union(Rect::new(p.x, p.y, 0.0, 0.0))
        })
        .inflate(threshold * 2.0);
    let dot = |a: Point, b: Point| a.x * b.x + a.y * b.y;
    let doc = &editor.document;
    let mut best: Option<Point> = None;
    let (mut x, mut y): (Option<f64>, Option<f64>) = (None, None);
    for (&owner, guides) in &doc.guides {
        if excluded
            .iter()
            .any(|&id| owner == id || doc.is_ancestor(id, owner))
        {
            continue;
        }
        for &guide in guides {
            let Some((a, b)) = doc.ruler_guide_segment(editor.active_page, owner, guide, bounds)
            else {
                continue;
            };
            let tangent = b - a;
            let len = a.distance(b);
            if len <= crate::GEOMETRY_EPSILON {
                continue;
            }
            let normal = Point::new(-tangent.y, tangent.x) * (1.0 / len);
            let ray = direction.unwrap_or(normal);
            let denom = dot(ray, normal);
            if denom.abs() < crate::GEOMETRY_EPSILON {
                continue;
            }
            for &p in points {
                let delta = ray * (dot(a - p, normal) / denom);
                let distance = delta.x.hypot(delta.y);
                if distance > threshold
                    || crate::distance_to_segment(p + delta, a, b) > crate::GEOMETRY_EPSILON
                {
                    continue;
                }
                if direction.is_none() && normal.y.abs() < crate::GEOMETRY_EPSILON {
                    if x.is_none_or(|old| delta.x.abs() < old.abs()) {
                        x = Some(delta.x);
                    }
                } else if direction.is_none() && normal.x.abs() < crate::GEOMETRY_EPSILON {
                    if y.is_none_or(|old| delta.y.abs() < old.abs()) {
                        y = Some(delta.y);
                    }
                } else if best.is_none_or(|old| distance < old.x.hypot(old.y)) {
                    best = Some(delta);
                }
            }
        }
    }
    if x.is_some() || y.is_some() {
        let axis = Point::new(x.unwrap_or(0.0), y.unwrap_or(0.0));
        if best.is_none_or(|d| axis.x.hypot(axis.y) <= d.x.hypot(d.y)) {
            best = Some(axis);
        }
    }
    best.map(|delta| crate::LayoutGuideSnap {
        delta,
        lines: [None, None],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Node, GEOMETRY_EPSILON};

    #[test]
    fn ruler_guides_transfer_without_losing_frame_space_or_history() {
        let mut doc = Document::new("Rulers");
        let page = doc.pages[0].id;
        let frame = doc
            .insert_node(
                page,
                Node::new(NodeKind::Frame, "Card", 50., 30., 100., 80.),
            )
            .unwrap();
        doc.nodes.get_mut(&frame).unwrap().rotation = 30.;
        let mut e = Editor::new(doc);
        let guide = Guide {
            horizontal: true,
            position: 25.,
        };
        let page_guide = e
            .edit_ruler_guide(None, Some((page, guide)), false)
            .unwrap()
            .unwrap();
        let id = e
            .edit_ruler_guide(Some(page_guide), Some((frame, guide)), false)
            .unwrap()
            .unwrap();
        assert!(!e.document.guides.contains_key(&page));
        let world = e.document.world_transform(frame).unwrap();
        let segment = e
            .document
            .ruler_guide_segment(page, frame, guide, Rect::new(-1000., -1000., 2000., 2000.))
            .unwrap();
        assert!(segment.0.distance(world.apply(Point::new(0., 25.))) < GEOMETRY_EPSILON);
        let pos = world.apply(Point::new(40., 27.));
        let snap = snap_ruler_guides(&e, &[pos], 5., &[], None).unwrap();
        assert!((pos + snap.delta).distance(world.apply(Point::new(40., 25.))) < GEOMETRY_EPSILON);
        assert!(snap_ruler_guides(&e, &[pos], 5., &[frame], None).is_none());
        assert_eq!(
            e.edit_ruler_guide(Some(id), Some((frame, guide)), false)
                .unwrap(),
            Some(id)
        );
        assert_eq!(e.history_len(), 2);
        e.edit_ruler_guide(Some(id), None, false).unwrap();
        assert!(e.undo());
        assert_eq!(e.document.ruler_guide(id), Some(&guide));
        assert!(e.undo());
        assert_eq!(e.document.ruler_guide(page_guide), Some(&guide));
    }

    #[test]
    fn ruler_guides_follow_copied_subtrees_and_are_removed_and_restored_with_the_owner() {
        let mut doc = Document::new("Copies");
        let page = doc.pages[0].id;
        let frame = doc
            .insert_node(
                page,
                Node::new(NodeKind::Frame, "Frame", 10., 20., 50., 50.),
            )
            .unwrap();
        let guides = vec![Guide {
            horizontal: false,
            position: 17.,
        }];
        doc.guides.insert(frame, guides.clone());
        let mut e = Editor::new(doc);
        e.selection = vec![frame];
        assert!(e.copy_selection());
        let copy = e.paste()[0];
        assert_eq!(e.document.guides[&copy], guides);
        let duplicate = e.duplicate_selection()[0];
        assert_eq!(e.document.guides[&duplicate], guides);
        e.delete_selection();
        assert!(!e.document.guides.contains_key(&duplicate));
        assert!(e.undo());
        assert_eq!(e.document.guides[&duplicate], guides);
        let json = e.document.to_json().unwrap();
        assert_eq!(
            Document::from_json(&json).unwrap().guides,
            e.document.guides
        );
    }
}
