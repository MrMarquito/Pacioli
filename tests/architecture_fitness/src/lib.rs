#[cfg(test)]
mod tests {
    use cargo_metadata::MetadataCommand;
    use std::collections::HashSet;

    #[test]
    fn test_enforce_module_architecture_boundaries() {
        let metadata = MetadataCommand::new()
            .exec()
            .expect("Failed to parse workspace metadata using cargo_metadata");

        let resolve = metadata
            .resolve
            .expect("Missing workspace dependency graph");

        let forbidden_dependencies: [(&str, &str); 7] = [
            ("ledger-core", "ledger-journal"),
            ("ledger-core", "ledger-accounts"),
            ("ledger-core", "ledger-balances"),
            ("ledger-accounts", "ledger-journal"),
            ("ledger-balances", "ledger-journal"),
            ("ledger-audit", "ledger-outbox"),
            ("ledger-reconciliation", "ledger-journal"),
        ];

        for node in resolve.nodes {
            let package_name = node.id.repr.split(' ').next().unwrap();

            for (source_crate, forbidden_target) in forbidden_dependencies {
                if package_name == source_crate {
                    let dep_names: HashSet<_> = node.deps.iter().map(|d| d.name.as_str()).collect();

                    assert!(
                        !dep_names.contains(forbidden_target),
                        "ARCHITECTURE INVARIANT VIOLATION: '{}' is strictly prohibited from depending on '{}'.",
                        source_crate,
                        forbidden_target
                    );
                }
            }
        }
    }
}
