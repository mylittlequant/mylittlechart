//! The chart context menu as uzor DOM (Wave A): `domkit::menu` with nested
//! submenus, positioned at the app's `ContextMenuState`. Hits carry the kit's
//! index ids; [`legacy_action`] maps them back to the app's `context_menu:{action}`
//! ids so the existing dispatcher handles picks unchanged.

use mlc_app_layout::ui::context_menu::{ContextMenuItemState, ContextMenuState};
use uzor::types::Rect;
use uzor::ui::dom::El;
use uzor::ui::domkit::{self, MenuItem};

fn mi(it: &ContextMenuItemState) -> MenuItem {
    let mut m = MenuItem::new(it.label.clone());
    m.danger = it.is_danger;
    m.disabled = !it.enabled;
    m.separator = it.is_separator;
    if !it.submenu.is_empty() {
        m.sub = Some(it.submenu.iter().map(mi).collect());
    }
    m
}

/// The menu tree at the app's position; the item whose `submenu_open` is set
/// carries `.open` so its nested panel shows.
pub fn tree(s: &ContextMenuState) -> El {
    let items: Vec<MenuItem> = s.items.iter().map(mi).collect();
    let open = s.items.iter().position(|it| it.submenu_open);
    domkit::menu_with_open("context_menu", None, &items, Rect::new(s.x, s.y, 220.0, 0.0), open)
}

/// Map a DOM menu hit (`context_menu:item:{i}` / `context_menu:sub:{i}:{j}`) back
/// to the legacy `context_menu:{action}` id the app dispatcher understands.
pub fn legacy_action(s: &ContextMenuState, hit: &str) -> Option<String> {
    if let Some(rest) = hit.strip_prefix("context_menu:item:") {
        let i: usize = rest.parse().ok()?;
        return Some(format!("context_menu:{}", s.items.get(i)?.action));
    }
    if let Some(rest) = hit.strip_prefix("context_menu:sub:") {
        let (i, j) = rest.split_once(':')?;
        let i: usize = i.parse().ok()?;
        let j: usize = j.parse().ok()?;
        return Some(format!("context_menu:{}", s.items.get(i)?.submenu.get(j)?.action));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use mlc_app_layout::ui::context_menu::ContextMenuTarget;

    #[test]
    fn context_menu_tree_hits_and_legacy_mapping() {
        let mut s = ContextMenuState::new();
        s.open(
            100.0,
            100.0,
            ContextMenuTarget::ChartBackground,
            vec![
                ContextMenuItemState::action("clone", "Clone"),
                ContextMenuItemState::action("more", "More").with_submenu(vec![ContextMenuItemState::action("del", "Delete")]),
            ],
        );
        s.items[1].submenu_open = true;
        let el = tree(&s);
        let mut ctx = uzor_render_tiny_skia::TinySkiaCpuRenderContext::new(400, 400, 1.0);
        let sheet = uzor::ui::domkit::sheet();
        let dom = uzor::ui::dom::lay_out_in(&mut ctx, &sheet, &el, 0.0, 0.0, 400.0, None, &|_| uzor::ui::style::State::NONE);
        let ids: Vec<&str> = dom.hits().map(|(i, _)| i).collect();
        assert!(ids.contains(&"context_menu:item:0"));
        assert!(ids.contains(&"context_menu:sub:1:0"));
        assert_eq!(legacy_action(&s, "context_menu:item:0").as_deref(), Some("context_menu:clone"));
        assert_eq!(legacy_action(&s, "context_menu:sub:1:0").as_deref(), Some("context_menu:del"));
    }
}
