//! Activity bar: the slim icon strip on the far left edge of the window.
//!
//! Hosts the Files (workspace toggle), Search, and Problems entries, with
//! Settings pinned to the bottom. Purely a launcher surface: every action
//! routes to state the menus already control, so the bar adds no new modes.

use eframe::egui;
use egui::{Rect, Sense, Vec2};

use super::App;
use crate::app::chrome;
use crate::app::resolved_theme::ACTIVITY_ITEM_HEIGHT;

/// What the user activated this frame, applied by the caller after layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ActivityAction {
    None,
    ToggleWorkspace,
    OpenSearch,
    OpenProblems,
    OpenSettings,
}

/// One entry in the bar, resolved before painting.
struct ActivityItem {
    icon: &'static str,
    label: &'static str,
    action: ActivityAction,
    active: bool,
    badge: Option<usize>,
    /// Renders dimmed with an explanatory tooltip instead of a dead badge
    /// (used when the problem store failed to open).
    degraded: bool,
}

impl App {
    /// Renders the activity bar contents and returns the activated action.
    pub(crate) fn show_activity_bar(&mut self, ui: &mut egui::Ui) -> ActivityAction {
        let items = [
            ActivityItem {
                icon: crate::icons::FILES,
                label: "Workspace Explorer",
                action: ActivityAction::ToggleWorkspace,
                active: self.workspace_sidebar.visible,
                badge: None,
                degraded: false,
            },
            ActivityItem {
                icon: crate::icons::MAGNIFYING_GLASS,
                label: "Find & Replace",
                action: ActivityAction::OpenSearch,
                active: self.find_replace.visible,
                badge: None,
                degraded: false,
            },
            ActivityItem {
                icon: crate::icons::WARNING,
                label: "Problems",
                action: ActivityAction::OpenProblems,
                active: self.problems_open,
                badge: (self.problems_unread > 0).then_some(self.problems_unread),
                degraded: crate::problem_log::store().is_none(),
            },
        ];

        let mut action = ActivityAction::None;
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.add_space(4.0);
            for item in &items {
                if self.activity_item(ui, item).clicked() {
                    action = item.action;
                }
            }

            // Settings pins to the bottom edge.
            let bottom = ui.available_height() - ACTIVITY_ITEM_HEIGHT - 4.0;
            if bottom > 0.0 {
                ui.add_space(bottom);
            }
            let settings = ActivityItem {
                icon: crate::icons::SLIDERS,
                label: "Preferences",
                action: ActivityAction::OpenSettings,
                active: self.settings_open,
                badge: None,
                degraded: false,
            };
            if self.activity_item(ui, &settings).clicked() {
                action = settings.action;
            }
        });
        action
    }

    /// Renders one activity item: hover tint, active indicator, icon, and
    /// optional count badge.
    ///
    /// Uses the non-focusable click sense so the bar never captures keyboard
    /// focus from the editor or the workspace tree; keyboard ownership in
    /// this app is click-latched, never hover- or focus-implied.
    fn activity_item(&self, ui: &mut egui::Ui, item: &ActivityItem) -> egui::Response {
        let chrome_theme = &self.theme_ctrl.chrome;
        let metrics = &self.theme_ctrl.metrics;

        let width = ui.available_width();
        let (rect, response) =
            ui.allocate_exact_size(Vec2::new(width, ACTIVITY_ITEM_HEIGHT), Sense::CLICK);
        let accessible_label = if item.degraded {
            format!("{} (problem log unavailable)", item.label)
        } else {
            item.label.to_string()
        };
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &accessible_label)
        });

        // Indicator/hover backgrounds inset slightly so the pill shape reads
        // inside the narrow bar.
        let bg_rect = Rect::from_center_size(
            rect.center(),
            Vec2::new(width - 8.0, ACTIVITY_ITEM_HEIGHT - 4.0),
        );
        if item.active {
            chrome::accent_indicator(ui.painter(), bg_rect, chrome_theme, metrics);
        } else if response.hovered() {
            chrome::hover_tint(ui.painter(), bg_rect, chrome_theme, metrics);
        }

        let icon_color = if item.degraded {
            chrome_theme.text_faint
        } else if item.active {
            chrome_theme.accent
        } else {
            chrome_theme.text_muted
        };
        let galley = ui.painter().layout_no_wrap(
            item.icon.to_owned(),
            egui::FontId::proportional(18.0),
            icon_color,
        );
        let icon_pos = rect.center() - galley.size() / 2.0;
        ui.painter().galley(icon_pos, galley, icon_color);

        if let Some(count) = item.badge {
            chrome::count_badge(
                ui.painter(),
                rect.center() + Vec2::new(9.0, -9.0),
                count,
                chrome_theme.warn,
                chrome_theme.activity_bg,
            );
        }

        let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
        if item.degraded {
            response.on_hover_text("Problem log unavailable")
        } else {
            response.on_hover_text(item.label)
        }
    }

    /// Applies an activity-bar action to application state. Mirrors what the
    /// corresponding menu items do.
    pub(crate) fn handle_activity_action(&mut self, action: ActivityAction) {
        match action {
            ActivityAction::None => {}
            ActivityAction::ToggleWorkspace => {
                self.workspace_sidebar.visible = !self.workspace_sidebar.visible;
            }
            ActivityAction::OpenSearch => {
                self.find_replace.open();
            }
            ActivityAction::OpenProblems => {
                self.problems_open = true;
                self.refresh_problem_count();
            }
            ActivityAction::OpenSettings => {
                self.settings_open = true;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toggle_workspace_flips_sidebar_visibility() {
        let mut app = crate::app::tests::test_app();
        let before = app.workspace_sidebar.visible;
        app.handle_activity_action(ActivityAction::ToggleWorkspace);
        assert_eq!(app.workspace_sidebar.visible, !before);
        app.handle_activity_action(ActivityAction::ToggleWorkspace);
        assert_eq!(app.workspace_sidebar.visible, before);
    }

    #[test]
    fn open_search_opens_find_replace() {
        let mut app = crate::app::tests::test_app();
        assert!(!app.find_replace.visible);
        app.handle_activity_action(ActivityAction::OpenSearch);
        assert!(app.find_replace.visible);
    }

    #[test]
    fn open_problems_sets_dialog_flag() {
        let mut app = crate::app::tests::test_app();
        app.handle_activity_action(ActivityAction::OpenProblems);
        assert!(app.problems_open);
    }

    #[test]
    fn open_settings_sets_dialog_flag() {
        let mut app = crate::app::tests::test_app();
        app.handle_activity_action(ActivityAction::OpenSettings);
        assert!(app.settings_open);
    }

    #[test]
    fn none_action_changes_nothing() {
        let mut app = crate::app::tests::test_app();
        let sidebar = app.workspace_sidebar.visible;
        app.handle_activity_action(ActivityAction::None);
        assert_eq!(app.workspace_sidebar.visible, sidebar);
        assert!(!app.problems_open);
        assert!(!app.settings_open);
    }
}
