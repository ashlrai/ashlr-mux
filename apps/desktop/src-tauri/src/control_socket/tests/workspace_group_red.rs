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
