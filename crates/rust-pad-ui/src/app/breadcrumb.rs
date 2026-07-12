//! Breadcrumb strip between the tab strip and the editor.
//!
//! Shows the active document's location as `folder › folder › file`, using
//! workspace-relative segments when the file lives under an open workspace
//! folder and the parent directory otherwise. Purely informational.

use std::path::{Path, PathBuf};

use eframe::egui;

use super::App;
use crate::app::workspace_ops::copy_path_root_for;

const SEPARATOR: &str = "\u{203A}";
const BREADCRUMB_FONT_SIZE: f32 = 11.5;

/// Cached path-to-segments computation, keyed by the active document's
/// identity so it is not recomputed every frame.
#[derive(Default)]
pub(crate) struct BreadcrumbCache {
    key: Option<BreadcrumbKey>,
    segments: Vec<String>,
    extension_hint: String,
}

#[derive(PartialEq, Eq, Clone)]
struct BreadcrumbKey {
    path: Option<PathBuf>,
    title: String,
}

impl BreadcrumbCache {
    /// Returns the segments + extension hint for the given document state,
    /// recomputing only when the document identity changed.
    fn resolve(
        &mut self,
        path: Option<&Path>,
        title: &str,
        workspace_root: Option<&Path>,
    ) -> (&[String], &str) {
        let key = BreadcrumbKey {
            path: path.map(Path::to_path_buf),
            title: title.to_string(),
        };
        if self.key.as_ref() != Some(&key) {
            self.segments = compute_segments(path, title, workspace_root);
            self.extension_hint = path
                .and_then(|p| p.extension())
                .map(|e| e.to_string_lossy().to_string())
                .unwrap_or_default();
            self.key = Some(key);
        }
        (&self.segments, &self.extension_hint)
    }
}

/// Sanitizes one breadcrumb segment for display.
///
/// The extension hint stays byte-derived, so it is immune to bidi
/// reordering and acts as the honest counter-signal.
fn sanitize_segment(raw: &str) -> String {
    crate::text_sanitize::sanitize_display_text(raw)
}

/// Computes display segments for a document.
///
/// Workspace files: workspace folder name, then each path component below
/// it. Non-workspace files: parent directory name + file name. Unsaved
/// documents: just the (sanitized) title.
fn compute_segments(
    path: Option<&Path>,
    title: &str,
    workspace_root: Option<&Path>,
) -> Vec<String> {
    let Some(path) = path else {
        return vec![sanitize_segment(title)];
    };

    let mut segments = Vec::new();
    if let Some(root) = workspace_root {
        if let Some(root_name) = root.file_name() {
            segments.push(sanitize_segment(&root_name.to_string_lossy()));
        }
        if let Ok(relative) = path.strip_prefix(root) {
            for component in relative.components() {
                segments.push(sanitize_segment(&component.as_os_str().to_string_lossy()));
            }
        }
        if segments.len() > 1 {
            return segments;
        }
        segments.clear();
    }

    if let Some(parent_name) = path.parent().and_then(Path::file_name) {
        segments.push(sanitize_segment(&parent_name.to_string_lossy()));
    }
    if let Some(file_name) = path.file_name() {
        segments.push(sanitize_segment(&file_name.to_string_lossy()));
    } else {
        segments.push(sanitize_segment(title));
    }
    segments
}

impl App {
    /// Renders the breadcrumb strip contents.
    pub(crate) fn show_breadcrumb_strip(&mut self, ui: &mut egui::Ui) {
        let chrome = self.theme_ctrl.chrome.clone();
        let doc = self.tabs.active_doc();
        let path = doc.file_path.clone();
        let title = doc.title.clone();
        let workspace_root = path
            .as_deref()
            .and_then(|p| copy_path_root_for(&self.workspace_sidebar.tree, p))
            .map(Path::to_path_buf);

        let (segments, hint) =
            self.breadcrumb_cache
                .resolve(path.as_deref(), &title, workspace_root.as_deref());
        let segments: Vec<String> = segments.to_vec();
        let hint = hint.to_string();

        // 1px bottom border separating the strip from the editor body.
        let rect = ui.max_rect();
        ui.painter().line_segment(
            [rect.left_bottom(), rect.right_bottom()],
            egui::Stroke::new(1.0, chrome.border),
        );

        let font = egui::FontId::proportional(BREADCRUMB_FONT_SIZE);
        ui.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            let last = segments.len().saturating_sub(1);
            for (i, segment) in segments.iter().enumerate() {
                if i > 0 {
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(SEPARATOR)
                                .font(font.clone())
                                .color(chrome.text_faint),
                        )
                        .selectable(false),
                    );
                }
                let color = if i == last {
                    self.theme_ctrl.theme.text_color
                } else {
                    chrome.text_muted
                };
                ui.add(
                    egui::Label::new(egui::RichText::new(segment).font(font.clone()).color(color))
                        .selectable(false),
                );
            }

            if !hint.is_empty() {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(hint)
                                .font(egui::FontId::monospace(BREADCRUMB_FONT_SIZE))
                                .color(chrome.text_faint),
                        )
                        .selectable(false),
                    );
                });
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsaved_document_shows_title_only() {
        let segments = compute_segments(None, "Untitled 3", None);
        assert_eq!(segments, vec!["Untitled 3"]);
    }

    #[test]
    fn workspace_file_shows_root_relative_segments() {
        let root = PathBuf::from("/home/me/rust-pad");
        let path = PathBuf::from("/home/me/rust-pad/notes/plan.md");
        let segments = compute_segments(Some(&path), "plan.md", Some(&root));
        assert_eq!(segments, vec!["rust-pad", "notes", "plan.md"]);
    }

    #[test]
    fn non_workspace_file_shows_parent_and_name() {
        let path = PathBuf::from("/tmp/scratch/notes.txt");
        let segments = compute_segments(Some(&path), "notes.txt", None);
        assert_eq!(segments, vec!["scratch", "notes.txt"]);
    }

    #[test]
    fn file_outside_the_workspace_falls_back_to_parent() {
        let root = PathBuf::from("/home/me/rust-pad");
        let path = PathBuf::from("/opt/other/readme.md");
        let segments = compute_segments(Some(&path), "readme.md", Some(&root));
        assert_eq!(segments, vec!["other", "readme.md"]);
    }

    // Sanitization behavior (control chars, bidi overrides, pass-through) is
    // covered by `crate::text_sanitize` tests; segments route through it.

    #[test]
    fn segments_are_sanitized() {
        let segments = compute_segments(None, "evil\u{202E}name", None);
        assert_eq!(segments, vec!["evil\u{FFFD}name"]);
    }

    #[test]
    fn cache_recomputes_only_on_identity_change() {
        let mut cache = BreadcrumbCache::default();
        let path = PathBuf::from("/tmp/a/file.txt");
        let (first, _) = cache.resolve(Some(&path), "file.txt", None);
        let first = first.to_vec();
        let (second, _) = cache.resolve(Some(&path), "file.txt", None);
        assert_eq!(first, second.to_vec());

        let other = PathBuf::from("/tmp/b/other.txt");
        let (third, _) = cache.resolve(Some(&other), "other.txt", None);
        assert_eq!(third, vec!["b", "other.txt"]);
    }
}
