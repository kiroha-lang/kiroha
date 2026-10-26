//! The normative inventory and generation card must describe the actual prelude.

#[test]
fn specification_card_and_runtime_have_the_same_api() {
    let spec = kiroha::cli::SPEC;
    let mut functions = Vec::new();
    for line in spec.lines() {
        let columns: Vec<_> = line.split('|').map(str::trim).collect();
        if columns.len() < 4 || columns[1].parse::<usize>().is_err() {
            continue;
        }
        let signature = columns[2].trim_matches('`');
        functions.push(signature.split('(').next().unwrap());
    }
    let actual: Vec<_> = kiroha::builtins::ALL.iter().map(|b| b.name()).collect();
    assert_eq!(functions.len(), 51);
    assert_eq!(functions, actual);
    assert_eq!(kiroha::builtins::ATOMS, ["str", "num", "int", "bool", "json"]);
    let card = kiroha::conformance::extract_block(spec, "GENERATION_CARD", "text").unwrap();
    let words: std::collections::HashSet<_> = card.split(|c: char| !c.is_ascii_alphanumeric() && c != '_').collect();
    for name in actual.iter().chain(kiroha::builtins::ATOMS.iter()) {
        assert!(words.contains(name), "generation card omits {name}");
    }
    kiroha::compile(&format!("[{}]", actual.join(","))).expect("every documented function resolves");
}
