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
    render_at(view, 70, 60)
}

/// The panel drawn at `width` columns, the way the frame does: the lines are fitted to the
/// width and the screen is that wide, so what overflows would show as cut text.
fn render_at(view: &Workshop, width: u16, rows: u16) -> String {
    let (lines, _) = workshop(view, width as usize);
    let lines = super::super::frame::fit(lines, width as usize + 2);
    let mut terminal = Terminal::new(TestBackend::new(width + 2, rows)).expect("test terminal");
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
    // Stars stay off the rows: an official listing carries none.
    assert!(!screen.contains('\u{2605}'), "{screen}");
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
            review: None,
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
            review: None,
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
            review: None,
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

fn of_kind(name: &str, kind: Kind) -> Listing {
    Listing {
        kind,
        ..listing(name)
    }
}

fn kinds() -> Vec<Listing> {
    let mut installed = of_kind("zeta", Kind::Plugin);
    installed.installed = Some("1.0.0".into());
    vec![
        of_kind("alpha", Kind::Plugin),
        of_kind("beta", Kind::Skill),
        of_kind("gamma", Kind::Pack),
        of_kind("delta", Kind::Agent),
        installed,
    ]
}

#[test]
fn kind_is_in_brackets_at_the_right_of_the_name() {
    let view = Workshop {
        listings: Some(kinds()),
        ..Workshop::default()
    };
    for width in [60, 80, 100, 140] {
        let screen = render_at(&view, width, 40);
        for (name, tag) in [
            ("alpha", "[plugin]"),
            ("beta", "[skill]"),
            ("gamma", "[pack]"),
            ("delta", "[agent]"),
        ] {
            let row = screen
                .lines()
                .find(|line| line.contains(&format!("{name} 1.0.0")))
                .unwrap_or_else(|| panic!("no row for {name}\n{screen}"));
            assert!(row.contains(tag), "{width}: {row}");
        }
        let row = screen
            .lines()
            .find(|line| line.contains("zeta 1.0.0"))
            .expect("zeta");
        assert!(row.contains("[plugin]  installed"), "{width}: {row}");
        // The kind is not said twice.
        assert!(!screen.contains("Official \u{00B7} plugin"), "{screen}");
    }
}

#[test]
fn panel_uses_the_width_up_to_one_hundred() {
    let mut long = listing("wide");
    long.description = "word ".repeat(40);
    let view = Workshop {
        listings: Some(vec![long]),
        ..Workshop::default()
    };
    let widest = |width: u16| {
        render_at(&view, width, 40)
            .lines()
            .map(|line| line.chars().count())
            .max()
            .unwrap_or(0)
    };
    assert!(widest(60) <= 62);
    assert!(widest(80) <= 82);
    assert!(widest(100) <= 102);
    assert!(widest(100) > 90, "the text wraps at 100, not at 70");
    // Wider than 100 changes nothing in how the text wraps.
    assert!(widest(140) <= 142);
    assert_eq!(
        render_at(&view, 140, 40).lines().next(),
        render_at(&view, 100, 40).lines().next()
    );
    let screen = render_at(&view, 100, 40);
    let rows: Vec<_> = screen
        .lines()
        .filter(|line| line.contains("word"))
        .collect();
    assert!(
        rows.iter().all(|line| line.chars().count() <= 102),
        "{screen}"
    );
}

#[test]
fn long_wide_name_is_clipped_and_keeps_the_kind_and_state() {
    let mut wide = listing(&"\u{65E5}\u{672C}\u{8A9E}".repeat(30));
    wide.kind = Kind::Plugin;
    wide.installed = Some("1.0.0".into());
    wide.description = "\u{8AAC}\u{660E}".repeat(80);
    let view = Workshop {
        listings: Some(vec![wide]),
        ..Workshop::default()
    };
    for width in [60u16, 80, 100, 140] {
        let screen = render_at(&view, width, 40);
        let row = screen
            .lines()
            .find(|line| line.contains("[plugin]"))
            .unwrap_or_else(|| panic!("{width}\n{screen}"));
        assert!(row.contains("installed"), "{width}: {row}");
        assert!(row.contains(ELLIPSIS), "{width}: {row}");
    }
}

#[test]
fn hostile_name_has_no_escape_at_any_width() {
    let mut hostile = listing("x");
    hostile.name = "evil\x1b[2J\x1b]0;pwned\x07\u{202E}gnp.exe\u{2066}".into();
    hostile.description = "go\x1b[31m\u{202E}red\u{2069}\u{9b}".into();
    hostile.category = "c\x1bat".into();
    let view = Workshop {
        listings: Some(vec![hostile]),
        category: Some("a\x1b[0m".into()),
        ..Workshop::default()
    };
    for width in [60u16, 80, 100, 140] {
        let screen = render_at(&view, width, 40);
        assert!(screen.contains("evil"), "{screen}");
        for bad in ['\x1b', '\x07', '\u{9b}'] {
            assert!(!screen.contains(bad), "{width}: {bad:?}");
        }
        assert!(!screen.contains("\u{202E}"), "{width}: bidi override");
        assert!(!screen.contains("\u{2066}") && !screen.contains("\u{2069}"));
    }
}

/// A mirror's stars are its repository's: off the row, and in its window only with the
/// repository named.
#[test]
fn a_mirror_s_stars_show_in_its_window_with_their_repository() {
    let mut friends = listing("friends-notes");
    friends.tier = Tier::Community;
    friends.source = "friends".into();
    friends.repo_stars = Some(dusk_protocol::plugin::RepoStars {
        repo: "someone/notes".into(),
        stars: 122,
    });
    let view = Workshop {
        listings: Some(vec![friends.clone()]),
        ..Workshop::default()
    };
    let row = render_at(&view, 100, 60);
    assert!(!row.contains("122"), "{row}");
    let view = Workshop {
        confirm: Some(Confirm {
            listing: friends,
            focus: 0,
            review: None,
        }),
        ..Workshop::default()
    };
    let text = render(&view);
    assert!(text.contains("122 GitHub stars of someone/notes"), "{text}");
}
