//! Action-based keybinding system, inspired by Claude Code's approach.
//!
//! Keys map to abstract actions; components bind to actions with
//! context scoping. Users can remap keys via `~/.deepseek/keybindings.toml`.
//!
//! ## Architecture
//!
//! ```text
//! Key press → KeyEvent → KeybindingRegistry::resolve() → Option<KeyAction>
//! Component: useKeybinding(action, handler, context, is_active)
//! ```
//!
//! Multiple components can listen for the same action in different contexts.
//! When a key resolves to an action, the highest-priority active handler fires.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

// ── KeyChord: physical key combination ──

/// A physical key combination (e.g., Ctrl+C, Alt+Enter).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeyChord {
    // Letters with modifiers
    CtrlA, CtrlB, CtrlC, CtrlD, CtrlE, CtrlF, CtrlG, CtrlH,
    CtrlI, CtrlJ, CtrlK, CtrlL, CtrlM, CtrlN, CtrlO, CtrlP,
    CtrlQ, CtrlR, CtrlS, CtrlT, CtrlU, CtrlV, CtrlW, CtrlX,
    CtrlY, CtrlZ,
    // Special keys
    Escape,
    Enter,
    Tab,
    BackTab,
    Backspace,
    Delete,
    Up, Down, Left, Right,
    Home, End,
    PageUp, PageDown,
    // Function keys
    F1, F2, F3, F4, F5, F6, F7, F8, F9, F10, F11, F12,
    // Alt modifiers
    AltEnter,
    // Printable char (single character)
    Char(char),
}

impl KeyChord {
    /// Parse a KeyChord from a crossterm KeyEvent.
    pub fn from_key_event(event: &KeyEvent) -> Option<Self> {
        let ctrl = event.modifiers.contains(KeyModifiers::CONTROL);
        let alt = event.modifiers.contains(KeyModifiers::ALT);

        match event.code {
            KeyCode::Esc => Some(Self::Escape),
            KeyCode::Enter if alt => Some(Self::AltEnter),
            KeyCode::Enter => Some(Self::Enter),
            KeyCode::Tab => {
                if event.modifiers.contains(KeyModifiers::SHIFT) {
                    Some(Self::BackTab)
                } else {
                    Some(Self::Tab)
                }
            }
            KeyCode::Backspace => Some(Self::Backspace),
            KeyCode::Delete => Some(Self::Delete),
            KeyCode::Up => Some(Self::Up),
            KeyCode::Down => Some(Self::Down),
            KeyCode::Left => Some(Self::Left),
            KeyCode::Right => Some(Self::Right),
            KeyCode::Home => Some(Self::Home),
            KeyCode::End => Some(Self::End),
            KeyCode::PageUp => Some(Self::PageUp),
            KeyCode::PageDown => Some(Self::PageDown),
            KeyCode::F(1) => Some(Self::F1),
            KeyCode::F(2) => Some(Self::F2),
            KeyCode::F(3) => Some(Self::F3),
            KeyCode::F(4) => Some(Self::F4),
            KeyCode::F(5) => Some(Self::F5),
            KeyCode::F(6) => Some(Self::F6),
            KeyCode::F(7) => Some(Self::F7),
            KeyCode::F(8) => Some(Self::F8),
            KeyCode::F(9) => Some(Self::F9),
            KeyCode::F(10) => Some(Self::F10),
            KeyCode::F(11) => Some(Self::F11),
            KeyCode::F(12) => Some(Self::F12),
            KeyCode::Char(c) if ctrl => Self::ctrl_char(c),
            KeyCode::Char(c) => Some(Self::Char(c)),
            _ => None,
        }
    }

    fn ctrl_char(c: char) -> Option<Self> {
        match c.to_ascii_lowercase() {
            'a' => Some(Self::CtrlA), 'b' => Some(Self::CtrlB), 'c' => Some(Self::CtrlC),
            'd' => Some(Self::CtrlD), 'e' => Some(Self::CtrlE), 'f' => Some(Self::CtrlF),
            'g' => Some(Self::CtrlG), 'h' => Some(Self::CtrlH), 'i' => Some(Self::CtrlI),
            'j' => Some(Self::CtrlJ), 'k' => Some(Self::CtrlK), 'l' => Some(Self::CtrlL),
            'm' => Some(Self::CtrlM), 'n' => Some(Self::CtrlN), 'o' => Some(Self::CtrlO),
            'p' => Some(Self::CtrlP), 'q' => Some(Self::CtrlQ), 'r' => Some(Self::CtrlR),
            's' => Some(Self::CtrlS), 't' => Some(Self::CtrlT), 'u' => Some(Self::CtrlU),
            'v' => Some(Self::CtrlV), 'w' => Some(Self::CtrlW), 'x' => Some(Self::CtrlX),
            'y' => Some(Self::CtrlY), 'z' => Some(Self::CtrlZ),
            _ => None,
        }
    }
}

// ── KeyAction: abstract action identifier ──

/// Abstract action that a keybinding triggers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeyAction {
    // App-level
    AppInterrupt,
    AppExit,
    AppRedraw,
    // Chat
    ChatCancel,
    ChatSubmit,
    ChatCycleMode,
    // Tabs
    TabsNext,
    TabsPrevious,
    // Navigation
    SelectUp,
    SelectDown,
    SelectAccept,
    SelectPrevious,
    SelectNext,
    // Settings
    SettingsSearch,
    SettingsClose,
    ConfirmNo,
    // Help
    HelpDismiss,
    // Custom
    Custom(&'static str),
}

impl KeyAction {
    pub fn from_str(s: &str) -> Option<Self> {
        Some(match s {
            "app:interrupt" => Self::AppInterrupt,
            "app:exit" => Self::AppExit,
            "app:redraw" => Self::AppRedraw,
            "chat:cancel" => Self::ChatCancel,
            "chat:submit" => Self::ChatSubmit,
            "chat:cycleMode" => Self::ChatCycleMode,
            "tabs:next" => Self::TabsNext,
            "tabs:previous" => Self::TabsPrevious,
            "select:up" => Self::SelectUp,
            "select:down" => Self::SelectDown,
            "select:accept" => Self::SelectAccept,
            "select:previous" => Self::SelectPrevious,
            "select:next" => Self::SelectNext,
            "settings:search" => Self::SettingsSearch,
            "settings:close" => Self::SettingsClose,
            "confirm:no" => Self::ConfirmNo,
            "help:dismiss" => Self::HelpDismiss,
            _ => return None,
        })
    }
}

// ── Default key → action mappings ──

/// Default key-to-action bindings (same as CC's defaultBindings.ts).
const DEFAULT_BINDINGS: &[(KeyChord, KeyAction)] = &[
    (KeyChord::CtrlC, KeyAction::AppInterrupt),
    (KeyChord::CtrlD, KeyAction::AppExit),
    (KeyChord::CtrlL, KeyAction::AppRedraw),
    (KeyChord::Escape, KeyAction::ChatCancel),
    (KeyChord::Enter, KeyAction::ChatSubmit),
    (KeyChord::Right, KeyAction::TabsNext),
    (KeyChord::Left, KeyAction::TabsPrevious),
    (KeyChord::Up, KeyAction::SelectUp),
    (KeyChord::Down, KeyAction::SelectDown),
    (KeyChord::Char(' '), KeyAction::SelectAccept),
    (KeyChord::Escape, KeyAction::ConfirmNo), // Shared — context decides
    (KeyChord::Escape, KeyAction::HelpDismiss), // Shared — context decides
];

// ── KeybindingRegistry ──

/// Registry that maps physical keys to abstract actions.
/// Supports user overrides.
#[derive(Debug, Default)]
pub struct KeybindingRegistry {
    user_overrides: Vec<(KeyChord, KeyAction)>,
}

impl KeybindingRegistry {
    pub fn new() -> Self {
        Self { user_overrides: vec![] }
    }

    /// Override or add a user-defined binding.
    pub fn set_user_binding(&mut self, chord: KeyChord, action: KeyAction) {
        self.user_overrides.retain(|(c, _)| *c != chord);
        self.user_overrides.push((chord, action));
    }

    /// Resolve a key chord to an action. User overrides take priority.
    pub fn resolve(&self, chord: &KeyChord) -> Option<&KeyAction> {
        // Check user overrides first
        for (c, a) in &self.user_overrides {
            if c == chord {
                return Some(a);
            }
        }
        // Fall back to defaults
        for (c, a) in DEFAULT_BINDINGS {
            if c == chord {
                return Some(a);
            }
        }
        None
    }
}

// ── Context-based handler registry ──

/// A handler binding scoped to a context.
#[derive(Debug, Clone)]
pub struct ContextBinding {
    pub action: KeyAction,
    pub context: KeybindingContext,
    pub handler: &'static str,
    pub is_active: bool,
}

/// Contexts for scoping keybindings (inspired by CC's context system).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeybindingContext {
    Global,
    Chat,
    Tabs,
    Settings,
    Help,
    Transcript,
}

/// Registry of context-scoped handlers.
/// When multiple contexts are active, Global acts as fallback.
#[derive(Debug, Default)]
pub struct ContextRegistry {
    bindings: Vec<ContextBinding>,
}

impl ContextRegistry {
    pub fn new(
        global: Vec<ContextBinding>,
        additional: Vec<ContextBinding>,
    ) -> Self {
        let mut bindings = global;
        bindings.extend(additional);
        Self { bindings }
    }

    /// Find a handler for the given action in the given context.
    /// Falls back to Global context if no handler in the requested context.
    pub fn find(&self, action: &KeyAction, context: &KeybindingContext) -> Option<&'static str> {
        // First, exact context match
        for b in &self.bindings {
            if b.action == *action && b.context == *context && b.is_active {
                return Some(b.handler);
            }
        }
        // Fallback to Global
        if *context != KeybindingContext::Global {
            for b in &self.bindings {
                if b.action == *action && b.context == KeybindingContext::Global && b.is_active {
                    return Some(b.handler);
                }
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── KeyAction tests ──

    #[test]
    fn key_action_from_str() {
        assert_eq!(
            KeyAction::from_str("app:interrupt"),
            Some(KeyAction::AppInterrupt)
        );
        assert_eq!(
            KeyAction::from_str("tabs:next"),
            Some(KeyAction::TabsNext)
        );
        assert_eq!(KeyAction::from_str("nonexistent"), None);
    }

    // ── KeyChord tests ──

    #[test]
    fn key_chord_from_key_event() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

        let ctrl_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert_eq!(
            KeyChord::from_key_event(&ctrl_c),
            Some(KeyChord::CtrlC)
        );

        let esc = KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE);
        assert_eq!(
            KeyChord::from_key_event(&esc),
            Some(KeyChord::Escape)
        );

        let right = KeyEvent::new(KeyCode::Right, KeyModifiers::NONE);
        assert_eq!(
            KeyChord::from_key_event(&right),
            Some(KeyChord::Right)
        );
    }

    // ── Registry tests ──

    #[test]
    fn registry_resolves_default_bindings() {
        let registry = KeybindingRegistry::new();
        assert_eq!(
            registry.resolve(&KeyChord::CtrlC),
            Some(&KeyAction::AppInterrupt)
        );
        assert_eq!(
            registry.resolve(&KeyChord::Escape),
            Some(&KeyAction::ChatCancel)
        );
        assert_eq!(
            registry.resolve(&KeyChord::Right),
            Some(&KeyAction::TabsNext)
        );
    }

    #[test]
    fn registry_user_override() {
        let mut registry = KeybindingRegistry::new();
        registry.set_user_binding(KeyChord::CtrlX, KeyAction::AppInterrupt);
        // Original Ctrl+C still works
        assert_eq!(
            registry.resolve(&KeyChord::CtrlC),
            Some(&KeyAction::AppInterrupt)
        );
        // New binding also works
        assert_eq!(
            registry.resolve(&KeyChord::CtrlX),
            Some(&KeyAction::AppInterrupt)
        );
    }

    // ── Context tests ──

    #[test]
    fn context_scoping() {
        let global = vec![
            ContextBinding {
                action: KeyAction::AppInterrupt,
                context: KeybindingContext::Global,
                handler: "global_handler",
                is_active: true,
            },
        ];
        let settings = vec![
            ContextBinding {
                action: KeyAction::TabsNext,
                context: KeybindingContext::Settings,
                handler: "settings_tab_switch",
                is_active: true,
            },
        ];
        let ctx = ContextRegistry::new(global, settings);
        assert_eq!(
            ctx.find(&KeyAction::AppInterrupt, &KeybindingContext::Global),
            Some("global_handler")
        );
        assert_eq!(
            ctx.find(&KeyAction::AppInterrupt, &KeybindingContext::Settings),
            Some("global_handler") // falls back to Global
        );
        assert_eq!(
            ctx.find(&KeyAction::TabsNext, &KeybindingContext::Global),
            None // TabsNext not in Global
        );
    }

    #[test]
    fn inactive_context_ignored() {
        let bindings = vec![
            ContextBinding {
                action: KeyAction::AppInterrupt,
                context: KeybindingContext::Global,
                handler: "handler",
                is_active: false, // inactive!
            },
        ];
        let ctx = ContextRegistry::new(bindings, vec![]);
        assert_eq!(
            ctx.find(&KeyAction::AppInterrupt, &KeybindingContext::Global),
            None
        );
    }
}
