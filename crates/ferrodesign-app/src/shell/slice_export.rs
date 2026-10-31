//! Each selected slice is a separate named viewport. An ordinary multi-layer
//! selection still exports as one composition.
use super::*;

struct ExportTarget {
    stem: String,
    ids: Vec<NodeId>,
}

fn safe_filename(name: &str) -> String {
    let stem: String = name.chars().take(100).map(|c| {
        if c.is_control() || r#"\/:*?"<>|"#.contains(c) { '_' } else { c }
    }).collect();
    let stem = stem.trim_matches([' ', '.']);
    if stem.is_empty() { return "Slice".into(); }
    let prefix = stem.split('.').next().unwrap_or_default().to_ascii_uppercase();
    if matches!(prefix.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (prefix.len() == 4 && (prefix.starts_with("COM") || prefix.starts_with("LPT"))
            && matches!(prefix.as_bytes()[3], b'1'..=b'9'))
    { return format!("_{stem}"); }
    stem.into()
}

fn targets(editor: &Editor) -> Vec<ExportTarget> {
    let (slices, artwork): (Vec<_>, Vec<_>) = editor.selection.iter().copied()
        .partition(|id| editor.document.nodes.get(id).is_some_and(|node| node.kind == NodeKind::Slice));
    let mut targets = Vec::new();
    let mut used = BTreeSet::new();
    if slices.is_empty() || !artwork.is_empty() {
        used.insert("ferrodesign".to_owned());
        targets.push(ExportTarget { stem: "FerroDesign".into(), ids: artwork });
    }
    for id in slices {
        let original = safe_filename(&editor.document.nodes[&id].name);
        let mut stem = original.clone();
        let mut suffix = 2;
        while !used.insert(stem.to_lowercase()) {
            stem = format!("{original}-{suffix}");
            suffix += 1;
        }
        targets.push(ExportTarget { stem, ids: vec![id] });
    }
    targets
}

impl EditorShell {
    pub(super) fn export_artwork(&self, svg: bool) {
        let targets = targets(&self.editor.borrow());
        let mut warnings = Vec::new();
        for target in targets {
            if let [id] = target.ids.as_slice() {
                let editor = self.editor.borrow();
                if editor.document.nodes.get(id).is_some_and(|node| node.kind == NodeKind::Slice)
                    && editor.document.export_slice(editor.active_page, &target.ids).is_none()
                {
                    warnings.push(format!("{} has empty or singular bounds.", target.stem));
                    continue;
                }
            }
            if svg {
                let report = {
                    let editor = self.editor.borrow();
                    editor.document.export_svg_report(editor.active_page, &target.ids)
                };
                if !platform::download_bytes(&format!("{}.svg", target.stem), report.source.as_bytes(), "image/svg+xml") {
                    warnings.push(format!("Could not save {}.svg.", target.stem));
                }
                warnings.extend(report.warnings);
            } else {
                match self.canvas.export_png_for(&target.ids) {
                    Ok(bytes) => {
                        if !platform::download_bytes(&format!("{}.png", target.stem), &bytes, "image/png") {
                            warnings.push(format!("Could not save {}.png.", target.stem));
                        }
                    }
                    Err(message) => warnings.push(message),
                }
            }
        }
        if svg { self.close_modal(); }
        if let Some(first) = warnings.first() {
            if warnings.len() == 1 { self.toast(first); }
            else { self.toast(&format!("{first} ({} export warnings.)", warnings.len())); }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mixed_selection_has_a_safe_distinct_filename_per_slice() {
        let mut doc = core::Document::new("Slice names");
        let page = doc.pages[0].id;
        let ids: Vec<_> = [(NodeKind::Slice, "../A"), (NodeKind::Slice, "_A"), (NodeKind::Rectangle, "R")]
            .into_iter().map(|(kind, name)| doc.insert_node(page, Node::new(kind, name, 0.0, 0.0, 10.0, 20.0)).unwrap()).collect();
        let mut editor = Editor::new(doc);
        editor.selection = ids.clone();
        let targets = targets(&editor);
        assert_eq!(targets.iter().map(|t| t.stem.as_str()).collect::<Vec<_>>(), ["FerroDesign", "_A", "_A-2"]);
        assert_eq!(targets[0].ids, [ids[2]]);
        assert_eq!(targets[1].ids, [ids[0]]);
        assert_eq!(safe_filename("CON.preview"), "_CON.preview");
    }
}
