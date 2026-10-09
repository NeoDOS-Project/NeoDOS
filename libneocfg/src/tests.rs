//! Host tests for the UI-agnostic core, using the mock seams.

use crate::i18n_keys as k;
use crate::mocks::{MockPlatform, MockTranslator, MockUi};
use crate::model::{FieldValue, Intent, Text, View};
use crate::platform::{CfgPlatform, PowerPlan};
use crate::{App, MODULES};

fn message_body(ui: &MockUi, index: usize) -> &[Text] {
    match &ui.views[index] {
        View::Message { body, .. } => body,
        other => panic!("view {index} is not a Message: {other:?}"),
    }
}

#[test]
fn registry_lists_the_five_modules_in_order() {
    let ids: Vec<_> = MODULES.iter().map(|m| m.id()).collect();
    assert_eq!(ids.len(), 5);
    assert_eq!(ids[0], crate::ModuleId::System);
    assert_eq!(ids[1], crate::ModuleId::Power);
    assert_eq!(ids[2], crate::ModuleId::Locale);
    assert_eq!(ids[3], crate::ModuleId::Keyboard);
    assert_eq!(ids[4], crate::ModuleId::About);
}

#[test]
fn main_menu_renders_all_modules() {
    let tr = MockTranslator::new();
    let mut ui = MockUi::new(vec![]);
    let app = App::new(MODULES, &tr, &mut ui);
    match app.main_menu_view() {
        View::Menu {
            items, selected, ..
        } => {
            assert_eq!(items.len(), 5);
            assert_eq!(selected, 0);
            assert!(items.iter().all(|i| i.enabled));
        }
        other => panic!("expected Menu, got {other:?}"),
    }
}

#[test]
fn quit_exits_with_first_view_being_the_menu() {
    let tr = MockTranslator::new();
    let mut ui = MockUi::new(vec![Intent::Quit]);
    let platform = MockPlatform::default();
    {
        let mut app = App::new(MODULES, &tr, &mut ui);
        app.run(&platform).expect("run is infallible for the stubs");
    }
    assert_eq!(ui.views.len(), 1);
    assert!(matches!(ui.views[0], View::Menu { .. }));
}

#[test]
fn esc_at_main_menu_exits() {
    let tr = MockTranslator::new();
    let mut ui = MockUi::new(vec![Intent::Back]);
    let platform = MockPlatform::default();
    let mut app = App::new(MODULES, &tr, &mut ui);
    app.run(&platform).unwrap();
    // Back at top level terminates without entering a module.
    assert_eq!(ui.views.len(), 1);
}

#[test]
fn selecting_system_runs_it_and_back_returns_to_menu() {
    let tr = MockTranslator::new();
    let mut ui = MockUi::new(vec![Intent::Select(0), Intent::Back]);
    let platform = MockPlatform::default();
    {
        let mut app = App::new(MODULES, &tr, &mut ui);
        app.run(&platform).unwrap();
    }
    assert_eq!(ui.views.len(), 3);
    assert!(matches!(ui.views[0], View::Menu { .. }));
    match &ui.views[1] {
        View::Message { title, .. } => assert_eq!(*title, Text::Key(k::MODULE_SYSTEM_NAME)),
        other => panic!("expected System message, got {other:?}"),
    }
    assert!(matches!(ui.views[2], View::Menu { .. }));
}

#[test]
fn keyboard_stub_is_reachable_by_letter_shortcut() {
    // Index 3 -> the fourth entry; the UI would map '4' to Select(3).
    let tr = MockTranslator::new();
    let mut ui = MockUi::new(vec![Intent::Select(3), Intent::Char('x')]);
    let platform = MockPlatform::default();
    {
        let mut app = App::new(MODULES, &tr, &mut ui);
        app.run(&platform).unwrap();
    }
    match &ui.views[1] {
        View::Message { title, .. } => assert_eq!(*title, Text::Key(k::MODULE_KEYBOARD_NAME)),
        other => panic!("expected Keyboard message, got {other:?}"),
    }
}

#[test]
fn power_stub_reports_not_available_without_power_manager() {
    let tr = MockTranslator::new();
    let mut ui = MockUi::new(vec![Intent::Select(1), Intent::Char('x')]);
    let platform = MockPlatform::default(); // power() == None
    {
        let mut app = App::new(MODULES, &tr, &mut ui);
        app.run(&platform).unwrap();
    }
    let body = message_body(&ui, 1);
    assert!(body.contains(&Text::Key(k::POWER_NOT_AVAILABLE)));
    assert!(!body.contains(&Text::Key(k::MODULE_PENDING)));
}

#[test]
fn power_stub_switches_to_ready_path_when_power_manager_exists() {
    let tr = MockTranslator::new();
    let mut ui = MockUi::new(vec![Intent::Select(1), Intent::Char('x')]);
    let platform = MockPlatform::with_power(PowerPlan::Performance);
    {
        let mut app = App::new(MODULES, &tr, &mut ui);
        app.run(&platform).unwrap();
    }
    let body = message_body(&ui, 1);
    assert!(!body.contains(&Text::Key(k::POWER_NOT_AVAILABLE)));
    assert!(body.contains(&Text::Key(k::MODULE_PENDING)));
}

#[test]
fn locale_stub_reports_not_available_without_i18n_runtime() {
    let tr = MockTranslator::new();
    let mut ui = MockUi::new(vec![Intent::Select(2), Intent::Char('x')]);
    let platform = MockPlatform::default(); // locale() == None
    {
        let mut app = App::new(MODULES, &tr, &mut ui);
        app.run(&platform).unwrap();
    }
    let body = message_body(&ui, 1);
    assert!(body.contains(&Text::Key(k::LOCALE_NOT_AVAILABLE)));
}

#[test]
fn locale_stub_switches_to_ready_path_when_runtime_exists() {
    let tr = MockTranslator::new();
    let mut ui = MockUi::new(vec![Intent::Select(2), Intent::Char('x')]);
    let platform = MockPlatform::with_locale("es-ES", &["en-US", "es-ES"]);
    {
        let mut app = App::new(MODULES, &tr, &mut ui);
        app.run(&platform).unwrap();
    }
    let body = message_body(&ui, 1);
    assert!(!body.contains(&Text::Key(k::LOCALE_NOT_AVAILABLE)));
    assert!(body.contains(&Text::Key(k::MODULE_PENDING)));
}

#[test]
fn about_is_reachable_by_activate_after_navigation() {
    // Down x4 moves the highlight to About, then Enter activates it.
    let tr = MockTranslator::new();
    let mut ui = MockUi::new(vec![
        Intent::Down,
        Intent::Down,
        Intent::Down,
        Intent::Down,
        Intent::Activate,
        Intent::Char('x'),
    ]);
    let platform = MockPlatform::default();
    {
        let mut app = App::new(MODULES, &tr, &mut ui);
        app.run(&platform).unwrap();
    }
    // views: Menu x5, About(detail), Menu
    match &ui.views[5] {
        View::Detail { title, .. } => assert_eq!(*title, Text::Key(k::ABOUT_TITLE)),
        other => panic!("expected About detail, got {other:?}"),
    }
}

#[test]
fn about_shows_version_arch_and_neofs() {
    let tr = MockTranslator::new();
    let mut ui = MockUi::new(vec![Intent::Select(4), Intent::Char('x')]);
    let platform = MockPlatform::default();
    {
        let mut app = App::new(MODULES, &tr, &mut ui);
        app.run(&platform).unwrap();
    }

    let fields = match &ui.views[1] {
        View::Detail { title, fields, .. } => {
            assert_eq!(*title, Text::Key(k::ABOUT_TITLE));
            fields
        }
        other => panic!("expected About detail, got {other:?}"),
    };

    let text_of = |key: u32| -> &str {
        fields
            .iter()
            .find(|f| f.label == Text::Key(key))
            .and_then(|f| match &f.value {
                FieldValue::Text(Text::Owned(v)) => Some(v.as_str()),
                _ => None,
            })
            .unwrap_or_else(|| panic!("missing field {key}"))
    };

    assert!(text_of(k::ABOUT_NEODOS).contains("v0."));
    assert_eq!(text_of(k::ABOUT_ARCH), "x86_64");
    assert!(text_of(k::ABOUT_NEOFS).contains("NE2"));
    assert!(text_of(k::ABOUT_ABI).contains('8'));
    assert!(text_of(k::ABOUT_LIBNEODOS).contains('7'));
}

#[test]
fn about_is_read_only() {
    // About must not mutate platform state: a second view is identical.
    let platform = MockPlatform::default();
    let session = {
        let m = MODULES[4];
        assert_eq!(m.id(), crate::ModuleId::About);
        m.create()
    };
    let first = session.view(&platform);
    let second = session.view(&platform);
    assert_eq!(first, second);
}

#[test]
fn keyboard_layout_roundtrips_through_the_platform_seam() {
    let platform = MockPlatform::default();
    assert_eq!(platform.keyboard_layout().unwrap(), "us");
    platform.set_keyboard_layout("es").unwrap();
    assert_eq!(platform.keyboard_layout().unwrap(), "es");
}

#[test]
fn version_error_is_propagated_by_the_mock() {
    let mut platform = MockPlatform::default();
    platform.version_error = Some(crate::CfgError::ModuleUnavailable);
    assert_eq!(platform.version().unwrap_err(), crate::CfgError::ModuleUnavailable);
}
