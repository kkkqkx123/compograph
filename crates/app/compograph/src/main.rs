//! compograph desktop application entry point.

mod algo_panel;
mod algo_params;
mod algo_tasks;
mod app_state;
mod file_io;
mod frame_plans;
mod io_actions;
mod selection;
mod view_canvas;
mod view_sidebar;
mod view_status;
mod view_toolbar;

use app_state::GraphWindow;
use gpui::{
    App, AppContext, Bounds, Context, IntoElement, ParentElement, Render, Styled, Window,
    WindowBounds, WindowOptions, div, px, size,
};
use gpui_platform::application;

impl Render for GraphWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let frame = self.prepare_frame(window, cx);
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(self.toolbar_view(cx))
            .child(
                div()
                    .flex_1()
                    .flex()
                    .flex_row()
                    .child(self.canvas_view(cx, frame))
                    .child(self.sidebar_view(cx)),
            )
            .child(self.status_view(cx))
    }
}

fn main() {
    application().run(|cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(1024.0), px(768.0)), cx);
        let opened = cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |_, cx| cx.new(GraphWindow::new),
        );
        match opened {
            Ok(window) => {
                if window
                    .update(cx, |view, window, cx| {
                        window.focus(&view.focus, cx);
                    })
                    .is_err()
                {
                    eprintln!("failed to focus graph window");
                }
            }
            Err(error) => {
                eprintln!("failed to open graph window: {error}");
            }
        }
        cx.activate(true);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::TestAppContext;

    use crate::algo_panel::AlgoOutcome;
    use crate::app_state::{endpoint_slot, is_dismiss_key};
    use cg_graph::{GraphDocument, NodeEntry, NodeIndex};
    use cg_render::NodeStylePatch;

    #[test]
    fn only_escape_dismisses_the_selection() {
        assert!(is_dismiss_key("escape"));
        assert!(!is_dismiss_key("Enter"));
        assert!(!is_dismiss_key(""));
    }

    #[test]
    fn endpoint_slot_resolves_ordinals_against_the_live_order() {
        let ids = vec![NodeIndex::new(0), NodeIndex::new(1), NodeIndex::new(2)];
        assert_eq!(endpoint_slot(&ids, NodeIndex::new(1)), Some(1));
        assert_eq!(endpoint_slot(&ids, NodeIndex::new(9)), None);
    }

    #[gpui::test]
    fn stale_algo_write_back_is_dropped(cx: &mut TestAppContext) {
        let view = cx.update(|cx: &mut App| cx.new(GraphWindow::new));
        cx.update(|cx| {
            view.update(cx, |this, cx| {
                this.run_shortest_path(cx);
                this.run_components(cx);
            })
        });
        cx.update(|cx| {
            view.update(cx, |this, cx| {
                assert_eq!(this.algo_generation, 2);
                assert!(this.algo_busy);
                let stale = AlgoOutcome {
                    summary: "stale".to_string(),
                    ..AlgoOutcome::default()
                };
                assert!(!this.commit_outcome(1, stale));
                assert_ne!(this.algo_summary, "stale");
                let current = AlgoOutcome {
                    nodes: vec![(NodeIndex::new(0), NodeStylePatch::selected())],
                    summary: "current".to_string(),
                    ..AlgoOutcome::default()
                };
                assert!(this.commit_outcome(2, current));
                assert_eq!(this.algo_summary, "current");
                assert!(!this.algo_busy);
                assert!(this.bypass.node_bypass(NodeIndex::new(0)).is_some());
                cx.notify();
            })
        });
    }

    #[gpui::test]
    fn selection_and_algo_highlights_share_the_bypass(cx: &mut TestAppContext) {
        let view = cx.update(|cx: &mut App| cx.new(GraphWindow::new));
        cx.update(|cx| {
            view.update(cx, |this, cx| {
                this.selection.select(NodeIndex::new(0));
                let outcome = AlgoOutcome {
                    nodes: vec![(NodeIndex::new(1), NodeStylePatch::selected())],
                    ..AlgoOutcome::default()
                };
                this.algo_generation = 1;
                assert!(this.commit_outcome(1, outcome));
                assert!(this.bypass.node_bypass(NodeIndex::new(0)).is_some());
                assert!(this.bypass.node_bypass(NodeIndex::new(1)).is_some());
                this.clear_selection();
                assert!(this.bypass.node_bypass(NodeIndex::new(0)).is_none());
                assert!(this.bypass.node_bypass(NodeIndex::new(1)).is_some());
                cx.notify();
            })
        });
    }

    #[gpui::test]
    fn json_import_restores_positions_after_driver_reactions(cx: &mut TestAppContext) {
        use cg_types::Point2;
        let view = cx.update(|cx: &mut App| cx.new(GraphWindow::new));
        let document = GraphDocument {
            nodes: vec![
                NodeEntry {
                    id: 0,
                    label: "a".to_string(),
                    position: Some([11.0, 22.0]),
                    ..NodeEntry::default()
                },
                NodeEntry {
                    id: 1,
                    label: "b".to_string(),
                    position: Some([33.0, 44.0]),
                    ..NodeEntry::default()
                },
            ],
            edges: vec![cg_graph::EdgeEntry {
                source: 0,
                target: 1,
                weight: 1.0,
                ..cg_graph::EdgeEntry::default()
            }],
        };
        cx.update(|cx| {
            view.update(cx, |this, cx| {
                this.apply_document(&document, cx);
            })
        });
        cx.update(|cx| {
            view.update(cx, |this, cx| {
                assert_eq!(this.store.read(cx).node_count(), 2);
                let positions = this.layout.read(cx).positions().clone();
                assert_eq!(positions.len(), 2);
                let mut points: Vec<Point2> = positions.values().copied().collect();
                points.sort_by(|a, b| a.x.partial_cmp(&b.x).unwrap_or(std::cmp::Ordering::Equal));
                assert_eq!(points[0], Point2::new(11.0, 22.0));
                assert_eq!(points[1], Point2::new(33.0, 44.0));
            })
        });
    }
}
