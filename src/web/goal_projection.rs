use std::collections::{HashMap, HashSet};

use uuid::Uuid;

use crate::goal_models::GoalGraphSnapshot;

pub const PROJECTION_VERSION: &str = "goal-lanes-v1";

#[derive(Clone, Debug)]
pub struct GoalGraphProjection {
    pub lanes: Vec<GoalLane>,
    pub selected_session_id: Option<Uuid>,
}

#[derive(Clone, Debug)]
pub struct GoalLane {
    pub branch_id: Uuid,
    pub parent_branch_id: Option<Uuid>,
    pub name: String,
    pub status: String,
    pub depth: usize,
    pub head_session_id: Uuid,
    pub session_ids: Vec<Uuid>,
}

impl GoalGraphProjection {
    pub fn build(snapshot: &GoalGraphSnapshot, requested_session: Option<Uuid>) -> Self {
        let parent_by_branch = snapshot
            .branches
            .iter()
            .map(|branch| (branch.id, branch.parent_goal_branch_id))
            .collect::<HashMap<_, _>>();
        let branch_ids = snapshot
            .branches
            .iter()
            .map(|branch| branch.id)
            .collect::<HashSet<_>>();
        let lanes = snapshot
            .branches
            .iter()
            .map(|branch| GoalLane {
                branch_id: branch.id,
                parent_branch_id: branch.parent_goal_branch_id,
                name: branch.name.clone(),
                status: branch.status.clone(),
                depth: branch_depth(branch.id, &parent_by_branch, &mut HashSet::new()),
                head_session_id: branch.head_session_id,
                session_ids: snapshot
                    .sessions
                    .iter()
                    .filter(|session| session.goal_branch_id == branch.id)
                    .map(|session| session.id)
                    .collect(),
            })
            .collect();
        let selected_session_id = requested_session
            .filter(|session_id| snapshot.sessions.iter().any(|item| item.id == *session_id))
            .or_else(|| {
                snapshot
                    .attention_items
                    .iter()
                    .rev()
                    .find(|item| item.status == "open")
                    .and_then(|item| item.session_id)
            })
            .or_else(|| {
                snapshot
                    .sessions
                    .iter()
                    .rev()
                    .find(|item| item.status == "running")
                    .map(|item| item.id)
            })
            .or_else(|| {
                snapshot
                    .branches
                    .iter()
                    .rev()
                    .find(|branch| branch_ids.contains(&branch.id))
                    .map(|branch| branch.head_session_id)
            });
        Self {
            lanes,
            selected_session_id,
        }
    }
}

fn branch_depth(
    branch_id: Uuid,
    parents: &HashMap<Uuid, Option<Uuid>>,
    visiting: &mut HashSet<Uuid>,
) -> usize {
    if !visiting.insert(branch_id) {
        return 0;
    }
    let depth = parents
        .get(&branch_id)
        .copied()
        .flatten()
        .filter(|parent_id| parents.contains_key(parent_id))
        .map(|parent_id| branch_depth(parent_id, parents, visiting) + 1)
        .unwrap_or_default();
    visiting.remove(&branch_id);
    depth
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn branch_depth_is_a_projection_choice_and_cycles_stay_bounded() {
        let root = Uuid::new_v4();
        let child = Uuid::new_v4();
        let grandchild = Uuid::new_v4();
        let parents = HashMap::from([(root, None), (child, Some(root)), (grandchild, Some(child))]);
        assert_eq!(branch_depth(root, &parents, &mut HashSet::new()), 0);
        assert_eq!(branch_depth(child, &parents, &mut HashSet::new()), 1);
        assert_eq!(branch_depth(grandchild, &parents, &mut HashSet::new()), 2);

        let cycle = HashMap::from([(root, Some(child)), (child, Some(root))]);
        assert!(branch_depth(root, &cycle, &mut HashSet::new()) <= cycle.len());
    }
}
