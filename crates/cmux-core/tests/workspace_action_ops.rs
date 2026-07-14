use cmux_core::session::{
    SessionTabManagerSnapshot, SessionWorkspaceGroupSnapshot, SessionWorkspaceSnapshot,
};
use cmux_core::session_ops::{move_workspace_to_top, set_workspace_color};

const GROUP: &str = "90000000-0000-4000-8000-000000000001";

fn workspace(id: &str, pinned: bool, group_id: Option<&str>) -> SessionWorkspaceSnapshot {
    SessionWorkspaceSnapshot {
        workspace_id: Some(id.to_owned()),
        process_title: id.to_owned(),
        is_pinned: pinned.then_some(true),
        group_id: group_id.map(str::to_owned),
        ..Default::default()
    }
}

fn ids(tabs: &SessionTabManagerSnapshot) -> Vec<&str> {
    tabs.workspaces
        .iter()
        .map(|workspace| workspace.workspace_id.as_deref().unwrap())
        .collect()
}

#[test]
fn move_top_uses_the_current_pin_tier_and_preserves_selected_identity() {
    let p1 = "10000000-0000-4000-8000-000000000001";
    let p2 = "10000000-0000-4000-8000-000000000002";
    let u1 = "20000000-0000-4000-8000-000000000001";
    let target = "20000000-0000-4000-8000-000000000002";
    let u2 = "20000000-0000-4000-8000-000000000003";
    let mut tabs = SessionTabManagerSnapshot {
        selected_workspace_index: Some(3),
        workspaces: vec![
            workspace(p1, true, None),
            workspace(p2, true, None),
            workspace(u1, false, None),
            workspace(target, false, None),
            workspace(u2, false, None),
        ],
        workspace_groups: None,
    };

    assert!(move_workspace_to_top(&mut tabs, 3));
    assert_eq!(ids(&tabs), [p1, p2, target, u1, u2]);
    assert_eq!(tabs.selected_workspace_index, Some(2));

    assert!(move_workspace_to_top(&mut tabs, 1));
    assert_eq!(ids(&tabs), [p2, p1, target, u1, u2]);
    assert_eq!(tabs.selected_workspace_index, Some(2));
}

#[test]
fn move_top_of_group_member_hoists_the_whole_group_after_pinned_rows() {
    let pinned = "10000000-0000-4000-8000-000000000001";
    let solo = "20000000-0000-4000-8000-000000000001";
    let anchor = "30000000-0000-4000-8000-000000000001";
    let member = "30000000-0000-4000-8000-000000000002";
    let tail = "40000000-0000-4000-8000-000000000001";
    let mut tabs = SessionTabManagerSnapshot {
        selected_workspace_index: Some(3),
        workspaces: vec![
            workspace(pinned, true, None),
            workspace(solo, false, None),
            workspace(anchor, false, Some(GROUP)),
            workspace(member, false, Some(GROUP)),
            workspace(tail, false, None),
        ],
        workspace_groups: Some(vec![SessionWorkspaceGroupSnapshot {
            id: GROUP.into(),
            name: "group".into(),
            anchor_workspace_id: Some(anchor.into()),
            anchor_member_index: Some(0),
            ..Default::default()
        }]),
    };

    assert!(move_workspace_to_top(&mut tabs, 3));
    assert_eq!(ids(&tabs), [pinned, anchor, member, solo, tail]);
    assert_eq!(tabs.selected_workspace_index, Some(2));
    assert_eq!(tabs.workspaces[1].group_id.as_deref(), Some(GROUP));
    assert_eq!(tabs.workspaces[2].group_id.as_deref(), Some(GROUP));
}

#[test]
fn color_writer_is_change_gated_and_preserves_normalized_value_verbatim() {
    let id = "10000000-0000-4000-8000-000000000001";
    let mut tabs = SessionTabManagerSnapshot {
        workspaces: vec![workspace(id, false, None)],
        ..Default::default()
    };
    assert!(set_workspace_color(&mut tabs, 0, Some("#ABCDEF")));
    assert_eq!(tabs.workspaces[0].custom_color.as_deref(), Some("#ABCDEF"));
    assert!(!set_workspace_color(&mut tabs, 0, Some("#ABCDEF")));
    assert!(set_workspace_color(&mut tabs, 0, None));
    assert_eq!(tabs.workspaces[0].custom_color, None);
}
