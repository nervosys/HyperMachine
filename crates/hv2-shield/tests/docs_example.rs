//! The shield docs/SHIELDS.md shows is one this crate accepts.

#[test]
fn the_documented_example_is_a_valid_shield() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/SHIELDS.md");
    let doc = std::fs::read_to_string(&path).unwrap();
    let start = doc.find("```json").expect("an example");
    let body = &doc[start..];
    let body = &body[body.find('\n').unwrap() + 1..];
    let example = &body[..body.find("```").unwrap()];
    let spec = hv2_shield::ShieldSpec::parse(example).unwrap();
    assert_eq!(spec.syscalls.len(), 2);
    assert_eq!(spec.network.len(), 2);
}
