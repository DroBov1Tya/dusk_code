use super::*;
use crate::app::{Choice, Confirm, PackPage, choices};
use dusk_protocol::{
    catalog::PackSkill,
    plugin::{Client, Os, SourceFailure},
};
use ratatui::{Terminal, backend::TestBackend, widgets::Paragraph};

fn listing(name: &str) -> Listing {
    Listing {
        name: name.into(),
        version: "1.0.0".into(),
        description: format!("{name} does a thing"),
        source: "official".into(),
        ..Listing::default()
    }
}

fn store() -> Vec<Listing> {
    let mut notes = listing("notes");
    notes.clients = vec![Client::Gui];
    notes.permissions = vec!["reach https://api.example.com".into()];
    notes.stars = 42;
    let mut weather = listing("weather");
    weather.installed = Some("0.9.0".into());
    weather.update = Some("1.0.0".into());
    let mut screen = listing("screen");
    screen.platforms = vec![Os::Windows];
    screen.unfit = Some("only for windows".into());
    // A store text must not steer the terminal.
    screen.description = "grabs\x1b[2J the screen".into();
    let mut mirror = listing("mirror-notes");
    mirror.tier = Tier::Verified;
    mirror.source = "dusk-mirror".into();
    mirror.clash = true;
    vec![notes, weather, screen, mirror]
}

fn packs() -> Vec<SkillPack> {
    vec![SkillPack {
        name: "duskcode".into(),
        title: String::new(),
        description: String::new(),
        builtin: true,
        enabled: true,
        skills: vec![
            PackSkill {
                name: "duskcode".into(),
                enabled: true,
            },
            PackSkill {
                name: "token-thrift".into(),
                enabled: false,
            },
        ],
    }]
}

fn render(view: &Workshop) -> String {
    let (lines, _) = workshop(view, 70);
    let lines = super::super::frame::fit(lines, 72);
    let mut terminal = Terminal::new(TestBackend::new(72, 60)).expect("test terminal");
    terminal
        .draw(|frame| frame.render_widget(Paragraph::new(lines), frame.area()))
        .expect("draw");
    let buffer = terminal.backend().buffer();
    (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
                .trim_end()
                .to_owned()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn list_shows_sections_marks_state_and_unfit_reason() {
    let view = Workshop {
        listings: Some(store()),
        packs: Some(packs()),
        failed: vec![SourceFailure {
            source: "old-mirror".into(),
            error: "unsigned".into(),
        }],
        ..Workshop::default()
    };
    let screen = render(&view);
    assert!(screen.contains("\u{2713} Official"), "{screen}");
    assert!(screen.contains("\u{25C6} Verified"), "{screen}");
    assert!(screen.contains("\u{2713} notes 1.0.0"), "{screen}");
    assert!(screen.contains("\u{2605} 42"), "{screen}");
    assert!(screen.contains("for gui"), "{screen}");
    assert!(screen.contains("update to 1.0.0"), "{screen}");
    assert!(screen.contains("unfit"), "{screen}");
    assert!(
        screen.contains("cannot install here: only for windows"),
        "{screen}"
    );
    assert!(
        screen.contains("another source offers the same name"),
        "{screen}"
    );
    assert!(screen.contains("Built-in and local packs"), "{screen}");
    assert!(screen.contains("1 of 2 skills on"), "{screen}");
    assert!(
        screen.contains("old-mirror could not be read: unsigned"),
        "{screen}"
    );
    assert!(screen.contains("grabs [2J the screen"), "{screen}");
    assert!(!screen.contains('\x1b'));
}

#[test]
fn loading_and_filter() {
    let mut view = Workshop::default();
    assert!(render(&view).contains("Loading the store"));
    view.listings = Some(store());
    view.filter = "weath".into();
    let screen = render(&view);
    assert!(screen.contains("weather"), "{screen}");
    assert!(!screen.contains("notes 1.0.0"), "{screen}");
}

#[test]
fn pack_page_lists_skills_with_switches() {
    let view = Workshop {
        listings: Some(store()),
        packs: Some(packs()),
        pack: Some(PackPage {
            name: "duskcode".into(),
            selected: 0,
        }),
        ..Workshop::default()
    };
    let screen = render(&view);
    assert!(screen.contains("Built-in"), "{screen}");
    assert!(screen.contains("the pack"), "{screen}");
    assert!(screen.contains("token-thrift"), "{screen}");
    assert!(screen.contains("off"), "{screen}");
}

#[test]
fn confirm_lists_permissions_source_and_the_terminal_note() {
    let [notes, weather, screen, mirror] = <[Listing; 4]>::try_from(store()).expect("four");
    let view = Workshop {
        confirm: Some(Confirm {
            listing: notes,
            focus: 0,
        }),
        ..Workshop::default()
    };
    let text = render(&view);
    assert!(text.contains("Install notes 1.0.0?"), "{text}");
    assert!(text.contains("From official (\u{2713} Official)"), "{text}");
    assert!(text.contains("reach https://api.example.com"), "{text}");
    assert!(
        text.contains("Its panels do not show in the terminal."),
        "{text}"
    );
    assert!(
        text.contains("Install") && text.contains("Cancel"),
        "{text}"
    );

    let view = Workshop {
        confirm: Some(Confirm {
            listing: mirror,
            focus: 0,
        }),
        ..Workshop::default()
    };
    let text = render(&view);
    assert!(
        text.contains("From dusk-mirror (\u{25C6} Verified)"),
        "{text}"
    );
    assert!(
        text.contains("Another source offers the same name"),
        "{text}"
    );

    let view = Workshop {
        confirm: Some(Confirm {
            listing: weather.clone(),
            focus: 0,
        }),
        ..Workshop::default()
    };
    let text = render(&view);
    assert!(
        text.contains("Update weather from 0.9.0 to 1.0.0?"),
        "{text}"
    );
    assert!(
        text.contains("It asks for nothing beyond its own folder."),
        "{text}"
    );
    assert!(!text.contains("do not show in the terminal"), "{text}");

    assert_eq!(
        choices(&weather),
        [Choice::Update, Choice::Remove, Choice::Cancel]
    );
    assert_eq!(choices(&screen), [Choice::Cancel]);
}
