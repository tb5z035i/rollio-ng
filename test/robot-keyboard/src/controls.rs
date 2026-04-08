use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JogMagnitude {
    Small,
    Large,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlAction {
    Quit,
    NextRobot,
    SelectRobot(usize),
    SelectPrevJoint,
    SelectNextJoint,
    JogActiveJoint {
        direction: i8,
        magnitude: JogMagnitude,
    },
    ResetActiveTarget,
    ToggleMode,
}

pub fn map_key_event(key: KeyEvent) -> Option<ControlAction> {
    if matches!(key.kind, KeyEventKind::Release) {
        return None;
    }

    match key.code {
        KeyCode::Esc => Some(ControlAction::Quit),
        KeyCode::Tab => Some(ControlAction::NextRobot),
        KeyCode::Left | KeyCode::Char('[') => Some(ControlAction::SelectPrevJoint),
        KeyCode::Right | KeyCode::Char(']') => Some(ControlAction::SelectNextJoint),
        KeyCode::Up => Some(ControlAction::JogActiveJoint {
            direction: 1,
            magnitude: if key.modifiers.contains(KeyModifiers::SHIFT) {
                JogMagnitude::Large
            } else {
                JogMagnitude::Small
            },
        }),
        KeyCode::Down => Some(ControlAction::JogActiveJoint {
            direction: -1,
            magnitude: if key.modifiers.contains(KeyModifiers::SHIFT) {
                JogMagnitude::Large
            } else {
                JogMagnitude::Small
            },
        }),
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            Some(ControlAction::Quit)
        }
        KeyCode::Char('q') => Some(ControlAction::Quit),
        KeyCode::Char('r') => Some(ControlAction::ResetActiveTarget),
        KeyCode::Char('m') => Some(ControlAction::ToggleMode),
        KeyCode::Char('j') => Some(ControlAction::JogActiveJoint {
            direction: -1,
            magnitude: JogMagnitude::Small,
        }),
        KeyCode::Char('k') => Some(ControlAction::JogActiveJoint {
            direction: 1,
            magnitude: JogMagnitude::Small,
        }),
        KeyCode::Char('J') => Some(ControlAction::JogActiveJoint {
            direction: -1,
            magnitude: JogMagnitude::Large,
        }),
        KeyCode::Char('K') => Some(ControlAction::JogActiveJoint {
            direction: 1,
            magnitude: JogMagnitude::Large,
        }),
        KeyCode::Char('+') | KeyCode::Char('=') => Some(ControlAction::JogActiveJoint {
            direction: 1,
            magnitude: JogMagnitude::Large,
        }),
        KeyCode::Char('-') | KeyCode::Char('_') => Some(ControlAction::JogActiveJoint {
            direction: -1,
            magnitude: JogMagnitude::Large,
        }),
        KeyCode::Char(digit @ '1'..='9') => Some(ControlAction::SelectRobot(
            digit.to_digit(10).unwrap_or(1) as usize - 1,
        )),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{map_key_event, ControlAction, JogMagnitude};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    #[test]
    fn maps_navigation_keys() {
        let action = map_key_event(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(action, Some(ControlAction::NextRobot));

        let action = map_key_event(KeyEvent::new(KeyCode::Char('3'), KeyModifiers::NONE));
        assert_eq!(action, Some(ControlAction::SelectRobot(2)));

        let action = map_key_event(KeyEvent::new(KeyCode::Char(']'), KeyModifiers::NONE));
        assert_eq!(action, Some(ControlAction::SelectNextJoint));
    }

    #[test]
    fn maps_jog_keys() {
        let action = map_key_event(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        assert_eq!(
            action,
            Some(ControlAction::JogActiveJoint {
                direction: 1,
                magnitude: JogMagnitude::Small,
            })
        );

        let action = map_key_event(KeyEvent::new(KeyCode::Up, KeyModifiers::SHIFT));
        assert_eq!(
            action,
            Some(ControlAction::JogActiveJoint {
                direction: 1,
                magnitude: JogMagnitude::Large,
            })
        );

        let action = map_key_event(KeyEvent::new(KeyCode::Char('-'), KeyModifiers::NONE));
        assert_eq!(
            action,
            Some(ControlAction::JogActiveJoint {
                direction: -1,
                magnitude: JogMagnitude::Large,
            })
        );
    }
}
