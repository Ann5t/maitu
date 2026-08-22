use std::collections::{HashMap, HashSet};

use uuid::Uuid;

use crate::goal_models::GoalGraphSnapshot;

pub const PROJECTION_VERSION: &str = "goal-worksite-v2";

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
        let branches_by_id = snapshot
            .branches
            .iter()
            .map(|branch| (branch.id, branch))
            .collect::<HashMap<_, _>>();
        let ordered_branch_ids = branch_preorder(
            &snapshot
                .branches
                .iter()
                .map(|branch| branch.id)
                .collect::<Vec<_>>(),
            &parent_by_branch,
        );
        let mut sessions_by_branch = HashMap::<Uuid, Vec<Uuid>>::new();
        for session in &snapshot.sessions {
            sessions_by_branch
                .entry(session.goal_branch_id)
                .or_default()
                .push(session.id);
        }
        let mut depth_cache = HashMap::new();
        let lanes = ordered_branch_ids
            .into_iter()
            .filter_map(|branch_id| branches_by_id.get(&branch_id).copied())
            .map(|branch| GoalLane {
                branch_id: branch.id,
                parent_branch_id: branch.parent_goal_branch_id,
                name: branch.name.clone(),
                status: branch.status.clone(),
                depth: branch_depth_cached(
                    branch.id,
                    &parent_by_branch,
                    &mut depth_cache,
                    &mut HashSet::new(),
                ),
                head_session_id: branch.head_session_id,
                session_ids: sessions_by_branch.remove(&branch.id).unwrap_or_default(),
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

fn branch_preorder(source_order: &[Uuid], parents: &HashMap<Uuid, Option<Uuid>>) -> Vec<Uuid> {
    let known = source_order.iter().copied().collect::<HashSet<_>>();
    let mut children = HashMap::<Uuid, Vec<Uuid>>::new();
    let mut roots = Vec::new();
    for branch_id in source_order {
        match parents.get(branch_id).copied().flatten() {
            Some(parent_id) if known.contains(&parent_id) => {
                children.entry(parent_id).or_default().push(*branch_id);
            }
            _ => roots.push(*branch_id),
        }
    }
    let mut ordered = Vec::with_capacity(source_order.len());
    let mut visited = HashSet::with_capacity(source_order.len());
    let walk = |start: Uuid, ordered: &mut Vec<Uuid>, visited: &mut HashSet<Uuid>| {
        let mut stack = vec![start];
        while let Some(branch_id) = stack.pop() {
            if !visited.insert(branch_id) {
                continue;
            }
            ordered.push(branch_id);
            if let Some(descendants) = children.get(&branch_id) {
                stack.extend(descendants.iter().rev().copied());
            }
        }
    };
    for root in roots {
        walk(root, &mut ordered, &mut visited);
    }
    for branch_id in source_order {
        walk(*branch_id, &mut ordered, &mut visited);
    }
    ordered
}

#[cfg(test)]
fn branch_depth(
    branch_id: Uuid,
    parents: &HashMap<Uuid, Option<Uuid>>,
    visiting: &mut HashSet<Uuid>,
) -> usize {
    branch_depth_cached(branch_id, parents, &mut HashMap::new(), visiting)
}

fn branch_depth_cached(
    branch_id: Uuid,
    parents: &HashMap<Uuid, Option<Uuid>>,
    cache: &mut HashMap<Uuid, usize>,
    visiting: &mut HashSet<Uuid>,
) -> usize {
    if let Some(depth) = cache.get(&branch_id) {
        return *depth;
    }
    if !visiting.insert(branch_id) {
        return 0;
    }
    let depth = parents
        .get(&branch_id)
        .copied()
        .flatten()
        .filter(|parent_id| parents.contains_key(parent_id))
        .map(|parent_id| branch_depth_cached(parent_id, parents, cache, visiting) + 1)
        .unwrap_or_default();
    visiting.remove(&branch_id);
    cache.insert(branch_id, depth);
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

        let shuffled = [grandchild, root, child];
        assert_eq!(
            branch_preorder(&shuffled, &parents),
            [root, child, grandchild]
        );
        let cyclic_order = branch_preorder(&[root, child], &cycle);
        assert_eq!(cyclic_order.len(), 2);
        assert_eq!(
            cyclic_order.iter().copied().collect::<HashSet<_>>().len(),
            2
        );
    }
}
