use findoxide::planner::{RuntimeAction, RuntimeExpr, RuntimePredicate};

pub fn action_labels<F>(expr: &RuntimeExpr, mut label: F) -> Vec<&'static str>
where
    F: FnMut(&RuntimeAction) -> Option<&'static str>,
{
    let mut out = Vec::new();
    walk_actions(expr, &mut |action| {
        if let Some(label) = label(action) {
            out.push(label);
        }
    });
    out
}

pub fn contains_action<F>(expr: &RuntimeExpr, mut predicate: F) -> bool
where
    F: FnMut(&RuntimeAction) -> bool,
{
    let mut found = false;
    walk_actions(expr, &mut |action| found |= predicate(action));
    found
}

pub fn predicate_labels<F>(expr: &RuntimeExpr, mut label: F) -> Vec<&'static str>
where
    F: FnMut(&RuntimePredicate) -> Option<&'static str>,
{
    let mut out = Vec::new();
    walk_predicates(expr, &mut |predicate| {
        if let Some(label) = label(predicate) {
            out.push(label);
        }
    });
    out
}

pub fn contains_predicate<F>(expr: &RuntimeExpr, mut predicate: F) -> bool
where
    F: FnMut(&RuntimePredicate) -> bool,
{
    let mut found = false;
    walk_predicates(expr, &mut |runtime_predicate| {
        found |= predicate(runtime_predicate)
    });
    found
}

fn walk_actions<F>(expr: &RuntimeExpr, visit: &mut F)
where
    F: FnMut(&RuntimeAction),
{
    match expr {
        RuntimeExpr::And(items) => {
            for item in items {
                walk_actions(item, visit);
            }
        }
        RuntimeExpr::Or(left, right) => {
            walk_actions(left, visit);
            walk_actions(right, visit);
        }
        RuntimeExpr::Not(inner) => walk_actions(inner, visit),
        RuntimeExpr::Action(action) => visit(action),
        RuntimeExpr::Predicate(_) | RuntimeExpr::Barrier => {}
    }
}

fn walk_predicates<F>(expr: &RuntimeExpr, visit: &mut F)
where
    F: FnMut(&RuntimePredicate),
{
    match expr {
        RuntimeExpr::And(items) => {
            for item in items {
                walk_predicates(item, visit);
            }
        }
        RuntimeExpr::Or(left, right) => {
            walk_predicates(left, visit);
            walk_predicates(right, visit);
        }
        RuntimeExpr::Not(inner) => walk_predicates(inner, visit),
        RuntimeExpr::Predicate(predicate) => visit(predicate),
        RuntimeExpr::Action(_) | RuntimeExpr::Barrier => {}
    }
}
