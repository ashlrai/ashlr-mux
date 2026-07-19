use super::*;

const CANONICAL_WORKSPACE_GROUP_METHODS: [&str; 17] = [
    "workspace.group.list",
    "workspace.group.create",
    "workspace.group.ungroup",
    "workspace.group.delete",
    "workspace.group.rename",
    "workspace.group.collapse",
    "workspace.group.expand",
    "workspace.group.pin",
    "workspace.group.unpin",
    "workspace.group.add",
    "workspace.group.remove",
    "workspace.group.set_anchor",
    "workspace.group.new_workspace",
    "workspace.group.set_color",
    "workspace.group.set_icon",
    "workspace.group.move",
    "workspace.group.focus",
];

#[test]
fn canonical_workspace_group_family_is_first_class_and_complete() {
    for method in CANONICAL_WORKSPACE_GROUP_METHODS {
        assert!(
            CONTROL_SOCKET_METHODS.contains(&method),
            "canonical workspace-group route is not advertised: {method}"
        );
    }
}

#[test]
fn canonical_workspace_group_family_does_not_depend_on_the_legacy_collapse_alias() {
    assert!(CONTROL_SOCKET_METHODS.contains(&"workspace.group.collapse"));
    assert!(CONTROL_SOCKET_METHODS.contains(&"workspace.group.expand"));
}

#[test]
fn canonical_workspace_group_payload_has_exact_keys_ordered_members_and_nullable_metadata() {
    let mut snapshot = test_snapshot();
    let tabs = &mut snapshot.windows[0].tab_manager;
    let first_id = "11111111-1111-4111-8111-111111111111";
    let second_id = "22222222-2222-4222-8222-222222222222";
    let group_id = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
    tabs.workspaces[0].workspace_id = Some(first_id.into());
    tabs.workspaces[0].group_id = Some(group_id.into());
    let mut second = tabs.workspaces[0].clone();
    second.workspace_id = Some(second_id.into());
    tabs.workspaces.push(second);
    let group = SessionWorkspaceGroupSnapshot {
        id: group_id.into(),
        name: "Backend".into(),
        is_collapsed: true,
        anchor_workspace_id: Some(second_id.into()),
        anchor_member_index: None,
        is_pinned: Some(true),
        custom_color: None,
        icon_symbol: None,
    };

    let payload = workspace_group_payload_with(&snapshot.windows[0], &group, &mut |kind, id| {
        format!("{kind}:{id}")
    });

    assert_eq!(
        payload,
        json!({
            "id": group_id,
            "ref": format!("workspace_group:{group_id}"),
            "name": "Backend",
            "is_collapsed": true,
            "is_pinned": true,
            "anchor_workspace_id": second_id,
            "anchor_workspace_ref": format!("workspace:{second_id}"),
            "custom_color": null,
            "icon_symbol": null,
            "member_workspace_ids": [first_id, second_id],
            "member_workspace_refs": [
                format!("workspace:{first_id}"),
                format!("workspace:{second_id}"),
            ],
            "member_count": 2,
        })
    );
}

#[test]
fn workspace_group_remove_requires_only_the_workspace_id() {
    let params = serde_json::Map::from_iter([(
        "workspace_id".to_string(),
        json!("22222222-2222-4222-8222-222222222222"),
    )]);

    assert_eq!(
        workspace_group_required_param_error("workspace.group.remove", &params),
        None,
        "canonical workspace.group.remove resolves the owning group from workspace_id"
    );
}

#[test]
fn workspace_group_reorder_members_follow_workspace_order() {
    let mut snapshot = test_snapshot();
    let group_id = Uuid::parse_str("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa").unwrap();
    snapshot.windows[0].tab_manager.workspaces[0].workspace_id =
        Some("11111111-1111-4111-8111-111111111111".into());
    snapshot.windows[0].tab_manager.workspaces[0].group_id = Some(group_id.to_string());
    let mut second = snapshot.windows[0].tab_manager.workspaces[0].clone();
    second.workspace_id = Some("22222222-2222-4222-8222-222222222222".into());
    snapshot.windows[0].tab_manager.workspaces.push(second);

    assert_eq!(
        workspace_group_member_ids(&snapshot, 0, group_id),
        [
            "11111111-1111-4111-8111-111111111111",
            "22222222-2222-4222-8222-222222222222",
        ]
    );
}

#[test]
fn workspace_group_anchor_materialization_matches_canonical_event_order() {
    let snapshot = test_snapshot();
    let events = workspace_group_created_event_specs(&snapshot, 0, 0)
        .expect("workspace with an initial terminal surface");
    assert_eq!(
        events.iter().map(|event| event.name).collect::<Vec<_>>(),
        [
            "surface.selected",
            "pane.focused",
            "surface.focused",
            "workspace.created",
            "surface.created",
        ]
    );
    let created = events
        .iter()
        .find(|event| event.name == "workspace.created")
        .unwrap();
    assert_eq!(created.payload["selected"], json!(false));
    let surface = events
        .iter()
        .find(|event| event.name == "surface.created")
        .unwrap();
    assert_eq!(surface.payload["focused"], json!(false));
}

#[test]
fn workspace_group_delete_closes_non_anchor_members_before_the_anchor() {
    let mut snapshot = test_snapshot();
    let group_id = Uuid::parse_str("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa").unwrap();
    let anchor_id = "11111111-1111-4111-8111-111111111111";
    let member_id = "22222222-2222-4222-8222-222222222222";
    snapshot.windows[0].tab_manager.workspaces[0].workspace_id = Some(anchor_id.into());
    snapshot.windows[0].tab_manager.workspaces[0].group_id = Some(group_id.to_string());
    let mut member = snapshot.windows[0].tab_manager.workspaces[0].clone();
    member.workspace_id = Some(member_id.into());
    snapshot.windows[0].tab_manager.workspaces.push(member);
    snapshot.windows[0].tab_manager.workspace_groups = Some(vec![SessionWorkspaceGroupSnapshot {
        id: group_id.to_string(),
        name: "Backend".into(),
        is_collapsed: false,
        anchor_workspace_id: Some(anchor_id.into()),
        anchor_member_index: None,
        is_pinned: Some(false),
        custom_color: None,
        icon_symbol: None,
    }]);

    assert_eq!(
        workspace_group_delete_member_ids(&snapshot, 0, group_id),
        [member_id, anchor_id]
    );
}
