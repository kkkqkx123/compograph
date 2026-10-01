//! Canvas: painted graph plus pointer, wheel, and key handling.
//!
//! This module builds the middle-row canvas only. It consumes one frame of
//! owned paint plans and wires the mouse and keyboard gestures to the
//! selection module; it never runs algorithms or touches dialogs.

use cg_graph::GraphView;
use cg_interact::{
    NODE_HALF_EXTENT, apply_point_select, can_begin_drag, can_grab_node, compound_toggle_target,
    drag_position, hover_node_shaped, press_hit_compound, press_hit_shaped,
    should_clear_on_blank, wheel_zoom_factor,
};
use cg_render::{DetailLevel, graph_view};
use gpui::{
    App, Context, InteractiveElement, IntoElement, KeyDownEvent, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, ParentElement, ScrollDelta, ScrollWheelEvent, Styled, Window,
    div, prelude::FluentBuilder, px,
};

use crate::app_state::{GraphWindow, is_dismiss_key};
use crate::frame_plans::FrameParts;

impl GraphWindow {
    pub(crate) fn canvas_view(
        &self,
        cx: &Context<Self>,
        parts: FrameParts,
    ) -> impl IntoElement {
        let FrameParts {
            containers,
            nodes,
            edges,
            arrows,
            labels,
            edge_labels,
            rubber_band,
        } = parts;
        let lod = self.lod;
        let hover_text = self.hover_label(cx);
        let hover_anchor = self.hover_anchor;
        let hovering =
            self.hovered.is_some() && !self.rubber.is_active() && lod != DetailLevel::Minimal;
        let view = cx.entity();
        div()
            .flex_1()
            .relative()
            .track_focus(&self.focus)
            .child(graph_view(
                containers,
                nodes,
                edges,
                arrows,
                labels,
                edge_labels,
                rubber_band,
            ))
            .when(hovering && hover_text.is_some(), |canvas| {
                canvas.child(
                    div()
                        .absolute()
                        .left(px(hover_anchor.x + 12.0))
                        .top(px(hover_anchor.y + 12.0))
                        .px_2()
                        .py_1()
                        .child(hover_text.unwrap_or_default()),
                )
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, _window, cx| {
                    let viewport_point = Self::viewport_point(event.position);
                    let additive = event.modifiers.shift;
                    let world = this
                        .camera
                        .viewport_to_world(this.viewport, viewport_point);
                    let positions =
                        this.layout.read(cx).positions().clone();
                    let version = this.layout.read(cx).positions_version();
                    this.refresh_spatial(&positions, version);
                    let locks = this.locks;
                    let mode = this.select_mode;
                    let click_count = event.click_count;
                    let hit = if can_grab_node(&locks) {
                        let store = this.store.read(cx);
                        if store.has_compound() {
                            press_hit_compound(
                                world,
                                store,
                                &positions,
                                NODE_HALF_EXTENT,
                                this.hit_radius(),
                            )
                            .map(|node| {
                                let offset = positions
                                    .get(&node)
                                    .map(|center| world - *center)
                                    .unwrap_or_default();
                                (node, offset)
                            })
                        } else {
                            press_hit_shaped(
                                world,
                                &positions,
                                &this.spatial,
                                this.hit_radius(),
                                |node| this.node_shape(cx, node),
                            )
                        }
                    } else {
                        None
                    };
                    match hit {
                        Some((node, offset)) => {
                            let toggleable = {
                                let store = this.store.read(cx);
                                compound_toggle_target(store, node)
                            };
                            if toggleable && click_count >= 2 {
                                let store_entity = this.store.clone();
                                this.store.update(cx, |graph, cx| {
                                    let collapsed = graph.is_collapsed(node);
                                    graph.set_collapsed(cx, node, !collapsed);
                                });
                                let _ = store_entity;
                                this.selection.select(node);
                                this.rubber.cancel();
                                this.rebuild_bypass();
                                cx.notify();
                                return;
                            }
                            if can_begin_drag(&locks) {
                                this.drag.begin(node, offset);
                            }
                            apply_point_select(
                                &mut this.selection,
                                mode,
                                node,
                                additive,
                            );
                            this.rubber.cancel();
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
                        }
                        None => {
                            if should_clear_on_blank(&locks, additive) {
                                this.clear_selection();
                            }
                            this.rubber.begin(viewport_point);
                        }
                    }
                    cx.notify();
                }),
            )
            .on_mouse_move(cx.listener(
                |this, event: &MouseMoveEvent, _window, cx| {
                    let viewport_point = Self::viewport_point(event.position);
                    if let Some(node) = this.drag.active_node() {
                        if !can_begin_drag(&this.locks) {
                            this.drag.end();
                            cx.notify();
                            return;
                        }
                        let offset = this
                            .drag
                            .active
                            .as_ref()
                            .map(|gesture| gesture.grab_offset)
                            .unwrap_or_default();
                        let world = this
                            .camera
                            .viewport_to_world(this.viewport, viewport_point);
                        let target = drag_position(world, offset);
                        let moved = this.layout.update(cx, |driver, cx| {
                            driver.move_pinned(node, target, cx)
                        });
                        if moved {
                            this.retained.mark_moved([node]);
                        }
                        cx.notify();
                    } else if this.rubber.is_active() {
                        this.rubber.update(viewport_point);
                        cx.notify();
                    } else {
                        let world = this
                            .camera
                            .viewport_to_world(this.viewport, viewport_point);
                        let positions =
                            this.layout.read(cx).positions().clone();
                        let version = this.layout.read(cx).positions_version();
                        this.refresh_spatial(&positions, version);
                        let hovered = {
                            let store = this.store.read(cx);
                            if store.has_compound() {
                                press_hit_compound(
                                    world,
                                    store,
                                    &positions,
                                    NODE_HALF_EXTENT,
                                    this.hit_radius(),
                                )
                            } else {
                                hover_node_shaped(
                                    world,
                                    &positions,
                                    &this.spatial,
                                    this.hit_radius(),
                                    |node| this.node_shape(cx, node),
                                )
                            }
                        };
                        if hovered != this.hovered {
                            this.hovered = hovered;
                            this.hover_anchor = viewport_point;
                            this.rebuild_bypass();
                            cx.notify();
                        } else if hovered.is_some() {
                            this.hover_anchor = viewport_point;
                            cx.notify();
                        }
                    }
                },
            ))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, event: &MouseUpEvent, _window, cx| {
                    let dragged = this.drag.end().is_some();
                    if dragged {
                        this.rubber.cancel();
                        let store = this.store.clone();
                        this.layout.update(cx, |driver, cx| {
                            driver.request_refine(&store, cx);
                        });
                    } else if let Some(rect) = this.rubber.rect() {
                        this.rubber.cancel();
                        this.finish_rubber(rect, event.modifiers.shift, cx);
                    }
                    cx.notify();
                }),
            )
            .on_scroll_wheel(cx.listener(
                |this, event: &ScrollWheelEvent, _window, cx| {
                    let anchor = Self::viewport_point(event.position);
                    let lines = match &event.delta {
                        ScrollDelta::Pixels(pixels) => {
                            f32::from(pixels.y) / 16.0
                        }
                        ScrollDelta::Lines(lines) => lines.y,
                    };
                    this.camera.zoom_at(
                        this.viewport,
                        anchor,
                        wheel_zoom_factor(lines),
                    );
                    cx.notify();
                },
            ))
            .on_key_down(
                move |event: &KeyDownEvent, _window: &mut Window, cx: &mut App| {
                    if is_dismiss_key(event.keystroke.key.as_str()) {
                        view.update(cx, |this, cx| {
                            this.clear_selection();
                            cx.notify();
                        });
                    }
                },
            )
    }
}
