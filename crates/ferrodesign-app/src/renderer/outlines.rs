//! A paint-independent view of editable paths. Its spatial index includes
//! Boolean operands, clipped children and hidden layers. Camera changes only
//! query the index; scoped document edits invalidate the affected entries.
use super::*;
use ferrodesign_core::{SpatialIndex, MAX_DOCUMENT_DEPTH};

#[derive(Default)]
pub(crate) struct OutlineScene {
    token: Option<EditorRenderToken>,
    page: Option<NodeId>,
    spatial: SpatialIndex,
    entries: HashMap<NodeId, Entry>,
}

struct Entry {
    order: usize,
    world: Transform,
    hidden: bool,
    operand: bool,
    paths: Option<Paths>,
}

struct Paths {
    edges: Vec<Ref<Geometry>>,
    bounds: Ref<Geometry>,
}

fn placed(path: Ref<Geometry>, transform: Transform) -> Ref<Geometry> {
    if transform == Transform::IDENTITY {
        return path;
    }
    // Widened paths can be immutable. Transform the collection, leaving the
    // original geometry and its fill rule available to other operations.
    let group = GeometryGroup::new();
    group.children().add(path);
    group.set_transform(MatrixTransform::with_matrix(ui_matrix(transform)));
    group.upcast()
}

fn silhouette(document: &Document, node: &Node, depth: usize) -> Ref<Geometry> {
    match node.kind {
        NodeKind::Text => {
            let text = make_text_layout(node, solid_brush(Color::BLACK, 1.));
            text_outline(&text.layout, text.origin)
        }
        NodeKind::Boolean | NodeKind::Group => {
            let mode = if node.kind == NodeKind::Boolean {
                boolean_mode(node.boolean_operation)
            } else {
                GeometryCombineMode::Union
            };
            let result = (depth < MAX_DOCUMENT_DEPTH)
                .then(|| {
                    node.children
                        .iter()
                        .filter_map(|id| document.nodes.get(id))
                        .filter(|child| child.visible)
                        .map(|child| {
                            placed(
                                silhouette(document, child, depth + 1),
                                child.local_transform(),
                            )
                        })
                        .reduce(|a, b| combined(mode, a, b))
                })
                .flatten();
            result.unwrap_or_else(|| StreamGeometry::new().upcast())
        }
        _ => shape_geometry(node),
    }
}

impl Paths {
    fn build(document: &Document, node: &Node, world: Transform) -> Self {
        let bounds = placed(
            RectangleGeometry::with_rect(ui_rect(node.local_bounds())).upcast(),
            world,
        );
        let visible_strokes = || {
            node.strokes
                .iter()
                .filter(|s| s.visible && s.width > 0. && s.opacity > 0.)
        };
        let mut edges = Vec::new();
        if node.kind != NodeKind::Group || visible_strokes().next().is_some() {
            let body = silhouette(document, node, 0);
            let path = node
                .vector_network
                .as_ref()
                .map(|n| path_geometry(&n.stroke_path()))
                .unwrap_or_else(|| body.clone());
            edges.push(placed(path.clone(), world));
            let ink = Rc::new(ImmutableSolidColorBrush::from_uint32(0xFF000000));
            for stroke in visible_strokes() {
                if let Some(s) = CachedStroke::prepare(node, stroke, &body, &path, ink.clone()) {
                    edges.push(placed(s.band(&path), world));
                    if let Some(arrow) = s.arrow {
                        edges.push(placed(arrow, world));
                    }
                }
            }
        }
        Self { bounds, edges }
    }
}

impl OutlineScene {
    fn index(&mut self, editor: &Editor, id: NodeId, order: usize) {
        self.spatial.remove(id);
        self.entries.remove(&id);
        let Some(node) = editor.document.nodes.get(&id) else {
            return;
        };
        let Some(&world) = editor.scene.transforms.get(&id) else {
            return;
        };

        let mut hidden = false;
        let mut operand = false;
        let mut current = Some(node);
        while let Some(n) = current {
            hidden |= !n.visible;
            operand |= n.id != id && n.kind == NodeKind::Boolean;
            current = n.parent.and_then(|p| editor.document.nodes.get(&p));
        }
        // Text can overflow its authored box, as can path control points,
        // widened strokes and Boolean operands. Index their exact extents.
        let exact = matches!(
            node.kind,
            NodeKind::Text | NodeKind::Vector | NodeKind::Boolean
        ) || node.strokes.iter().any(|s| s.visible && s.width > 0.);
        let paths = exact.then(|| Paths::build(&editor.document, node, world));
        let mut extent = world.transform_rect(node.local_bounds());
        if let Some(paths) = &paths {
            for p in &paths.edges {
                extent = extent.union(core_rect(p.bounds()));
            }
        }
        self.spatial.insert(id, extent);
        self.entries.insert(
            id,
            Entry {
                order,
                world,
                hidden,
                operand,
                paths,
            },
        );
    }

    fn prepare(&mut self, editor: &Editor) {
        let token = editor.render_change_token();
        if self.page == Some(editor.active_page) && self.token.as_ref() == Some(&token) {
            return;
        }
        let changes = self
            .token
            .as_ref()
            .filter(|_| self.page == Some(editor.active_page))
            .map(|t| editor.render_changes_since(t))
            .unwrap_or(EditorRenderChanges::Full);
        match changes {
            EditorRenderChanges::Full => {
                self.spatial.clear();
                self.entries.clear();
                for (order, id) in editor
                    .document
                    .draw_order(editor.active_page)
                    .into_iter()
                    .enumerate()
                {
                    self.index(editor, id, order);
                }
            }
            EditorRenderChanges::Nodes { nodes, ancestors } => {
                for id in nodes.into_iter().chain(ancestors) {
                    if let Some(entry) = self.entries.get(&id) {
                        self.index(editor, id, entry.order);
                    }
                }
            }
        }
        self.page = Some(editor.active_page);
        self.token = Some(token);
    }

    fn candidates(&mut self, editor: &Editor, viewport: Rect, hidden: bool) -> Vec<NodeId> {
        self.prepare(editor);
        let mut ids = self.spatial.query(viewport);
        ids.retain(|id| hidden || !self.entries[id].hidden);
        ids.sort_unstable_by_key(|id| self.entries[id].order);
        ids
    }

    pub(crate) fn render(
        &mut self,
        ctx: &mut DrawingContext,
        editor: &Editor,
        viewport: Rect,
        zoom: f64,
        hidden: bool,
        bounds: bool,
        dark: bool,
        roots: &[NodeId],
    ) {
        let ink = |color| Rc::new(ImmutablePen::from_uint32(color, 1. / zoom)) as Rc<dyn IPen>;
        let normal = ink(if dark { 0xFFE2E2E2 } else { 0xFF555555 });
        let secondary = ink(if dark { 0xFFAAAAAA } else { 0xFF9D9D9D });
        let dashed: Rc<dyn IPen> = Rc::new(ImmutablePen::new(
            Some(Rc::new(ImmutableSolidColorBrush::from_uint32(if dark {
                0xFFAAAAAA
            } else {
                0xFF888888
            }))),
            1. / zoom,
            Some(Rc::new(ImmutableDashStyle::new(Some(&[4., 3.]), 0.))),
            PenLineCap::Flat,
            PenLineJoin::Miter,
            10.,
        ));
        let selected: HashSet<_> = roots.iter().copied().collect();
        for id in self.candidates(editor, viewport.inflate(1. / zoom), hidden) {
            if editor.document.nodes[&id].kind == NodeKind::Slice {
                continue;
            }
            if !selected.is_empty() {
                let mut current = Some(id);
                while let Some(parent) = current {
                    if selected.contains(&parent) {
                        break;
                    }
                    current = editor.document.nodes.get(&parent).and_then(|n| n.parent);
                }
                if current.is_none() {
                    continue;
                }
            }
            let entry = self.entries.get_mut(&id).unwrap();
            let paths = entry.paths.get_or_insert_with(|| {
                Paths::build(&editor.document, &editor.document.nodes[&id], entry.world)
            });
            if bounds {
                ctx.draw_geometry(None, Some(&dashed), &paths.bounds);
            }
            for p in &paths.edges {
                ctx.draw_geometry(
                    None,
                    Some(if entry.operand || entry.hidden {
                        &secondary
                    } else {
                        &normal
                    }),
                    p,
                );
            }
        }
    }

    pub(crate) fn enclosed(&mut self, editor: &Editor, rect: Rect, hidden: bool) -> Vec<NodeId> {
        self.candidates(editor, rect, hidden)
            .into_iter()
            .filter(|id| {
                !editor.document.effective_locked(*id)
                    && self
                        .spatial
                        .bounds(*id)
                        .is_some_and(|b| rect.contains_rect(b))
            })
            .collect()
    }

    pub(crate) fn hits(
        &mut self,
        editor: &Editor,
        point: Point,
        tolerance: f64,
        hidden: bool,
        bounds: bool,
        include_locked: bool,
    ) -> Vec<NodeId> {
        let envelope = Rect::new(point.x, point.y, 0., 0.).inflate(tolerance);
        let pen = ImmutablePen::from_uint32(0xFF000000, 2. * tolerance);
        self.candidates(editor, envelope, hidden)
            .into_iter()
            .rev()
            .filter(|id| {
                if !include_locked && editor.document.effective_locked(*id) {
                    return false;
                }
                let entry = self.entries.get_mut(id).unwrap();
                let paths = entry.paths.get_or_insert_with(|| {
                    Paths::build(&editor.document, &editor.document.nodes[id], entry.world)
                });
                let hit = |p: &Ref<Geometry>| p.stroke_contains(&pen, ui_point(point));
                (bounds && hit(&paths.bounds)) || paths.edges.iter().any(hit)
            })
            .collect()
    }
}
