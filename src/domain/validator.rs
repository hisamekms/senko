use std::collections::{HashSet, VecDeque};

/// Check if adding dep_id as a dependency of task_id would create a cycle.
/// Performs BFS from dep_id following its dependencies; if task_id is reachable, it's a cycle.
///
/// `get_dependencies` returns the dependency IDs for a given task.
pub fn has_cycle<F>(task_id: i64, dep_id: i64, get_dependencies: F) -> bool
where
    F: Fn(i64) -> Vec<i64>,
{
    let mut visited = HashSet::new();
    let mut queue = VecDeque::new();
    queue.push_back(dep_id);
    visited.insert(dep_id);

    while let Some(current) = queue.pop_front() {
        for d in get_dependencies(current) {
            if d == task_id {
                return true;
            }
            if visited.insert(d) {
                queue.push_back(d);
            }
        }
    }
    false
}

/// Async version of cycle detection for use in the application layer.
pub async fn has_cycle_async<F, Fut>(task_id: i64, dep_id: i64, get_dependencies: F) -> bool
where
    F: Fn(i64) -> Fut,
    Fut: std::future::Future<Output = Vec<i64>>,
{
    let mut visited = HashSet::new();
    let mut queue = VecDeque::new();
    queue.push_back(dep_id);
    visited.insert(dep_id);

    while let Some(current) = queue.pop_front() {
        for d in get_dependencies(current).await {
            if d == task_id {
                return true;
            }
            if visited.insert(d) {
                queue.push_back(d);
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn make_graph(edges: &[(i64, Vec<i64>)]) -> impl Fn(i64) -> Vec<i64> + '_ {
        let map: HashMap<i64, &Vec<i64>> = edges.iter().map(|(k, v)| (*k, v)).collect();
        move |id| map.get(&id).map(|v| v.to_vec()).unwrap_or_default()
    }

    #[test]
    fn no_cycle_linear_chain() {
        // 1 -> 2 -> 3, adding 4 depends on 3
        let edges = [(3, vec![2]), (2, vec![1]), (1, vec![])];
        assert!(!has_cycle(4, 3, make_graph(&edges)));
    }

    #[test]
    fn direct_cycle() {
        // 1 -> 2, adding 2 depends on 1 would create: 2 -> 1 -> 2
        let edges = [(1, vec![2]), (2, vec![])];
        assert!(has_cycle(2, 1, make_graph(&edges)));
    }

    #[test]
    fn indirect_cycle() {
        // 1 -> 2 -> 3, adding 3 depends on 1 would create: 3 -> 1 -> 2 -> 3
        let edges = [(1, vec![2]), (2, vec![3]), (3, vec![])];
        assert!(has_cycle(3, 1, make_graph(&edges)));
    }

    #[test]
    fn diamond_no_cycle() {
        // 1 -> 2, 1 -> 3, 2 -> 4, 3 -> 4, adding 5 depends on 4
        let edges = [
            (4, vec![2, 3]),
            (2, vec![1]),
            (3, vec![1]),
            (1, vec![]),
        ];
        assert!(!has_cycle(5, 4, make_graph(&edges)));
    }

    #[test]
    fn self_dependency_not_detected() {
        // has_cycle doesn't check self-dependency (task_id == dep_id);
        // that's validated separately at the call site
        let edges = [(1, vec![])];
        assert!(!has_cycle(1, 1, make_graph(&edges)));
    }

    #[test]
    fn no_dependencies_no_cycle() {
        let edges: [(i64, Vec<i64>); 0] = [];
        assert!(!has_cycle(2, 1, make_graph(&edges)));
    }
}
