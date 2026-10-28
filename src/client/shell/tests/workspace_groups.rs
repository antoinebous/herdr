use super::*;

fn grouped_snapshot(groups: &[Option<&str>]) -> ClientShellSnapshot {
    let mut snapshot = snapshot();
    let template = snapshot.workspaces[0].clone();
    snapshot.workspaces = groups
        .iter()
        .enumerate()
        .map(|(index, group)| ClientShellWorkspace {
            workspace_id: format!("ws_{}", index + 1),
            number: index + 1,
            label: format!("space-{}", index + 1),
            focused: index == 0,
            group: group.map(str::to_owned),
            ..template.clone()
        })
        .collect();
    snapshot
}

fn grouped_state(snapshot: ClientShellSnapshot) -> ClientShellState {
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    state.set_snapshot(Box::new(snapshot));
    state.set_pane_surface(surface());
    state.compose(106, 40).expect("composed frame");
    state
}

fn row_summary(snapshot: &ClientShellSnapshot, collapsed: &HashSet<String>) -> Vec<String> {
    render::sidebar_rows(snapshot, collapsed)
        .iter()
        .map(|row| match row {
            SidebarRow::Group(header) => format!("[{}]", header.name),
            SidebarRow::Workspace(entry) => format!(
                "{}{}{}",
                if entry.grouped { "  " } else { "" },
                if entry.indented { "└" } else { "" },
                snapshot.workspaces[entry.index].workspace_id
            ),
        })
        .collect()
}

fn mouse(
    state: &mut ClientShellState,
    kind: MouseEventKind,
    column: u16,
    row: u16,
) -> ClientShellInput {
    state.handle_raw_events(vec![RawInputEvent::Mouse(MouseEvent {
        kind,
        column,
        row,
        modifiers: KeyModifiers::empty(),
    })])
}

fn set_group_request(
    outcome: &ClientShellInput,
) -> Option<&crate::api::schema::WorkspaceSetGroupParams> {
    outcome.actions.iter().find_map(|action| match action {
        ClientShellAction::Endpoint { request, .. } => match &request.method {
            crate::api::schema::Method::WorkspaceSetGroup(params) => Some(params),
            _ => None,
        },
        _ => None,
    })
}

fn menu_index(state: &ClientShellState, action: ClientContextMenuAction) -> Option<usize> {
    match state.overlay.as_ref() {
        Some(ClientShellOverlay::ContextMenu(menu)) => {
            menu.items().iter().position(|item| item.action == action)
        }
        _ => None,
    }
}

fn ids(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

#[test]
fn groups_appear_at_their_first_member_and_gather_scattered_members() {
    let snapshot = grouped_snapshot(&[None, Some("data"), None, Some("data"), Some("ops")]);

    assert_eq!(
        row_summary(&snapshot, &HashSet::new()),
        ["ws_1", "[data]", "  ws_2", "  ws_4", "ws_3", "[ops]", "  ws_5"]
    );
    assert_eq!(
        render::workspace_entries(&snapshot, &HashSet::new())
            .iter()
            .map(|entry| snapshot.workspaces[entry.index].workspace_id.as_str())
            .collect::<Vec<_>>(),
        ["ws_1", "ws_2", "ws_4", "ws_3", "ws_5"]
    );
}

#[test]
fn collapsed_group_keeps_the_focused_member_and_shows_the_most_urgent_status() {
    let mut snapshot = grouped_snapshot(&[None, Some("data"), Some("data"), None]);
    snapshot.workspaces[0].focused = false;
    snapshot.workspaces[2].focused = true;
    snapshot.focused_workspace_id = Some("ws_3".into());
    snapshot.workspaces[1].agent_status = AgentStatus::Blocked;
    let collapsed = HashSet::from([sidebar::group_collapse_key("data")]);

    assert_eq!(
        row_summary(&snapshot, &collapsed),
        ["ws_1", "[data]", "  ws_3", "ws_4"]
    );

    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    state.collapsed_groups = collapsed;
    state.set_snapshot(Box::new(snapshot));
    state.set_pane_surface(surface());
    let frame = state.compose(106, 24).expect("collapsed group");
    let header = state.hits.workspace_groups[0].rect;
    let rows = frame_rows(&frame);
    assert!(rows[usize::from(header.y)].contains("▸ data · 2"));
    let status_cell = usize::from(header.y) * usize::from(frame.width)
        + usize::from(header.right().saturating_sub(2));
    assert_eq!(
        frame.cells[status_cell].fg,
        crate::protocol::color_to_u32(state.config.palette.red)
    );
}

#[test]
fn clicking_a_group_header_toggles_collapse() {
    let mut state = grouped_state(grouped_snapshot(&[None, Some("data"), Some("data")]));
    let header = state.hits.workspace_groups[0].rect;

    mouse(
        &mut state,
        MouseEventKind::Down(MouseButton::Left),
        header.x + 2,
        header.y,
    );
    let release = mouse(
        &mut state,
        MouseEventKind::Up(MouseButton::Left),
        header.x + 2,
        header.y,
    );

    assert!(
        release.actions.is_empty(),
        "a header click must not focus a workspace"
    );
    assert!(state.collapsed_groups.contains("group:data"));
    state.compose(106, 24).expect("collapsed");
    assert!(!state
        .hits
        .workspaces
        .iter()
        .any(|hit| hit.workspace_id == "ws_2" || hit.workspace_id == "ws_3"));

    let header = state.hits.workspace_groups[0].rect;
    mouse(
        &mut state,
        MouseEventKind::Down(MouseButton::Left),
        header.x + 2,
        header.y,
    );
    mouse(
        &mut state,
        MouseEventKind::Up(MouseButton::Left),
        header.x + 2,
        header.y,
    );
    assert!(!state.collapsed_groups.contains("group:data"));
}

#[test]
fn group_menu_ungroups_and_renames_every_member() {
    let mut state = grouped_state(grouped_snapshot(&[None, Some("data"), None, Some("data")]));
    let header = state.hits.workspace_groups[0].rect;
    mouse(
        &mut state,
        MouseEventKind::Down(MouseButton::Right),
        header.x + 2,
        header.y,
    );
    assert!(matches!(
        state.overlay,
        Some(ClientShellOverlay::ContextMenu(ClientContextMenuOverlay {
            target: ClientContextMenuTarget::WorkspaceGroup { ref name, collapsed: false },
            ..
        })) if name == "data"
    ));

    let mut outcome = ClientShellInput::default();
    let ungroup = menu_index(&state, ClientContextMenuAction::Ungroup).expect("ungroup item");
    state.activate_context_menu_item(ungroup, &mut outcome);
    let request = set_group_request(&outcome).expect("ungroup request");
    assert_eq!(request.workspace_ids, ids(&["ws_2", "ws_4"]));
    assert_eq!(request.group, None);

    state.collapsed_groups.insert("group:data".into());
    state.open_group_context_menu("data".into(), header.x, header.y);
    let rename = menu_index(&state, ClientContextMenuAction::Rename).expect("rename item");
    let mut outcome = ClientShellInput::default();
    state.activate_context_menu_item(rename, &mut outcome);
    let Some(ClientShellOverlay::Rename(overlay)) = state.overlay.as_mut() else {
        panic!("rename overlay");
    };
    assert_eq!(overlay.input.trim(), "data");
    overlay.input = TextEditor::new("ml", false);
    let mut outcome = ClientShellInput::default();
    state.save_rename_overlay(&mut outcome);
    let request = set_group_request(&outcome).expect("rename request");
    assert_eq!(request.workspace_ids, ids(&["ws_2", "ws_4"]));
    assert_eq!(request.group.as_deref(), Some("ml"));
    assert!(state.collapsed_groups.contains("group:ml"));
    assert!(!state.collapsed_groups.contains("group:data"));
}

#[test]
fn workspace_menu_moves_a_repo_parent_with_its_worktrees() {
    let mut snapshot = grouped_snapshot(&[None, None, None]);
    for (index, linked) in [(0, false), (1, true)] {
        snapshot.workspaces[index].worktree = Some(ClientShellWorktree {
            key: "repo".into(),
            label: "repo".into(),
            is_linked_worktree: linked,
        });
    }
    let mut state = grouped_state(snapshot);

    state.open_workspace_context_menu("ws_1".into(), 1, 1);
    assert_eq!(
        menu_index(&state, ClientContextMenuAction::RemoveFromGroup),
        None
    );
    let move_to = menu_index(&state, ClientContextMenuAction::MoveToGroup).expect("move item");
    let mut outcome = ClientShellInput::default();
    state.activate_context_menu_item(move_to, &mut outcome);
    let Some(ClientShellOverlay::Rename(overlay)) = state.overlay.as_mut() else {
        panic!("group overlay");
    };
    assert_eq!(overlay.input.trim(), "");
    overlay.input = TextEditor::new("  data ", false);
    let mut outcome = ClientShellInput::default();
    state.save_rename_overlay(&mut outcome);
    let request = set_group_request(&outcome).expect("set group request");
    assert_eq!(request.workspace_ids, ids(&["ws_1", "ws_2"]));
    assert_eq!(request.group.as_deref(), Some("data"));
}

#[test]
fn workspace_menu_offers_removal_and_hides_grouping_on_older_servers() {
    let mut state = grouped_state(grouped_snapshot(&[Some("data"), None]));

    state.open_workspace_context_menu("ws_1".into(), 1, 1);
    let remove = menu_index(&state, ClientContextMenuAction::RemoveFromGroup).expect("remove item");
    let mut outcome = ClientShellInput::default();
    state.activate_context_menu_item(remove, &mut outcome);
    let request = set_group_request(&outcome).expect("remove request");
    assert_eq!(request.workspace_ids, ids(&["ws_1"]));
    assert_eq!(request.group, None);

    state.set_endpoint_methods(Some(vec!["workspace.rename".into()]));
    state.open_workspace_context_menu("ws_1".into(), 1, 1);
    assert_eq!(
        menu_index(&state, ClientContextMenuAction::MoveToGroup),
        None
    );
    assert_eq!(
        menu_index(&state, ClientContextMenuAction::RemoveFromGroup),
        None
    );
}

#[test]
fn next_workspace_skips_members_of_a_collapsed_group() {
    let mut state = grouped_state(grouped_snapshot(&[None, Some("data"), Some("data"), None]));
    state.collapsed_groups.insert("group:data".into());

    let mut next = ClientShellInput::default();
    state.record_binding(
        crate::input::KeybindMatch::Action(crate::input::KeybindAction::NextWorkspace),
        &mut next,
    );

    assert!(matches!(
        &next.actions[..],
        [ClientShellAction::Endpoint { request, .. }]
            if matches!(
                &request.method,
                crate::api::schema::Method::WorkspaceFocus(target) if target.workspace_id == "ws_4"
            )
    ));
}

#[test]
fn dragging_a_group_header_moves_the_whole_group_and_members_stay_put() {
    let mut state = grouped_state(grouped_snapshot(&[None, Some("data"), Some("data"), None]));
    let member = state
        .hits
        .workspaces
        .iter()
        .find(|hit| hit.workspace_id == "ws_2")
        .expect("member row")
        .rect;
    mouse(
        &mut state,
        MouseEventKind::Down(MouseButton::Left),
        member.x + 2,
        member.y,
    );
    mouse(
        &mut state,
        MouseEventKind::Drag(MouseButton::Left),
        member.x + 2,
        member.y + 3,
    );
    assert!(
        state.chrome_drag.is_none(),
        "group members drag only through the header"
    );
    mouse(
        &mut state,
        MouseEventKind::Up(MouseButton::Left),
        member.x + 2,
        member.y + 3,
    );

    let header = state.hits.workspace_groups[0].rect;
    let last = state
        .hits
        .workspaces
        .iter()
        .find(|hit| hit.workspace_id == "ws_4")
        .expect("last row")
        .rect;
    mouse(
        &mut state,
        MouseEventKind::Down(MouseButton::Left),
        header.x + 2,
        header.y,
    );
    mouse(
        &mut state,
        MouseEventKind::Drag(MouseButton::Left),
        header.x + 2,
        last.bottom(),
    );
    let drop = mouse(
        &mut state,
        MouseEventKind::Up(MouseButton::Left),
        header.x + 2,
        last.bottom(),
    );

    assert!(matches!(
        &drop.actions[..],
        [ClientShellAction::Endpoint { request, .. }]
            if matches!(
                &request.method,
                crate::api::schema::Method::WorkspaceMoveBlock(params)
                    if params.workspace_ids == ids(&["ws_2", "ws_3"])
                        && params.before_workspace_id.is_none()
            )
    ));
}

#[test]
fn worktree_tree_nests_inside_a_group_only_when_both_share_it() {
    let mut snapshot = grouped_snapshot(&[Some("data"), Some("data"), None]);
    for (index, linked) in [(0, false), (1, true)] {
        snapshot.workspaces[index].worktree = Some(ClientShellWorktree {
            key: "repo".into(),
            label: "repo".into(),
            is_linked_worktree: linked,
        });
    }
    assert_eq!(
        row_summary(&snapshot, &HashSet::new()),
        ["[data]", "  ws_1", "  └ws_2", "ws_3"]
    );
    let frame = grouped_state(snapshot.clone())
        .compose(106, 24)
        .expect("nested worktree");
    assert!(frame_rows(&frame).iter().any(|row| row.contains("└─")));

    snapshot.workspaces[1].group = None;
    assert_eq!(
        row_summary(&snapshot, &HashSet::new()),
        ["[data]", "  ws_1", "ws_2", "ws_3"]
    );
}
