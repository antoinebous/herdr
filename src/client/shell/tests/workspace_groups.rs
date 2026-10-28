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

fn row_rect(state: &ClientShellState, workspace_id: &str) -> Rect {
    state
        .hits
        .workspaces
        .iter()
        .find(|hit| hit.workspace_id == workspace_id)
        .expect("workspace row")
        .rect
}

/// Drags from one sidebar cell to another and returns the requested methods.
fn drag(
    state: &mut ClientShellState,
    from: (u16, u16),
    to: (u16, u16),
) -> Vec<crate::api::schema::Method> {
    mouse(
        state,
        MouseEventKind::Down(MouseButton::Left),
        from.0,
        from.1,
    );
    mouse(state, MouseEventKind::Drag(MouseButton::Left), to.0, to.1);
    mouse(state, MouseEventKind::Up(MouseButton::Left), to.0, to.1)
        .actions
        .into_iter()
        .filter_map(|action| match action {
            ClientShellAction::Endpoint { request, .. } => Some(request.method),
            _ => None,
        })
        .collect()
}

fn set_group(workspace_ids: &[&str], group: Option<&str>) -> crate::api::schema::Method {
    crate::api::schema::Method::WorkspaceSetGroup(crate::api::schema::WorkspaceSetGroupParams {
        workspace_ids: ids(workspace_ids),
        group: group.map(str::to_owned),
    })
}

fn move_block(workspace_ids: &[&str], before: Option<&str>) -> crate::api::schema::Method {
    crate::api::schema::Method::WorkspaceMoveBlock(crate::api::schema::WorkspaceMoveBlockParams {
        workspace_ids: ids(workspace_ids),
        before_workspace_id: before.map(str::to_owned),
    })
}

#[test]
fn dragging_a_group_header_moves_the_whole_group() {
    let mut state = grouped_state(grouped_snapshot(&[None, Some("data"), Some("data"), None]));
    let header = state.hits.workspace_groups[0].rect;
    let last = row_rect(&state, "ws_4");

    let methods = drag(
        &mut state,
        (header.x + 2, header.y),
        (header.x + 2, last.bottom()),
    );

    assert_eq!(methods, [move_block(&["ws_2", "ws_3"], None)]);
}

#[test]
fn dropping_a_workspace_on_a_header_appends_it_to_the_group() {
    let mut state = grouped_state(grouped_snapshot(&[None, Some("data"), Some("data"), None]));
    let source = row_rect(&state, "ws_4");
    let header = state.hits.workspace_groups[0].rect;

    mouse(
        &mut state,
        MouseEventKind::Down(MouseButton::Left),
        source.x + 2,
        source.y,
    );
    mouse(
        &mut state,
        MouseEventKind::Drag(MouseButton::Left),
        header.x + 2,
        header.y,
    );
    let frame = state.compose(106, 40).expect("header drop highlight");
    let highlighted = usize::from(header.y) * usize::from(frame.width) + usize::from(header.x + 5);
    assert_eq!(
        frame.cells[highlighted].bg,
        crate::protocol::color_to_u32(state.config.palette.selection_bg)
    );
    let drop = mouse(
        &mut state,
        MouseEventKind::Up(MouseButton::Left),
        header.x + 2,
        header.y,
    );

    assert!(matches!(
        &drop.actions[..],
        [ClientShellAction::Endpoint { request, .. }]
            if request.method == set_group(&["ws_4"], Some("data"))
    ));
}

#[test]
fn dropping_between_members_joins_the_group_at_that_position() {
    let mut state = grouped_state(grouped_snapshot(&[None, Some("data"), Some("data"), None]));
    let source = row_rect(&state, "ws_1");
    let second_member = row_rect(&state, "ws_3");

    let methods = drag(
        &mut state,
        (source.x + 2, source.y),
        (second_member.x + 4, second_member.y - 1),
    );

    assert_eq!(
        methods,
        [
            set_group(&["ws_1"], Some("data")),
            move_block(&["ws_1"], Some("ws_3")),
        ]
    );
}

#[test]
fn dragging_a_member_out_of_its_group_or_within_it() {
    let mut state = grouped_state(grouped_snapshot(&[None, Some("data"), Some("data"), None]));
    let first_member = row_rect(&state, "ws_2");
    let second_member = row_rect(&state, "ws_3");
    let last = row_rect(&state, "ws_4");

    let methods = drag(
        &mut state,
        (first_member.x + 4, first_member.y),
        (last.x + 2, last.bottom()),
    );
    assert_eq!(
        methods,
        [set_group(&["ws_2"], None), move_block(&["ws_2"], None)]
    );

    let methods = drag(
        &mut state,
        (second_member.x + 4, second_member.y),
        (first_member.x + 4, first_member.y),
    );
    assert_eq!(
        methods,
        [crate::api::schema::Method::WorkspaceMove(
            crate::api::schema::WorkspaceMoveParams {
                workspace_id: "ws_3".into(),
                insert_index: 1,
            }
        )]
    );
}

#[test]
fn move_to_group_lists_the_other_existing_groups() {
    let mut state = grouped_state(grouped_snapshot(&[Some("data"), Some("ops"), None]));
    let labels = |state: &ClientShellState| match state.overlay.as_ref() {
        Some(ClientShellOverlay::ContextMenu(menu)) => menu
            .items()
            .iter()
            .map(|item| item.label.to_string())
            .collect::<Vec<_>>(),
        _ => Vec::new(),
    };

    state.open_workspace_context_menu("ws_3".into(), 1, 1);
    let move_to = menu_index(&state, ClientContextMenuAction::MoveToGroup).expect("move item");
    let mut outcome = ClientShellInput::default();
    state.activate_context_menu_item(move_to, &mut outcome);
    assert_eq!(labels(&state), ["data", "ops", "New group..."]);
    let ops = menu_index(&state, ClientContextMenuAction::PickGroup(1)).expect("ops item");
    let mut outcome = ClientShellInput::default();
    state.activate_context_menu_item(ops, &mut outcome);
    let request = set_group_request(&outcome).expect("pick request");
    assert_eq!(request.workspace_ids, ids(&["ws_3"]));
    assert_eq!(request.group.as_deref(), Some("ops"));

    state.open_workspace_context_menu("ws_1".into(), 1, 1);
    let move_to = menu_index(&state, ClientContextMenuAction::MoveToGroup).expect("move item");
    state.activate_context_menu_item(move_to, &mut ClientShellInput::default());
    assert_eq!(labels(&state), ["ops", "New group..."]);
    let new_group = menu_index(&state, ClientContextMenuAction::NewGroup).expect("new item");
    state.activate_context_menu_item(new_group, &mut ClientShellInput::default());
    assert!(matches!(
        state.overlay,
        Some(ClientShellOverlay::Rename(ClientRenameOverlay {
            title: "new group",
            target: ClientRenameTarget::WorkspaceGroup { ref workspace_ids },
            ..
        })) if workspace_ids == &ids(&["ws_1"])
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
