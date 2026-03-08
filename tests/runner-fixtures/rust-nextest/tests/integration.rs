use tapcue_nextest_fixture::add;

#[test]
fn fixture_passes() {
    assert_eq!(add(2, 2), 4);
}

#[test]
fn fixture_fails() {
    assert_eq!(add(1, 1), 3);
}
