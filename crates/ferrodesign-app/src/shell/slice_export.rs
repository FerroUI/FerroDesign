//! Each selected slice is a separate named viewport. An ordinary multi-layer
//! selection still exports as one composition.
use super::*;
use std::io::{Cursor, Write};
use zip::{write::SimpleFileOptions, CompressionMethod, ZipWriter};

struct ExportTarget {
    stem: String,
    ids: Vec<NodeId>,
}

fn safe_filename(name: &str) -> String {
    // Leave space in a 255-byte filename for deduplication and the extension.
    let mut stem = String::new();
    for c in name.chars() {
        let c = if c.is_control() || r#"\/:*?"<>|"#.contains(c) { '_' } else { c };
        if stem.len() + c.len_utf8() > 200 { break; }
        stem.push(c);
    }
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
    let mut next_suffix = BTreeMap::new();
    for id in slices {
        let original = safe_filename(&editor.document.nodes[&id].name);
        let stem = if used.insert(original.to_lowercase()) {
            original
        } else {
            let suffix = next_suffix.entry(original.to_lowercase()).or_insert(2usize);
            loop {
                let candidate = format!("{original}-{suffix}");
                *suffix += 1;
                if used.insert(candidate.to_lowercase()) { break candidate; }
            }
        };
        targets.push(ExportTarget { stem, ids: vec![id] });
    }
    targets
}

/// Render one file at a time into a stored archive. Keep one browser download
/// per user gesture, and never publish a partial archive after an export fails.
fn prepare_download(
    targets: &[ExportTarget],
    extension: &str,
    mut render: impl FnMut(&ExportTarget) -> Result<Vec<u8>, String>,
) -> Result<(String, Vec<u8>), String> {
    if let [target] = targets {
        return Ok((format!("{}.{extension}", target.stem), render(target)?));
    }
    let mut archive = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    let mut total = 0usize;
    for target in targets {
        let bytes = render(target)?;
        total = total.saturating_add(bytes.len());
        if total > 256 * 1024 * 1024 {
            return Err("Export fewer slices at once. An archive supports 256 MiB of content.".into());
        }
        archive.start_file(format!("{}.{extension}", target.stem), options)
            .map_err(|error| format!("Could not create the slice archive: {error}"))?;
        archive.write_all(&bytes).map_err(|error| format!("Could not write the slice archive: {error}"))?;
    }
    let bytes = archive.finish().map_err(|error| format!("Could not finish the slice archive: {error}"))?;
    Ok((format!("FerroDesign-{extension}.zip"), bytes.into_inner()))
}

impl EditorShell {
    pub(super) fn export_artwork(&self, svg: bool) {
        let targets = targets(&self.editor.borrow());
        let extension = if svg { "svg" } else { "png" };
        let mut warnings = Vec::new();
        let output = prepare_download(&targets, extension, |target| {
            let editor = self.editor.borrow();
            if let [id] = target.ids.as_slice() {
                if editor.document.nodes.get(id).is_some_and(|node| node.kind == NodeKind::Slice)
                    && editor.document.export_slice(editor.active_page, &target.ids).is_none()
                {
                    return Err(format!("{} has empty or singular bounds.", target.stem));
                }
            }
            if svg {
                let report = editor.document.export_svg_report(editor.active_page, &target.ids);
                warnings.extend(report.warnings);
                Ok(report.source.into_bytes())
            } else {
                drop(editor);
                self.canvas.export_png_for(&target.ids)
            }
        });
        if svg { self.close_modal(); }
        match output {
            Ok((name, bytes)) => {
                let mime = if targets.len() > 1 { "application/zip" }
                    else if svg { "image/svg+xml" } else { "image/png" };
                if !platform::download_bytes(&name, &bytes, mime) {
                    self.toast(&format!("Could not save {name}."));
                } else if let Some(first) = warnings.first() {
                    if warnings.len() == 1 { self.toast(first); }
                    else { self.toast(&format!("{first} ({} export warnings.)", warnings.len())); }
                }
            }
            Err(message) => self.toast(&message),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn repeated_slice_names_skip_explicit_suffixes_in_selection_order() {
        let mut doc = core::Document::new("Duplicate slice names");
        let page = doc.pages[0].id;
        let ids = ["Logo", "logo-2", "LOGO", "Logo", "logo-2"].into_iter().map(|name| {
            doc.insert_node(page, Node::new(NodeKind::Slice, name, 0.0, 0.0, 10.0, 10.0)).unwrap()
        }).collect();
        let mut editor = Editor::new(doc);
        editor.selection = ids;
        let names: Vec<_> = targets(&editor).into_iter().map(|target| target.stem).collect();
        assert_eq!(names, ["Logo", "logo-2", "LOGO-3", "Logo-4", "logo-2-2"]);
    }

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
        let unicode = safe_filename(&"😀".repeat(100));
        assert_eq!(unicode.len(), 200);
        let (name, bytes) = prepare_download(&targets, "svg", |t| Ok(t.ids[0].to_le_bytes().to_vec())).unwrap();
        assert_eq!(name, "FerroDesign-svg.zip");
        let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        assert_eq!(zip.len(), 3);
        for target in &targets {
            let mut entry = zip.by_name(&format!("{}.svg", target.stem)).unwrap();
            assert_eq!(entry.compression(), CompressionMethod::Stored);
            let mut bytes = Vec::new();
            std::io::Read::read_to_end(&mut entry, &mut bytes).unwrap();
            assert_eq!(bytes, target.ids[0].to_le_bytes());
        }
        assert!(prepare_download(&targets, "png", |target| {
            if target.ids == [ids[0]] { Err("singular slice".into()) } else { Ok(vec![1]) }
        }).is_err());
        let (name, bytes) = prepare_download(&targets[1..2], "png", |_| Ok(vec![1, 2])).unwrap();
        assert_eq!((name.as_str(), bytes), ("_A.png", vec![1, 2]));
        assert_eq!(safe_filename("CON.preview"), "_CON.preview");
    }
}
