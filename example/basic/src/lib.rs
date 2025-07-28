#[no_mangle]
pub extern "C" fn example() {
    println!("Hello Android!");
}

#[test]
fn test_example() {
    example();
}
