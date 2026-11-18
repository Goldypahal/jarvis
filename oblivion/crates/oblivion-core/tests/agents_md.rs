use oblivion_core::agents_md::{apply_update, propose_update};
use oblivion_core::extract::ProposedMemory;
use oblivion_core::prompt;
use oblivion_tools::Shell;

#[test]
fn agents_md_is_read_at_start_into_system_prompt() {
    let d = tempfile::tempdir().unwrap();
    let agents_md_content =
        "# Project Guidelines\n- Always run cargo fmt before commit.\n- Keep dependencies minimal.";
    std::fs::write(d.path().join("AGENTS.md"), agents_md_content).unwrap();

    let p = prompt::build(d.path(), Shell::PowerShell, "2026-11-19", &[]);
    assert!(p.contains("<project_instructions>"));
    assert!(p.contains("Project instructions from AGENTS.md:"));
    assert!(p.contains("Always run cargo fmt before commit."));
    assert!(p.contains("Keep dependencies minimal."));

    let loaded = prompt::project_instructions(d.path());
    assert!(loaded.is_some());
    let (name, text) = loaded.unwrap();
    assert_eq!(name, "AGENTS.md");
    assert_eq!(text, agents_md_content);
}

#[test]
fn propose_update_detects_new_project_facts_and_ignores_existing_ones() {
    let d = tempfile::tempdir().unwrap();
    let initial_content = "# Project Instructions\n- Always run cargo fmt before commit.";
    std::fs::write(d.path().join("AGENTS.md"), initial_content).unwrap();

    let memories = vec![
        // Already in AGENTS.md:
        ProposedMemory::new("project_fact", "Always run cargo fmt before commit.", 5),
        // New project fact:
        ProposedMemory::new(
            "project_fact",
            "SQLite database is stored in ~/.oblivion/store.db",
            4,
        ),
        // New recipe:
        ProposedMemory::new("recipe", "Run cargo clippy --all-targets to check lints", 5),
        // User preference (should not go into AGENTS.md project instructions):
        ProposedMemory::new("user_pref", "User prefers dark mode UI", 4),
    ];

    let proposal =
        propose_update(d.path(), &memories).expect("should propose updates for new facts");
    assert_eq!(proposal.path, d.path().join("AGENTS.md"));
    assert!(proposal.exists);
    assert_eq!(proposal.additions.len(), 2);
    assert_eq!(
        proposal.additions[0],
        "SQLite database is stored in ~/.oblivion/store.db"
    );
    assert_eq!(
        proposal.additions[1],
        "Run cargo clippy --all-targets to check lints"
    );

    // Apply the proposal
    apply_update(&proposal).expect("apply_update should succeed");

    let updated_content = std::fs::read_to_string(d.path().join("AGENTS.md")).unwrap();
    assert!(
        updated_content.contains(initial_content),
        "Original content must be preserved"
    );
    assert!(updated_content.contains("SQLite database is stored in ~/.oblivion/store.db"));
    assert!(updated_content.contains("Run cargo clippy --all-targets to check lints"));

    // Proposing again should return None because everything is now present
    let second_proposal = propose_update(d.path(), &memories);
    assert!(
        second_proposal.is_none(),
        "Should not propose update when all facts are already in file"
    );
}

#[test]
fn propose_update_creates_new_agents_md_if_missing() {
    let d = tempfile::tempdir().unwrap();
    assert!(!d.path().join("AGENTS.md").exists());

    let memories = vec![ProposedMemory::new(
        "project_fact",
        "Rust 2024 edition is used",
        5,
    )];

    let proposal = propose_update(d.path(), &memories).expect("should propose update for new file");
    assert_eq!(proposal.path, d.path().join("AGENTS.md"));
    assert!(!proposal.exists);
    assert_eq!(proposal.additions.len(), 1);

    apply_update(&proposal).expect("apply_update should create file");

    assert!(d.path().join("AGENTS.md").exists());
    let content = std::fs::read_to_string(d.path().join("AGENTS.md")).unwrap();
    assert!(content.contains("Rust 2024 edition is used"));
}
