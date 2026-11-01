//! A workflow plan stops at the child cap and at the descendant limit.
//! Every objection that did not fit is named. Nothing is filed past either.

use ljos_cli::converge::{plan_round, Objection, PanelState, RoundPlan, ScopeBudget};

fn blocking(n: usize) -> Objection {
    Objection::blocking(
        "carol",
        "a real failure",
        &format!("distinct bug number {n}"),
        "p",
    )
}

#[test]
fn a_full_child_list_names_every_objection_left_out() {
    let limits = ScopeBudget::panel();
    let mut state = PanelState::at_round(2);
    state.objections = (1..=10).map(blocking).collect();
    let RoundPlan::FileBlocking { objections, .. } = plan_round(&state, &limits) else {
        panic!("expected a filing");
    };
    assert_eq!(objections.len(), limits.max_children);
    state.children_filed = objections.len();
    state.seen = objections.iter().map(Objection::key).collect();
    let plan = plan_round(&state, &limits);
    let RoundPlan::Hold { why } = plan else {
        panic!("expected a pause, got {plan:?}");
    };
    assert!(
        why.contains("distinct bug number 9") && why.contains("distinct bug number 10"),
        "{why}"
    );
}

#[test]
fn a_plan_does_not_file_past_the_descendant_limit() {
    let limits = ScopeBudget::panel();
    let mut state = PanelState::at_round(2);
    state.descendants = limits.max_descendants - 1;
    state.objections = (1..=5).map(blocking).collect();
    let plan = plan_round(&state, &limits);
    let RoundPlan::FileBlocking { objections, .. } = &plan else {
        panic!("one descendant of room should file, got {plan:?}");
    };
    assert!(
        state.descendants + objections.len() <= limits.max_descendants,
        "files {} with {} of {} descendants",
        objections.len(),
        state.descendants,
        limits.max_descendants
    );
    assert_eq!(objections.len(), 1, "{objections:?}");
}

#[test]
fn a_full_descendant_list_names_every_objection_left_out() {
    let limits = ScopeBudget::panel();
    let mut state = PanelState::at_round(2);
    state.descendants = limits.max_descendants;
    state.objections = (1..=2).map(blocking).collect();
    let plan = plan_round(&state, &limits);
    let RoundPlan::Hold { why } = plan else {
        panic!("expected a pause, got {plan:?}");
    };
    assert!(why.contains("distinct bug number 1"), "{why}");
    assert!(why.contains("distinct bug number 2"), "{why}");
}
