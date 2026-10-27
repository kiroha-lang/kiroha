//! The normative inventory and generation card describe the public API.
#[test]
fn specification_card_and_runtime_have_the_same_api() {
    let spec = kiroha::cli::SPEC;
    let names: Vec<_> = spec.lines().filter_map(|line| {
        let columns: Vec<_> = line.split('|').map(str::trim).collect();
        if columns.len() < 4 || columns[1].parse::<usize>().is_err() { return None; }
        let signature = columns[2].trim_matches('`');
        Some(signature.split(['(', '{']).next().unwrap().rsplit('.').next().unwrap())
    }).collect();
    let actual: Vec<_> = kiroha::builtins::ALL.iter()
        .filter(|b| **b != kiroha::builtins::Builtin::Validate)
        .map(|b| b.name()).collect();
    assert_eq!(names, actual);
    assert_eq!(actual.len(), 50);
    assert_eq!(kiroha::builtins::GLOBALS.len(), 30);
    assert!(!kiroha::builtins::GLOBALS.contains(&kiroha::builtins::Builtin::Validate));
    assert_eq!(kiroha::methods::ALL.len(), 20);
    assert_eq!(kiroha::methods::ALL.iter().filter(|m| m.property).count(), 1);
    let card = kiroha::conformance::extract_block(spec, "GENERATION_CARD", "text").unwrap();
    let words: std::collections::HashSet<_> = card.split(|c: char| !c.is_ascii_alphanumeric() && c != '_').collect();
    for name in actual.iter().chain(kiroha::builtins::ATOMS.iter()) {
        assert!(words.contains(name), "generation card omits {name}");
    }
    let globals: Vec<_> = kiroha::builtins::GLOBALS.iter().map(|b| b.name()).chain(kiroha::builtins::ATOMS.iter().copied()).collect();
    kiroha::compile(&format!("[{}]", globals.join(","))).unwrap();
    let mut seen = std::collections::HashSet::new();
    for member in kiroha::methods::ALL {
        assert!(seen.insert(member.name));
        assert!(!kiroha::builtins::GLOBALS.contains(&member.builtin));
        assert!(member.builtin.signature().min >= 1);
        assert_eq!(member.signature().min + 1, member.builtin.signature().min);
        assert!(kiroha::compile(member.name).is_err());
    }
}
