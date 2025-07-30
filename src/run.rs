use std::error::Error;
use std::time::{Duration, Instant};

use crossterm::event::{self, KeyCode, KeyEvent, KeyModifiers};

use crate::bad::App;
use crate::{Action, PaneAction, MoveTarget};


enum AfterActions {
    Render,
    Quit,
    Noop,
}

impl App {
    pub fn run(mut self, mut out: &mut dyn std::io::Write) -> Result<(), Box<dyn Error>> {
        const POLL_TIMEOUT: Duration = Duration::from_millis(16);

        let mut need_to_render = true;
        loop {
            let frame = Instant::now();
            if need_to_render {
                self.render(&mut out)?;
            }
            while crossterm::event::poll(POLL_TIMEOUT.saturating_sub(frame.elapsed()))? {
                let event = crossterm::event::read()?;
                let action = get_action(&event);
                self.enqueue(action);
            }
            match self.process_queued_actions() {
                AfterActions::Quit => return Ok(()),
                AfterActions::Render => need_to_render = true,
                AfterActions::Noop => need_to_render = false,
            }
        }
    }

    pub fn enqueue(&mut self, action: Action) {
        self.action_queue.push_back(action);
    }

    fn process_queued_actions(&mut self) -> AfterActions {
        let mut after = AfterActions::Noop;
        while let Some(action) = self.action_queue.pop_front() {
            match action {
                Action::Quit => return AfterActions::Quit,
                action => {
                    after = AfterActions::Render;
                    self.handle_action(action);
                }
            }
        }
        after
    }
}

pub fn get_action(ev: &event::Event) -> Action {
    use event::Event::*;
    match ev.to_owned() {
        FocusGained => Action::None,
        FocusLost => Action::None,
        Resize(_, _) => Action::None,
        Mouse(_) => todo!(),
        // Only emitted when bracketed paste has been enabled
        Paste(s) => Action::HandledByPane(PaneAction::Insert(s)),
        Key(
            kevent @ KeyEvent {
                code,
                modifiers,
                kind: _,
                state: _,
            },
        ) => {
            let ctrl = modifiers.contains(KeyModifiers::CONTROL);
            let shift = modifiers.contains(KeyModifiers::SHIFT);
            let only_shift = (modifiers - KeyModifiers::SHIFT).is_empty();
            // TODO: no hard coding, read keybindings from a config file
            match code {
                KeyCode::Char('q') if ctrl => Action::Quit,
                KeyCode::Char('e') if ctrl => Action::CommandPrompt,
                KeyCode::Char('o') if ctrl => Action::CommandPromptEdit("open ".into()),
                KeyCode::Char('z') if ctrl => Action::HandledByPane(PaneAction::Undo),
                KeyCode::Char('y') if ctrl => Action::HandledByPane(PaneAction::Redo),
                KeyCode::Char('f') if ctrl => Action::CommandPromptEdit("find ".into()),
                KeyCode::Char('b') if ctrl => Action::HandledByPane(PaneAction::RepeatFindBackward),
                KeyCode::Char('n') if ctrl => Action::HandledByPane(PaneAction::RepeatFind),
                KeyCode::Char('d') if ctrl => Action::HandledByPane(PaneAction::QuickAddNext),
                KeyCode::Char('c') if ctrl => Action::Copy,
                KeyCode::Char('v') if ctrl => Action::Paste,
                KeyCode::Char(c) if only_shift => Action::HandledByPane(PaneAction::Insert(c.to_string())),
                KeyCode::Up =>
                    if shift { Action::HandledByPane(PaneAction::SelectTo(MoveTarget::Up(1))) }
                    else     { Action::HandledByPane(PaneAction::MoveTo(MoveTarget::Up(1))) },
                KeyCode::Down =>
                    if shift { Action::HandledByPane(PaneAction::SelectTo(MoveTarget::Down(1))) }
                    else     { Action::HandledByPane(PaneAction::MoveTo(MoveTarget::Down(1))) },
                KeyCode::Left =>
                    if shift { Action::HandledByPane(PaneAction::SelectTo(MoveTarget::Left(1))) }
                    else     { Action::HandledByPane(PaneAction::MoveTo(MoveTarget::Left(1))) },
                KeyCode::Right =>
                    if shift { Action::HandledByPane(PaneAction::SelectTo(MoveTarget::Right(1))) }
                    else     { Action::HandledByPane(PaneAction::MoveTo(MoveTarget::Right(1))) },
                KeyCode::Home if ctrl =>
                    if shift { Action::HandledByPane(PaneAction::SelectTo(MoveTarget::Start)) }
                    else     { Action::HandledByPane(PaneAction::MoveTo(MoveTarget::Start)) },
                KeyCode::Home =>
                    if shift { Action::HandledByPane(PaneAction::SelectTo(MoveTarget::StartOfLine)) }
                    else     { Action::HandledByPane(PaneAction::MoveTo(MoveTarget::StartOfLine)) },
                KeyCode::End if ctrl =>
                    if shift { Action::HandledByPane(PaneAction::SelectTo(MoveTarget::End)) }
                    else     { Action::HandledByPane(PaneAction::MoveTo(MoveTarget::End)) },
                KeyCode::End =>
                    if shift { Action::HandledByPane(PaneAction::SelectTo(MoveTarget::EndOfLine)) }
                    else     { Action::HandledByPane(PaneAction::MoveTo(MoveTarget::EndOfLine)) },
                KeyCode::PageUp => Action::HandledByPane(PaneAction::MoveTo(MoveTarget::Up(25))),
                KeyCode::PageDown => Action::HandledByPane(PaneAction::MoveTo(MoveTarget::Down(25))),
                KeyCode::Enter => Action::HandledByPane(PaneAction::Insert("\n".into())),
                KeyCode::Tab => Action::HandledByPane(PaneAction::Indent),
                KeyCode::BackTab => Action::HandledByPane(PaneAction::Dedent),
                KeyCode::Backspace => Action::HandledByPane(PaneAction::DeleteBackward),
                KeyCode::Delete => Action::HandledByPane(PaneAction::DeleteForward),
                KeyCode::Esc => Action::Esc,
                _ => Action::SetInfo(format!("{kevent:?}")),
            }
        }
    }
}
