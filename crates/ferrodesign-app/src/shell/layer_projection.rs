//! A cached row projection over the editor's complete, bounded change feed.
//! Native controls consume these rows only when their displayed metadata changes.
use ferrodesign_core::{Editor, EditorRenderChanges, EditorRenderToken, Node, NodeId, NodeKind};
use ferroui_base::input::KeyModifiers;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq)]
pub(super) struct LayerRow {
    pub(super) id: NodeId,
    pub(super) name: String,
    pub(super) kind: NodeKind,
    pub(super) depth: usize,
    pub(super) expanded: Option<bool>,
    pub(super) has_children: bool,
    children_forward: bool,
    pub(super) visible: bool,
    pub(super) locked: bool,
    pub(super) assets: bool,
}

impl LayerRow {
    fn from_node(node: &Node, depth: usize, expanded: Option<bool>, assets: bool) -> Self {
        Self {
            id: node.id,
            name: node.name.clone(),
            kind: node.kind,
            depth,
            expanded,
            has_children: !node.children.is_empty(),
            children_forward: node.layer_rows_follow_child_order(),
            visible: node.visible,
            locked: node.locked,
            assets,
        }
    }

    fn matches_node(&self, node: &Node) -> bool {
        self.id == node.id
            && self.name == node.name
            && self.kind == node.kind
            && self.has_children == !node.children.is_empty()
            && self.children_forward == node.layer_rows_follow_child_order()
            && self.visible == node.visible
            && self.locked == node.locked
    }
}

struct ProjectionContext {
    token: EditorRenderToken,
    page: NodeId,
    assets: bool,
    filter: String,
    expanded: BTreeSet<NodeId>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct LayerProjectionStats {
    pub(super) builds: u64,
    pub(super) reuses: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum TreeAction {
    Expand,
    Collapse,
    CollapseOthers,
    SelectionOnly,
}

#[derive(Default)]
pub(super) struct LayerProjection {
    rows: Vec<LayerRow>,
    indices: BTreeMap<NodeId, usize>,
    context: Option<ProjectionContext>,
    open_branches: usize,
    closed_branches: usize,
    stats: LayerProjectionStats,
}

impl LayerProjection {
    pub(super) fn rows(&self) -> &[LayerRow] {
        &self.rows
    }

    /// A range follows displayed rows, including expanded children and the current
    /// filter. Its first selected row stays the anchor when the range reverses.
    pub(super) fn select_row(&self, editor: &mut Editor, id: NodeId, modifiers: KeyModifiers) {
        let Some(&end) = self.indices.get(&id) else { return; };
        if modifiers.contains(KeyModifiers::SHIFT) {
            if let Some(&start) = editor.selection.first().and_then(|anchor| self.indices.get(anchor)) {
                editor.selection.clear();
                if start <= end {
                    editor.selection.extend(self.rows[start..=end].iter().map(|row| row.id));
                } else {
                    editor.selection.extend(self.rows[end..=start].iter().rev().map(|row| row.id));
                }
                return;
            }
        }
        editor.select(id, modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::META));
    }

    pub(super) fn index_of(&self, id: NodeId) -> Option<usize> {
        self.indices.get(&id).copied()
    }

    /// Retain only disclosures already visible on the selected paths. An
    /// individually folded branch stays folded even if its artwork is selected.
    fn selected_disclosures(&self, editor: &Editor) -> BTreeSet<NodeId> {
        let mut keep = BTreeSet::new();
        for id in &editor.selection {
            let Some(index) = self.index_of(*id) else { continue };
            let mut current = if self.rows[index].expanded == Some(true) {
                Some(*id)
            } else {
                editor.document.nodes.get(id).and_then(|node| node.parent)
            };
            while let Some(branch) = current {
                if branch == editor.active_page || !keep.insert(branch) { break; }
                current = editor.document.nodes.get(&branch).and_then(|node| node.parent);
            }
        }
        keep
    }

    pub(super) fn tree_action(&self, editor: &Editor) -> Option<TreeAction> {
        if self.open_branches == 0 {
            return (self.closed_branches > 0).then_some(TreeAction::Expand);
        }
        let keep = self.selected_disclosures(editor).len();
        Some(if keep == 0 { TreeAction::Collapse }
            else if keep < self.open_branches { TreeAction::CollapseOthers }
            else if self.closed_branches != 0 { TreeAction::Expand }
            else { TreeAction::SelectionOnly })
    }

    pub(super) fn toggle_tree(&self, editor: &Editor, expanded: &mut BTreeSet<NodeId>) -> bool {
        match self.tree_action(editor) {
            Some(TreeAction::Expand) => {
                let Some(page) = editor.document.page(editor.active_page) else { return false; };
                let mut pending = page.children.clone();
                while let Some(id) = pending.pop() {
                    let Some(node) = editor.document.nodes.get(&id) else { continue; };
                    if !node.children.is_empty() {
                        expanded.insert(id);
                        pending.extend(node.children.iter().copied());
                    }
                }
            }
            Some(TreeAction::Collapse | TreeAction::CollapseOthers) => {
                let keep = self.selected_disclosures(editor);
                expanded.retain(|id| keep.contains(id) || editor.document.page_of(*id) != Some(editor.active_page));
            }
            _ => return false,
        }
        true
    }

    pub(super) fn stats(&self) -> LayerProjectionStats {
        self.stats
    }

    /// Refresh pure row data; return whether the native ItemsSource must change.
    /// The caller releases its editor/cache borrows before any native setters.
    pub(super) fn refresh(
        &mut self,
        editor: &Editor,
        assets: bool,
        filter: &str,
        expanded: &BTreeSet<NodeId>,
    ) -> bool {
        if self.can_reuse(editor, assets, filter, expanded) {
            // Consume empty or equivalent changes too, so repeated no-op UI
            // refreshes cannot unnecessarily age this token out of the feed.
            self.context.as_mut().unwrap().token = editor.render_change_token();
            self.stats.reuses = self.stats.reuses.saturating_add(1);
            return false;
        }

        let rows = project_rows(editor, assets, filter, expanded);
        self.stats.builds = self.stats.builds.saturating_add(1);
        let changed = self.rows != rows;
        if changed {
            self.open_branches = rows.iter().filter(|row| row.expanded == Some(true)).count();
            self.closed_branches = rows.iter().filter(|row| row.expanded == Some(false)).count();
            self.indices = rows.iter().enumerate().map(|(index, row)| (row.id, index)).collect();
            self.rows = rows;
        }
        self.context = Some(ProjectionContext {
            token: editor.render_change_token(),
            page: editor.active_page,
            assets,
            filter: filter.to_owned(),
            // Filtered paths and the flat catalog are independent of the
            // stored disclosure set, including expanded branches on other pages.
            expanded: if assets || !filter.is_empty() { BTreeSet::new() } else { expanded.clone() },
        });
        changed
    }

    fn can_reuse(
        &self,
        editor: &Editor,
        assets: bool,
        filter: &str,
        expanded: &BTreeSet<NodeId>,
    ) -> bool {
        let Some(context) = &self.context else { return false; };
        if context.page != editor.active_page
            || context.assets != assets
            || context.filter != filter
            || (!assets && filter.is_empty() && &context.expanded != expanded)
        {
            return false;
        }
        let EditorRenderChanges::Nodes { nodes, ancestors } =
            editor.render_changes_since(&context.token)
        else {
            return false;
        };
        // Assets lists components without child rows, yet a child's name can
        // make a component enter or leave the results. We cannot infer its
        // former match from the displayed rows after a scoped edit.
        if assets && !filter.is_empty() && !nodes.is_empty() {
            return false;
        }
        for id in nodes.into_iter().chain(ancestors) {
            let Some(&index) = self.indices.get(&id) else {
                // A scoped rename can introduce a search result. Topology and
                // membership changes already publish Full. Other unseen edits
                // need no traversal of the page or reconstruction of its rows.
                if !filter.is_empty() {
                    let Some(node) = editor.document.nodes.get(&id) else { return false; };
                    if node.name.to_lowercase().contains(filter)
                        && editor.document.page_of(id) == Some(editor.active_page)
                    {
                        return false;
                    }
                }
                continue;
            };
            let Some(node) = editor.document.nodes.get(&id) else { return false; };
            if !self.rows[index].matches_node(node) {
                return false;
            }
        }
        true
    }
}

fn project_rows(
    editor: &Editor,
    assets: bool,
    filter: &str,
    expanded: &BTreeSet<NodeId>,
) -> Vec<LayerRow> {
    if assets {
        // Components are reusable across every page. A descendant name can
        // match its component even though Assets displays a flat catalog.
        let mut included = BTreeSet::new();
        if !filter.is_empty() {
            for node in editor.document.nodes.values()
                .filter(|node| node.name.to_lowercase().contains(filter))
            {
                let mut id = Some(node.id);
                while let Some(current) = id {
                    if !included.insert(current) { break; }
                    id = editor.document.nodes.get(&current).and_then(|node| node.parent);
                }
            }
        }
        return editor.document.nodes.values().rev()
            .filter(|node| node.kind == NodeKind::Component &&
                (filter.is_empty() || included.contains(&node.id)))
            .map(|node| LayerRow::from_node(node, 0, None, true))
            .collect();
    }

    let Some(page) = editor.document.page(editor.active_page) else { return Vec::new(); };
    let mut pending: Vec<_> = page.children.iter().map(|&id| (id, 0)).collect();
    let mut rows = Vec::new();
    let mut path = Vec::new();
    let mut shown_depth = 0;
    while let Some((id, depth)) = pending.pop() {
        let Some(node) = editor.document.nodes.get(&id) else { continue; };
        if filter.is_empty() {
            let open = expanded.contains(&id);
            rows.push(LayerRow::from_node(node, depth, (!node.children.is_empty()).then_some(open), false));
            if !open { continue; }
        } else {
            // Carry the path down the active page only. On a match, append
            // ancestors after the last emitted prefix, in display order.
            path.truncate(depth);
            path.push(node);
            shown_depth = shown_depth.min(depth);
            if node.name.to_lowercase().contains(filter) {
                for (index, ancestor) in path.iter().enumerate().skip(shown_depth) {
                    rows.push(LayerRow::from_node(ancestor, index, None, false));
                }
                shown_depth = path.len();
            }
        }
        if node.layer_rows_follow_child_order() {
            pending.extend(node.children.iter().rev().map(|&child| (child, depth + 1)));
        } else {
            pending.extend(node.children.iter().map(|&child| (child, depth + 1)));
        }
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferrodesign_core::Document;

    fn flat_editor(count: usize) -> (Editor, Vec<NodeId>) {
        let mut document = Document::new("Layer cache");
        let page = document.pages[0].id;
        let ids = (0..count).map(|index| {
            document.insert_node(page, Node::new(NodeKind::Rectangle,
                format!("Layer {index}"), index as f64 * 20.0, 0.0, 12.0, 12.0)).unwrap()
        }).collect();
        (Editor::new(document), ids)
    }

    fn tree_editor() -> (Editor, NodeId, NodeId, NodeId) {
        let mut document = Document::new("Layer tree");
        let page = document.pages[0].id;
        let parent = document.insert_node(page,
            Node::new(NodeKind::Frame, "Frame", 0.0, 0.0, 100.0, 100.0)).unwrap();
        let child = document.insert_node(parent,
            Node::new(NodeKind::Rectangle, "Child", 1.0, 2.0, 10.0, 10.0)).unwrap();
        let sibling = document.insert_node(page,
            Node::new(NodeKind::Rectangle, "Sibling", 120.0, 0.0, 10.0, 10.0)).unwrap();
        (Editor::new(document), parent, child, sibling)
    }

    fn ids(projection: &LayerProjection) -> Vec<NodeId> {
        projection.rows().iter().map(|row| row.id).collect()
    }

    #[test]
    fn range_and_toggle_follow_projected_order_and_keep_the_anchor() {
        let (mut editor, parent, child, sibling) = tree_editor();
        let mut projection = LayerProjection::default();
        projection.refresh(&editor, false, "", &BTreeSet::from([parent]));
        assert_eq!(ids(&projection), vec![sibling, parent, child]);
        projection.select_row(&mut editor, child, KeyModifiers::empty());
        projection.select_row(&mut editor, sibling, KeyModifiers::SHIFT);
        assert_eq!(editor.selection, vec![child, parent, sibling]);
        projection.select_row(&mut editor, parent, KeyModifiers::SHIFT);
        assert_eq!(editor.selection, vec![child, parent]);
        projection.select_row(&mut editor, parent, KeyModifiers::META);
        projection.select_row(&mut editor, sibling, KeyModifiers::CONTROL);
        assert_eq!(editor.selection, vec![child, sibling]);

        projection.refresh(&editor, false, "", &BTreeSet::new());
        projection.select_row(&mut editor, parent, KeyModifiers::SHIFT);
        assert_eq!(editor.selection, vec![parent], "collapsed anchor starts a new selection");
    }

    #[test]
    fn layer_projection_scoped_move_undo_redo_reuses_10000_rows_and_names() {
        let (mut editor, nodes) = flat_editor(10_000);
        let expanded = BTreeSet::new();
        let mut projection = LayerProjection::default();
        assert!(projection.refresh(&editor, false, "", &expanded));
        assert_eq!(projection.rows().len(), 10_000);
        let row_buffer = projection.rows().as_ptr();
        let first_name = projection.rows()[0].name.as_ptr();
        editor.selection = vec![nodes[5_000]];
        editor.move_selection(1.0, 0.0);
        assert!(!projection.refresh(&editor, false, "", &expanded));
        assert!(editor.undo());
        assert!(!projection.refresh(&editor, false, "", &expanded));
        assert!(editor.redo());
        assert!(!projection.refresh(&editor, false, "", &expanded));
        assert_eq!(projection.stats(), LayerProjectionStats { builds: 1, reuses: 3 });
        assert_eq!(projection.rows().as_ptr(), row_buffer);
        assert_eq!(projection.rows()[0].name.as_ptr(), first_name);
        assert_eq!(ids(&projection), nodes.into_iter().rev().collect::<Vec<_>>());
    }

    #[test]
    fn layer_projection_preview_cancel_and_repeated_noops_advance_the_token() {
        let (mut editor, nodes) = flat_editor(2);
        let expanded = BTreeSet::new();
        let mut projection = LayerProjection::default();
        projection.refresh(&editor, false, "", &expanded);
        editor.begin_node_transaction(&[nodes[0]], "Move preview");
        editor.document.nodes.get_mut(&nodes[0]).unwrap().x += 5.0;
        editor.refresh_preview_nodes(&[nodes[0]]);
        assert!(!projection.refresh(&editor, false, "", &expanded));
        editor.cancel_transaction();
        assert!(!projection.refresh(&editor, false, "", &expanded));
        assert_eq!(editor.history_len(), 0);
        // More than the feed's retained revision count: every successful reuse
        // must advance its token, including unchanged scoped refreshes.
        for _ in 0..160 {
            editor.refresh_preview_nodes(&[]);
            assert!(!projection.refresh(&editor, false, "", &expanded));
        }
        assert_eq!(projection.stats(), LayerProjectionStats { builds: 1, reuses: 162 });
    }

    #[test]
    fn layer_projection_rebuilds_changed_names_lock_and_visibility() {
        let (mut editor, nodes) = flat_editor(1);
        let expanded = BTreeSet::new();
        let mut projection = LayerProjection::default();
        projection.refresh(&editor, false, "", &expanded);
        editor.update_node(nodes[0], "Rename", |node| node.name = "Renamed".into());
        assert!(projection.refresh(&editor, false, "", &expanded));
        assert_eq!(projection.rows()[0].name, "Renamed");
        editor.toggle_locked(nodes[0]);
        assert!(projection.refresh(&editor, false, "", &expanded));
        assert!(projection.rows()[0].locked);
        editor.toggle_visibility(nodes[0]);
        assert!(projection.refresh(&editor, false, "", &expanded));
        assert!(!projection.rows()[0].visible);
        assert!(editor.undo());
        assert!(projection.refresh(&editor, false, "", &expanded));
        assert!(projection.rows()[0].visible);
        assert_eq!(projection.stats(), LayerProjectionStats { builds: 5, reuses: 0 });
    }

    #[test]
    fn layer_projection_expansion_snapshot_exposes_previously_excluded_edits() {
        let (mut editor, parent, child, sibling) = tree_editor();
        let mut expanded = BTreeSet::new();
        let mut projection = LayerProjection::default();
        projection.refresh(&editor, false, "", &expanded);
        assert_eq!(ids(&projection), vec![sibling, parent]);
        editor.update_node(child, "Rename hidden row", |node| node.name = "Updated child".into());
        assert!(!projection.refresh(&editor, false, "", &expanded));
        expanded.insert(parent);
        assert!(projection.refresh(&editor, false, "", &expanded));
        assert_eq!(ids(&projection), vec![sibling, parent, child]);
        assert_eq!(projection.rows()[2].name, "Updated child");
        assert_eq!(projection.rows()[2].depth, 1);
        assert_eq!(projection.rows()[1].expanded, Some(true));
        expanded.clear();
        assert!(projection.refresh(&editor, false, "", &expanded));
        assert_eq!(ids(&projection), vec![sibling, parent]);
        assert_eq!(projection.stats(), LayerProjectionStats { builds: 3, reuses: 1 });
    }

    #[test]
    fn layer_projection_topology_order_and_membership_use_full_projection() {
        let (mut editor, parent, child, sibling) = tree_editor();
        let expanded = BTreeSet::from([parent]);
        let mut projection = LayerProjection::default();
        projection.refresh(&editor, false, "", &expanded);
        let page = editor.active_page;
        editor.transact("Reparent", |document| document.reparent(child, page, 0).unwrap());
        assert!(projection.refresh(&editor, false, "", &expanded));
        assert_eq!(ids(&projection), vec![sibling, parent, child]);
        assert_eq!(projection.rows()[2].depth, 0);
        assert!(!projection.rows()[1].has_children);
        editor.transact("Reorder roots", |document| document.page_mut(page).unwrap().children.reverse());
        assert!(projection.refresh(&editor, false, "", &expanded));
        assert_eq!(ids(&projection), vec![child, parent, sibling]);
        editor.selection = vec![parent];
        editor.delete_selection();
        assert!(projection.refresh(&editor, false, "", &expanded));
        assert_eq!(ids(&projection), vec![child, sibling]);
        assert_eq!(projection.stats(), LayerProjectionStats { builds: 4, reuses: 0 });
    }

    #[test]
    fn layer_projection_search_rebuilds_when_an_excluded_node_becomes_a_match() {
        let (mut editor, parent, child, sibling) = tree_editor();
        let expanded = BTreeSet::new();
        let mut projection = LayerProjection::default();
        projection.refresh(&editor, false, "missing", &expanded);
        assert!(projection.rows().is_empty());
        assert!(!projection.refresh(&editor, false, "missing", &expanded));
        editor.update_node(sibling, "Move", |node| node.x += 2.0);
        assert!(!projection.refresh(&editor, false, "missing", &expanded));
        editor.update_node(child, "Rename", |node| node.name = "Now missing matches".into());
        assert!(projection.refresh(&editor, false, "missing", &expanded));
        assert_eq!(ids(&projection), vec![parent, child]);
        editor.update_node(child, "Rename", |node| node.name = "Child".into());
        assert!(projection.refresh(&editor, false, "missing", &expanded));
        assert!(projection.rows().is_empty());
        assert!(projection.refresh(&editor, false, "", &expanded));
        assert_eq!(ids(&projection), vec![sibling, parent]);
        assert!(!projection.refresh(&editor, false, "", &expanded));
        assert_eq!(projection.stats(), LayerProjectionStats { builds: 4, reuses: 3 });
    }

    #[test]
    fn page_search_emits_only_matching_paths_once_in_layer_order() {
        let (mut editor, parent, child, sibling) = tree_editor();
        let nested = editor.document.insert_node(parent,
            Node::new(NodeKind::Frame, "Nested", 0.0, 0.0, 40.0, 40.0)).unwrap();
        let leaf = editor.document.insert_node(nested,
            Node::new(NodeKind::Text, "Child title", 0.0, 0.0, 16.0, 16.0)).unwrap();
        let other_page = editor.document.add_page("Other page");
        let other = editor.document.insert_node(other_page,
            Node::new(NodeKind::Text, "Child on another page", 0.0, 0.0, 16.0, 16.0)).unwrap();
        let expanded = BTreeSet::from([parent]);
        let mut projection = LayerProjection::default();
        projection.refresh(&editor, false, "child", &expanded);
        assert_eq!(ids(&projection), [parent, nested, leaf, child]);
        assert_eq!(projection.rows().iter().map(|row| row.depth).collect::<Vec<_>>(), [0, 1, 2, 1]);
        assert!(projection.rows().iter().all(|row| row.expanded.is_none()));
        let builds = projection.stats().builds;
        assert!(!projection.refresh(&editor, false, "child", &BTreeSet::new()));
        assert_eq!(projection.stats().builds, builds);
        projection.refresh(&editor, false, "", &expanded);
        assert_eq!(ids(&projection), [sibling, parent, nested, child]);
        editor.set_active_page(other_page);
        projection.refresh(&editor, false, "child", &expanded);
        assert_eq!(ids(&projection), [other]);
    }

    #[test]
    fn asset_search_tracks_unlisted_children_entering_and_leaving_results() {
        let (mut editor, parent, child, _) = tree_editor();
        editor.document.nodes.get_mut(&parent).unwrap().kind = NodeKind::Component;
        editor.refresh();
        let mut projection = LayerProjection::default();
        let expanded = BTreeSet::new();
        projection.refresh(&editor, true, "needle", &expanded);
        assert!(projection.rows().is_empty());
        editor.update_node(child, "Rename", |node| node.name = "Needle".into());
        assert!(projection.refresh(&editor, true, "needle", &expanded));
        assert_eq!(ids(&projection), [parent]);
        editor.update_node(child, "Rename", |node| node.name = "Thread".into());
        assert!(projection.refresh(&editor, true, "needle", &expanded));
        assert!(projection.rows().is_empty());
    }

    #[test]
    fn layer_projection_pages_assets_and_kind_changes_are_context_boundaries() {
        let (mut editor, parent, child, sibling) = tree_editor();
        let first_page = editor.active_page;
        let second_page = editor.document.add_page("Second page");
        let component = editor.document.insert_node(second_page,
            Node::new(NodeKind::Component, "Component", 0.0, 0.0, 10.0, 10.0)).unwrap();
        editor.refresh();
        let expanded = BTreeSet::new();
        let mut projection = LayerProjection::default();
        projection.refresh(&editor, false, "", &expanded);
        assert_eq!(ids(&projection), vec![sibling, parent]);
        editor.update_node(component, "Move on another page", |node| node.x += 1.0);
        assert!(!projection.refresh(&editor, false, "", &expanded));
        assert!(editor.set_active_page(second_page));
        assert!(projection.refresh(&editor, false, "", &expanded));
        assert_eq!(ids(&projection), vec![component]);
        assert!(editor.set_active_page(first_page));
        assert!(projection.refresh(&editor, false, "", &expanded));
        assert!(projection.refresh(&editor, true, "", &expanded));
        assert_eq!(ids(&projection), vec![component]);
        assert!(projection.rows()[0].assets);
        editor.update_node(child, "Rename non-asset", |node| node.name = "Changed".into());
        assert!(!projection.refresh(&editor, true, "", &expanded));
        editor.update_node(sibling, "Make component", |node| node.kind = NodeKind::Component);
        assert!(projection.refresh(&editor, true, "", &expanded));
        assert_eq!(ids(&projection), vec![component, sibling]);
        assert_eq!(projection.stats(), LayerProjectionStats { builds: 5, reuses: 2 });
    }

    #[test]
    fn layer_projection_identity_full_refresh_and_expired_feed_are_conservative() {
        let (mut editor, nodes) = flat_editor(1);
        let expanded = BTreeSet::new();
        let mut projection = LayerProjection::default();
        projection.refresh(&editor, false, "", &expanded);
        let replacement = Editor::new(editor.document.clone());
        assert!(!projection.refresh(&replacement, false, "", &expanded));
        assert_eq!(projection.stats().builds, 2, "equal rows do not imply a valid editor identity");
        editor = replacement;
        editor.refresh();
        assert!(!projection.refresh(&editor, false, "", &expanded));
        assert_eq!(projection.stats().builds, 3);
        for _ in 0..160 {
            editor.update_node(nodes[0], "Move", |node| node.x += 1.0);
        }
        assert!(!projection.refresh(&editor, false, "", &expanded));
        assert_eq!(projection.stats().builds, 4, "a missed ledger window rebuilds even equal rows");
        editor.document.revision = editor.document.revision.wrapping_add(1);
        assert!(!projection.refresh(&editor, false, "", &expanded));
        assert_eq!(projection.stats().builds, 5, "revision mismatch cannot reuse cached rows");
    }
}
