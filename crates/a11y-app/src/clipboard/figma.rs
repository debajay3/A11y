//! Transfers Figma clipboard layers and the definitions they refer to.

use crate::{app::App, edit::Tx, layout};
use a11y_doc::{
    Action, Color, Command, DocError, Document, Expression, Node, NodeId, NodeKind, PropKind, PropValue, VariableValue, Vec2,
};
use std::collections::BTreeMap;

type NodeMap = BTreeMap<NodeId, NodeId>;

pub(super) fn paste(app: &mut App, source: Document, roots: &[NodeId], at: Vec2) -> Result<(), String> {
    let parent = app.container_for_insert();
    let inverse = app.doc.world_transform(parent).invert().ok_or("Cannot place into a collapsed layer")?;
    let at = inverse.apply(at);
    let origin = roots.iter().map(|id| source.world_transform(*id).translation()).reduce(|a, b| Vec2::new(a.x.min(b.x), a.y.min(b.y)));
    let Some(origin) = origin else { return Err("No selected layers".into()) };
    let prefix = format!("figma-paste:{}/", uuid::Uuid::new_v4());
    let visible = allocate_nodes(&mut app.doc, &source, roots);

    // Main components outside the selection are often on the source's hidden library page.
    let definitions: Vec<_> = source
        .nodes
        .values()
        .filter(|node| !visible.contains_key(&node.id) && is_component(node) && !inside_component(&source, node))
        .map(|node| node.id)
        .collect();
    let assets = allocate_nodes(&mut app.doc, &source, &definitions);
    let mut refs = visible.clone();
    refs.extend(assets.iter().map(|(&from, &to)| (from, to)));

    let mut tx = Tx::new();
    let selected: Vec<_> = roots.iter().map(|id| visible[id]).collect();
    let result = (|| {
        transfer_definitions(&mut tx, &mut app.doc, &source, &prefix)?;
        if !definitions.is_empty() {
            let page = components_page(&mut tx, &mut app.doc)?;
            let mut y = app.doc.nodes[&page]
                .children
                .iter()
                .filter_map(|id| app.doc.get(*id))
                .map(|node| node.transform.ty + node.size.y + 48.0)
                .fold(0.0, f32::max);
            for root in definitions {
                let mut nodes = copy_nodes(&source, root, &assets, &refs, &prefix);
                nodes[0].transform.tx = 0.0;
                nodes[0].transform.ty = y;
                y += nodes[0].size.y + 48.0;
                let index = app.doc.nodes[&page].children.len();
                tx.cmd(&mut app.doc, Command::Insert { parent: page, index, nodes })?;
            }
        }
        for &root in roots {
            let mut nodes = copy_nodes(&source, root, &visible, &refs, &prefix);
            let transform = &mut nodes[0].transform;
            *transform = source.world_transform(root);
            transform.tx += at.x - origin.x;
            transform.ty += at.y - origin.y;
            let index = app.doc.nodes[&parent].children.len();
            tx.cmd(&mut app.doc, Command::Insert { parent, index, nodes })?;
        }
        Ok::<(), DocError>(())
    })();
    if let Err(error) = result {
        tx.cancel(&mut app.doc);
        return Err(error.to_string());
    }
    layout::relayout_around_preserving(&mut app.doc, &mut tx.ed, parent, &selected);
    if app.run_tx(tx) {
        app.selection = selected;
        app.fonts.load_for(&app.doc);
    }
    Ok(())
}

fn components_page(tx: &mut Tx, doc: &mut Document) -> Result<NodeId, DocError> {
    const NAME: &str = "Figma Components";
    if let Some(&id) = doc.pages.iter().find(|id| doc.get(**id).is_some_and(|node| node.name == NAME)) {
        return Ok(id);
    }
    let id = doc.alloc_id();
    let mut page = Node::new(id, NodeKind::Page { background: Color::rgb(0xE5, 0xE5, 0xE5) });
    page.name = NAME.into();
    let index = doc.pages.len();
    tx.cmd(doc, Command::InsertPage { index, nodes: vec![page] })?;
    Ok(id)
}

fn is_component(node: &Node) -> bool {
    matches!(node.kind, NodeKind::Component(_) | NodeKind::ComponentSet(_))
}

fn inside_component(source: &Document, node: &Node) -> bool {
    let mut parent = node.parent;
    while let Some(ancestor) = parent.and_then(|id| source.get(id)) {
        if is_component(ancestor) {
            return true;
        }
        parent = ancestor.parent;
    }
    false
}

fn allocate_nodes(doc: &mut Document, source: &Document, roots: &[NodeId]) -> NodeMap {
    let mut map = NodeMap::new();
    for &id in roots {
        source.walk(id, |node, _| {
            map.entry(node.id).or_insert_with(|| doc.alloc_id());
        });
    }
    map
}

fn copy_nodes(source: &Document, root: NodeId, tree: &NodeMap, refs: &NodeMap, prefix: &str) -> Vec<Node> {
    let mut nodes = Vec::new();
    source.walk(root, |node, _| {
        let mut copy = node.clone();
        copy.id = tree[&node.id];
        copy.source_id = None;
        copy.parent = if node.id == root { None } else { node.parent.and_then(|id| tree.get(&id).copied()) };
        copy.children = node.children.iter().map(|id| tree[id]).collect();
        copy.component.source = copy.component.source.and_then(|id| refs.get(&id).copied());
        if let NodeKind::Instance { component, .. } = &mut copy.kind {
            *component = component.and_then(|id| refs.get(&id).copied());
        }
        copy.component.props.retain_mut(|prop| {
            if let PropKind::InstanceSwap { preferred } = &mut prop.kind {
                preferred.retain_mut(|id| replace_id(id, refs));
            }
            match &mut prop.default {
                PropValue::Instance(id) => replace_id(id, refs),
                _ => true,
            }
        });
        copy.component.values.retain(|_, value| match value {
            PropValue::Instance(id) => replace_id(id, refs),
            _ => true,
        });
        copy.style_refs.retain(|_, key| replace_key(key, &source.styles, prefix));
        copy.variables.bindings.retain(|_, key| replace_key(key, &source.variables, prefix));
        if node.id == root {
            for collection in source.variable_collections.keys() {
                if let Some(mode) = source.variable_mode(node.id, collection) {
                    copy.variables.modes.insert(collection.clone(), mode);
                }
            }
        }
        copy.variables.modes = std::mem::take(&mut copy.variables.modes)
            .into_iter()
            .filter(|(key, _)| source.variable_collections.contains_key(key))
            .map(|(key, mode)| (format!("{prefix}{key}"), mode))
            .collect();
        copy.prototype.fixed_children.retain_mut(|id| replace_id(id, tree));
        copy.prototype.reactions.retain_mut(|reaction| {
            reaction.actions.retain_mut(|action| remap_action(action, source, refs, prefix));
            !reaction.actions.is_empty()
        });
        if let NodeKind::Connector(connector) = &mut copy.kind {
            for end in [&mut connector.start, &mut connector.end] {
                end.node = end.node.and_then(|id| tree.get(&id).copied());
            }
        }
        nodes.push(copy);
    });
    nodes
}

fn transfer_definitions(tx: &mut Tx, doc: &mut Document, source: &Document, prefix: &str) -> Result<(), DocError> {
    if !source.styles.is_empty() {
        let mut styles = doc.styles.clone();
        let mut style_meta = doc.style_meta.clone();
        for (key, style) in &source.styles {
            let local = format!("{prefix}{key}");
            styles.insert(local.clone(), style.clone());
            let mut meta = source.style_meta.get(key).cloned().unwrap_or_default();
            if meta.name.is_empty() {
                meta.name = source.style_name(key).to_owned();
            }
            style_meta.insert(local, meta);
        }
        tx.cmd(doc, Command::SetStyles { styles, style_meta })?;
    }
    if !source.variable_collections.is_empty() {
        let mut variables = doc.variables.clone();
        let mut collections = doc.variable_collections.clone();
        for collection in source.variable_collections.values() {
            let mut copy = collection.clone();
            copy.id = format!("{prefix}{}", copy.id);
            copy.variables.retain_mut(|id| replace_key(id, &source.variables, prefix));
            collections.insert(copy.id.clone(), copy);
        }
        for variable in source.variables.values() {
            let mut copy = variable.clone();
            if !replace_key(&mut copy.collection, &source.variable_collections, prefix) {
                continue;
            }
            copy.id = format!("{prefix}{}", copy.id);
            copy.values.retain(|_, value| match value {
                VariableValue::Alias(id) => replace_key(id, &source.variables, prefix),
                _ => true,
            });
            variables.insert(copy.id.clone(), copy);
        }
        tx.cmd(doc, Command::SetVariables { variables, collections })?;
    }
    Ok(())
}

fn replace_id(id: &mut NodeId, map: &NodeMap) -> bool {
    let Some(&local) = map.get(id) else { return false };
    *id = local;
    true
}

fn replace_key<T>(id: &mut String, source: &BTreeMap<String, T>, prefix: &str) -> bool {
    if !source.contains_key(id) {
        return false;
    }
    id.insert_str(0, prefix);
    true
}

fn remap_expr(expr: &mut Expression, source: &Document, prefix: &str) -> bool {
    match expr {
        Expression::Literal(_) => true,
        Expression::Variable(id) => replace_key(id, &source.variables, prefix),
        Expression::Unary { arg, .. } => remap_expr(arg, source, prefix),
        Expression::Binary { lhs, rhs, .. } => remap_expr(lhs, source, prefix) && remap_expr(rhs, source, prefix),
        Expression::Ternary { cond, then, otherwise } => {
            remap_expr(cond, source, prefix) && remap_expr(then, source, prefix) && remap_expr(otherwise, source, prefix)
        }
    }
}

fn remap_action(action: &mut Action, source: &Document, refs: &NodeMap, prefix: &str) -> bool {
    match action {
        Action::Navigate { dest, .. }
        | Action::OpenOverlay { dest, .. }
        | Action::SwapOverlay { dest }
        | Action::ScrollTo { dest, .. }
        | Action::ChangeTo { dest }
        | Action::MediaControl { dest, .. } => replace_id(dest, refs),
        Action::Animation { dest, .. } => dest.as_mut().is_none_or(|id| replace_id(id, refs)),
        Action::SetVariable { variable, value } => replace_key(variable, &source.variables, prefix) && remap_expr(value, source, prefix),
        Action::SetVariableMode { collection, .. } => replace_key(collection, &source.variable_collections, prefix),
        Action::Conditional { branches, otherwise } => {
            branches.retain_mut(|(condition, actions)| {
                actions.retain_mut(|action| remap_action(action, source, refs, prefix));
                remap_expr(condition, source, prefix)
            });
            otherwise.retain_mut(|action| remap_action(action, source, refs, prefix));
            !branches.is_empty() || !otherwise.is_empty()
        }
        Action::CloseOverlay | Action::Back | Action::OpenUrl { .. } => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use a11y_doc::{create_instance, FrameProps, Paint, Rect, Style, StyleMeta, VariableKind, VariableStore};

    #[test]
    fn figma_paste_preserves_components_and_style_and_variable_links() {
        let mut source = Document::new("Source");
        let page = source.add_page("Source page");
        let main = source.alloc_id();
        let mut component = Node::new(main, NodeKind::Component(FrameProps::default()));
        component.name = "Button".into();
        component.size = Vec2::new(120.0, 48.0);
        source.insert(page, component).unwrap();
        let child = source.alloc_id();
        let mut shape = Node::rectangle(child, Rect::new(0.0, 0.0, 120.0, 48.0));
        shape.style_refs.insert("fill".into(), "brand".into());
        source.styles.insert("brand".into(), Style::Fill(vec![Paint::solid(Color::rgb(0x10, 0x73, 0xDE))]));
        source.style_meta.insert("brand".into(), StyleMeta { name: "Brand Blue".into(), ..Default::default() });
        let mut store = VariableStore::from_doc(&source);
        let collection = store.add_collection("Sizing");
        let width = store.add_variable(&collection, "Width", VariableKind::Number).unwrap();
        let mode = store.collections[&collection].default_mode.clone();
        store.set_value(&width, &mode, VariableValue::Number(120.0));
        store.command().apply(&mut source).unwrap();
        shape.variables.bindings.insert("width".into(), width);
        source.insert(main, shape).unwrap();
        create_instance(&mut source, main, page, Vec2::new(150.0, 0.0)).apply(&mut source).unwrap();
        let instance = source.get(page).unwrap().children[1];

        let mut target = Document::new("Target");
        let target_page = target.add_page("Canvas");
        target.styles.insert("brand".into(), Style::Fill(Vec::new()));
        let mut app = App::new(target);
        paste(&mut app, source, &[instance], Vec2::new(50.0, 60.0)).unwrap();

        let pasted = app.doc.get(app.selection[0]).unwrap();
        let NodeKind::Instance { component: Some(component), .. } = &pasted.kind else { panic!("missing main component link") };
        let main = app.doc.get(*component).unwrap();
        assert_ne!(main.parent, Some(target_page));
        assert_eq!(app.doc.get(app.doc.page_of(main.id).unwrap()).unwrap().name, "Figma Components");
        let child = app.doc.get(pasted.children[0]).unwrap();
        assert_eq!(child.component.source, Some(main.children[0]));
        let key = &child.style_refs["fill"];
        assert_ne!(key, "brand");
        assert_eq!(app.doc.style_name(key), "Brand Blue");
        assert!(matches!(app.doc.styles.get("brand"), Some(Style::Fill(paints)) if paints.is_empty()));
        assert!(app.doc.variables.contains_key(&child.variables.bindings["width"]));

        app.undo();
        assert_eq!(app.doc.pages, vec![target_page]);
        assert!(app.doc.nodes[&target_page].children.is_empty());
        assert_eq!(app.doc.styles.len(), 1);
        assert!(app.doc.variables.is_empty());
        app.redo();
        assert_eq!(app.doc.nodes[&target_page].children.len(), 1);
    }
}
