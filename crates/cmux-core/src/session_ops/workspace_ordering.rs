use super::*;

/// Placement of an existing or freshly-created workspace inside a group.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceGroupPlacement {
    /// Immediately after a supplied reference member, or after the anchor
    /// when no reference is supplied.
    AfterCurrent,
    /// Immediately after the anchor.
    Top,
    /// After the group's last member.
    End,
}

/// Pure-model failures that command adapters can translate into their own
/// transport error vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceGroupMutationError {
    GroupNotFound,
    GroupAlreadyExists,
    AnchorWorkspaceNotFound,
    WorkspaceNotFound,
    WorkspaceNotGrouped,
    WorkspaceNotGroupMember,
    WorkspaceIsOtherGroupAnchor,
    InvalidReferenceWorkspace,
}

fn workspace_snapshot_index(tabs: &SessionTabManagerSnapshot, workspace_id: Uuid) -> Option<usize> {
    tabs.workspaces.iter().position(|workspace| {
        workspace
            .workspace_id
            .as_deref()
            .and_then(|raw| Uuid::parse_str(raw).ok())
            == Some(workspace_id)
    })
}

fn workspace_group_snapshot_index(
    tabs: &SessionTabManagerSnapshot,
    group_id: Uuid,
) -> Option<usize> {
    tabs.workspace_groups
        .as_deref()?
        .iter()
        .position(|group| Uuid::parse_str(&group.id).ok() == Some(group_id))
}

fn selected_workspace_uuid(tabs: &SessionTabManagerSnapshot) -> Option<Uuid> {
    let index = usize::try_from(tabs.selected_workspace_index?).ok()?;
    tabs.workspaces
        .get(index)?
        .workspace_id
        .as_deref()
        .and_then(|raw| Uuid::parse_str(raw).ok())
}

fn restore_selected_workspace_uuid(
    tabs: &mut SessionTabManagerSnapshot,
    selected_workspace_id: Option<Uuid>,
) {
    let Some(selected_workspace_id) = selected_workspace_id else {
        return;
    };
    if let Some(index) = workspace_snapshot_index(tabs, selected_workspace_id) {
        tabs.selected_workspace_index = Some(index as i64);
    }
}

fn resolved_workspace_group_name(tabs: &SessionTabManagerSnapshot, requested: &str) -> String {
    let trimmed = requested.trim();
    if !trimmed.is_empty() {
        return trimmed.to_string();
    }
    let groups = tabs.workspace_groups.as_deref().unwrap_or(&[]);
    let used: HashSet<&str> = groups.iter().map(|group| group.name.as_str()).collect();
    let mut number = groups.len() + 1;
    loop {
        let candidate = format!("Group {number}");
        if !used.contains(candidate.as_str()) {
            return candidate;
        }
        number += 1;
    }
}

/// Form a new group around an already-created fresh anchor workspace.
///
/// This is the pure snapshot half of canonical `createWorkspaceGroup`: the
/// host creates the anchor/runtime first, then calls this operation with its
/// identity. Existing pinned workspaces and anchors of other groups are
/// ineligible and are silently skipped, matching the coordinator. Eligible
/// children are adopted in tab order, and the new contiguous anchor-first run
/// occupies the first eligible child's former top-level slot. An empty eligible
/// set leaves an anchor-only group in the normal unpinned tier. Selection is
/// preserved by workspace identity.
pub fn create_workspace_group_snapshot(
    tabs: &mut SessionTabManagerSnapshot,
    group_id: Uuid,
    name: &str,
    anchor_workspace_id: Uuid,
    child_workspace_ids: &[Uuid],
) -> Result<SessionWorkspaceGroupSnapshot, WorkspaceGroupMutationError> {
    if workspace_group_snapshot_index(tabs, group_id).is_some() {
        return Err(WorkspaceGroupMutationError::GroupAlreadyExists);
    }
    let anchor_index = workspace_snapshot_index(tabs, anchor_workspace_id)
        .ok_or(WorkspaceGroupMutationError::AnchorWorkspaceNotFound)?;
    let selected_workspace_id = selected_workspace_uuid(tabs);

    let original_tab_order: Vec<Uuid> = tabs
        .workspaces
        .iter()
        .filter_map(|workspace| {
            workspace
                .workspace_id
                .as_deref()
                .and_then(|raw| Uuid::parse_str(raw).ok())
        })
        .filter(|id| *id != anchor_workspace_id)
        .collect();
    let existing_anchor_ids: HashSet<Uuid> = tabs
        .workspace_groups
        .as_deref()
        .unwrap_or(&[])
        .iter()
        .filter_map(|group| {
            group
                .anchor_workspace_id
                .as_deref()
                .and_then(|raw| Uuid::parse_str(raw).ok())
        })
        .collect();
    let requested: HashSet<Uuid> = child_workspace_ids.iter().copied().collect();
    let eligible_children: Vec<Uuid> = original_tab_order
        .iter()
        .copied()
        .filter(|id| requested.contains(id))
        .filter(|id| !existing_anchor_ids.contains(id))
        .filter(|id| {
            workspace_snapshot_index(tabs, *id)
                .is_some_and(|index| tabs.workspaces[index].is_pinned != Some(true))
        })
        .collect();

    let group_id_string = group_id.to_string();
    tabs.workspaces[anchor_index].group_id = Some(group_id_string.clone());
    for child_id in &eligible_children {
        if let Some(index) = workspace_snapshot_index(tabs, *child_id) {
            tabs.workspaces[index].group_id = Some(group_id_string.clone());
        }
    }
    let created = SessionWorkspaceGroupSnapshot {
        id: group_id_string,
        name: resolved_workspace_group_name(tabs, name),
        is_collapsed: false,
        anchor_workspace_id: Some(anchor_workspace_id.to_string()),
        anchor_member_index: None,
        is_pinned: None,
        custom_color: None,
        icon_symbol: None,
    };
    tabs.workspace_groups
        .get_or_insert_with(Vec::new)
        .push(created.clone());

    let (rows, groups) = workspace_mirror(tabs);
    let original_row_ids: Vec<Uuid> = rows.iter().map(|row| row.id).collect();
    let (final_rows, final_groups) = if let Some(first_child) = eligible_children.first() {
        let child_set: HashSet<Uuid> = eligible_children.iter().copied().collect();
        let mut desired = Vec::with_capacity(rows.len());
        for id in &original_tab_order {
            if id == first_child {
                desired.push(anchor_workspace_id);
                desired.extend(eligible_children.iter().copied());
            }
            if !child_set.contains(id) {
                desired.push(*id);
            }
        }
        let preferred = top_level_workspace_ids_preserving_order(&rows, &groups, &desired);
        normalize_workspace_group_contiguity(&rows, &groups, Some(&preferred))
    } else {
        normalize_workspace_group_contiguity(&rows, &groups, None)
    };
    let final_row_ids: Vec<Uuid> = final_rows.iter().map(|row| row.id).collect();
    let final_group_ids: Vec<Uuid> = final_groups.iter().map(|group| group.id).collect();
    if final_row_ids != original_row_ids {
        write_back_reordered(tabs, &original_row_ids, &final_row_ids, &final_group_ids);
    } else if let Some(groups) = tabs.workspace_groups.as_mut() {
        let order: HashMap<Uuid, usize> = final_group_ids
            .iter()
            .enumerate()
            .map(|(index, id)| (*id, index))
            .collect();
        groups.sort_by_key(|group| {
            Uuid::parse_str(&group.id)
                .ok()
                .and_then(|id| order.get(&id).copied())
                .unwrap_or(usize::MAX)
        });
    }
    restore_selected_workspace_uuid(tabs, selected_workspace_id);
    Ok(created)
}

/// Dissolve a group in place, preserving all member workspaces and their exact
/// row positions. Returns member ids in row order for host event emission.
pub fn ungroup_workspace_group_snapshot(
    tabs: &mut SessionTabManagerSnapshot,
    group_id: Uuid,
) -> Result<Vec<Uuid>, WorkspaceGroupMutationError> {
    let group_index = workspace_group_snapshot_index(tabs, group_id)
        .ok_or(WorkspaceGroupMutationError::GroupNotFound)?;
    let member_ids: Vec<Uuid> = tabs
        .workspaces
        .iter()
        .filter(|workspace| {
            workspace
                .group_id
                .as_deref()
                .and_then(|raw| Uuid::parse_str(raw).ok())
                == Some(group_id)
        })
        .filter_map(|workspace| {
            workspace
                .workspace_id
                .as_deref()
                .and_then(|raw| Uuid::parse_str(raw).ok())
        })
        .collect();
    for workspace in &mut tabs.workspaces {
        if workspace
            .group_id
            .as_deref()
            .and_then(|raw| Uuid::parse_str(raw).ok())
            == Some(group_id)
        {
            workspace.group_id = None;
        }
    }
    tabs.workspace_groups
        .as_mut()
        .expect("group index implies group storage")
        .remove(group_index);
    Ok(member_ids)
}

/// Rename a group after canonical whitespace trimming. A blank name is a
/// successful no-op; `Ok(false)` also represents an already-equal value.
pub fn rename_workspace_group_snapshot(
    tabs: &mut SessionTabManagerSnapshot,
    group_id: Uuid,
    name: &str,
) -> Result<bool, WorkspaceGroupMutationError> {
    let index = workspace_group_snapshot_index(tabs, group_id)
        .ok_or(WorkspaceGroupMutationError::GroupNotFound)?;
    let trimmed = name.trim();
    if trimmed.is_empty() || tabs.workspace_groups.as_ref().unwrap()[index].name == trimmed {
        return Ok(false);
    }
    tabs.workspace_groups.as_mut().unwrap()[index].name = trimmed.to_string();
    Ok(true)
}

/// Set a group's pin state and restore canonical group contiguity/pin tiers.
pub fn set_workspace_group_pinned_snapshot(
    tabs: &mut SessionTabManagerSnapshot,
    group_id: Uuid,
    pinned: bool,
) -> Result<bool, WorkspaceGroupMutationError> {
    let index = workspace_group_snapshot_index(tabs, group_id)
        .ok_or(WorkspaceGroupMutationError::GroupNotFound)?;
    if (tabs.workspace_groups.as_ref().unwrap()[index].is_pinned == Some(true)) == pinned {
        return Ok(false);
    }
    tabs.workspace_groups.as_mut().unwrap()[index].is_pinned = pinned.then_some(true);
    normalize_workspace_groups_in_snapshot(tabs);
    Ok(true)
}

/// Set or clear a group's exact custom color value.
pub fn set_workspace_group_color_snapshot(
    tabs: &mut SessionTabManagerSnapshot,
    group_id: Uuid,
    color: Option<String>,
) -> Result<bool, WorkspaceGroupMutationError> {
    let index = workspace_group_snapshot_index(tabs, group_id)
        .ok_or(WorkspaceGroupMutationError::GroupNotFound)?;
    let group = &mut tabs.workspace_groups.as_mut().unwrap()[index];
    if group.custom_color == color {
        return Ok(false);
    }
    group.custom_color = color;
    Ok(true)
}

/// Store the host-normalized group icon symbol (or clear it).
pub fn set_workspace_group_icon_snapshot(
    tabs: &mut SessionTabManagerSnapshot,
    group_id: Uuid,
    icon_symbol: Option<String>,
) -> Result<bool, WorkspaceGroupMutationError> {
    let index = workspace_group_snapshot_index(tabs, group_id)
        .ok_or(WorkspaceGroupMutationError::GroupNotFound)?;
    let group = &mut tabs.workspace_groups.as_mut().unwrap()[index];
    if group.icon_symbol == icon_symbol {
        return Ok(false);
    }
    group.icon_symbol = icon_symbol;
    Ok(true)
}

fn place_workspace_within_group(
    tabs: &mut SessionTabManagerSnapshot,
    workspace_id: Uuid,
    group_id: Uuid,
    placement: WorkspaceGroupPlacement,
    reference_workspace_id: Option<Uuid>,
) {
    let Some(current_index) = workspace_snapshot_index(tabs, workspace_id) else {
        return;
    };
    let Some(group_index) = workspace_group_snapshot_index(tabs, group_id) else {
        return;
    };
    let anchor_id = tabs.workspace_groups.as_ref().unwrap()[group_index]
        .anchor_workspace_id
        .as_deref()
        .and_then(|raw| Uuid::parse_str(raw).ok());
    let member_indices: Vec<usize> = tabs
        .workspaces
        .iter()
        .enumerate()
        .filter(|(index, workspace)| {
            *index != current_index
                && workspace
                    .group_id
                    .as_deref()
                    .and_then(|raw| Uuid::parse_str(raw).ok())
                    == Some(group_id)
        })
        .map(|(index, _)| index)
        .collect();
    let target_index = match placement {
        WorkspaceGroupPlacement::AfterCurrent => reference_workspace_id
            .and_then(|reference| workspace_snapshot_index(tabs, reference))
            .map(|index| index + 1)
            .or_else(|| anchor_id.and_then(|anchor| workspace_snapshot_index(tabs, anchor)))
            .map(|index| index + 1)
            .or_else(|| member_indices.first().copied()),
        WorkspaceGroupPlacement::Top => anchor_id
            .and_then(|anchor| workspace_snapshot_index(tabs, anchor))
            .map(|index| index + 1)
            .or_else(|| member_indices.first().copied()),
        WorkspaceGroupPlacement::End => member_indices
            .last()
            .copied()
            .map(|index| index + 1)
            .or_else(|| {
                anchor_id
                    .and_then(|anchor| workspace_snapshot_index(tabs, anchor))
                    .map(|index| index + 1)
            }),
    };
    let Some(target_index) = target_index else {
        return;
    };
    if current_index == target_index {
        return;
    }
    let selected_workspace_id = selected_workspace_uuid(tabs);
    let workspace = tabs.workspaces.remove(current_index);
    let insert_at = if current_index < target_index {
        target_index - 1
    } else {
        target_index
    }
    .min(tabs.workspaces.len());
    tabs.workspaces.insert(insert_at, workspace);
    restore_selected_workspace_uuid(tabs, selected_workspace_id);
}

/// Add an existing workspace as a non-anchor group member. The move preserves
/// the group's former top-level slot, rejects anchors of other groups, and
/// optionally places the member at a canonical in-group position.
pub fn add_workspace_to_group_snapshot(
    tabs: &mut SessionTabManagerSnapshot,
    group_id: Uuid,
    workspace_id: Uuid,
    placement: Option<WorkspaceGroupPlacement>,
    reference_workspace_id: Option<Uuid>,
) -> Result<bool, WorkspaceGroupMutationError> {
    if workspace_group_snapshot_index(tabs, group_id).is_none() {
        return Err(WorkspaceGroupMutationError::GroupNotFound);
    }
    let workspace_index = workspace_snapshot_index(tabs, workspace_id)
        .ok_or(WorkspaceGroupMutationError::WorkspaceNotFound)?;
    if let Some(reference_id) = reference_workspace_id {
        let valid_reference = workspace_snapshot_index(tabs, reference_id).is_some_and(|index| {
            tabs.workspaces[index]
                .group_id
                .as_deref()
                .and_then(|raw| Uuid::parse_str(raw).ok())
                == Some(group_id)
        });
        if !valid_reference {
            return Err(WorkspaceGroupMutationError::InvalidReferenceWorkspace);
        }
    }
    let current_group = tabs.workspaces[workspace_index]
        .group_id
        .as_deref()
        .and_then(|raw| Uuid::parse_str(raw).ok());
    if current_group == Some(group_id) {
        return Ok(false);
    }
    let is_other_anchor = tabs
        .workspace_groups
        .as_deref()
        .unwrap_or(&[])
        .iter()
        .any(|group| {
            Uuid::parse_str(&group.id).ok() != Some(group_id)
                && group
                    .anchor_workspace_id
                    .as_deref()
                    .and_then(|raw| Uuid::parse_str(raw).ok())
                    == Some(workspace_id)
        });
    if is_other_anchor {
        return Err(WorkspaceGroupMutationError::WorkspaceIsOtherGroupAnchor);
    }

    let (rows, groups) = workspace_mirror(tabs);
    let original_row_ids: Vec<Uuid> = rows.iter().map(|row| row.id).collect();
    let original_top_level_ids = sidebar_top_level_workspace_ids(&rows, &groups, None);
    tabs.workspaces[workspace_index].group_id = Some(group_id.to_string());
    let (assigned_rows, assigned_groups) = workspace_mirror(tabs);
    let expanded_groups = expand_workspace_group_for_selection_if_needed(
        &assigned_rows,
        &assigned_groups,
        selected_workspace_uuid(tabs),
    );
    if expanded_groups != assigned_groups {
        let expanded_by_id: HashMap<Uuid, bool> = expanded_groups
            .iter()
            .map(|group| (group.id, group.is_collapsed))
            .collect();
        for group in tabs.workspace_groups.as_mut().unwrap() {
            if let Some(collapsed) = Uuid::parse_str(&group.id)
                .ok()
                .and_then(|id| expanded_by_id.get(&id).copied())
            {
                group.is_collapsed = collapsed;
            }
        }
    }
    let preferred: Vec<Uuid> = original_top_level_ids
        .into_iter()
        .filter(|id| *id != workspace_id)
        .collect();
    let (final_rows, final_groups) =
        normalize_workspace_group_contiguity(&assigned_rows, &assigned_groups, Some(&preferred));
    let final_row_ids: Vec<Uuid> = final_rows.iter().map(|row| row.id).collect();
    let final_group_ids: Vec<Uuid> = final_groups.iter().map(|group| group.id).collect();
    if final_row_ids != original_row_ids {
        write_back_reordered(tabs, &original_row_ids, &final_row_ids, &final_group_ids);
    }
    if let Some(placement) = placement {
        place_workspace_within_group(
            tabs,
            workspace_id,
            group_id,
            placement,
            reference_workspace_id,
        );
    }
    Ok(true)
}

/// Remove a workspace from its group. Removing the anchor dissolves the whole
/// group in place; removing a child restores normal global group/pin ordering.
pub fn remove_workspace_from_group_snapshot(
    tabs: &mut SessionTabManagerSnapshot,
    workspace_id: Uuid,
) -> Result<bool, WorkspaceGroupMutationError> {
    let workspace_index = workspace_snapshot_index(tabs, workspace_id)
        .ok_or(WorkspaceGroupMutationError::WorkspaceNotFound)?;
    let group_id = tabs.workspaces[workspace_index]
        .group_id
        .as_deref()
        .and_then(|raw| Uuid::parse_str(raw).ok())
        .ok_or(WorkspaceGroupMutationError::WorkspaceNotGrouped)?;
    let group_index = workspace_group_snapshot_index(tabs, group_id)
        .ok_or(WorkspaceGroupMutationError::GroupNotFound)?;
    let anchor_id = tabs.workspace_groups.as_ref().unwrap()[group_index]
        .anchor_workspace_id
        .as_deref()
        .and_then(|raw| Uuid::parse_str(raw).ok());
    if anchor_id == Some(workspace_id) {
        ungroup_workspace_group_snapshot(tabs, group_id)?;
        return Ok(true);
    }
    tabs.workspaces[workspace_index].group_id = None;
    normalize_workspace_groups_in_snapshot(tabs);
    Ok(true)
}

/// Make an existing member the group's anchor and hoist it to the front of the
/// contiguous member run. Legacy index-based anchor state is cleared so the
/// explicit stable identity remains authoritative.
pub fn set_workspace_group_anchor_snapshot(
    tabs: &mut SessionTabManagerSnapshot,
    group_id: Uuid,
    workspace_id: Uuid,
) -> Result<bool, WorkspaceGroupMutationError> {
    let group_index = workspace_group_snapshot_index(tabs, group_id)
        .ok_or(WorkspaceGroupMutationError::GroupNotFound)?;
    let workspace_index = workspace_snapshot_index(tabs, workspace_id)
        .ok_or(WorkspaceGroupMutationError::WorkspaceNotFound)?;
    let is_member = tabs.workspaces[workspace_index]
        .group_id
        .as_deref()
        .and_then(|raw| Uuid::parse_str(raw).ok())
        == Some(group_id);
    if !is_member {
        return Err(WorkspaceGroupMutationError::WorkspaceNotGroupMember);
    }
    let group = &mut tabs.workspace_groups.as_mut().unwrap()[group_index];
    if group
        .anchor_workspace_id
        .as_deref()
        .and_then(|raw| Uuid::parse_str(raw).ok())
        == Some(workspace_id)
        && group.anchor_member_index.is_none()
    {
        return Ok(false);
    }
    group.anchor_workspace_id = Some(workspace_id.to_string());
    group.anchor_member_index = None;
    normalize_workspace_groups_in_snapshot(tabs);
    Ok(true)
}

/// Move a group to a final group-array index, clamped to the source group's pin
/// tier, then project the new group-slot order back into the workspace rows
/// without moving ungrouped top-level slots.
pub fn move_workspace_group_snapshot(
    tabs: &mut SessionTabManagerSnapshot,
    group_id: Uuid,
    target_index: i64,
) -> Result<bool, WorkspaceGroupMutationError> {
    let current_index = workspace_group_snapshot_index(tabs, group_id)
        .ok_or(WorkspaceGroupMutationError::GroupNotFound)?;
    let groups = tabs.workspace_groups.as_ref().unwrap();
    let group_count = groups.len();
    let pinned = groups[current_index].is_pinned == Some(true);
    let same_tier: Vec<usize> = groups
        .iter()
        .enumerate()
        .filter(|(_, group)| (group.is_pinned == Some(true)) == pinned)
        .map(|(index, _)| index)
        .collect();
    let first = *same_tier
        .first()
        .expect("source group is in its own pin tier");
    let last = *same_tier
        .last()
        .expect("source group is in its own pin tier");
    let clamped = target_index.max(first as i64).min(last as i64) as usize;
    if current_index == clamped {
        return Ok(false);
    }
    let moved = tabs
        .workspace_groups
        .as_mut()
        .unwrap()
        .remove(current_index);
    tabs.workspace_groups
        .as_mut()
        .unwrap()
        .insert(clamped.min(group_count - 1), moved);

    let (rows, model_groups) = workspace_mirror(tabs);
    let original_row_ids: Vec<Uuid> = rows.iter().map(|row| row.id).collect();
    let top_level_ids = sidebar_top_level_workspace_ids(&rows, &model_groups, None);
    let pinned_ids = sidebar_top_level_pinned_workspace_ids(&rows, &model_groups);
    let tiered_top_level_ids: Vec<Uuid> = top_level_ids
        .iter()
        .copied()
        .filter(|id| pinned_ids.contains(id))
        .chain(
            top_level_ids
                .iter()
                .copied()
                .filter(|id| !pinned_ids.contains(id)),
        )
        .collect();
    let pinned_anchors: Vec<Uuid> = model_groups
        .iter()
        .filter(|group| group.is_pinned)
        .map(|group| group.anchor_workspace_id)
        .collect();
    let unpinned_anchors: Vec<Uuid> = model_groups
        .iter()
        .filter(|group| !group.is_pinned)
        .map(|group| group.anchor_workspace_id)
        .collect();
    let groups_by_anchor: HashMap<Uuid, &WorkspaceGroup> = model_groups
        .iter()
        .map(|group| (group.anchor_workspace_id, group))
        .collect();
    let mut pinned_index = 0;
    let mut unpinned_index = 0;
    let desired: Vec<Uuid> = tiered_top_level_ids
        .into_iter()
        .map(|id| match groups_by_anchor.get(&id) {
            Some(group) if group.is_pinned => {
                let replacement = pinned_anchors[pinned_index];
                pinned_index += 1;
                replacement
            }
            Some(_) => {
                let replacement = unpinned_anchors[unpinned_index];
                unpinned_index += 1;
                replacement
            }
            None => id,
        })
        .collect();
    let final_rows =
        normalize_workspace_group_runs_preserving_order(&rows, &model_groups, &desired);
    let final_groups = sync_workspace_groups_order_to_anchor_order(&final_rows, &model_groups);
    let final_row_ids: Vec<Uuid> = final_rows.iter().map(|row| row.id).collect();
    let final_group_ids: Vec<Uuid> = final_groups.iter().map(|group| group.id).collect();
    write_back_reordered(tabs, &original_row_ids, &final_row_ids, &final_group_ids);
    Ok(true)
}

/// Set the collapsed flag of workspace group `group_id`. Mirrors canonical
/// `WorkspaceGroupCoordinator.setWorkspaceGroupCollapsed`
/// (`WorkspaceGroupCoordinator.swift:408-412`): the **pure data** variant —
/// unknown group id and already-at-value are both no-ops, and selection is
/// never touched (the anchor-selecting behavior belongs to the UI-only
/// `toggleWorkspaceGroupCollapsed`, not ported here). Returns whether the
/// flag actually changed.
pub fn set_group_collapsed(
    tabs: &mut SessionTabManagerSnapshot,
    group_id: &str,
    collapsed: bool,
) -> bool {
    let Some(groups) = tabs.workspace_groups.as_mut() else {
        return false;
    };
    let Some(group) = groups.iter_mut().find(|g| g.id == group_id) else {
        return false;
    };
    if group.is_collapsed == collapsed {
        return false;
    }
    group.is_collapsed = collapsed;
    true
}

/// Mirror a snapshot's workspaces/groups into the `cmux-workspaces` value types
/// the ported clamp/normalize math consumes. Returns rows PARALLEL to
/// `tabs.workspaces` positions plus the mapped groups in stored order.
///
/// DIVERGENCE from `sidebar_render`'s projection: rows are NEVER skipped — the
/// reorder clamps are positional, so skipping a row would shift indices. A
/// workspace with an absent/unparseable `workspace_id` gets a freshly MINTED v4
/// id (collision with stored ids is negligible and the version bits differ; a
/// duplicated stored id is de-duplicated the same way), purely to give the row
/// a stable handle for the permutation write-back — the snapshot itself is
/// never rewritten with minted ids. A row `group_id` that parses but references
/// no known group is LEFT dangling: the crate fns already fall back correctly
/// (`isGlobalPinnedRow`'s nil-group arm, `WorkspacesModel+Ordering.swift`
/// :201-207) and the normalize pass clears it in the MIRROR only.
///
/// Groups map with the same rules as `sidebar_render.rs`: unparseable id and
/// member-less groups are skipped, a duplicate group id keeps the first
/// occurrence, and the anchor resolves via the oracle's 3-tier fallback
/// (`TabManager.swift:6018-6027`: `anchor_member_index` into the members in
/// row order → stored `anchor_workspace_id` when still a member → first
/// member). `name`/`custom_color`/`icon_symbol` pass through (inert for
/// ordering).
fn workspace_mirror(tabs: &SessionTabManagerSnapshot) -> (Vec<WorkspaceRow>, Vec<WorkspaceGroup>) {
    let mut used_ids: HashSet<Uuid> = HashSet::new();
    let rows: Vec<WorkspaceRow> = tabs
        .workspaces
        .iter()
        .map(|w| {
            let id = w
                .workspace_id
                .as_deref()
                .and_then(|s| Uuid::parse_str(s).ok())
                .filter(|id| !used_ids.contains(id))
                .unwrap_or_else(Uuid::new_v4);
            used_ids.insert(id);
            let group_id = w.group_id.as_deref().and_then(|s| Uuid::parse_str(s).ok());
            WorkspaceRow::new(id, group_id, w.is_pinned == Some(true))
        })
        .collect();

    // Members-by-group over the MIRROR rows, in row order — the oracle's
    // `workspaceIdsByGroupId` (TabManager.swift:6000-6008).
    let mut members_by_group_id: HashMap<Uuid, Vec<Uuid>> = HashMap::new();
    for row in &rows {
        if let Some(gid) = row.group_id {
            members_by_group_id.entry(gid).or_default().push(row.id);
        }
    }

    let mut seen: HashSet<Uuid> = HashSet::new();
    let mut groups: Vec<WorkspaceGroup> = Vec::new();
    for group in tabs.workspace_groups.as_deref().unwrap_or(&[]) {
        let Ok(id) = Uuid::parse_str(&group.id) else {
            continue;
        };
        let Some(members) = members_by_group_id.get(&id) else {
            continue;
        };
        if !seen.insert(id) {
            continue;
        }
        let stored_anchor = group
            .anchor_workspace_id
            .as_deref()
            .and_then(|s| Uuid::parse_str(s).ok());
        let anchor_workspace_id = group
            .anchor_member_index
            .and_then(|i| usize::try_from(i).ok())
            .and_then(|i| members.get(i).copied())
            .or_else(|| stored_anchor.filter(|a| members.contains(a)))
            .unwrap_or(members[0]);
        groups.push(WorkspaceGroup::new(
            id,
            group.name.clone(),
            group.is_collapsed,
            group.is_pinned.unwrap_or(false),
            anchor_workspace_id,
            group.custom_color.clone(),
            group.icon_symbol.clone(),
        ));
    }
    (rows, groups)
}

/// Permute `tabs.workspaces` into `final_row_ids` order (every final id maps to
/// exactly one original position via the parallel `original_row_ids`), permute
/// `tabs.workspace_groups` by the final mirror group order (snapshot groups
/// absent from the mirror — unparseable/member-less/duplicate ids — sort last,
/// stable, mirroring the crate sync's missing-anchor-last rule), and remap the
/// index-based `selected_workspace_index` through the old→new permutation so it
/// keeps following the same workspace (canonical selection is id-based and
/// untouched — the `set_workspace_pinned` precedent). `None`/out-of-range
/// selection stays as-is. Serialized objects are MOVED, never rewritten: the
/// mirror's dangling-`group_id` clears are NOT written back (snapshot strings
/// stay, the `set_workspace_pinned` posture).
fn write_back_reordered(
    tabs: &mut SessionTabManagerSnapshot,
    original_row_ids: &[Uuid],
    final_row_ids: &[Uuid],
    final_group_ids: &[Uuid],
) {
    let back_map: HashMap<Uuid, usize> = original_row_ids
        .iter()
        .enumerate()
        .map(|(i, id)| (*id, i))
        .collect();
    // Every row has exactly one mirror id (minted ids are unique), so this is a
    // full permutation of the original positions.
    let perm: Vec<usize> = final_row_ids.iter().map(|id| back_map[id]).collect();
    debug_assert_eq!(perm.len(), tabs.workspaces.len());
    let mut slots: Vec<Option<SessionWorkspaceSnapshot>> = std::mem::take(&mut tabs.workspaces)
        .into_iter()
        .map(Some)
        .collect();
    tabs.workspaces = perm
        .iter()
        .map(|&i| slots[i].take().expect("row permutation is a bijection"))
        .collect();

    if let Some(groups) = tabs.workspace_groups.as_mut() {
        let order: HashMap<Uuid, usize> = final_group_ids
            .iter()
            .enumerate()
            .map(|(i, id)| (*id, i))
            .collect();
        groups.sort_by_key(|g| {
            Uuid::parse_str(&g.id)
                .ok()
                .and_then(|id| order.get(&id).copied())
                .unwrap_or(usize::MAX)
        });
    }

    if let Some(sel) = tabs.selected_workspace_index {
        if sel >= 0 && (sel as usize) < perm.len() {
            if let Some(next) = perm.iter().position(|&old| old == sel as usize) {
                tabs.selected_workspace_index = Some(next as i64);
            }
        }
    }
}

/// Run canonical workspace-group contiguity over the session snapshot mirror and
/// write any resulting row/group ordering back to the serialized snapshot.
pub(super) fn normalize_workspace_groups_in_snapshot(tabs: &mut SessionTabManagerSnapshot) -> bool {
    let (rows, groups) = workspace_mirror(tabs);
    if groups.is_empty() {
        return false;
    }

    let original_row_ids: Vec<Uuid> = rows.iter().map(|row| row.id).collect();
    let original_group_ids: Vec<Uuid> = groups.iter().map(|group| group.id).collect();
    let (final_rows, final_groups) = normalize_workspace_group_contiguity(&rows, &groups, None);
    let final_row_ids: Vec<Uuid> = final_rows.iter().map(|row| row.id).collect();
    let final_group_ids: Vec<Uuid> = final_groups.iter().map(|group| group.id).collect();

    if final_row_ids == original_row_ids && final_group_ids == original_group_ids {
        return false;
    }

    write_back_reordered(tabs, &original_row_ids, &final_row_ids, &final_group_ids);
    true
}

/// Reorder the workspace at `index` toward `to_index` — the port of canonical
/// `WorkspaceReorderCoordinator.reorderSidebarWorkspace`
/// (`WorkspaceReorderCoordinator.swift:243-257`): a mover that anchors a group
/// (or `usesTopLevelRows`) routes to `reorderTopLevelWorkspaceItem` (:260-296),
/// everything else to plain `reorderWorkspace(tabId:toIndex:)` (:109-132). This
/// routing is LOAD-BEARING: a group anchor moved through the plain path snaps
/// back (normalize re-emits the group at its FIRST member's slot), so "move a
/// group" only works via the top-level path. The plural name reflects that an
/// anchor mover relocates its WHOLE group — every member row moves with it,
/// contiguously and anchor-first.
///
/// INDEX-SPACE CONTRACT: `index` identifies the MOVER as a position in
/// `tabs.workspaces` (the command-layer convention, matching select/close/
/// rename/pin). `to_index` is interpreted in the row space canonical uses for
/// that mover: a `tabs.workspaces` index for non-anchors, a TOP-LEVEL row index
/// for group anchors (canonical UI feeds indices from the matching space via
/// `sidebarReorderWorkspaceIds`, Coordinator:171-183; the web drag lane does
/// the same). The sidebar-drag-only `uses_top_level_rows` mode additionally
/// promotes a grouped child into that top-level row space, matching canonical
/// `reorderSidebarWorkspace(... usesTopLevelRows: true)`.
///
/// PLAIN path (Coordinator:109-132 + `workspaceReorderPlan` :142-151):
/// - Unknown id → plan nil → no-op (:143, :110); `tabs.count <= 1` → no
///   mutation and NO group inference (:116-118 — the canonical comment: no-op
///   reorders must not run inference, else socket `move_down` on the last
///   ungrouped row absorbs it into the group above).
/// - Clamp via `clampedReorderIndex` (`WorkspacesModel+Ordering.swift`
///   :143-156): `[0, count-1]`, then the in-section clamp for grouped
///   non-anchor members (`clampedGroupedMemberReorderIndex` :160-187 — section
///   `[firstIndex+1 .. lastIndex]`, pinned members in the
///   `[firstIndex+1 .. firstIndex+pinnedMemberCount]` sub-tier, unpinned in
///   `[firstIndex+1+pinnedMemberCount .. lastIndex]`), else the global
///   pin-tier clamp (pinned mover → `min(clamped, pinnedCount-1)`, unpinned →
///   `max(clamped, pinnedCount)` with `pinnedCount =
///   leadingGlobalPinnedRowCount` :190-197 counting rows by `isGlobalPinnedRow`
///   :201-207: grouped rows count by their GROUP's pin, a dangling group id
///   falls back to the row's own flag).
/// - `from == clamped` → no mutation, and crucially no normalization
///   (:116-118). Otherwise remove/insert (:120-121), then the non-drag tail
///   (:124-129): when groups exist, `normalizeWorkspaceGroupContiguity`
///   (`WorkspacesModel+GroupInvariants.swift:70-87`). The canonical
///   pre-sync-if-anchor step never changes normalize's ROW output (top-level
///   order derives from rows, never the groups array) and the crate's
///   `normalize_workspace_group_contiguity` already ends with the group-order
///   sync, so calling it alone is exact. The `isDragOperation=true`
///   group-membership inference (`applyDragInferredGroupMembership` :346-397)
///   is UI-drag semantics deferred to the sidebar drag lane; this is the
///   `isDragOperation=false` path.
///
/// TOP-LEVEL path (Coordinator:260-296, `promotesGroupedWorkspace=false`):
/// - `topLevelIds = sidebarTopLevelWorkspaceIds` (Ordering.swift:37-61, no
///   promotion); the mover absent from it → no-op (:268).
/// - Clamp via `clampedTopLevelReorderIndex` (Ordering.swift:109-125, pin tier
///   over `sidebarTopLevelPinnedWorkspaceIds` :97-106 — pinned groups by GROUP
///   pin, ungrouped rows by their own flag). `from == clamped` → no-op (:274 —
///   canonical returns `false` here, unlike the plain path's no-op-true; both
///   map to `changed = false` in the port).
/// - remove/insert in the top-level ids, then
///   `normalizeWorkspaceGroupRunsPreservingOrder(desired)` +
///   `syncWorkspaceGroupsOrderToAnchorOrder` (:276-286). NO pinned/unpinned
///   re-partition happens here — the clamp already enforced tiers, and :285
///   uses the desired order directly (a deliberate canonical divergence from
///   `normalizeWorkspaceGroupContiguity`'s desired computation).
///
/// DOCUMENTED DIVERGENCES (the port's changed-bool emit policy, same as
/// rename/pin): canonical's plain path returns `true` for its `count <= 1` and
/// `from == clamped` no-ops while the top-level path returns `false` for the
/// same — the port returns `true` iff the snapshot actually changed, which
/// drives the emit gate. Canonical also emits unconditionally post-mutation
/// (:130); the port compares the final row order against the original and only
/// writes back on a real change (normalization can revert the raw move, e.g. an
/// ungrouped row nudged into the middle of a group's section snaps back out).
///
/// Batch multi-id reorder (`reorderWorkspaces(orderedWorkspaceIds:)`,
/// Coordinator:414-444) is exposed separately by [`reorder_workspaces_many`];
/// canonical drag is single-row, so it is intentionally not part of this op.
pub fn reorder_workspaces(tabs: &mut SessionTabManagerSnapshot, index: i64, to_index: i64) -> bool {
    reorder_workspaces_with_mode(tabs, index, to_index, false)
}

/// Move one workspace to the top of its current pin tier, matching canonical
/// `WorkspaceReorderCoordinator.moveTabToTop`. Group members hoist their whole
/// top-level group row, and index-based selection is remapped to keep following
/// the same workspace identity.
pub fn move_workspace_to_top(tabs: &mut SessionTabManagerSnapshot, index: i64) -> bool {
    if index < 0 || index as usize >= tabs.workspaces.len() || tabs.workspaces.len() <= 1 {
        return false;
    }
    let (rows, groups) = workspace_mirror(tabs);
    let original_row_ids: Vec<Uuid> = rows.iter().map(|row| row.id).collect();
    let selected_id = rows[index as usize].id;

    let (final_rows, final_groups) = if groups.is_empty() {
        let mut reordered = rows;
        let selected = reordered.remove(index as usize);
        let destination = if selected.is_pinned {
            0
        } else {
            reordered.iter().take_while(|row| row.is_pinned).count()
        };
        reordered.insert(destination, selected);
        (reordered, groups)
    } else {
        let hoisted = move_workspace_group_members_after_anchors(&rows, &groups, &[selected_id]);
        let top_level = sidebar_top_level_workspace_ids(&hoisted, &groups, None);
        let Some(selected_row) = hoisted.iter().find(|row| row.id == selected_id) else {
            return false;
        };
        let selected_top_level =
            top_level_workspace_ids(std::slice::from_ref(selected_row), &groups);
        let selected_set: HashSet<Uuid> = selected_top_level.iter().copied().collect();
        let pinned_set: HashSet<Uuid> = sidebar_top_level_pinned_workspace_ids(&hoisted, &groups)
            .into_iter()
            .collect();
        let mut desired = Vec::with_capacity(top_level.len());
        desired.extend(
            selected_top_level
                .iter()
                .filter(|id| pinned_set.contains(id))
                .copied(),
        );
        desired.extend(
            top_level
                .iter()
                .filter(|id| pinned_set.contains(id) && !selected_set.contains(id))
                .copied(),
        );
        desired.extend(
            selected_top_level
                .iter()
                .filter(|id| !pinned_set.contains(id))
                .copied(),
        );
        desired.extend(
            top_level
                .iter()
                .filter(|id| !pinned_set.contains(id) && !selected_set.contains(id))
                .copied(),
        );
        let final_rows =
            normalize_workspace_group_runs_preserving_order(&hoisted, &groups, &desired);
        let final_groups = sync_workspace_groups_order_to_anchor_order(&final_rows, &groups);
        (final_rows, final_groups)
    };

    let final_row_ids: Vec<Uuid> = final_rows.iter().map(|row| row.id).collect();
    if final_row_ids == original_row_ids {
        return false;
    }
    let final_group_ids: Vec<Uuid> = final_groups.iter().map(|group| group.id).collect();
    write_back_reordered(tabs, &original_row_ids, &final_row_ids, &final_group_ids);
    true
}

/// Atomically reorder a requested leading subset within pinned and unpinned
/// tiers, returning canonical pre-application plan indexes. A dry run validates
/// and plans without touching the snapshot. Applying rebuilds the full row
/// order, then restores group contiguity and anchor ordering exactly like the
/// canonical batch coordinator.
pub fn reorder_workspaces_many(
    tabs: &mut SessionTabManagerSnapshot,
    ordered_workspace_ids: &[Uuid],
    dry_run: bool,
) -> Result<Vec<WorkspaceReorderPlanItem>, WorkspaceBatchReorderError> {
    let (rows, groups) = workspace_mirror(tabs);
    let current: Vec<WorkspaceOrderSnapshot> = rows
        .iter()
        .map(|row| WorkspaceOrderSnapshot::new(row.id, row.is_pinned))
        .collect();
    let planner = WorkspaceReorderPlanner::new();
    let plan = planner.batch_reorder_plan(ordered_workspace_ids, &current)?;
    if dry_run || !plan.iter().any(|item| item.from_index != item.to_index) {
        return Ok(plan);
    }

    let original_ids: Vec<Uuid> = rows.iter().map(|row| row.id).collect();
    let rows_by_id: HashMap<Uuid, WorkspaceRow> = rows.iter().map(|row| (row.id, *row)).collect();
    let final_ids = planner.batch_reorder_final_ids(ordered_workspace_ids, &current);
    let reordered_rows: Vec<WorkspaceRow> = final_ids
        .iter()
        .filter_map(|id| rows_by_id.get(id).copied())
        .collect();
    let (final_rows, final_groups) = if groups.is_empty() {
        (reordered_rows, groups)
    } else {
        let synced_groups = sync_workspace_groups_order_to_anchor_order(&reordered_rows, &groups);
        normalize_workspace_group_contiguity(&reordered_rows, &synced_groups, None)
    };
    let final_row_ids: Vec<Uuid> = final_rows.iter().map(|row| row.id).collect();
    let final_group_ids: Vec<Uuid> = final_groups.iter().map(|group| group.id).collect();
    write_back_reordered(tabs, &original_ids, &final_row_ids, &final_group_ids);
    Ok(plan)
}

/// Sidebar-reorder variant of [`reorder_workspaces`]. When
/// `uses_top_level_rows` is `true`, the mover is planned in top-level row space
/// even if it is a non-anchor grouped child, mirroring canonical
/// `reorderSidebarWorkspace(... usesTopLevelRows: true)` for "drag this member
/// out of its group / into top-level space". In that promotion case, the moved
/// workspace's serialized `group_id` is cleared on write-back too.
pub fn reorder_workspaces_with_mode(
    tabs: &mut SessionTabManagerSnapshot,
    index: i64,
    to_index: i64,
    uses_top_level_rows: bool,
) -> bool {
    if index < 0 || index as usize >= tabs.workspaces.len() {
        return false;
    }
    // Canonical's plain path treats count <= 1 as a successful no-op WITHOUT
    // mutating or running group inference (Coordinator:116-118); the port's
    // changed-gate maps that to `false`.
    if tabs.workspaces.len() <= 1 {
        return false;
    }
    let from = index as usize;
    let (rows, groups) = workspace_mirror(tabs);
    let original_row_ids: Vec<Uuid> = rows.iter().map(|r| r.id).collect();
    let mover_id = rows[from].id;
    let mover_is_anchor = is_workspace_group_anchor(&groups, mover_id);
    let promotes_grouped_workspace =
        uses_top_level_rows && !mover_is_anchor && rows[from].group_id.is_some();

    let (final_rows, final_groups) = if mover_is_anchor || uses_top_level_rows {
        // TOP-LEVEL (group-row) move, Coordinator:260-296.
        let top = sidebar_top_level_workspace_ids(
            &rows,
            &groups,
            promotes_grouped_workspace.then_some(mover_id),
        );
        let Some(top_from) = top.iter().position(|id| *id == mover_id) else {
            // Coordinator:268 — mover absent from the top-level rows.
            return false;
        };
        let clamped = clamped_top_level_reorder_index(&rows, &groups, mover_id, to_index, &top);
        if clamped as usize == top_from {
            // Coordinator:274 — canonical returns false for this no-op too.
            return false;
        }
        let mut desired = top;
        desired.remove(top_from);
        desired.insert(clamped as usize, mover_id);
        let base_rows = if promotes_grouped_workspace {
            // Canonical `reorderTopLevelWorkspaceItem(... promotesGroupedWorkspace:
            // true)` clears the mover's group membership before materializing the
            // desired top-level order, so the dragged child becomes its own
            // top-level row instead of snapping back into the old group run.
            assign_group(&rows, mover_id, None)
        } else {
            rows.clone()
        };
        let new_rows =
            normalize_workspace_group_runs_preserving_order(&base_rows, &groups, &desired);
        let new_groups = sync_workspace_groups_order_to_anchor_order(&new_rows, &groups);
        (new_rows, new_groups)
    } else {
        // PLAIN single move, Coordinator:109-132.
        let clamped = clamped_reorder_index(&rows, &groups, &rows[from], to_index);
        if clamped as usize == from {
            // Must NOT normalize on a no-op reorder (Coordinator:111-118).
            return false;
        }
        let mut new_rows = rows;
        let moved = new_rows.remove(from);
        new_rows.insert(clamped as usize, moved);
        if groups.is_empty() {
            // Canonical guard (:124): the non-drag tail only runs with groups.
            (new_rows, groups)
        } else {
            normalize_workspace_group_contiguity(&new_rows, &groups, None)
        }
    };

    let final_row_ids: Vec<Uuid> = final_rows.iter().map(|r| r.id).collect();
    if final_row_ids == original_row_ids {
        // Normalization restored the original order — nothing changed.
        return false;
    }
    let final_group_ids: Vec<Uuid> = final_groups.iter().map(|g| g.id).collect();
    write_back_reordered(tabs, &original_row_ids, &final_row_ids, &final_group_ids);
    if promotes_grouped_workspace {
        if let Some(new_index) = final_row_ids.iter().position(|id| *id == mover_id) {
            tabs.workspaces[new_index].group_id = None;
        }
    }
    true
}
