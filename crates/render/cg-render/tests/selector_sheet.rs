//! Checks that one query string filters and styles the same elements.
//!
//! Filtering runs through the parsed query directly while stylesheet
//! matching runs through the selector sheet; both must agree so a rule
//! highlights exactly the elements the same string selects.

use cg_graph::{DataValue, GraphStore, GraphView, NodeIndex, parse_selector};
use cg_render::{EdgeStyle, EdgeStylePatch, NodeStyle, NodeStylePatch, SelectorSheet};
use gpui::{App, AppContext, Entity, TestAppContext};

fn setup(cx: &mut TestAppContext) -> Entity<GraphStore> {
    cx.update(|cx: &mut App| {
        let store = cx.new(|_| GraphStore::new());
        store.update(cx, |graph, cx| {
            let hub = graph.add_node(cx, "hub");
            assert!(graph.add_node_class(cx, hub, "hub"));
            assert!(graph.set_node_attr(cx, hub, "score", DataValue::Number(9.0)));
            let leaf = graph.add_node(cx, "leaf");
            assert!(graph.set_node_attr(cx, leaf, "score", DataValue::Number(1.0)));
            graph.add_edge(cx, hub, leaf, 2.0);
        });
        store
    })
}

#[gpui::test]
fn same_query_filters_and_styles_the_same_nodes(cx: &mut TestAppContext) {
    let store = setup(cx);
    for text in ["node.hub", ".hub", "[score >= 5]", "#hub", "node.hub, #leaf"] {
        let filtered: Vec<NodeIndex> = cx.update(|cx| {
            let graph = store.read(cx);
            parse_selector(text)
                .expect("capped subset parses")
                .select_nodes(graph)
        });
        assert!(
            !filtered.is_empty(),
            "query {text:?} selects the fixture hub"
        );
        let mut sheet = SelectorSheet::new();
        sheet
            .parse_node_rule(text, NodeStylePatch::selected())
            .expect("stylesheet takes the same subset");
        let base = NodeStyle::default();
        let styled: Vec<NodeIndex> = cx.update(|cx| {
            let graph = store.read(cx);
            let mut hits: Vec<NodeIndex> = graph
                .node_ids()
                .filter(|node| sheet.resolve_node(graph, &base, *node) != base)
                .collect();
            hits.sort_unstable_by_key(|node| node.index());
            hits
        });
        assert_eq!(
            filtered, styled,
            "query {text:?} styles exactly its filter set"
        );
    }
}

#[gpui::test]
fn same_query_filters_and_styles_the_same_edges(cx: &mut TestAppContext) {
    let store = setup(cx);
    let text = "edge[weight >= 2]";
    let filtered = cx.update(|cx| {
        let graph = store.read(cx);
        parse_selector(text)
            .expect("capped subset parses")
            .select_edges(graph)
    });
    assert_eq!(filtered.len(), 1, "the heavy edge is selected");
    let mut sheet = SelectorSheet::new();
    sheet
        .parse_edge_rule(text, EdgeStylePatch::highlighted())
        .expect("stylesheet takes the same subset");
    let base = EdgeStyle::default();
    cx.update(|cx| {
        let graph = store.read(cx);
        let pairs: &dyn GraphView = graph;
        for (source, target) in pairs.edges() {
            let styled = sheet.resolve_edge(graph, &base, source, target);
            assert_eq!(
                styled != base,
                filtered.contains(&(source, target)),
                "edge ({source:?}, {target:?}) agrees across both paths"
            );
        }
    });
}
