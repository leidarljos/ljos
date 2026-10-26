//! The finish tool does not offer a close. The command line flag stays for the person.

#[test]
fn the_loop_does_not_say_it_closes_itself() {
    let src = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/lib.rs")).unwrap();
    assert!(
        !src.contains("The loop closes itself"),
        "the workflow note still says the loop closes itself"
    );
}
