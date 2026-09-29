//! `ctrl+e` in the `/agent` picker: the agent's description and prompt
//! edited in two fields, docked like every panel, and written back to its
//! file — `esc` saves on its way out, `ctrl+c` discards.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use moon_agent::AgentFile;

use super::*;
use crate::input::ChatInput;

/// Which of the two fields the keys go to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptField {
    Description,
    Prompt,
}

/// One agent's description and prompt being edited, over the file as it
/// was opened: everything else in it is kept.
pub struct PromptEdit {
    pub name: String,
    pub file: AgentFile,
    pub description: ChatInput,
    pub prompt: ChatInput,
    pub field: PromptField,
    /// `ctrl+c` with changes said so once; the next one discards them.
    pub warned: bool,
}

impl PromptEdit {
    pub fn new(name: &str, file: AgentFile) -> Self {
        let mut description = ChatInput::new();
        description.max_height = 1;
        description.set_text(file.description.trim());
        let mut prompt = ChatInput::new();
        prompt.max_height = u16::MAX;
        prompt.set_text(file.prompt.as_deref().unwrap_or("").trim_end());
        Self {
            name: name.to_string(),
            file,
            description,
            prompt,
            field: PromptField::Prompt,
            warned: false,
        }
    }

    /// The field the keys go to.
    pub fn focused(&mut self) -> &mut ChatInput {
        match self.field {
            PromptField::Description => &mut self.description,
            PromptField::Prompt => &mut self.prompt,
        }
    }

    /// Something differs from the file as it was opened.
    pub fn changed(&self) -> bool {
        self.description.text().trim() != self.file.description.trim()
            || self.prompt.text().trim_end() != self.file.prompt.as_deref().unwrap_or("").trim_end()
    }

    /// The file with what was typed: the prompt ends in a newline so the
    /// closing `"""` sits on a line of its own, and an empty one is no
    /// prompt — moon's own.
    pub(super) fn edited(&self) -> AgentFile {
        let description = self.description.text().trim().to_string();
        let prompt = self.prompt.text().trim_end().to_string();
        AgentFile {
            description,
            prompt: (!prompt.trim().is_empty()).then(|| format!("{prompt}\n")),
            ..self.file.clone()
        }
    }

    /// Text pasted in: the description is one line, so its newlines turn
    /// into spaces.
    pub fn paste(&mut self, s: &str) {
        self.warned = false;
        match self.field {
            PromptField::Description => {
                let one: String = s
                    .chars()
                    .map(|c| if c == '\n' || c == '\r' { ' ' } else { c })
                    .filter(|c| !c.is_control())
                    .collect();
                self.description.insert_str(&one);
            }
            PromptField::Prompt => self.prompt.insert_str(&s.replace("\r\n", "\n")),
        }
    }
}

impl App {
    /// `ctrl+e` in the picker: the highlighted agent's description and
    /// prompt, from the definition as last read.
    pub(super) fn open_agent_prompt(&mut self, name: &str) {
        let Some(def) = self.def_of(name) else {
            self.notify(format!("no agent named {name}"));
            return;
        };
        let file = AgentFile::from_def(&def);
        self.panel = Some(Panel::AgentPrompt(Box::new(PromptEdit::new(name, file))));
    }

    pub(super) fn handle_agent_prompt_key(&mut self, key: KeyEvent) {
        enum Next {
            Stay,
            /// Said once: unsaved changes, ctrl+c again.
            Warn,
            Save,
            Close,
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let Some(Panel::AgentPrompt(e)) = self.panel.as_mut() else {
            return;
        };
        let next = match key.code {
            // as in the table: esc saves on its way out; ctrl+c discards,
            // and with changes the first one only says so
            KeyCode::Esc => {
                if e.changed() {
                    Next::Save
                } else {
                    Next::Close
                }
            }
            KeyCode::Char('c') if ctrl => {
                if e.changed() && !e.warned {
                    e.warned = true;
                    Next::Warn
                } else {
                    Next::Close
                }
            }
            KeyCode::Char('s') if ctrl => Next::Save,
            KeyCode::Tab | KeyCode::BackTab => {
                e.warned = false;
                e.field = match e.field {
                    PromptField::Description => PromptField::Prompt,
                    PromptField::Prompt => PromptField::Description,
                };
                Next::Stay
            }
            // a new line in the prompt; the description is one line
            KeyCode::Enter => {
                if e.field == PromptField::Prompt {
                    e.prompt.newline();
                }
                Next::Stay
            }
            KeyCode::Backspace => {
                e.focused().backspace();
                Next::Stay
            }
            KeyCode::Delete => {
                e.focused().delete();
                Next::Stay
            }
            KeyCode::Char('u') if ctrl => {
                e.focused().kill_to_start();
                Next::Stay
            }
            KeyCode::Char('w') if ctrl => {
                e.focused().delete_word_back();
                Next::Stay
            }
            KeyCode::Char('a') if ctrl => {
                e.focused().home();
                Next::Stay
            }
            KeyCode::Char('e') if ctrl => {
                e.focused().end();
                Next::Stay
            }
            KeyCode::Char(ch) if !ctrl => {
                e.focused().insert_char(ch);
                Next::Stay
            }
            KeyCode::Left => {
                e.focused().left();
                Next::Stay
            }
            KeyCode::Right => {
                e.focused().right();
                Next::Stay
            }
            KeyCode::Up => {
                e.focused().up();
                Next::Stay
            }
            KeyCode::Down => {
                e.focused().down();
                Next::Stay
            }
            KeyCode::Home => {
                e.focused().home();
                Next::Stay
            }
            KeyCode::End => {
                e.focused().end();
                Next::Stay
            }
            _ => Next::Stay,
        };
        match next {
            Next::Stay => {}
            Next::Warn => self.notify("unsaved changes · ctrl+c again to discard"),
            Next::Close => {
                self.panel = None;
                self.open_agent_picker();
            }
            Next::Save => {
                let Some(Panel::AgentPrompt(e)) = self.panel.as_ref() else {
                    return;
                };
                let name = e.name.clone();
                let def = match e.edited().def(&name) {
                    Ok(d) => d,
                    Err(why) => {
                        self.notify(why);
                        return;
                    }
                };
                self.save_def(def);
                self.panel = None;
                self.open_agent_picker();
                self.notify(format!("agent {name} saved"));
            }
        }
    }
}
