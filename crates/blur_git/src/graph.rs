//! Commit graph lane layout for rendering a visual DAG row by row.

use crate::types::CommitInfo;

/// An edge connecting a lane on the current row to a lane on the next row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GraphEdge {
    pub from_lane: usize,
    pub to_lane: usize,
    pub color: usize,
}

/// Layout data for a single commit row in the commit graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphRow {
    /// Lane index where this commit's node sits.
    pub node_lane: usize,
    /// Color index assigned to this commit's lane.
    pub color: usize,
    /// Active lanes entering this row (`Some(color)` if occupied, `None` if empty).
    pub lanes_before: Vec<Option<usize>>,
    /// Edges drawn from this row toward the next row (pass-throughs, branches, merges).
    pub edges: Vec<GraphEdge>,
    /// Total number of lanes needed to draw this row without clipping.
    pub lane_count: usize,
}

/// Compute a stable visual lane layout for a topologically ordered list of commits.
pub fn layout(commits: &[CommitInfo]) -> Vec<GraphRow> {
    let mut rows = Vec::with_capacity(commits.len());
    // Each active lane tracks `(expected_commit_id, color_index)`.
    let mut active: Vec<Option<(String, usize)>> = Vec::new();
    let mut next_color: usize = 0;

    let alloc_color = |counter: &mut usize| -> usize {
        let c = *counter;
        *counter += 1;
        c
    };

    for commit in commits {
        let lanes_before: Vec<Option<usize>> =
            active.iter().map(|slot| slot.as_ref().map(|(_, c)| *c)).collect();

        // Find all lanes currently expecting this commit.
        let matching_lanes: Vec<usize> = active
            .iter()
            .enumerate()
            .filter_map(|(idx, slot)| {
                slot.as_ref()
                    .and_then(|(oid, _)| (oid == &commit.id).then_some(idx))
            })
            .collect();

        let (node_lane, color) = if let Some(&first) = matching_lanes.first() {
            let color = active[first].as_ref().unwrap().1;
            (first, color)
        } else {
            let color = alloc_color(&mut next_color);
            if let Some(free) = active.iter().position(|s| s.is_none()) {
                active[free] = Some((commit.id.clone(), color));
                (free, color)
            } else {
                let idx = active.len();
                active.push(Some((commit.id.clone(), color)));
                (idx, color)
            }
        };

        let mut edges = Vec::new();

        // Any secondary lanes expecting this same commit converge into `node_lane`.
        for &dup_lane in matching_lanes.iter().skip(1) {
            if let Some((_, dup_color)) = active[dup_lane].take() {
                edges.push(GraphEdge {
                    from_lane: dup_lane,
                    to_lane: node_lane,
                    color: dup_color,
                });
            }
        }

        // Continuing pass-through edges for all other occupied lanes.
        for (idx, slot) in active.iter().enumerate() {
            if idx == node_lane {
                continue;
            }
            if let Some((_, lane_color)) = slot {
                edges.push(GraphEdge {
                    from_lane: idx,
                    to_lane: idx,
                    color: *lane_color,
                });
            }
        }

        // Route this commit's parents.
        if commit.parents.is_empty() {
            active[node_lane] = None;
        } else {
            let first_parent = &commit.parents[0];
            let existing_first = active.iter().enumerate().find_map(|(idx, slot)| {
                if idx == node_lane {
                    return None;
                }
                slot.as_ref()
                    .and_then(|(oid, c)| (oid == first_parent).then_some((idx, *c)))
            });

            if let Some((target_lane, _)) = existing_first {
                active[node_lane] = None;
                edges.push(GraphEdge {
                    from_lane: node_lane,
                    to_lane: target_lane,
                    color,
                });
            } else {
                active[node_lane] = Some((first_parent.clone(), color));
                edges.push(GraphEdge {
                    from_lane: node_lane,
                    to_lane: node_lane,
                    color,
                });
            }

            // Secondary (merge) parents.
            for parent_id in commit.parents.iter().skip(1) {
                let existing = active.iter().enumerate().find_map(|(idx, slot)| {
                    slot.as_ref()
                        .and_then(|(oid, c)| (oid == parent_id).then_some((idx, *c)))
                });

                if let Some((target_lane, target_color)) = existing {
                    edges.push(GraphEdge {
                        from_lane: node_lane,
                        to_lane: target_lane,
                        color: target_color,
                    });
                } else {
                    let branch_color = alloc_color(&mut next_color);
                    let target_lane = if let Some(free) = active.iter().position(|s| s.is_none()) {
                        active[free] = Some((parent_id.clone(), branch_color));
                        free
                    } else {
                        let idx = active.len();
                        active.push(Some((parent_id.clone(), branch_color)));
                        idx
                    };
                    edges.push(GraphEdge {
                        from_lane: node_lane,
                        to_lane: target_lane,
                        color: branch_color,
                    });
                }
            }
        }

        // Trim trailing empty slots to keep lane_count compact.
        while active.last().is_some_and(|s| s.is_none()) {
            active.pop();
        }

        let max_edge_lane = edges
            .iter()
            .map(|e| e.from_lane.max(e.to_lane) + 1)
            .max()
            .unwrap_or(0);
        let lane_count = lanes_before
            .len()
            .max(active.len())
            .max(node_lane + 1)
            .max(max_edge_lane);

        rows.push(GraphRow {
            node_lane,
            color,
            lanes_before,
            edges,
            lane_count,
        });
    }

    rows
}
