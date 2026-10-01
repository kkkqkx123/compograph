//! Top toolbar: layout menu, file buttons, and view toggles.
//!
//! This module builds the upper bar only. Each button delegates to an action
//! module; the bar itself holds no graph logic.

use cg_graph::GraphView;
use cg_interact::SelectMode;
use cg_render::{DetailLevel, ExportScope};
use gpui::{
    ClickEvent, Context, InteractiveElement, IntoElement, ParentElement,
    StatefulInteractiveElement, Styled, div, prelude::FluentBuilder,
};

use crate::app_state::GraphWindow;

impl GraphWindow {
    pub(crate) fn toolbar_view(&self, cx: &Context<Self>) -> impl IntoElement {
        let engine_name = self.layout.read(cx).engine_name().to_string();
        let layouts = self.layouts.clone();
        let menu_open = self.menu_open;
        let current_layout = engine_name.clone();
        let lod_label = match self.lod {
            DetailLevel::Full => "full",
            DetailLevel::Simplified => "simplified",
            DetailLevel::Minimal => "minimal",
        };
        let aggregate_label = if self.aggregate {
            "aggregate:on"
        } else {
            "aggregate:off"
        };
        let retained_label = if self.retained.enabled() {
            "retained:on"
        } else {
            "retained:off"
        };
        let mode_label = match self.select_mode {
            SelectMode::Single => "select:single",
            SelectMode::Additive => "select:additive",
        };
        let lock_drag_label = if self.locks.lock_drag {
            "lock-drag:on"
        } else {
            "lock-drag:off"
        };
        let no_grab_label = if self.locks.no_grab {
            "no-grab:on"
        } else {
            "no-grab:off"
        };
        let no_deselect_label = if self.locks.no_deselect {
            "no-deselect:on"
        } else {
            "no-deselect:off"
        };
        let neighbor_label = format!("neighbors:{}", self.neighbor_hops);
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .px_2()
            .py_1()
            .child(
                div()
                    .id("layout-menu-toggle")
                    .child(format!("layout: {engine_name}"))
                    .on_click(cx.listener(|this, _event: &ClickEvent, _window, cx| {
                        this.menu_open = !this.menu_open;
                        cx.notify();
                    })),
            )
            .when(menu_open, |bar| {
                let mut bar = bar;
                for (ordinal, name) in layouts.iter().enumerate() {
                    let picked: &'static str = name;
                    let label = if picked == current_layout.as_str() {
                        format!("*{picked}")
                    } else {
                        picked.to_string()
                    };
                    bar = bar.child(
                        div()
                            .id(("layout-pick", ordinal))
                            .px_2()
                            .py_1()
                            .child(label)
                            .on_click(cx.listener(move |this, _event: &ClickEvent, _window, cx| {
                                this.switch_layout(picked, cx);
                            })),
                    );
                }
                bar
            })
            .child(
                div()
                    .id("io-export-json")
                    .px_2()
                    .child("export json")
                    .on_click(cx.listener(|this, _event: &ClickEvent, _window, cx| {
                        this.export_json(cx);
                    })),
            )
            .child(
                div()
                    .id("io-import-json")
                    .px_2()
                    .child("import json")
                    .on_click(cx.listener(|this, _event: &ClickEvent, _window, cx| {
                        this.import_json(cx);
                    })),
            )
            .child(
                div()
                    .id("io-export-dot")
                    .px_2()
                    .child("export dot")
                    .on_click(cx.listener(|this, _event: &ClickEvent, _window, cx| {
                        this.export_dot(cx);
                    })),
            )
            .child(
                div()
                    .id("io-import-dot")
                    .px_2()
                    .child("import dot")
                    .on_click(cx.listener(|this, _event: &ClickEvent, _window, cx| {
                        this.import_dot(cx);
                    })),
            )
            .child(
                div()
                    .id("view-aggregate-toggle")
                    .px_2()
                    .child(aggregate_label)
                    .on_click(cx.listener(|this, _event: &ClickEvent, _window, cx| {
                        this.aggregate = !this.aggregate;
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .id("view-retained-toggle")
                    .px_2()
                    .child(retained_label)
                    .on_click(cx.listener(|this, _event: &ClickEvent, _window, cx| {
                        this.retained.set_enabled(!this.retained.enabled());
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .id("select-mode-toggle")
                    .px_2()
                    .child(mode_label)
                    .on_click(cx.listener(|this, _event: &ClickEvent, _window, cx| {
                        this.select_mode = match this.select_mode {
                            SelectMode::Single => SelectMode::Additive,
                            SelectMode::Additive => SelectMode::Single,
                        };
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .id("lock-drag-toggle")
                    .px_2()
                    .child(lock_drag_label)
                    .on_click(cx.listener(|this, _event: &ClickEvent, _window, cx| {
                        this.locks.lock_drag = !this.locks.lock_drag;
                        this.drag.end();
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .id("no-grab-toggle")
                    .px_2()
                    .child(no_grab_label)
                    .on_click(cx.listener(|this, _event: &ClickEvent, _window, cx| {
                        this.locks.no_grab = !this.locks.no_grab;
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .id("no-deselect-toggle")
                    .px_2()
                    .child(no_deselect_label)
                    .on_click(cx.listener(|this, _event: &ClickEvent, _window, cx| {
                        this.locks.no_deselect = !this.locks.no_deselect;
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .id("neighbor-hops-cycle")
                    .px_2()
                    .child(neighbor_label)
                    .on_click(cx.listener(|this, _event: &ClickEvent, _window, cx| {
                        this.neighbor_hops = (this.neighbor_hops + 1) % 4;
                        let store = this.store.read(cx);
                        let view: &dyn GraphView = store;
                        let (fringe, neighbor_edges) =
                            GraphWindow::derive_neighborhood(
                                view,
                                &this.selection,
                                &this.selected_edges,
                                this.neighbor_hops,
                            );
                        this.neighbor_nodes = fringe;
                        this.neighbor_edges = neighbor_edges;
                        this.rebuild_bypass();
                        cx.notify();
                    })),
            )
            .child(div().px_2().child(format!("lod:{lod_label}")))
            .child(
                div()
                    .id("io-export-viewport")
                    .px_2()
                    .child("export view")
                    .on_click(cx.listener(|this, _event: &ClickEvent, _window, cx| {
                        this.export_image(ExportScope::Viewport, cx);
                    })),
            )
            .child(
                div()
                    .id("io-export-full")
                    .px_2()
                    .child("export full")
                    .on_click(cx.listener(|this, _event: &ClickEvent, _window, cx| {
                        this.export_image(ExportScope::FullGraph, cx);
                    })),
            )
    }
}
