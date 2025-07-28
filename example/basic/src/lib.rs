#[no_mangle]
pub extern "C" fn example() {
    println!("Hello Android!");
}

#[test]
fn test_example() {
    example();
}

#[test]
fn failing_test() {
    assert_eq!(1, 2, "This test is supposed to fail");
}