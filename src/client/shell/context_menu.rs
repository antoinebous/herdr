use super::*;

impl ClientContextMenuOverlay {
    pub(super) fn items(&self) -> Vec<ClientContextMenuItem> {
        use ClientContextMenuAction as Action;

        let item = |label: &'static str, action| ClientContextMenuItem {
            label: label.into(),
            action,
        };
        let mut items = match &self.target {
            ClientContextMenuTarget::Workspace { is_git: false, .. } => {
                vec![item("Rename", Action::Rename), item("Close", Action::Close)]
            }
            ClientContextMenuTarget::Workspace {
                is_linked_worktree: false,
                has_worktree_children: false,
                ..
            } => vec![
                item("Rename", Action::Rename),
                item("Close", Action::Close),
                item("New worktree", Action::NewWorktree),
                item("Open worktree...", Action::OpenWorktree),
            ],
            ClientContextMenuTarget::Workspace {
                is_linked_worktree: true,
                ..
            } => vec![
                item("Rename", Action::Rename),
                item("Close", Action::Close),
                item("Delete worktree checkout...", Action::RemoveWorktree),
            ],
            ClientContextMenuTarget::Workspace {
                has_worktree_children: true,
                close_group,
                collapsed,
                ..
            } => vec![
                item("Rename", Action::Rename),
                item(
                    if *close_group { "Close group" } else { "Close" },
                    Action::Close,
                ),
                item("New worktree", Action::NewWorktree),
                item("Open worktree...", Action::OpenWorktree),
                item(
                    if *collapsed { "Expand" } else { "Collapse" },
                    Action::ToggleGroup,
                ),
            ],
            ClientContextMenuTarget::GroupPicker { groups, .. } => groups
                .iter()
                .enumerate()
                .map(|(index, group)| ClientContextMenuItem {
                    label: group.clone().into(),
                    action: Action::PickGroup(index),
                })
                .chain(std::iter::once(item("New group...", Action::NewGroup)))
                .collect(),
            ClientContextMenuTarget::WorkspaceGroup { collapsed, .. } => vec![
                item("Rename group", Action::Rename),
                item("Ungroup", Action::Ungroup),
                item(
                    if *collapsed { "Expand" } else { "Collapse" },
                    Action::ToggleGroup,
                ),
            ],
            ClientContextMenuTarget::Tab { .. } => vec![
                item("New tab", Action::NewTab),
                item("Rename", Action::Rename),
                item("Close", Action::Close),
            ],
            ClientContextMenuTarget::Pane {
                source_pane_id,
                has_manual_label,
                right_click_passthrough,
                skip_agent_resume,
                ..
            } => {
                let mut items = vec![item("Rename pane", Action::RenamePane)];
                if *has_manual_label {
                    items.push(item("Clear pane name", Action::ClearPaneName));
                }
                if source_pane_id.is_some() {
                    items.push(item("Swap with focused pane", Action::SwapWithFocusedPane));
                }
                items.extend([
                    item("Split right", Action::SplitRight),
                    item("Split down", Action::SplitDown),
                    item("Zoom", Action::Zoom),
                    item(
                        if *right_click_passthrough {
                            "Use Herdr right-click menu"
                        } else {
                            "Send right-clicks to pane"
                        },
                        Action::ToggleRightClickPassthrough,
                    ),
                ]);
                if let Some(skip_agent_resume) = skip_agent_resume {
                    items.push(item(
                        if *skip_agent_resume {
                            "Resume on restart"
                        } else {
                            "Don't resume on restart"
                        },
                        Action::ToggleAgentResume,
                    ));
                }
                items.push(item("Close pane", Action::ClosePane));
                items
            }
        };
        if let ClientContextMenuTarget::Workspace {
            group,
            can_group: true,
            ..
        } = &self.target
        {
            items.push(item("Move to group...", Action::MoveToGroup));
            if group.is_some() {
                items.push(item("Remove from group", Action::RemoveFromGroup));
            }
        }
        if let ClientContextMenuTarget::Workspace {
            agent_resume_workspace_ids,
            skip_agent_resume: Some(skip_agent_resume),
            ..
        } = &self.target
        {
            let label = match (agent_resume_workspace_ids.len() > 1, *skip_agent_resume) {
                (true, true) => "Resume group on restart",
                (true, false) => "Don't resume group on restart",
                (false, true) => "Resume agents on restart",
                (false, false) => "Don't resume agents on restart",
            };
            items.push(item(label, Action::ToggleAgentResume));
        }
        items
    }
}

impl ClientShellState {
    pub(super) fn open_workspace_context_menu(&mut self, workspace_id: String, x: u16, y: u16) {
        let Some(snapshot) = self.snapshot.as_deref() else {
            return;
        };
        let Some(workspace) = snapshot
            .workspaces
            .iter()
            .find(|workspace| workspace.workspace_id == workspace_id)
        else {
            return;
        };
        let worktree = workspace.worktree.as_ref();
        let has_worktree_children = worktree.is_some_and(|worktree| {
            !worktree.is_linked_worktree
                && snapshot.workspaces.iter().any(|candidate| {
                    candidate.worktree.as_ref().is_some_and(|candidate| {
                        candidate.key == worktree.key && candidate.is_linked_worktree
                    })
                })
        });
        let close_group = super::sidebar::workspace_close_is_group(snapshot, workspace);
        let collapsed = worktree.is_some_and(|worktree| {
            self.group_is_collapsed(&self.active_endpoint_id, &worktree.key)
        });
        let group = workspace.group.clone();
        let can_group = self.supports_endpoint_method_name("workspace.set_group");
        let agent_resume_members = match worktree.filter(|_| close_group) {
            Some(worktree) => snapshot
                .workspaces
                .iter()
                .filter(|member| {
                    member
                        .worktree
                        .as_ref()
                        .is_some_and(|candidate| candidate.key == worktree.key)
                })
                .collect::<Vec<_>>(),
            None => vec![workspace],
        };
        let agent_resume_workspace_ids = agent_resume_members
            .iter()
            .map(|member| member.workspace_id.clone())
            .collect::<Vec<_>>();
        let skip_agent_resume = self
            .supports_endpoint_method(&crate::api::schema::Method::WorkspaceSetAgentResume(
                crate::api::schema::WorkspaceSetAgentResumeParams {
                    workspace_ids: agent_resume_workspace_ids.clone(),
                    enabled: true,
                },
            ))
            .then(|| {
                agent_resume_members
                    .iter()
                    .all(|member| member.skip_agent_resume)
            });
        self.overlay = Some(ClientShellOverlay::ContextMenu(ClientContextMenuOverlay {
            target: ClientContextMenuTarget::Workspace {
                workspace_id,
                is_git: worktree.is_some() || workspace.branch.is_some(),
                is_linked_worktree: worktree.is_some_and(|worktree| worktree.is_linked_worktree),
                has_worktree_children,
                close_group,
                collapsed,
                group,
                can_group,
                agent_resume_workspace_ids,
                skip_agent_resume,
            },
            x,
            y,
            highlighted: 0,
        }));
    }

    pub(super) fn open_group_context_menu(&mut self, name: String, x: u16, y: u16) {
        let collapsed = self.group_is_collapsed(
            &self.active_endpoint_id,
            &super::sidebar::group_collapse_key(&name),
        );
        self.overlay = Some(ClientShellOverlay::ContextMenu(ClientContextMenuOverlay {
            target: ClientContextMenuTarget::WorkspaceGroup { name, collapsed },
            x,
            y,
            highlighted: 0,
        }));
    }

    /// Lists the other existing groups, or asks for a new name when there are none.
    fn open_group_picker(&mut self, workspace_id: String, x: u16, y: u16) {
        let workspace_ids = self.workspace_with_worktree_children(&workspace_id);
        let mut groups = Vec::<String>::new();
        if let Some(snapshot) = self.snapshot.as_deref() {
            let current = snapshot
                .workspaces
                .iter()
                .find(|workspace| workspace.workspace_id == workspace_id)
                .and_then(|workspace| workspace.group.as_deref());
            for group in snapshot
                .workspaces
                .iter()
                .filter_map(|workspace| workspace.group.as_deref())
            {
                if Some(group) != current && !groups.iter().any(|known| known == group) {
                    groups.push(group.to_owned());
                }
            }
        }
        if groups.is_empty() {
            self.open_new_group_prompt(workspace_ids);
            return;
        }
        self.overlay = Some(ClientShellOverlay::ContextMenu(ClientContextMenuOverlay {
            target: ClientContextMenuTarget::GroupPicker {
                workspace_ids,
                groups,
            },
            x,
            y,
            highlighted: 0,
        }));
    }

    fn open_new_group_prompt(&mut self, workspace_ids: Vec<String>) {
        self.overlay = Some(ClientShellOverlay::Rename(ClientRenameOverlay {
            title: "new group",
            input: TextEditor::new("", false),
            target: ClientRenameTarget::WorkspaceGroup { workspace_ids },
        }));
    }

    /// Ids of every workspace in `group`, in snapshot order.
    pub(super) fn group_member_ids(&self, group: &str) -> Vec<String> {
        self.snapshot
            .as_deref()
            .map(|snapshot| {
                snapshot
                    .workspaces
                    .iter()
                    .filter(|workspace| workspace.group.as_deref() == Some(group))
                    .map(|workspace| workspace.workspace_id.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The workspace plus, for a repo parent, its linked worktrees in the same group.
    pub(super) fn workspace_with_worktree_children(&self, workspace_id: &str) -> Vec<String> {
        let Some(snapshot) = self.snapshot.as_deref() else {
            return vec![workspace_id.to_owned()];
        };
        let Some(parent) = snapshot
            .workspaces
            .iter()
            .find(|workspace| workspace.workspace_id == workspace_id)
        else {
            return vec![workspace_id.to_owned()];
        };
        std::iter::once(workspace_id.to_owned())
            .chain(
                parent
                    .worktree
                    .as_ref()
                    .filter(|worktree| !worktree.is_linked_worktree)
                    .into_iter()
                    .flat_map(|worktree| {
                        snapshot.workspaces.iter().filter(move |candidate| {
                            candidate.group == parent.group
                                && candidate.worktree.as_ref().is_some_and(|candidate| {
                                    candidate.key == worktree.key && candidate.is_linked_worktree
                                })
                        })
                    })
                    .map(|workspace| workspace.workspace_id.clone()),
            )
            .collect()
    }

    pub(super) fn open_tab_context_menu(&mut self, tab_id: String, x: u16, y: u16) {
        let Some(tab) = self
            .snapshot
            .as_deref()
            .and_then(|snapshot| snapshot.tabs.iter().find(|tab| tab.tab_id == tab_id))
        else {
            return;
        };
        self.overlay = Some(ClientShellOverlay::ContextMenu(ClientContextMenuOverlay {
            target: ClientContextMenuTarget::Tab {
                tab_id,
                workspace_id: tab.workspace_id.clone(),
            },
            x,
            y,
            highlighted: 0,
        }));
    }

    pub(super) fn open_pane_context_menu(&mut self, pane_id: String, x: u16, y: u16) {
        let Some(snapshot) = self.snapshot.as_deref() else {
            return;
        };
        let Some(pane) = snapshot.panes.iter().find(|pane| pane.pane_id == pane_id) else {
            return;
        };
        let source_pane_id = snapshot
            .focused_pane_id
            .clone()
            .filter(|focused| focused != &pane_id);
        let skip_agent_resume = self
            .supports_endpoint_method(&crate::api::schema::Method::PaneSetAgentResume(
                crate::api::schema::PaneSetAgentResumeParams {
                    pane_id: pane_id.clone(),
                    enabled: true,
                },
            ))
            .then_some(pane.skip_agent_resume);
        self.overlay = Some(ClientShellOverlay::ContextMenu(ClientContextMenuOverlay {
            target: ClientContextMenuTarget::Pane {
                pane_id,
                workspace_id: pane.workspace_id.clone(),
                source_pane_id,
                has_manual_label: pane.label.is_some(),
                right_click_passthrough: pane.right_click_passthrough,
                skip_agent_resume,
            },
            x,
            y,
            highlighted: 0,
        }));
    }

    pub(super) fn move_context_menu_selection(&mut self, delta: isize) {
        let Some(ClientShellOverlay::ContextMenu(menu)) = self.overlay.as_mut() else {
            return;
        };
        let item_count = menu.items().len();
        if item_count == 0 {
            return;
        }
        menu.highlighted = (menu.highlighted as isize + delta)
            .clamp(0, item_count.saturating_sub(1) as isize) as usize;
    }

    pub(super) fn activate_context_menu_item(
        &mut self,
        index: usize,
        outcome: &mut ClientShellInput,
    ) {
        let Some(ClientShellOverlay::ContextMenu(menu)) = self.overlay.take() else {
            return;
        };
        let Some(action) = menu.items().get(index).map(|item| item.action) else {
            outcome.repaint = true;
            return;
        };
        match menu.target {
            ClientContextMenuTarget::Workspace { workspace_id, .. }
                if action == ClientContextMenuAction::MoveToGroup =>
            {
                self.open_group_picker(workspace_id, menu.x, menu.y)
            }
            ClientContextMenuTarget::Workspace {
                agent_resume_workspace_ids,
                skip_agent_resume,
                ..
            } if action == ClientContextMenuAction::ToggleAgentResume => self.push_endpoint_method(
                crate::api::schema::Method::WorkspaceSetAgentResume(
                    crate::api::schema::WorkspaceSetAgentResumeParams {
                        workspace_ids: agent_resume_workspace_ids,
                        enabled: skip_agent_resume.unwrap_or_default(),
                    },
                ),
                outcome,
            ),
            ClientContextMenuTarget::Pane {
                pane_id,
                skip_agent_resume,
                ..
            } if action == ClientContextMenuAction::ToggleAgentResume => self.push_endpoint_method(
                crate::api::schema::Method::PaneSetAgentResume(
                    crate::api::schema::PaneSetAgentResumeParams {
                        pane_id,
                        enabled: skip_agent_resume.unwrap_or_default(),
                    },
                ),
                outcome,
            ),
            ClientContextMenuTarget::Workspace {
                workspace_id,
                close_group,
                ..
            } => self.activate_workspace_context_action(workspace_id, close_group, action, outcome),
            ClientContextMenuTarget::GroupPicker {
                workspace_ids,
                groups,
            } => match action {
                ClientContextMenuAction::PickGroup(index) => {
                    if let Some(group) = groups.get(index) {
                        self.push_set_group(workspace_ids, Some(group.clone()), outcome);
                    }
                }
                ClientContextMenuAction::NewGroup => self.open_new_group_prompt(workspace_ids),
                _ => {}
            },
            ClientContextMenuTarget::WorkspaceGroup { name, .. } => {
                self.activate_group_context_action(name, action, outcome)
            }
            ClientContextMenuTarget::Tab {
                tab_id,
                workspace_id,
            } => self.activate_tab_context_action(tab_id, workspace_id, action, outcome),
            ClientContextMenuTarget::Pane {
                pane_id,
                workspace_id,
                source_pane_id,
                right_click_passthrough,
                ..
            } => self.activate_pane_context_action(
                pane_id,
                workspace_id,
                source_pane_id,
                right_click_passthrough,
                action,
                outcome,
            ),
        }
        outcome.repaint = true;
    }

    fn activate_workspace_context_action(
        &mut self,
        workspace_id: String,
        close_group: bool,
        action: ClientContextMenuAction,
        outcome: &mut ClientShellInput,
    ) {
        use crate::input::KeybindAction;

        match action {
            ClientContextMenuAction::Rename => {
                let label = self
                    .snapshot
                    .as_deref()
                    .and_then(|snapshot| {
                        snapshot
                            .workspaces
                            .iter()
                            .find(|workspace| workspace.workspace_id == workspace_id)
                    })
                    .map(|workspace| workspace.label.clone());
                if let Some(label) = label {
                    self.overlay = Some(ClientShellOverlay::Rename(ClientRenameOverlay {
                        title: "rename workspace",
                        input: TextEditor::new(&label, false),
                        target: ClientRenameTarget::Workspace { workspace_id },
                    }));
                }
            }
            ClientContextMenuAction::Close => {
                self.request_workspace_close(workspace_id, Some(close_group), outcome);
            }
            ClientContextMenuAction::NewWorktree => {
                self.begin_worktree_action_for(KeybindAction::NewWorktree, workspace_id, outcome)
            }
            ClientContextMenuAction::OpenWorktree => {
                self.begin_worktree_action_for(KeybindAction::OpenWorktree, workspace_id, outcome)
            }
            ClientContextMenuAction::RemoveWorktree => {
                self.begin_worktree_action_for(KeybindAction::RemoveWorktree, workspace_id, outcome)
            }
            ClientContextMenuAction::ToggleGroup => {
                let key = self.snapshot.as_deref().and_then(|snapshot| {
                    snapshot
                        .workspaces
                        .iter()
                        .find(|workspace| workspace.workspace_id == workspace_id)
                        .and_then(|workspace| workspace.worktree.as_ref())
                        .map(|worktree| worktree.key.clone())
                });
                if let Some(key) = key {
                    let endpoint_id = self.active_endpoint_id.clone();
                    self.toggle_collapsed_group(&endpoint_id, key);
                    self.persist_chrome_preferences(outcome);
                }
            }
            ClientContextMenuAction::RemoveFromGroup => {
                let workspace_ids = self.workspace_with_worktree_children(&workspace_id);
                self.push_set_group(workspace_ids, None, outcome);
            }
            _ => {}
        }
    }

    fn activate_group_context_action(
        &mut self,
        name: String,
        action: ClientContextMenuAction,
        outcome: &mut ClientShellInput,
    ) {
        match action {
            ClientContextMenuAction::Rename => {
                self.overlay = Some(ClientShellOverlay::Rename(ClientRenameOverlay {
                    title: "rename group",
                    input: TextEditor::new(&name, false),
                    target: ClientRenameTarget::GroupName {
                        workspace_ids: self.group_member_ids(&name),
                        name,
                    },
                }));
            }
            ClientContextMenuAction::Ungroup => {
                let workspace_ids = self.group_member_ids(&name);
                self.push_set_group(workspace_ids, None, outcome);
            }
            ClientContextMenuAction::ToggleGroup => {
                let endpoint_id = self.active_endpoint_id.clone();
                self.toggle_collapsed_group(
                    &endpoint_id,
                    super::sidebar::group_collapse_key(&name),
                );
                self.persist_chrome_preferences(outcome);
            }
            _ => {}
        }
    }

    pub(super) fn push_set_group(
        &mut self,
        workspace_ids: Vec<String>,
        group: Option<String>,
        outcome: &mut ClientShellInput,
    ) {
        if workspace_ids.is_empty() {
            return;
        }
        self.push_endpoint_method(
            crate::api::schema::Method::WorkspaceSetGroup(
                crate::api::schema::WorkspaceSetGroupParams {
                    workspace_ids,
                    group,
                },
            ),
            outcome,
        );
    }

    fn activate_tab_context_action(
        &mut self,
        tab_id: String,
        workspace_id: String,
        action: ClientContextMenuAction,
        outcome: &mut ClientShellInput,
    ) {
        use crate::api::schema::{Method, TabTarget};

        self.push_endpoint_method(
            Method::TabFocus(TabTarget {
                tab_id: tab_id.clone(),
            }),
            outcome,
        );
        match action {
            ClientContextMenuAction::NewTab => {
                if self.config.prompt_new_tab_name {
                    let default_name = (self
                        .snapshot
                        .as_deref()
                        .map(|snapshot| {
                            snapshot
                                .tabs
                                .iter()
                                .filter(|tab| tab.workspace_id == workspace_id)
                                .count()
                        })
                        .unwrap_or(0)
                        + 1)
                    .to_string();
                    self.overlay = Some(ClientShellOverlay::Rename(ClientRenameOverlay {
                        title: "new tab",
                        input: TextEditor::new(&default_name, true),
                        target: ClientRenameTarget::NewTab {
                            workspace_id,
                            default_name,
                        },
                    }));
                } else {
                    self.push_endpoint_method(
                        Method::TabCreate(crate::api::schema::TabCreateParams {
                            workspace_id: Some(workspace_id),
                            cwd: None,
                            focus: true,
                            label: None,
                            env: Default::default(),
                        }),
                        outcome,
                    );
                }
            }
            ClientContextMenuAction::Rename => {
                let tab = self
                    .snapshot
                    .as_deref()
                    .and_then(|snapshot| snapshot.tabs.iter().find(|tab| tab.tab_id == tab_id));
                if let Some(tab) = tab {
                    self.overlay = Some(ClientShellOverlay::Rename(ClientRenameOverlay {
                        title: "rename tab",
                        input: TextEditor::new(&tab.label, false),
                        target: ClientRenameTarget::Tab {
                            tab_id,
                            auto_name: !tab.custom_label,
                            original_name: tab.label.clone(),
                        },
                    }));
                }
            }
            ClientContextMenuAction::Close => {
                self.request_tab_close(tab_id, outcome);
            }
            _ => {}
        }
    }

    fn activate_pane_context_action(
        &mut self,
        pane_id: String,
        workspace_id: String,
        source_pane_id: Option<String>,
        right_click_passthrough: bool,
        action: ClientContextMenuAction,
        outcome: &mut ClientShellInput,
    ) {
        use crate::api::schema::{
            Method, PaneInputSetParams, PaneRenameParams, PaneRightClickTarget, PaneSplitParams,
            PaneSwapParams, PaneTarget, PaneZoomMode, PaneZoomParams, SplitDirection,
        };

        match action {
            ClientContextMenuAction::RenamePane => {
                let label = self.snapshot.as_deref().and_then(|snapshot| {
                    snapshot
                        .panes
                        .iter()
                        .find(|pane| pane.pane_id == pane_id)
                        .and_then(|pane| pane.label.clone())
                });
                self.overlay = Some(ClientShellOverlay::Rename(ClientRenameOverlay {
                    title: "rename pane",
                    input: TextEditor::new(label.as_deref().unwrap_or_default(), label.is_none()),
                    target: ClientRenameTarget::Pane { pane_id },
                }));
            }
            ClientContextMenuAction::ClearPaneName => self.push_endpoint_method(
                Method::PaneRename(PaneRenameParams {
                    pane_id,
                    label: None,
                }),
                outcome,
            ),
            ClientContextMenuAction::SwapWithFocusedPane => {
                if let Some(source_pane_id) = source_pane_id {
                    self.push_endpoint_method(
                        Method::PaneSwap(PaneSwapParams {
                            pane_id: None,
                            direction: None,
                            source_pane_id: Some(source_pane_id.clone()),
                            target_pane_id: Some(pane_id),
                        }),
                        outcome,
                    );
                    self.push_endpoint_method(
                        Method::PaneFocus(PaneTarget {
                            pane_id: source_pane_id,
                        }),
                        outcome,
                    );
                }
            }
            ClientContextMenuAction::SplitRight | ClientContextMenuAction::SplitDown => {
                self.push_endpoint_method(
                    Method::PaneSplit(PaneSplitParams {
                        workspace_id: Some(workspace_id),
                        target_pane_id: Some(pane_id),
                        direction: if action == ClientContextMenuAction::SplitRight {
                            SplitDirection::Right
                        } else {
                            SplitDirection::Down
                        },
                        ratio: None,
                        cwd: None,
                        focus: true,
                        right_click: Default::default(),
                        env: Default::default(),
                    }),
                    outcome,
                );
            }
            ClientContextMenuAction::Zoom => self.push_endpoint_method(
                Method::PaneZoom(PaneZoomParams {
                    pane_id: Some(pane_id),
                    mode: PaneZoomMode::Toggle,
                }),
                outcome,
            ),
            ClientContextMenuAction::ToggleRightClickPassthrough => self.push_endpoint_method(
                Method::PaneInputSet(PaneInputSetParams {
                    pane_id,
                    right_click: if right_click_passthrough {
                        PaneRightClickTarget::Herdr
                    } else {
                        PaneRightClickTarget::Pane
                    },
                }),
                outcome,
            ),
            ClientContextMenuAction::ClosePane => self.request_pane_close(pane_id, outcome),
            _ => {}
        }
    }
}
