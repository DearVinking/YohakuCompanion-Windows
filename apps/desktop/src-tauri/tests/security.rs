use tauri::ipc::Origin;

#[test]
fn generated_commands_are_allowed_only_for_the_local_main_window() {
    let bindings = include_str!("../../src/api/generated.ts");
    let commands: Vec<_> = bindings
        .lines()
        .filter_map(|line| line.split_once(">(\""))
        .map(|(_, tail)| tail.split('"').next().unwrap())
        .collect();
    assert_eq!(commands.len(), 12, "all generated commands must be checked");

    let mut context: tauri::Context<tauri::Wry> = tauri::generate_context!();
    let authority = context.runtime_authority_mut();
    let remote = Origin::Remote {
        url: "https://example.com".parse().unwrap(),
    };
    for command in commands {
        assert!(
            authority
                .resolve_access(command, "main", "main", &Origin::Local)
                .is_some(),
            "main window has no explicit permission for {command}"
        );
        assert!(
            authority
                .resolve_access(command, "other", "other", &Origin::Local)
                .is_none(),
            "unrelated window can invoke {command}"
        );
        assert!(
            authority
                .resolve_access(command, "main", "main", &remote)
                .is_none(),
            "remote content can invoke {command}"
        );
    }
    assert!(
        authority
            .resolve_access("plugin:window|create", "main", "main", &Origin::Local)
            .is_none(),
        "the frontend must not acquire extra window creation permissions"
    );
}
