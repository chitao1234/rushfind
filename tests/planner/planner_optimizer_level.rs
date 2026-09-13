use crate::support::{argv, collect_predicate_labels};
use rushfind::parser::parse_command;
use rushfind::planner::plan_command;

fn predicate_order(args: &[&str]) -> Vec<&'static str> {
    let plan = plan_command(parse_command(&argv(args)).unwrap(), 1).unwrap();
    collect_predicate_labels(&plan.expr)
}

/// GNU find reorders the cheap, side-effect-free predicates in `-a` chains by
/// default, and `-O0` turns that off so the tests run in the order given.
#[test]
fn default_optimization_reorders_cheap_predicates_first() {
    assert_eq!(
        predicate_order(&[".", "-type", "f", "-name", "*.rs"]),
        vec!["name", "type"]
    );
}

#[test]
fn optimizer_level_zero_keeps_the_written_order() {
    assert_eq!(
        predicate_order(&["-O0", ".", "-type", "f", "-name", "*.rs"]),
        vec!["type", "name"]
    );
}

#[test]
fn higher_optimizer_levels_keep_the_default_reordering() {
    for level in ["-O1", "-O2", "-O3"] {
        assert_eq!(
            predicate_order(&[level, ".", "-type", "f", "-name", "*.rs"]),
            vec!["name", "type"],
            "{level} should behave like the default level"
        );
    }
}

#[test]
fn predicate_order_does_not_change_results() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("dir")).unwrap();
    std::fs::write(root.path().join("dir/file.rs"), "fn main() {}\n").unwrap();
    std::fs::write(root.path().join("dir/file.txt"), "text\n").unwrap();

    for level in [None, Some("-O0")] {
        let mut args = Vec::new();
        if let Some(level) = level {
            args.push(level);
        }
        args.push(root.path().to_str().unwrap());
        args.extend(["-type", "f", "-name", "*.rs"]);

        let output = crate::support::rushfind_command_with_workers(1)
            .args(&args)
            .output()
            .unwrap();

        assert!(
            String::from_utf8_lossy(&output.stdout).ends_with("file.rs\n"),
            "{args:?} produced {:?}",
            String::from_utf8_lossy(&output.stdout)
        );
    }
}
