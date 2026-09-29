//! `ctrl+e` in the agent picker: an agent's description and prompt edited
//! inside moon and written back to its file.

use moon_core::config::ids::READ_FILES;
use moon_core::Permission;

use super::tests::{app, key, type_text};
use super::tests_agent::{agents_dir, project};
use super::*;
use moon_agent::Policy;

const COMMITTER: &str = "description = \"stages and commits what you approve\"\n\
                         inherit     = false\n\
                         max_steps   = 6\n\
                         prompt      = \"You prepare commits.\"\n\n\
                         [permissions]\n\
                         \"read files\" = \"allow\"\n\
                         \"git diff\"   = \"allow\"\n";

fn ctrl(c: char) -> Action {
    Action::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL))
}

fn notice(app: &App) -> String {
    app.notice.clone().map(|(n, _)| n).unwrap_or_default()
}

/// The picker open, with the cursor on `name`.
fn picker_on(app: &mut App, tx: &Tx, name: &str) {
    app.open_agent_picker();
    for _ in 0..app.agents.len() {
        let Some(Panel::Agents(p)) = &app.panel else {
            panic!("expected the agent picker")
        };
        if p.current().is_some_and(|it| it.id == name) {
            return;
        }
        app.update(key(KeyCode::Down), tx);
    }
    panic!("{name} is not in the picker");
}

fn editing(app: &App) -> &PromptEdit {
    match &app.panel {
        Some(Panel::AgentPrompt(e)) => e,
        _ => panic!("expected the prompt editor"),
    }
}

#[tokio::test]
async fn the_prompt_is_edited_from_the_picker() {
    let dir = project();
    let agents = agents_dir();
    let path = agents.path().join("committer.toml");
    std::fs::write(&path, COMMITTER).unwrap();
    let (mut app, tx, _rx) = app();
    app.root = dir.path().to_path_buf();
    app.agents_dir = Some(agents.path().to_path_buf());
    app.set_policy(Policy::from_pairs([(READ_FILES, Permission::Allow)]));
    let _ = app.reload_agents();
    app.select_agent("committer");

    // default is a file like any other: its prompt is empty, moon's own
    picker_on(&mut app, &tx, "default");
    app.update(ctrl('e'), &tx);
    assert_eq!(editing(&app).name, "default");
    assert_eq!(editing(&app).prompt.text(), "");
    app.update(key(KeyCode::Esc), &tx);
    assert!(matches!(app.panel, Some(Panel::Agents(_))));

    // ctrl+e opens the file's two fields, the keys on the prompt
    picker_on(&mut app, &tx, "committer");
    app.update(ctrl('e'), &tx);
    let e = editing(&app);
    assert_eq!(e.field, PromptField::Prompt);
    assert_eq!(e.description.text(), "stages and commits what you approve");
    assert_eq!(e.prompt.text(), "You prepare commits.");
    assert!(!e.changed());
    assert!(app.panel_keys().contains(&("esc", "save & close")));

    // enter is a new line in the prompt; ctrl+c with changes only warns,
    // and a second one discards them, back to the picker
    app.update(key(KeyCode::End), &tx);
    app.update(key(KeyCode::Enter), &tx);
    type_text(&mut app, &tx, "Always in Spanish.");
    assert!(editing(&app).changed());
    app.update(ctrl('c'), &tx);
    assert!(
        notice(&app).contains("ctrl+c again to discard"),
        "{}",
        notice(&app)
    );
    assert!(matches!(app.panel, Some(Panel::AgentPrompt(_))));
    app.update(ctrl('c'), &tx);
    assert!(matches!(app.panel, Some(Panel::Agents(_))));
    // the file says what it said (brought up to the catalogue on the
    // way, as every file is when the picker opens)
    let kept = moon_agent::AgentFile::load(&path).unwrap();
    assert_eq!(kept.prompt.as_deref(), Some("You prepare commits."));
    assert_eq!(kept.description, "stages and commits what you approve");
    assert_eq!(kept.max_steps, Some(6));

    // written again, and esc saves on its way out: the file carries it,
    // the rest is kept, and the chosen agent's prompt has it from the
    // next message
    picker_on(&mut app, &tx, "committer");
    app.update(ctrl('e'), &tx);
    app.update(key(KeyCode::End), &tx);
    app.update(key(KeyCode::Enter), &tx);
    type_text(&mut app, &tx, "Always in Spanish.");
    // the description is one line: enter does nothing there, a paste
    // loses its newlines
    app.update(key(KeyCode::Tab), &tx);
    assert_eq!(editing(&app).field, PromptField::Description);
    assert!(app.panel_keys().contains(&("tab", "prompt")));
    app.update(key(KeyCode::Enter), &tx);
    app.update(key(KeyCode::End), &tx);
    app.update(Action::Paste(",\nnothing\nelse".into()), &tx);
    assert_eq!(
        editing(&app).description.text(),
        "stages and commits what you approve, nothing else"
    );
    app.update(key(KeyCode::Esc), &tx);
    assert!(matches!(app.panel, Some(Panel::Agents(_))));
    assert!(
        notice(&app).contains("agent committer saved"),
        "{}",
        notice(&app)
    );
    let saved = moon_agent::AgentFile::load(&path).unwrap();
    assert_eq!(
        saved.prompt.as_deref(),
        Some("You prepare commits.\nAlways in Spanish.\n")
    );
    assert_eq!(
        saved.description,
        "stages and commits what you approve, nothing else"
    );
    assert_eq!(saved.max_steps, Some(6));
    assert_eq!(saved.permissions.get("git diff"), Some(&Permission::Allow));
    let h = app.harness.as_ref().unwrap();
    assert!(h.prompt().contains("Always in Spanish."), "{}", h.prompt());

    // a prompt cleared out is no prompt: the file says so, and the agent
    // gets moon's own — the reader's, since it does not write
    app.update(ctrl('e'), &tx);
    for _ in 0..80 {
        app.update(key(KeyCode::Backspace), &tx);
    }
    assert_eq!(editing(&app).prompt.text(), "");
    app.update(key(KeyCode::Esc), &tx);
    assert!(matches!(app.panel, Some(Panel::Agents(_))));
    let saved = moon_agent::AgentFile::load(&path).unwrap();
    assert_eq!(saved.prompt, None);
    assert!(std::fs::read_to_string(&path)
        .unwrap()
        .contains("# no prompt: moon's own"),);
    assert_eq!(app.harness.as_ref().unwrap().agent().name, "reader");
}
