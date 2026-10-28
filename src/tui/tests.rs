//! Regression tests for the terminal reader.

use super::*;
use crate::config::Config;
use crate::fixtures;
use unicode_width::UnicodeWidthStr;

fn composer_for(text: &str, target: Network, overlay: Overlay) -> Composer {
    Composer {
        reply_to: None,
        quote: None,
        input: Input::from(text),
        targets: vec![target],
        raw: false,
        origin: None,
        base: Snapshot {
            targets: vec![target],
            ..Snapshot::default()
        },
        draft_id: None,
        autosaved: None,
        saved: 0,
        overlay,
    }
}

#[test]
fn signing_out_of_devnull_keeps_the_key_note_in_the_status() {
    assert_eq!(
        signed_out_status(Network::Bluesky, false),
        "signed out of Bluesky"
    );
    assert_eq!(
        signed_out_status(Network::Devnull, true),
        "signed out of dev/null. The device key stays on disk. Revoke it at dev-null.sh/settings/devices"
    );
}

fn border_and_hint(text: &str, target: Network) -> (String, String) {
    let mut state = State::boot(true, 15, &Config::default(), &Clients::empty(), None);
    state.status.clear();
    let lines = composer(
        &state,
        &composer_for(text, target, Overlay::None),
        100,
        false,
        None,
    )
    .lines;
    (lines[3].plain(), lines[4].plain())
}

#[test]
fn composer_counts_against_the_target_network() {
    let (border, hint) = border_and_hint("", Network::Bluesky);
    assert!(border.ends_with(" 300 ─╯"), "{border}");
    assert!(
        hint.ends_with("posting to bluesky as @you.bsky.social"),
        "{hint}"
    );
    let family = "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}";
    let (border, _) = border_and_hint(family, Network::Bluesky);
    assert!(border.ends_with(" 299 ─╯"), "{border}");
    let (border, _) = border_and_hint(family, Network::X);
    assert!(border.ends_with(" 278 ─╯"), "{border}");
    let (border, _) = border_and_hint("hi", Network::Threads);
    assert!(border.ends_with(" 498 ─╯"), "{border}");
    let (border, hint) = border_and_hint("", Network::Devnull);
    assert!(border.ends_with(" 4000 ─╯"), "{border}");
    assert!(hint.ends_with("posting to dev/null as @you"), "{hint}");
}

#[test]
fn the_selection_bar_sits_at_terminal_col_1() {
    let mut line = StyledLine::text(Kind::Fg, " ╭───╮");
    line.selected = true;
    line.bar = true;
    let text: String = to_line_in(&line, &Theme::default(), 20, 2, Kind::Blue, true)
        .spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect();
    assert_eq!(text.chars().position(|c| c == '▍'), Some(1));
    assert_eq!(text.chars().position(|c| c == '╭'), Some(3));
    assert_eq!(text.chars().count(), 20);
}

#[test]
fn composer_status_follows_the_target() {
    let mut state = State::boot(true, 15, &Config::default(), &Clients::empty(), None);
    state.status = "draft restored · saved 2m ago".into();
    let lines = composer(
        &state,
        &composer_for("hi", Network::X, Overlay::None),
        100,
        false,
        None,
    )
    .lines;
    let hint = lines[4].plain();
    assert!(
        hint.ends_with("posting to x as @you  draft restored · saved 2m ago"),
        "{hint}"
    );
    let lines = composer(
        &state,
        &composer_for("hi", Network::X, Overlay::None),
        72,
        false,
        None,
    )
    .lines;
    let hint = lines[4].plain();
    assert!(hint.contains("posting to x as @you  draft"), "{hint}");
    assert!(hint.ends_with('…'), "{hint}");
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn with(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
    KeyEvent::new(code, modifiers)
}

fn typed(input: &mut Input, text: &str) {
    for ch in text.chars() {
        input.insert(ch);
    }
}

fn temp(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "socli-tui-{name}-{}-{:08x}",
        std::process::id(),
        rand::random::<u32>()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn input_moves_and_deletes_whole_graphemes() {
    let family = "\u{1f468}\u{200d}\u{1f469}\u{200d}\u{1f467}";
    let flag = "\u{1f1ee}\u{1f1f9}";
    let mut input = Input::from(&format!("e\u{301}{family}{flag}日本"));
    input.backspace();
    assert_eq!(input.text(), format!("e\u{301}{family}{flag}日"));
    input.left();
    input.left();
    assert_eq!(input.before_cursor(), format!("e\u{301}{family}"));
    input.backspace();
    assert_eq!(input.text(), format!("e\u{301}{flag}日"));
    input.delete();
    assert_eq!(input.text(), "e\u{301}日");
    input.left();
    assert_eq!(input.before_cursor(), "");
    input.right();
    assert_eq!(input.before_cursor(), "e\u{301}");
    assert_eq!(line_width(input.before_cursor()), 1);
    input.end();
    assert_eq!(line_width(input.before_cursor()), 3);
    input.insert('\u{301}');
    assert_eq!(input.text(), "e\u{301}日\u{301}");
    assert_eq!(input.cursor, input.text().len());
}

#[test]
fn input_moves_and_deletes_by_word() {
    let mut input = Input::from("hello brave  new");
    input.word_left();
    assert_eq!(input.before_cursor(), "hello brave  ");
    input.word_left();
    assert_eq!(input.before_cursor(), "hello ");
    input.word_right();
    assert_eq!(input.before_cursor(), "hello brave");
    input.delete_word_back();
    assert_eq!(input.text(), "hello   new");
    assert_eq!(input.before_cursor(), "hello ");
    input.word_right();
    assert_eq!(input.before_cursor(), "hello   new");
    edit(&mut input, with(KeyCode::Char('w'), KeyModifiers::CONTROL));
    assert_eq!(input.text(), "hello   ");
    edit(&mut input, with(KeyCode::Backspace, KeyModifiers::ALT));
    assert_eq!(input.text(), "");
    let mut input = Input::from("one two");
    edit(&mut input, with(KeyCode::Left, KeyModifiers::CONTROL));
    assert_eq!(input.before_cursor(), "one ");
    edit(&mut input, with(KeyCode::Left, KeyModifiers::ALT));
    assert_eq!(input.before_cursor(), "");
    edit(&mut input, with(KeyCode::Right, KeyModifiers::ALT));
    assert_eq!(input.before_cursor(), "one");
}

#[test]
fn option_text_inserts_and_alt_b_f_skip_words() {
    let mut input = Input::from("one two three");
    edit(&mut input, with(KeyCode::Char('b'), KeyModifiers::ALT));
    assert_eq!(input.text(), "one two three");
    assert_eq!(input.before_cursor(), "one two ");
    edit(&mut input, with(KeyCode::Char('f'), KeyModifiers::ALT));
    assert_eq!(input.before_cursor(), "one two three");
    edit(&mut input, with(KeyCode::Char('@'), KeyModifiers::NONE));
    for ch in ['#', '`', '[', ']', 'ò', 'à', 'ù', 'è', '+'] {
        edit(&mut input, with(KeyCode::Char(ch), KeyModifiers::NONE));
    }
    assert_eq!(input.text(), "one two three@#`[]òàùè+");
    edit(&mut input, with(KeyCode::Char('ò'), KeyModifiers::ALT));
    assert_eq!(input.text(), "one two three@#`[]òàùè+ò");
    let flags = keyboard_flags();
    assert!(flags.contains(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES));
    assert!(flags.contains(KeyboardEnhancementFlags::REPORT_ALTERNATE_KEYS));
    assert!(flags.contains(KeyboardEnhancementFlags::REPORT_EVENT_TYPES));
    assert!(!flags.contains(KeyboardEnhancementFlags::REPORT_ALL_KEYS_AS_ESCAPE_CODES));
}

#[test]
fn input_lines_keep_the_display_column() {
    let mut input = Input::from("abcd\nxy\n日本");
    input.up();
    assert_eq!(input.before_cursor(), "abcd\nxy");
    input.up();
    assert_eq!(input.before_cursor(), "ab");
    input.up();
    assert_eq!(input.before_cursor(), "ab");
    input.down();
    assert_eq!(input.before_cursor(), "abcd\nxy");
    input.down();
    assert_eq!(input.before_cursor(), "abcd\nxy\n日");
    input.down();
    assert_eq!(input.before_cursor(), "abcd\nxy\n日");
    input.home();
    assert_eq!(input.before_cursor(), "abcd\nxy\n");
    input.end();
    assert_eq!(input.before_cursor(), "abcd\nxy\n日本");
    input.up();
    edit(&mut input, with(KeyCode::Char('a'), KeyModifiers::CONTROL));
    assert_eq!(input.before_cursor(), "abcd\n");
    edit(&mut input, with(KeyCode::Char('e'), KeyModifiers::CONTROL));
    assert_eq!(input.before_cursor(), "abcd\nxy");
    edit(&mut input, with(KeyCode::Char('u'), KeyModifiers::CONTROL));
    assert_eq!(input.text(), "abcd\n\n日本");
    assert_eq!(input.before_cursor(), "abcd\n");
    edit(&mut input, key(KeyCode::Home));
    edit(&mut input, key(KeyCode::End));
    assert_eq!(input.before_cursor(), "abcd\n");
    edit(&mut input, with(KeyCode::Char('k'), KeyModifiers::CONTROL));
    assert_eq!(input.text(), "abcd\n\n日本");
}

#[test]
fn undo_steps_group_typing_and_a_new_edit_drops_redo() {
    let mut input = Input::new();
    typed(&mut input, "hello world");
    input.undo();
    assert_eq!(input.text(), "hello ");
    input.undo();
    assert_eq!(input.text(), "");
    input.redo();
    assert_eq!(input.text(), "hello ");
    edit(&mut input, with(KeyCode::Char('y'), KeyModifiers::CONTROL));
    assert_eq!(input.text(), "hello world");
    edit(&mut input, with(KeyCode::Char('z'), KeyModifiers::CONTROL));
    assert_eq!(input.text(), "hello ");
    edit(
        &mut input,
        with(
            KeyCode::Char('z'),
            KeyModifiers::CONTROL | KeyModifiers::SHIFT,
        ),
    );
    assert_eq!(input.text(), "hello world");
    edit(
        &mut input,
        with(
            KeyCode::Char('Z'),
            KeyModifiers::CONTROL | KeyModifiers::SHIFT,
        ),
    );
    assert_eq!(input.text(), "hello world");

    let mut input = Input::new();
    typed(&mut input, "ab");
    input.left();
    typed(&mut input, "c");
    assert_eq!(input.text(), "acb");
    input.undo();
    assert_eq!((input.text(), input.before_cursor()), ("ab", "a"));
    input.insert_str("one\ntwo");
    assert_eq!(input.text(), "aone\ntwob");
    input.undo();
    assert_eq!(input.text(), "ab");
    input.insert('x');
    input.redo();
    assert_eq!(input.text(), "axb");
    input.undo();
    input.undo();
    assert_eq!(input.text(), "");
}

#[test]
fn undo_keeps_the_last_hundred_steps() {
    let mut input = Input::new();
    for _ in 0..150 {
        input.insert_str("a");
    }
    for _ in 0..120 {
        input.undo();
    }
    assert_eq!(input.text(), "a".repeat(50));
}

#[test]
fn a_paste_is_cleaned_and_never_acts_as_a_key() {
    assert_eq!(clean_paste("a\r\nb\rc\td\x1b[31me\x03"), "a\nb\nc    de");
    let mut ui = test_ui();
    ui.mode = Mode::Compose(Box::new(composer_for("", Network::X, Overlay::None)));
    paste(&mut ui, "line one\r\n:quit\n\x1b[31mred\x03");
    let Mode::Compose(c) = &mut ui.mode else {
        panic!("the paste left the composer");
    };
    assert_eq!(c.input.text(), "line one\n:quit\nred");
    c.input.undo();
    assert_eq!(c.input.text(), "");
    c.overlay = Overlay::Ask(After::Leave);
    paste(&mut ui, "dropped");
    let Mode::Compose(c) = &ui.mode else {
        panic!("the paste left the composer");
    };
    assert_eq!(c.input.text(), "");

    let mut ui = test_ui();
    paste(&mut ui, ":quit\n");
    assert!(matches!(ui.mode, Mode::Normal));
    ui.mode = Mode::Command {
        input: Input::new(),
        pick: 2,
    };
    paste(&mut ui, "quit\r\nnow\x1b");
    let Mode::Command { input, pick } = &ui.mode else {
        panic!("the paste left the command line");
    };
    assert_eq!((input.text(), *pick), ("quit now", 0));
    ui.mode = Mode::Search(Input::new());
    paste(&mut ui, "a\nb");
    let Mode::Search(input) = &ui.mode else {
        panic!("the paste left search");
    };
    assert_eq!(input.text(), "a b");
}

#[test]
fn a_composer_paste_drops_whole_escape_sequences() {
    let mut ui = test_ui();
    ui.mode = Mode::Compose(Box::new(composer_for("", Network::X, Overlay::None)));
    paste(&mut ui, "safe\x1b[31mred\x1b[0m\r\n:quit\n\x03end");
    let Mode::Compose(c) = &ui.mode else {
        panic!("the paste left the composer");
    };
    assert_eq!(c.input.text(), "safered\n:quit\nend");
}

#[test]
fn the_enter_setting_picks_the_submit_and_newline_keys() {
    let mut state = State::boot(true, 15, &Config::default(), &Clients::empty(), None);
    state.reveal_shortcuts = true;
    let targets = [Network::X];
    let mut c = composer_for("hi", Network::X, Overlay::None);
    let ctrl = |code| with(code, KeyModifiers::CONTROL);
    let shift_enter = with(KeyCode::Enter, KeyModifiers::SHIFT);
    assert_eq!(state.enter, Enter::Post);
    assert_eq!(
        composer_key(&mut state, &mut c, key(KeyCode::Enter), false, &targets),
        Step::Submit
    );
    assert_eq!(
        composer_key(
            &mut state,
            &mut c,
            ctrl(KeyCode::Char('s')),
            false,
            &targets
        ),
        Step::Stay
    );
    assert_eq!(
        composer_key(
            &mut state,
            &mut c,
            ctrl(KeyCode::Char('j')),
            false,
            &targets
        ),
        Step::Stay
    );
    assert_eq!(
        composer_key(&mut state, &mut c, shift_enter, true, &targets),
        Step::Stay
    );
    assert_eq!(c.input.text(), "hi\n\n");

    state.enter = Enter::Newline;
    let mut c = composer_for("hi", Network::X, Overlay::None);
    assert_eq!(
        composer_key(&mut state, &mut c, key(KeyCode::Enter), false, &targets),
        Step::Stay
    );
    assert_eq!(
        composer_key(&mut state, &mut c, shift_enter, true, &targets),
        Step::Stay
    );
    assert_eq!(c.input.text(), "hi\n\n");
    assert_eq!(
        composer_key(
            &mut state,
            &mut c,
            ctrl(KeyCode::Char('s')),
            false,
            &targets
        ),
        Step::Submit
    );
    assert_eq!(
        composer_key(&mut state, &mut c, ctrl(KeyCode::Enter), true, &targets),
        Step::Submit
    );
    assert_eq!(
        composer_key(&mut state, &mut c, ctrl(KeyCode::Enter), false, &targets),
        Step::Stay
    );

    let hint = |state: &State, enhanced| {
        composer(
            state,
            &composer_for("hi", Network::X, Overlay::None),
            100,
            enhanced,
            None,
        )
        .lines[4]
            .plain()
    };
    assert!(hint(&state, false).starts_with("ctrl-s post  ⏎ newline  esc cancel"));
    assert!(hint(&state, true).starts_with("ctrl-s/ctrl-⏎ post  ⏎ newline"));
    state.enter = Enter::Post;
    assert!(hint(&state, false).starts_with("⏎ post  ctrl-j newline  esc cancel"));
    assert!(hint(&state, true).starts_with("⏎ post  ⇧⏎/ctrl-j newline"));
}

#[test]
fn tab_retargets_what_the_composer_sends() {
    let dir = temp("retarget");
    let store = dir.join("drafts.json");
    let mut state = State::boot(true, 15, &Config::default(), &Clients::empty(), None);
    let targets = [Network::X, Network::Bluesky];
    let mut c = composer_for("hi", Network::X, Overlay::None);
    composer_key(&mut state, &mut c, key(KeyCode::Tab), false, &targets);
    assert_eq!(c.intent().targets, [Network::Bluesky]);
    assert!(submit(&mut state, &mut Clients::empty(), &mut c, &store));
    assert_eq!(state.status, "posted to Bluesky");
    assert!(drafts::load_from(&store).unwrap().is_empty());

    let mut reply = composer_for("hi", Network::X, Overlay::None);
    reply.reply_to = Some("x:p1".into());
    reply.quote = Some("x:p2".into());
    reply.draft_id = Some("d1".into());
    let intent = reply.intent();
    assert_eq!(intent.reply_to.as_deref(), Some("x:p1"));
    assert_eq!(intent.quote.as_deref(), Some("x:p2"));
    assert_eq!(intent.draft_id.as_deref(), Some("d1"));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_failed_draft_write_keeps_the_composer_and_its_text() {
    let dir = temp("kept");
    let store = dir.join("drafts.json");
    let mut state = State::boot(true, 15, &Config::default(), &Clients::empty(), None);
    let long = "a".repeat(290);
    let mut c = composer_for(&long, Network::X, Overlay::None);
    c.targets = vec![Network::X, Network::Bluesky];
    drafts::fail_after(0);
    assert!(!submit(&mut state, &mut Clients::empty(), &mut c, &store));
    assert!(
        state.status.contains("nothing was sent"),
        "{}",
        state.status
    );
    assert_eq!(c.input.text(), long);
    assert_eq!(c.targets, [Network::X, Network::Bluesky]);

    drafts::fail_after(1);
    assert!(!submit(&mut state, &mut Clients::empty(), &mut c, &store));
    assert!(
        state.status.contains("couldn't update the draft"),
        "{}",
        state.status
    );
    assert_eq!(c.input.text(), long);
    assert_eq!(c.targets, [Network::X]);
    let id = c.draft_id.clone().unwrap();
    let kept = drafts::load_from(&store).unwrap();
    assert_eq!(
        (kept[0].id.as_str(), kept[0].sending.len()),
        (id.as_str(), 2)
    );

    c.save_to(&state, &store)
        .expect("save after the interrupted send");
    let kept = drafts::load_from(&store).expect("saved draft");
    assert_eq!(kept[0].sending, ["x", "bluesky"]);
    c.autosaved = Some((Instant::now(), c.snapshot()));
    c.discard_at(&store).expect("discard later composer edits");
    let kept = drafts::load_from(&store).expect("pending send record");
    assert_eq!(kept[0].sending, ["x", "bluesky"]);

    c.targets = vec![Network::X, Network::Bluesky];
    assert!(submit(&mut state, &mut Clients::empty(), &mut c, &store));
    assert!(state.status.ends_with(" · draft saved"), "{}", state.status);
    let kept = drafts::load_from(&store).unwrap();
    assert_eq!(kept.len(), 1);
    assert_eq!(
        (kept[0].id.as_str(), kept[0].targets.as_slice()),
        (id.as_str(), ["x".to_string()].as_slice())
    );
    assert!(kept[0].sending.is_empty());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_loaded_composer_preserves_newer_pending_send_records() {
    let dir = temp("pending-autosave");
    let store = dir.join("drafts.json");
    let state = State::boot(true, 15, &Config::default(), &Clients::empty(), None);
    let mut original = Draft::new(vec!["x".into()], None, None);
    original.text = "before editing".into();
    drafts::upsert_at(&store, &original).expect("original draft");
    let mut composer = composer_for("", Network::X, Overlay::None);
    composer.load(original.clone());
    drafts::update_at(&store, |drafts| {
        drafts[0].sending = vec!["x".into()];
        Ok(())
    })
    .expect("another send recorded");
    composer.input = Input::from("after editing");
    composer.targets = vec![Network::Bluesky];
    composer.autosave(&state, &store).expect("autosave");
    assert_eq!(drafts::load_from(&store).expect("draft")[0].sending, ["x"]);
    composer.discard_at(&store).expect("discard edits");
    let restored = &drafts::load_from(&store).expect("draft")[0];
    assert_eq!(restored.sending, ["x"]);
    assert_eq!(restored.text, original.text);
    assert_eq!(restored.targets, original.targets);
    std::fs::remove_dir_all(dir).expect("temporary drafts");
}

#[test]
fn a_restored_draft_that_was_sending_asks_for_a_check() {
    let mut state = State::boot(true, 15, &Config::default(), &Clients::empty(), None);
    let mut draft = Draft::new(vec!["bluesky".into()], None, None);
    draft.text = "maybe posted".into();
    draft.sending = vec!["bluesky".into(), "x".into()];
    let overlay = Overlay::List {
        drafts: vec![draft],
        cursor: 0,
    };
    let mut c = composer_for("", Network::X, overlay);
    composer_key(
        &mut state,
        &mut c,
        key(KeyCode::Enter),
        false,
        &[Network::X],
    );
    assert_eq!(
        state.status,
        "last send may have reached X, Bluesky · check before posting again"
    );
    assert_eq!(c.input.text(), "maybe posted");
    assert_eq!(c.targets, [Network::Bluesky]);
    assert!(!c.dirty());
}

#[test]
fn dirty_compares_text_targets_reply_and_quote() {
    let dir = temp("dirty");
    let store = dir.join("drafts.json");
    let mut state = State::boot(true, 15, &Config::default(), &Clients::empty(), None);
    let targets = [Network::X, Network::Bluesky];
    let mut c = composer_for("", Network::X, Overlay::None);
    assert!(!c.dirty());
    composer_key(&mut state, &mut c, key(KeyCode::Char('a')), false, &targets);
    assert!(c.dirty());
    composer_key(&mut state, &mut c, key(KeyCode::Backspace), false, &targets);
    assert!(!c.dirty());
    c.autosave(&state, &store).unwrap();
    assert!(!store.exists());

    composer_key(&mut state, &mut c, key(KeyCode::Tab), false, &targets);
    assert!(c.dirty());
    c.autosave(&state, &store).unwrap();
    let kept = drafts::load_from(&store).unwrap();
    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0].targets, ["bluesky"]);
    assert_eq!(c.draft_id.as_deref(), Some(kept[0].id.as_str()));

    let mut loaded = composer_for("", Network::X, Overlay::None);
    let mut draft = Draft::new(vec!["x".into()], None, None);
    draft.text = "A".into();
    loaded.load(draft);
    assert!(!loaded.dirty());
    composer_key(
        &mut state,
        &mut loaded,
        key(KeyCode::Char('B')),
        false,
        &targets,
    );
    assert!(loaded.dirty());
    composer_key(
        &mut state,
        &mut loaded,
        key(KeyCode::Backspace),
        false,
        &targets,
    );
    assert!(!loaded.dirty());
    loaded.quote = Some("x:p1".into());
    assert!(loaded.dirty());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn quote_composer_counts_the_link_for_the_strictest_target() {
    let mut state = State::boot(true, 15, &Config::default(), &Clients::empty(), None);
    state.status.clear();
    let quoted = state.posts["threads:p10"].clone();
    let mut c = composer_for("<b>hi</b>", Network::Threads, Overlay::None);
    c.quote = Some(quoted.id.clone());
    c.targets = vec![Network::X, Network::Threads];
    let (net, left) = c.room(&state);
    assert_eq!((net, left), (Network::X, 280 - 4 - 25));
    c.raw = true;
    assert_eq!(c.room(&state).1, 280 - 9 - 25);
    let lines: Vec<String> = composer(&state, &c, 100, false, None)
        .lines
        .iter()
        .map(StyledLine::plain)
        .collect();
    assert!(lines[0].starts_with("╭─ Quote post ─"), "{}", lines[0]);
    assert_eq!(
        lines[1].trim_end_matches(['│', ' ']),
        "│ to  ● X  ● Threads"
    );
    assert!(
        lines
            .iter()
            .any(|l| l.contains(&format!("@{} · threads", quoted.author.username))),
        "{lines:#?}"
    );
    assert!(
        lines
            .last()
            .is_some_and(|l| l.ends_with("posting to x and threads")),
        "{lines:#?}"
    );
}

#[test]
fn full_screen_composer_fills_the_rows_it_gets() {
    let mut state = State::boot(true, 15, &Config::default(), &Clients::empty(), None);
    state.status.clear();
    let c = composer_for("hello", Network::Bluesky, Overlay::None);
    let block = composer(&state, &c, 100, false, Some(40));
    assert_eq!(block.lines.len(), 40);
    assert_eq!(block.cursor, Some((7, 1)));
    assert!(block.lines[38].plain().starts_with("╰"));
}

#[test]
fn the_composer_cursor_follows_wrapped_wide_text() {
    let mut state = State::boot(true, 15, &Config::default(), &Clients::empty(), None);
    state.status.clear();
    let mut c = composer_for("日本語 日本語 日本", Network::Bluesky, Overlay::None);
    let block = composer(&state, &c, 14, false, Some(40));
    assert_eq!(block.cursor, Some((6, 3)));
    c.input.word_left();
    let block = composer(&state, &c, 14, false, Some(40));
    assert_eq!(block.cursor, Some((4, 3)));
    c.input.home();
    c.input.word_right();
    let block = composer(&state, &c, 14, false, Some(40));
    assert_eq!(block.cursor, Some((4, 1)));
    c.input.cursor = "日本語 ".len();
    let block = composer(&state, &c, 14, false, Some(40));
    assert_eq!(block.cursor, Some((2, 2)));
}

#[test]
fn composer_keys_close_through_the_draft_dialog() {
    let mut state = State::boot(true, 15, &Config::default(), &Clients::empty(), None);
    let targets = [Network::X, Network::Bluesky];
    let mut c = composer_for("", Network::X, Overlay::None);
    let mut step = |c: &mut Composer, code| composer_key(&mut state, c, key(code), false, &targets);
    assert_eq!(step(&mut c, KeyCode::Enter), Step::Stay);
    assert_eq!(step(&mut c, KeyCode::Tab), Step::Stay);
    assert_eq!(c.targets, [Network::Bluesky]);
    assert_eq!(step(&mut c, KeyCode::Char('h')), Step::Stay);
    assert_eq!(step(&mut c, KeyCode::Esc), Step::Stay);
    assert!(matches!(c.overlay, Overlay::Ask(After::Leave)));
    assert_eq!(
        step(&mut c, KeyCode::Char('n')),
        Step::Closed {
            saved: false,
            quit: false
        }
    );
    assert_eq!(step(&mut c, KeyCode::Enter), Step::Submit);
    let mut clean = composer_for("", Network::X, Overlay::None);
    assert_eq!(
        step(&mut clean, KeyCode::Esc),
        Step::Closed {
            saved: false,
            quit: false
        }
    );
}

#[test]
fn posted_status_names_successes_then_failures() {
    let results = vec![
        (Network::X, Err("x is down".to_string())),
        (Network::Threads, Ok("demo".to_string())),
    ];
    assert_eq!(
        posted_status(&results),
        "posted to Threads · x failed: x is down"
    );
    let results = vec![
        (Network::X, Ok(String::new())),
        (Network::Threads, Ok(String::new())),
    ];
    assert_eq!(posted_status(&results), "posted to X and Threads");
}

fn test_ui() -> Ui {
    demo_ui()
}

#[test]
fn the_sign_in_form_draws_and_steps_to_the_next_field() {
    let mut state = State::boot(false, 15, &Config::default(), &Clients::empty(), None);
    let mut ui = test_ui();
    ui.mode = Mode::SignIn(SignInForm::open(Network::Bluesky));
    press(&mut state, &mut ui, KeyCode::Char('a'));
    press(&mut state, &mut ui, KeyCode::Enter);
    let Mode::SignIn(form) = &ui.mode else {
        panic!("the form closed");
    };
    assert_eq!(form.cursor, 1);
    assert_eq!(form.fields[0].value, "a");
    let buffer = frame(&mut state, &ui, 80, 24);
    let text: String = buffer.content().iter().map(|cell| cell.symbol()).collect();
    assert!(text.contains("Sign in to Bluesky"), "{text}");
    assert!(text.contains("Username"), "{text}");
    assert!(text.contains("App password"), "{text}");
    assert!(text.contains(FIELD_CURSOR), "{text}");
    assert!(!text.contains('▌'), "{text}");
}

#[test]
fn the_x_sign_in_screen_approves_the_socli_app() {
    let mut state = State::boot(false, 15, &Config::default(), &Clients::empty(), None);
    let mut ui = test_ui();
    ui.mode = Mode::SignIn(SignInForm::open(Network::X));
    state.detail = Detail::Rich;
    let buffer = frame(&mut state, &ui, 92, 36);
    let text: String = buffer.content().iter().map(|cell| cell.symbol()).collect();
    assert!(text.contains("Approve SoCLI on X"), "{text}");
    assert!(text.contains("Enter opens X"), "{text}");
    assert!(!text.contains("this build has no SoCLI X app"), "{text}");
    assert!(!text.contains("Client id"), "{text}");
    assert!(!text.contains("console.x.com"), "{text}");
    assert!(!text.contains("Create an app"), "{text}");
    assert!(!text.contains("socli login x"), "{text}");
    assert!(!text.contains("auth_token"), "{text}");
    assert!(!text.contains("This page cannot"), "{text}");
    assert!(
        !text.contains("this sign-in continues in the browser"),
        "{text}"
    );
    assert!(!text.contains("127.0.0.1:8741"), "{text}");
    let rows: Vec<String> = (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect()
        })
        .collect();
    let column_of = |needle: &str| {
        rows.iter()
            .find_map(|row| row.find(needle).map(|at| row[..at].width()))
    };
    let column = 2 + render::margin_for(92, Detail::Rich);
    assert_eq!(column_of("▄██████▄"), Some(column), "{rows:#?}");
    assert_eq!(column_of("Sign in to X"), Some(column), "{rows:#?}");
    assert_eq!(column_of("Sign-in method"), Some(column), "{rows:#?}");
    assert_eq!(column_of("Approve SoCLI on X"), Some(column), "{rows:#?}");
    press(&mut state, &mut ui, KeyCode::Enter);
    assert!(matches!(ui.mode, Mode::SignIn(_)));
    let buffer = frame(&mut state, &ui, 92, 36);
    let text: String = buffer.content().iter().map(|cell| cell.symbol()).collect();
    assert!(
        text.contains("this sign-in continues in the browser"),
        "{text}"
    );
    assert!(!text.contains("Create an app"), "{text}");
    assert!(!text.contains("console.x.com"), "{text}");
}

fn sign_in_text(net: Network, width: u16, height: u16) -> String {
    let mut state = State::boot(false, 15, &Config::default(), &Clients::empty(), None);
    state.detail = Detail::Rich;
    let mut ui = test_ui();
    ui.mode = Mode::SignIn(SignInForm::open(net));
    let buffer = frame(&mut state, &ui, width, height);
    buffer.content().iter().map(|cell| cell.symbol()).collect()
}

#[test]
fn the_mastodon_sign_in_screen_opens_the_server_approval() {
    let mut ui = test_ui();
    ui.mode = Mode::SignIn(SignInForm::open(Network::Mastodon));
    let Mode::SignIn(form) = &ui.mode else {
        panic!("the form closed");
    };
    assert_eq!(form.value("Server"), "mastodon.social");
    let text = sign_in_text(Network::Mastodon, 92, 36);
    assert!(text.contains("mastodon.social"), "{text}");
    assert!(text.contains("Approve SoCLI"), "{text}");
    assert!(text.contains("Server"), "{text}");
    assert!(!text.contains("Access token"), "{text}");
}

#[test]
fn the_threads_sign_in_screen_approves_the_socli_app() {
    let text = sign_in_text(Network::Threads, 92, 36);
    assert!(text.contains("Approve SoCLI on Threads"), "{text}");
    assert!(text.contains("Enter opens Threads"), "{text}");
    assert!(
        !text.contains("this build has no SoCLI Threads app"),
        "{text}"
    );
    assert!(!text.contains("Pass the token"), "{text}");
    assert!(!text.contains("Threads Tester"), "{text}");
    assert!(!text.contains("developers.facebook.com"), "{text}");
    assert!(!text.contains("Access token"), "{text}");
    assert!(matches!(
        SignInForm::open(Network::Threads).submit(),
        Err(message) if message == "Waiting for Meta approval"
    ));
}

#[test]
fn the_devnull_sign_in_screen_asks_for_the_callback() {
    let mut state = State::boot(false, 15, &Config::default(), &Clients::empty(), None);
    state.detail = Detail::Rich;
    let mut ui = test_ui();
    ui.mode = Mode::SignIn(SignInForm::open(Network::Devnull));
    let buffer = frame(&mut state, &ui, 92, 40);
    let text: String = buffer.content().iter().map(|cell| cell.symbol()).collect();
    assert!(text.contains("devnull://"), "{text}");
    assert!(text.contains("Paste the callback"), "{text}");
    assert!(text.contains("Location"), "{text}");
    assert!(text.contains("Callback"), "{text}");
    assert!(!text.contains("stays in the terminal"), "{text}");
    press(&mut state, &mut ui, KeyCode::Enter);
    assert!(matches!(ui.mode, Mode::SignIn(_)));
    let buffer = frame(&mut state, &ui, 92, 40);
    let text: String = buffer.content().iter().map(|cell| cell.symbol()).collect();
    assert!(text.contains("Paste the full devnull:// address"), "{text}");
}

fn press(state: &mut State, ui: &mut Ui, code: KeyCode) {
    handle_key(state, &mut Clients::empty(), ui, key(code)).unwrap();
}

fn cursor_of(ui: &Ui) -> Option<(PickKind, usize)> {
    match &ui.mode {
        Mode::Pick(picker) => Some((picker.kind, picker.cursor)),
        _ => None,
    }
}

#[test]
fn the_network_picker_opens_moves_closes_and_picks() {
    let mut state = State::boot(true, 15, &Config::default(), &Clients::empty(), None);
    let mut ui = test_ui();
    press(&mut state, &mut ui, KeyCode::Char('A'));
    assert_eq!(cursor_of(&ui), Some((PickKind::Network, 0)));
    press(&mut state, &mut ui, KeyCode::Char('j'));
    assert_eq!(cursor_of(&ui), Some((PickKind::Network, 1)));
    press(&mut state, &mut ui, KeyCode::BackTab);
    press(&mut state, &mut ui, KeyCode::Char('k'));
    assert_eq!(cursor_of(&ui), Some((PickKind::Network, 5)));
    press(&mut state, &mut ui, KeyCode::Esc);
    assert!(matches!(ui.mode, Mode::Normal));
    assert_eq!(state.scope(), Scope::All);

    press(&mut state, &mut ui, KeyCode::Char('A'));
    press(&mut state, &mut ui, KeyCode::Char('A'));
    assert!(matches!(ui.mode, Mode::Normal));

    press(&mut state, &mut ui, KeyCode::Char('A'));
    press(&mut state, &mut ui, KeyCode::Char('3'));
    assert!(matches!(ui.mode, Mode::Normal));
    assert_eq!(state.scope(), Scope::One(Network::Devnull));

    press(&mut state, &mut ui, KeyCode::Char('A'));
    assert_eq!(cursor_of(&ui), Some((PickKind::Network, 3)));
    press(&mut state, &mut ui, KeyCode::Char('j'));
    press(&mut state, &mut ui, KeyCode::Enter);
    assert_eq!(state.scope(), Scope::One(Network::Threads));
}

#[test]
fn a_network_that_is_not_signed_in_keeps_the_picker_open() {
    let mut state = State::boot(true, 15, &Config::default(), &Clients::empty(), None);
    state.nets.retain(|info| info.net != Network::Threads);
    let mut ui = test_ui();
    press(&mut state, &mut ui, KeyCode::Char('A'));
    press(&mut state, &mut ui, KeyCode::Char('4'));
    assert!(matches!(ui.mode, Mode::Pick(_)));
    assert_eq!(
        state.status,
        "threads is not signed in. Run `socli login threads`"
    );
}

#[test]
fn a_steps_the_scope_and_ctrl_a_still_asks() {
    let mut state = State::boot(true, 15, &Config::default(), &Clients::empty(), None);
    let mut ui = test_ui();
    press(&mut state, &mut ui, KeyCode::Char('a'));
    assert_eq!(state.scope(), Scope::One(Network::X));
    let ctrl_a = KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL);
    handle_key(&mut state, &mut Clients::empty(), &mut ui, ctrl_a).unwrap();
    assert!(matches!(ui.mode, Mode::Ask { .. }));
    assert_eq!(state.scope(), Scope::One(Network::X));
}

#[test]
fn the_detail_picker_opens_and_closes_without_changing_the_level() {
    let mut state = State::boot(true, 15, &Config::default(), &Clients::empty(), None);
    let level = state.detail;
    let mut ui = test_ui();
    press(&mut state, &mut ui, KeyCode::Char('D'));
    let expected = Detail::ALL.iter().position(|l| *l == level).unwrap();
    assert_eq!(cursor_of(&ui), Some((PickKind::Detail, expected)));
    press(&mut state, &mut ui, KeyCode::Char('9'));
    press(&mut state, &mut ui, KeyCode::Char('x'));
    assert!(matches!(ui.mode, Mode::Pick(_)));
    press(&mut state, &mut ui, KeyCode::Esc);
    assert!(matches!(ui.mode, Mode::Normal));
    assert_eq!(state.detail, level);

    state.open_settings();
    press(&mut state, &mut ui, KeyCode::Char('D'));
    assert_eq!(cursor_of(&ui).map(|(kind, _)| kind), Some(PickKind::Detail));
    press(&mut state, &mut ui, KeyCode::Char('D'));
    assert!(matches!(ui.mode, Mode::Normal));
    assert_eq!(state.detail, level);
}

fn theme_picker(state: &mut State, ui: &mut Ui) {
    let mut clients = Clients::empty();
    handle_key(state, &mut clients, ui, key(KeyCode::Char(':'))).unwrap();
    for ch in "theme".chars() {
        handle_key(state, &mut clients, ui, key(KeyCode::Char(ch))).unwrap();
    }
    handle_key(state, &mut clients, ui, key(KeyCode::Enter)).unwrap();
}

#[test]
fn colon_theme_opens_the_picker_on_the_current_row() {
    let mut state = State::boot(true, 15, &Config::default(), &Clients::empty(), None);
    state.scheme = Scheme::parse("nord-dark").unwrap();
    let mut ui = test_ui();
    theme_picker(&mut state, &mut ui);
    let at = Scheme::list()
        .iter()
        .position(|s| *s == state.scheme.base())
        .unwrap();
    assert_eq!(cursor_of(&ui), Some((PickKind::Theme, at)));
}

#[test]
fn the_theme_picker_previews_live_and_escape_restores() {
    let mut state = State::boot(true, 15, &Config::default(), &Clients::empty(), None);
    let mut ui = test_ui();
    theme_picker(&mut state, &mut ui);
    assert_eq!(state.scheme, Scheme::Auto);
    let before = state.theme.rgb(Kind::Blue);
    press(&mut state, &mut ui, KeyCode::Char('j'));
    assert_eq!(state.scheme, Scheme::Native);
    assert!(state.theme.native);
    press(&mut state, &mut ui, KeyCode::Char('j'));
    press(&mut state, &mut ui, KeyCode::Char('j'));
    press(&mut state, &mut ui, KeyCode::Char('j'));
    assert_eq!(state.scheme, Scheme::list()[4]);
    assert!(!state.theme.native);
    assert_ne!(state.theme.rgb(Kind::Blue), before);
    press(&mut state, &mut ui, KeyCode::Char('5'));
    assert_eq!(state.scheme, Scheme::list()[4]);
    press(&mut state, &mut ui, KeyCode::Esc);
    assert!(matches!(ui.mode, Mode::Normal));
    assert_eq!(state.scheme, Scheme::Auto);
    assert_eq!(state.theme.rgb(Kind::Blue), before);
}

#[test]
fn enter_on_the_settings_theme_row_opens_the_picker() {
    let mut state = State::boot(true, 15, &Config::default(), &Clients::empty(), None);
    let mut ui = test_ui();
    state.open_settings();
    state.settings_row = settings::ROWS
        .iter()
        .position(|row| matches!(row, Row::Theme))
        .unwrap();
    press(&mut state, &mut ui, KeyCode::Enter);
    assert_eq!(cursor_of(&ui), Some((PickKind::Theme, 0)));
    press(&mut state, &mut ui, KeyCode::Esc);
    assert!(matches!(ui.mode, Mode::Normal));
}

#[test]
fn a_tall_theme_picker_scrolls_to_the_cursor() {
    let mut state = State::boot(true, 15, &Config::default(), &Clients::empty(), None);
    let mut ui = test_ui();
    theme_picker(&mut state, &mut ui);
    for _ in 0..20 {
        press(&mut state, &mut ui, KeyCode::Char('j'));
    }
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 16)).unwrap();
    let frame = terminal
        .draw(|frame| draw(frame, &state, &ui, None))
        .unwrap();
    let text: Vec<String> = (0..16)
        .map(|y| {
            (0..100)
                .map(|x| frame.buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect();
    let label = Scheme::list()[20].word();
    assert!(text.iter().any(|line| line.contains(&label)), "{text:#?}");
    assert!(text.iter().any(|line| line.contains("21/31")), "{text:#?}");
}

/**
Draws at 104×44 and returns the selected post, the first body row of its block and the
first body row on screen.
*/
fn drawn(state: &State, ui: &Ui) -> (Option<String>, usize, usize) {
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(104, 44)).unwrap();
    terminal.draw(|frame| draw(frame, state, ui, None)).unwrap();
    let width = render::inner_width_for(104, state.detail);
    let focus = screens::linked_body(state, width, true).focus.unwrap();
    (state.selected_post_id(), focus.start, ui.scroll.get())
}

#[test]
fn f_and_slash_search_and_shift_f_filters() {
    for typed in ['f', '/'] {
        let mut state = State::boot(true, 15, &Config::default(), &Clients::empty(), None);
        let mut ui = test_ui();
        assert!(search_key(&mut state, &mut ui, key(KeyCode::Char(typed))));
        assert!(matches!(ui.mode, Mode::Search(_)), "{typed}");
        assert_eq!(state.section, Section::Search);
        state.apply(Cmd::Back, &mut Clients::empty()).unwrap();
        assert_eq!(state.section, Section::Home, "{typed}");
    }
    let mut state = State::boot(true, 15, &Config::default(), &Clients::empty(), None);
    let mut ui = test_ui();
    let shift_f = KeyEvent::new(KeyCode::Char('F'), KeyModifiers::SHIFT);
    assert!(search_key(&mut state, &mut ui, shift_f));
    assert!(matches!(ui.mode, Mode::Filter { .. }));
    assert_eq!(state.section, Section::Home);
    let ctrl_f = KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL);
    assert!(!search_key(&mut state, &mut test_ui(), ctrl_f));
    let alt_f = KeyEvent::new(KeyCode::Char('f'), KeyModifiers::ALT);
    assert!(!search_key(&mut state, &mut test_ui(), alt_f));
}

#[test]
fn alt_b_does_not_bookmark_and_alt_f_stays_on_the_feed() {
    let mut state = State::boot(true, 15, &Config::default(), &Clients::empty(), None);
    let mut clients = Clients::empty();
    let mut ui = test_ui();
    let section = state.section;
    handle_key(
        &mut state,
        &mut clients,
        &mut ui,
        with(KeyCode::Char('b'), KeyModifiers::ALT),
    )
    .unwrap();
    assert!(state.status_error.is_none());
    assert!(matches!(ui.mode, Mode::Normal));
    assert_eq!(state.section, section);
    handle_key(
        &mut state,
        &mut clients,
        &mut ui,
        with(KeyCode::Char('f'), KeyModifiers::ALT),
    )
    .unwrap();
    assert!(state.status_error.is_none());
    assert!(matches!(ui.mode, Mode::Normal));
    assert_eq!(state.section, section);
}

#[test]
fn colon_runs_the_picked_command() {
    let mut state = State::boot(true, 15, &Config::default(), &Clients::empty(), None);
    let mut clients = Clients::empty();
    let mut ui = test_ui();
    handle_key(&mut state, &mut clients, &mut ui, key(KeyCode::Char(':'))).unwrap();
    assert!(matches!(ui.mode, Mode::Command { .. }));
    for ch in "noti".chars() {
        handle_key(&mut state, &mut clients, &mut ui, key(KeyCode::Char(ch))).unwrap();
    }
    handle_key(&mut state, &mut clients, &mut ui, key(KeyCode::Enter)).unwrap();
    assert!(matches!(ui.mode, Mode::Normal));
    assert_eq!(state.section, crate::model::Section::Notifications);
}

#[test]
fn tab_completes_and_escape_closes_the_command_line() {
    let mut state = State::boot(true, 15, &Config::default(), &Clients::empty(), None);
    let mut clients = Clients::empty();
    let mut ui = test_ui();
    let ctrl_k = KeyEvent::new(KeyCode::Char('k'), KeyModifiers::CONTROL);
    handle_key(&mut state, &mut clients, &mut ui, ctrl_k).unwrap();
    for ch in "sh".chars() {
        handle_key(&mut state, &mut clients, &mut ui, key(KeyCode::Char(ch))).unwrap();
    }
    handle_key(&mut state, &mut clients, &mut ui, key(KeyCode::Tab)).unwrap();
    match &ui.mode {
        Mode::Command { input, .. } => assert_eq!(input.text(), "share "),
        _ => panic!("the command line closed"),
    }
    handle_key(&mut state, &mut clients, &mut ui, key(KeyCode::Esc)).unwrap();
    assert!(matches!(ui.mode, Mode::Normal));
}

#[test]
fn completion_keeps_the_arguments() {
    assert_eq!(completed("rep 2 hi", "reply", "[n] <text>"), "reply 2 hi");
    assert_eq!(completed("sh", "share", "[n]"), "share ");
    assert_eq!(completed("ho", "home", ""), "home");
    assert_eq!(command_line("sh", Some(&("share", "[n]", ""))), "share ");
    assert_eq!(command_line("like 2", Some(&("like", "[n]", ""))), "like 2");
    assert_eq!(command_matches("no").len(), 1);
}

#[test]
fn the_picked_suggestion_carries_the_bar() {
    let matches = command_matches("s");
    let lines = command_list(&matches, 1, 60);
    assert_eq!(lines.len(), matches.len() + 2);
    assert!(lines[2].plain().starts_with("│▍ "), "{}", lines[2].plain());
    assert!(lines[1].plain().starts_with("│  "), "{}", lines[1].plain());
    assert!(lines.iter().all(|line| line.plain().width() == 60));
}

#[test]
fn page_keys_move_the_selection_and_the_viewport_a_screen() {
    let mut state = State::boot(true, 15, &Config::default(), &Clients::empty(), None);
    let ui = test_ui();
    let (first, top, _) = drawn(&state, &ui);
    let height = ui.viewport.get().1;
    assert_eq!(
        ui.viewport.get(),
        (render::inner_width_for(104, state.detail), 36)
    );
    assert!(page_key(&mut state, &ui, key(KeyCode::PageDown)));
    let (paged, block, offset) = drawn(&state, &ui);
    assert_ne!(paged, first);
    assert!(block >= top + height - 2, "{block}");
    assert_eq!(offset, block + 1);
    assert!(page_key(&mut state, &ui, key(KeyCode::Char(' '))));
    let (again, further, _) = drawn(&state, &ui);
    assert_ne!(again, paged);
    assert!(further >= block + height - 2, "{further}");
    assert!(page_key(&mut state, &ui, key(KeyCode::PageUp)));
    let (back, earlier, offset) = drawn(&state, &ui);
    assert_ne!(back, again);
    assert!(earlier + height - 2 <= further, "{earlier}");
    assert_eq!(offset, earlier + 1);
    let ctrl_d = KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL);
    assert_eq!(page_rows(ctrl_d, height), Some(18));
    assert!(!page_key(&mut state, &ui, key(KeyCode::Char('j'))));
}

#[test]
fn follow_keeps_the_block_and_one_row_of_context() {
    let span = RowSpan { start: 30, end: 36 };
    let text = |_: usize| false;
    assert_eq!(follow_past_blanks(0, Some(span), 20, 100, text), 17);
    let span = RowSpan { start: 4, end: 8 };
    assert_eq!(follow_past_blanks(17, Some(span), 20, 100, text), 3);
    assert_eq!(follow_past_blanks(50, None, 20, 30, text), 10);
    let span = RowSpan { start: 10, end: 16 };
    let blank = |row| row == 9 || row == 16;
    // The block already fits, so the blank context rows stay put.
    assert_eq!(follow_past_blanks(0, Some(span), 20, 40, blank), 0);
    // Scrolling up to it pulls the line before the blank onto the top edge.
    assert_eq!(follow_past_blanks(30, Some(span), 20, 40, blank), 8);
    // No room for that line: the blank is dropped and the block's own row is the edge.
    assert_eq!(follow_past_blanks(30, Some(span), 8, 40, blank), 10);
    // A blank on the document's first row is the list pad, and it stays.
    let edge = RowSpan { start: 1, end: 5 };
    assert_eq!(
        follow_past_blanks(12, Some(edge), 10, 20, |row| row == 0),
        0
    );
}

#[test]
fn a_tall_block_keeps_an_offset_inside_it() {
    let span = RowSpan { start: 30, end: 70 };
    let text = |_: usize| false;
    assert_eq!(follow_past_blanks(0, Some(span), 10, 100, text), 29);
    assert_eq!(follow_past_blanks(80, Some(span), 10, 100, text), 61);
    assert_eq!(follow_past_blanks(40, Some(span), 10, 100, text), 40);
    assert_eq!(follow_past_blanks(65, Some(span), 10, 100, text), 61);
    let fitted = RowSpan { start: 30, end: 36 };
    assert_eq!(follow_past_blanks(0, Some(fitted), 20, 100, text), 17);
}

#[test]
fn pad_window_keeps_the_pads_at_the_ends() {
    let window = pad_window(0, 1, 100, 30);
    assert_eq!((window.top, window.lines, window.bottom), (1, 0..29, 0));
    let window = pad_window(1, 1, 100, 30);
    assert_eq!((window.top, window.lines, window.bottom), (0, 0..30, 0));
    let window = pad_window(72, 1, 100, 30);
    assert_eq!((window.top, window.lines, window.bottom), (0, 71..100, 1));
    let window = pad_window(71, 1, 100, 30);
    assert_eq!((window.top, window.lines, window.bottom), (0, 70..100, 0));
    let window = pad_window(0, 0, 10, 30);
    assert_eq!((window.top, window.lines, window.bottom), (0, 0..10, 0));
    let window = pad_window(0, 1, 2, 30);
    assert_eq!((window.top, window.lines, window.bottom), (1, 0..2, 1));
}

#[test]
fn reader_pad_is_one_row_on_a_list_and_none_on_settings_or_help() {
    let mut state = demo_state();
    let mut ui = test_ui();
    assert_eq!(reader_pad(&state, &ui), 1);
    state.density = Density::Compact;
    assert_eq!(reader_pad(&state, &ui), 1);
    state.open_settings();
    assert_eq!(reader_pad(&state, &ui), 0);
    state.settings = false;
    ui.mode = Mode::Help;
    assert_eq!(reader_pad(&state, &ui), 0);
}

#[test]
fn the_edge_pad_scrolls_out_of_the_row_under_the_rule() {
    let mut state = demo_state();
    let ui = test_ui();
    let rows = rows_of(&frame(&mut state, &ui, 100, 40));
    assert!(rows[5].trim().is_empty(), "top pad: {:?}", rows[5]);
    ui.scroll.set(1);
    let rows = rows_of(&frame(&mut state, &ui, 100, 40));
    assert!(
        rows[5].chars().any(|ch| !ch.is_whitespace()),
        "one row of scroll uses the row under the rule: {:?}",
        rows[5]
    );

    let mut state = demo_state();
    state.layout = Layout::Split;
    let ui = test_ui();
    let rows = rows_of(&frame(&mut state, &ui, 160, 40));
    assert!(
        rows[6].contains("Post"),
        "the thread bar sits under the pad: {:?}",
        rows[6]
    );
    assert!(
        !rows[5].contains("Post"),
        "the pad stays above Post: {:?}",
        rows[5]
    );
    let divider = rows[5].find('│').expect("divider");
    assert!(
        rows[5][..divider].trim().is_empty(),
        "feed pad: {:?}",
        rows[5]
    );
    assert!(
        rows[5][divider + '│'.len_utf8()..].trim().is_empty(),
        "thread pad: {:?}",
        rows[5]
    );
    assert!(
        rows[6][..divider].chars().any(|ch| !ch.is_whitespace()),
        "the feed's first line lines up with Post: {:?}",
        rows[6]
    );
}

#[test]
fn a_scrolled_post_uses_both_edges_of_the_body() {
    let mut state = demo_state();
    let mut ui = test_ui();
    frame(&mut state, &ui, 100, 40);
    for _ in 0..8 {
        press(&mut state, &mut ui, KeyCode::Char('j'));
        frame(&mut state, &ui, 100, 40);
        if ui.scroll.get() > 1 {
            break;
        }
    }
    assert!(ui.scroll.get() > 1, "the selection never left the top pad");
    let rows = rows_of(&frame(&mut state, &ui, 100, 40));
    let nav = rows.iter().rposition(|row| row.contains("Home")).unwrap();
    assert!(
        rows[5].chars().any(|ch| !ch.is_whitespace()),
        "top edge: {:?}",
        rows[5]
    );
    assert!(
        rows[nav - 2].chars().any(|ch| !ch.is_whitespace()),
        "bottom edge: {:?}",
        rows[nav - 2]
    );
}

/// Columns of one frame from `from_col` on, one row per line.
fn pane_text(buffer: &Buffer, from_col: u16) -> String {
    (0..buffer.area.height)
        .map(|y| {
            (from_col..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn a_short_split_scrolls_the_open_post_before_leaving_it() {
    let mut state = demo_state();
    state.layout = Layout::Split;
    state.detail = Detail::Rich;
    state.preview_ratio = Some(0.5);
    let mut ui = test_ui();
    let id = state
        .posts
        .values()
        .find(|post| post.text.starts_with("Half-block") && post.reposted_by.is_none())
        .map(|post| post.id.clone())
        .expect("river's post");
    frame(&mut state, &ui, 160, 40);
    assert!(state.select_post(&id, false), "the feed lists the post");
    press(&mut state, &mut ui, KeyCode::Enter);
    assert!(state.thread_focus.is_some());
    let buffer = frame(&mut state, &ui, 160, 22);
    let (width, height) = ui.viewport.get();
    let body = screens::linked_body(&state, width, true);
    let focus = body.focus.expect("the open post");
    let pad = reader_pad(&state, &ui);
    let span = RowSpan {
        start: focus.start + pad,
        end: focus.end + pad,
    };
    assert!(
        span.end - span.start > height,
        "the post fits the pane: {span:?} in {height} rows"
    );
    let divider = {
        let laid = panes(&mut state, &ui, 160).expect("split");
        (render::margin_for(160, state.detail) + laid.left + 1) as u16
    };
    let right = pane_text(&buffer, divider + 1);
    assert!(
        right.contains("Half-block"),
        "the open post starts on screen:\n{right}"
    );
    assert!(
        !right.contains("views"),
        "the short pane already shows the tail:\n{right}"
    );
    let opened = state.selected_post_id();
    let top = ui.scroll_right.get();
    press(&mut state, &mut ui, KeyCode::Char('j'));
    frame(&mut state, &ui, 160, 22);
    assert_eq!(state.selected_post_id(), opened);
    assert!(ui.scroll_right.get() > top, "j left the scroll at {top}");
    press(&mut state, &mut ui, KeyCode::Char('k'));
    frame(&mut state, &ui, 160, 22);
    assert_eq!(state.selected_post_id(), opened);
    assert_eq!(ui.scroll_right.get(), top);
    press(&mut state, &mut ui, KeyCode::Char(' '));
    frame(&mut state, &ui, 160, 22);
    assert_eq!(state.selected_post_id(), opened);
    assert!(ui.scroll_right.get() > top + 1, "{}", ui.scroll_right.get());

    let mut saw_tail = false;
    let mut previous = ui.scroll_right.get();
    for _ in 0..80 {
        let staying = state.selected_post_id();
        press(&mut state, &mut ui, KeyCode::Char('j'));
        let buffer = frame(&mut state, &ui, 160, 22);
        if pane_text(&buffer, divider + 1).contains("views") {
            saw_tail = true;
        }
        let now = ui.scroll_right.get();
        if state.selected_post_id() != staying {
            assert!(saw_tail, "j left the post before its tail was on screen");
            break;
        }
        if now == previous {
            assert!(saw_tail, "j stopped at {now} before the tail");
            break;
        }
        assert!(now > previous, "{now} yanked back from {previous}");
        previous = now;
    }
    assert!(saw_tail, "the tail never reached the pane");
    let at_tail = ui.scroll_right.get();
    let still = state.selected_post_id();
    press(&mut state, &mut ui, KeyCode::PageDown);
    frame(&mut state, &ui, 160, 22);
    if state.selected_post_id() == still {
        assert!(
            ui.scroll_right.get() >= at_tail,
            "page yanked a finished post back to {}",
            ui.scroll_right.get()
        );
    }
}

#[test]
fn the_end_pad_sits_on_the_row_above_the_footer() {
    let mut state = fixtures::demo_state(Detail::Standard);
    state.demo = false;
    let id = state.current_feed_id().unwrap();
    state
        .feeds
        .get_mut(&id)
        .unwrap()
        .next
        .insert(Network::X, "cursor".into());
    let posts = state.current_posts();
    let selectable = crate::layout::selectable(&state.lay(&posts));
    assert!(state.select_row(*selectable.last().unwrap()));
    let ui = test_ui();
    let rows = rows_of(&frame(&mut state, &ui, 80, 24));
    let nav = rows.iter().rposition(|row| row.contains("Home")).unwrap();
    assert!(
        rows[nav - 2].trim().is_empty(),
        "bottom pad: {:?} above {:?}",
        rows[nav - 2],
        rows[nav - 1]
    );
    assert!(rows[..nav - 2].iter().any(|row| row.contains("loading")));
}

#[test]
fn composer_rows_break_after_spaces() {
    let text = "hello brave new world";
    let rows: Vec<&str> = wrap_ranges(text, 11)
        .iter()
        .map(|(a, b)| &text[*a..*b])
        .collect();
    assert_eq!(rows, ["hello brave ", "new world"]);
    assert_eq!(wrap_ranges("a\nb", 10), [(0, 1), (2, 3)]);
}

#[test]
fn inspector_marks_the_cursor_row_and_expands_on_toggle() {
    let value = serde_json::json!({ "id": "p1", "metrics": { "likes": 48 } });
    let mut open = HashSet::new();
    let lines = inspect_lines(&value, &open, 1);
    let text: Vec<String> = lines.iter().map(|l| l.plain()).collect();
    assert_eq!(text, ["  id: \"p1\"", "▍ metrics: {… 1 }"]);
    assert!(!lines[0].selected && lines[1].selected);
    let nodes = jsonview::visible(&value, &open);
    jsonview::toggle(&mut open, &nodes[1].0);
    let text: Vec<String> = inspect_lines(&value, &open, 1)
        .iter()
        .map(|l| l.plain())
        .collect();
    assert_eq!(text, ["  id: \"p1\"", "▍ metrics: {", "    likes: 48"]);
}

#[test]
fn gallery_arrows_wrap_and_close_keys_close() {
    let urls = vec!["demo://wave".to_string(), "demo://spark".to_string()];
    let mut gallery = Gallery::new(None, urls);
    assert_eq!(
        gallery_key(&mut gallery, key(KeyCode::Right)),
        GalleryStep::Stay
    );
    assert_eq!(gallery.index, 1);
    assert_eq!(
        gallery_key(&mut gallery, key(KeyCode::Right)),
        GalleryStep::Stay
    );
    assert_eq!(gallery.index, 0);
    assert_eq!(
        gallery_key(&mut gallery, key(KeyCode::Char('h'))),
        GalleryStep::Stay
    );
    assert_eq!(gallery.index, 1);
    assert_eq!(
        gallery_key(&mut gallery, key(KeyCode::Char('o'))),
        GalleryStep::Open("demo://spark".into())
    );
    assert_eq!(
        gallery_key(&mut gallery, key(KeyCode::Char('s'))),
        GalleryStep::Stay
    );
    for code in [
        KeyCode::Esc,
        KeyCode::Backspace,
        KeyCode::Char('v'),
        KeyCode::Char('q'),
    ] {
        assert_eq!(gallery_key(&mut gallery, key(code)), GalleryStep::Close);
    }
}

#[test]
fn a_single_picture_ignores_the_arrows() {
    let mut gallery = Gallery::new(None, vec!["demo://spark".to_string()]);
    assert_eq!(
        gallery_key(&mut gallery, key(KeyCode::Left)),
        GalleryStep::Stay
    );
    assert_eq!(gallery.index, 0);
}

#[test]
fn slots_paint_only_when_wholly_on_screen_and_load_one_screen_around() {
    let slot = Slot {
        url: "u".into(),
        x: 6,
        y: 20,
        cols: 40,
        rows: 10,
        avatar: false,
        square: false,
    };
    assert_eq!(on_screen(&slot, 0, 30), Some(20));
    assert_eq!(on_screen(&slot, 0, 29), None);
    assert_eq!(on_screen(&slot, 21, 30), None);
    assert!(wanted(&slot, 0, 10));
    assert!(!wanted(&slot, 0, 6));
    assert!(wanted(&slot, 35, 10));
    assert!(!wanted(&slot, 41, 10));
}

#[test]
fn alt_text_keeps_two_lines_and_ends_with_an_ellipsis() {
    assert_eq!(
        alt_lines("a short alt", 40),
        vec!["a short alt".to_string()]
    );
    let lines = alt_lines("one two three four five six seven eight nine ten", 10);
    assert_eq!(lines.len(), 2);
    assert!(lines[1].ends_with('…'), "{lines:?}");
    assert!(alt_lines("   ", 10).is_empty());
}

#[test]
fn centered_puts_a_smaller_box_in_the_middle() {
    let area = Rect::new(4, 4, 100, 30);
    assert_eq!(
        centered(area, Rect::new(0, 0, 60, 30)),
        Rect::new(24, 4, 60, 30)
    );
    assert_eq!(
        centered(area, Rect::new(4, 4, 100, 10)),
        Rect::new(4, 14, 100, 10)
    );
}

#[test]
fn the_spinner_turns_every_120_ms() {
    let frames: String = [0, 119, 120, 240, 360, 480]
        .map(|ms| spin_frame(Duration::from_millis(ms)))
        .iter()
        .collect();
    assert_eq!(frames, "◐◐◓◑◒◐");
}

#[test]
fn the_loading_line_turns_only_in_rich() {
    let line = busy_line("loading", true, false, Duration::from_millis(130), false);
    assert_eq!(line, "◓ loading");
    assert_eq!(
        busy_line("loading", false, false, Duration::ZERO, false),
        "loading…"
    );
    assert_eq!(
        busy_line("sending", false, true, Duration::ZERO, true),
        "sending... · ctrl-c again to quit"
    );
}

#[test]
fn the_loading_line_keeps_the_last_frame() {
    let state = State::boot(true, 15, &Config::default(), &Clients::empty(), None);
    let ui = test_ui();
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(104, 44)).unwrap();
    let last = terminal
        .draw(|frame| draw(frame, &state, &ui, None))
        .unwrap()
        .buffer
        .clone();
    terminal
        .draw(|frame| paint_busy(frame, &last, &state.theme, state.detail, "loading…"))
        .unwrap();
    let buffer = terminal.backend().buffer().clone();
    let row = |buffer: &Buffer, y: u16| {
        (0..104)
            .map(|x| buffer[(x, y)].symbol().to_string())
            .collect::<String>()
    };
    assert_eq!(row(&buffer, 1), row(&last, 1));
    assert_eq!(row(&buffer, 20), row(&last, 20));
    assert_eq!(row(&buffer, 43).trim_end(), "loading…");
}

#[test]
fn rich_avatars_paint_over_the_letter_box_with_a_graphics_protocol() {
    let mut state = State::boot(true, 15, &Config::default(), &Clients::empty(), None);
    state.detail = theme::Detail::Rich;
    state.retheme();
    state.preview_ratio = Some(0.5);
    assert!(state
        .view(100, true)
        .previews
        .iter()
        .all(|slot| !slot.avatar));
    state.graphics = true;
    assert!(state
        .view(100, true)
        .previews
        .iter()
        .any(|slot| slot.avatar));

    let ui = test_ui();
    let mut picker = ratatui_image::picker::Picker::halfblocks();
    picker.set_protocol_type(ratatui_image::picker::ProtocolType::Iterm2);
    let mut png = Vec::new();
    image::RgbaImage::new(8, 8)
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .unwrap();
    let fetch: preview::Fetch = Arc::new(move |_| Ok(png.clone()));
    ui.previews.replace(Some(Previews::new(picker, fetch)));
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(104, 44)).unwrap();
    terminal
        .draw(|frame| draw(frame, &state, &ui, None))
        .unwrap();
    let center = render::margin(104) as u16 + 3;
    let buffer = terminal.backend().buffer().clone();
    let spinning = (0..44).any(|y| {
        let symbol = buffer[(center, y)].symbol();
        SPINNER.iter().any(|frame| symbol == frame.to_string())
    });
    assert!(spinning, "no avatar showed the spinner while loading");

    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(previews) = ui.previews.borrow_mut().as_mut() {
            previews.poll();
        }
        terminal
            .draw(|frame| draw(frame, &state, &ui, None))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let corner = render::margin(104) as u16 + 1;
        if (0..44).any(|y| buffer[(corner, y)].symbol().contains("1337;File")) {
            break;
        }
        assert!(Instant::now() < deadline, "no avatar was painted");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn plain_detail_draws_ascii_chrome_without_color() {
    use ratatui::style::{Color, Modifier};
    let mut state = State::boot(true, 15, &Config::default(), &Clients::empty(), None);
    state.detail = theme::Detail::Plain;
    state.retheme();
    let ui = test_ui();
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(104, 44)).unwrap();
    terminal
        .draw(|frame| draw(frame, &state, &ui, None))
        .unwrap();
    let buffer = terminal.backend().buffer().clone();
    let row = |y: u16| {
        (0..104)
            .map(|x| buffer[(x, y)].symbol().to_string())
            .collect::<String>()
    };
    for y in [1, 3, 4, 41, 42, 43] {
        let text = row(y);
        assert!(text.chars().all(|ch| ch.is_ascii() || ch == '·'), "{text}");
    }
    assert!(row(4).starts_with("  ---"), "{}", row(4));
    assert!(row(43).contains("move  enter open"), "{}", row(43));
    assert!((5..41).any(|y| buffer[(1, y)].symbol() == ">"));
    for y in 0..44 {
        for x in 0..104 {
            let cell = &buffer[(x, y)];
            assert_eq!((cell.fg, cell.bg), (Color::Reset, Color::Reset), "{x},{y}");
            assert!(
                !cell.modifier.intersects(Modifier::DIM | Modifier::REVERSED),
                "{x},{y}"
            );
        }
    }
}

#[test]
fn the_viewer_draws_byline_count_rules_and_keys() {
    let mut state = State::boot(true, 15, &Config::default(), &Clients::empty(), None);
    state.reveal_shortcuts = true;
    let mut ui = test_ui();
    let urls = vec!["demo://wave".to_string(), "demo://spark".to_string()];
    ui.mode = Mode::Gallery(Box::new(Gallery::new(None, urls)));
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(104, 44)).unwrap();
    terminal
        .draw(|frame| draw(frame, &state, &ui, None))
        .unwrap();
    let buffer = terminal.backend().buffer().clone();
    let row = |y: u16| {
        (0..104)
            .map(|x| buffer[(x, y)].symbol().to_string())
            .collect::<String>()
    };
    assert!(row(1).trim_end().ends_with("1 of 2"), "{}", row(1));
    assert!(row(2).contains("────"));
    assert!(row(42).contains("────"));
    assert!(
        row(43).contains("← → previous, next  o open original  esc close"),
        "{}",
        row(43)
    );
    assert!(
        row(22).contains("image unavailable · o opens it"),
        "{}",
        row(22)
    );
}

#[test]
fn enter_opens_the_thread_and_backspace_goes_back() {
    let mut state = State::boot(true, 15, &Config::default(), &Clients::empty(), None);
    let mut clients = Clients::empty();
    let mut ui = test_ui();
    assert!(state.selected_post().is_some());
    handle_key(&mut state, &mut clients, &mut ui, key(KeyCode::Enter)).unwrap();
    assert!(state.thread_focus.is_some());
    handle_key(&mut state, &mut clients, &mut ui, key(KeyCode::Backspace)).unwrap();
    assert!(state.thread_focus.is_none());
    handle_key(&mut state, &mut clients, &mut ui, key(KeyCode::Char('}'))).unwrap();
    assert_eq!(state.shown_merged_tab(), MergedTab::Discover);
    handle_key(&mut state, &mut clients, &mut ui, key(KeyCode::Char('1'))).unwrap();
    assert_eq!(state.shown_merged_tab(), MergedTab::Home);
}

#[test]
fn wide_screens_split_into_feed_and_thread() {
    let mut state = State::boot(true, 15, &Config::default(), &Clients::empty(), None);
    state.layout = Layout::Auto;
    let ui = test_ui();
    assert!(panes(&mut state, &ui, 120).is_none());
    let p = panes(&mut state, &ui, 160).expect("160 columns split under auto");
    assert!(state.thread_focus.is_none());
    assert_eq!((p.left, p.right), (58, 98));
    assert!(!p.thread_focused);
    let root = state.selected_post().unwrap().text.clone();
    let first_words: String = root.chars().take(12).collect();
    assert!(p
        .thread
        .lines
        .iter()
        .any(|line| line.plain().contains(&first_words)));
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(160, 44)).unwrap();
    terminal
        .draw(|frame| draw(frame, &state, &ui, Some(&p)))
        .unwrap();
    let buffer = terminal.backend().buffer();
    let divider = (render::margin_for(160, state.detail) + p.left + 1) as u16;
    assert_eq!(buffer[(divider, RULE_ROW as u16)].symbol(), "┬");
    assert_eq!(buffer[(divider, 10)].symbol(), "│");
    let sub = (0..160)
        .map(|x| buffer[(x, 3)].symbol().to_string())
        .collect::<String>();
    assert!(
        sub.trim_end().ends_with("Saved"),
        "feed tabs stay on the right edge, got {sub}"
    );
    assert!(
        !sub.contains("oldest"),
        "reply order stays in the thread pane, got {sub}"
    );
    assert_eq!(ui.viewport.get().0, 58);
    state.layout = Layout::Split;
    assert!(panes(&mut state, &ui, 120).is_some());
    assert!(panes(&mut state, &ui, 99).is_none());
    state.layout = Layout::Single;
    assert!(panes(&mut state, &ui, 200).is_none());
}

#[test]
fn a_focused_split_keeps_the_feed_bar_and_seats_the_avatar_under_post() {
    let mut state = fixtures::demo_state(Detail::Rich);
    state.layout = Layout::Split;
    state.graphics = true;
    let mut ui = test_ui();
    press(&mut state, &mut ui, KeyCode::Right);
    assert!(state.thread_focus.is_some());
    let mut picker = ratatui_image::picker::Picker::halfblocks();
    picker.set_protocol_type(ratatui_image::picker::ProtocolType::Iterm2);
    let mut png = Vec::new();
    image::RgbaImage::new(8, 8)
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .unwrap();
    let fetch: preview::Fetch = std::sync::Arc::new(move |_| Ok(png.clone()));
    ui.previews.replace(Some(Previews::new(picker, fetch)));

    let buffer = frame(&mut state, &ui, 160, 40);
    let row = |y: u16| {
        (0..160)
            .map(|x| buffer[(x, y)].symbol().to_string())
            .collect::<String>()
    };
    let sub = row(3);
    assert!(
        sub.trim_end().ends_with("Saved"),
        "feed tabs stay on the right edge while the thread has the keys, got {sub}"
    );
    assert!(
        !sub.contains("oldest"),
        "reply order stays in the thread pane, got {sub}"
    );
    let header = (0..40u16)
        .find(|y| row(*y).contains("oldest"))
        .expect("the thread pane header");
    let post_at = (0..157u16)
        .rev()
        .find(|&x| {
            buffer[(x, header)].symbol() == "P"
                && buffer[(x + 1, header)].symbol() == "o"
                && buffer[(x + 2, header)].symbol() == "s"
                && buffer[(x + 3, header)].symbol() == "t"
        })
        .expect("Post");
    let top = post_at + 1;
    let mid = post_at + 3;
    assert_eq!(
        buffer[(top, header + 1)].symbol(),
        "╭",
        "{}",
        row(header + 1)
    );
    assert_eq!(
        buffer[(mid, header + 1)].symbol(),
        "─",
        "the avatar top stays under Post, got {}",
        row(header + 1)
    );
    let letter = buffer[(mid, header + 2)].symbol();
    assert!(
        SPINNER.iter().any(|frame| letter == frame.to_string()),
        "the loading picture sits on the letter, got {letter} on {}",
        row(header + 2)
    );

    state.layout = Layout::Single;
    let buffer = frame(&mut state, &ui, 120, 40);
    let sub: String = (0..120)
        .map(|x| buffer[(x, 3)].symbol().to_string())
        .collect();
    assert!(
        sub.contains("Post") && sub.contains("oldest"),
        "a single column still uses the subbar as the thread bar, got {sub}"
    );
}

#[test]
fn an_open_thread_takes_the_keys_in_the_split() {
    let mut state = State::boot(true, 15, &Config::default(), &Clients::empty(), None);
    state.layout = Layout::Auto;
    let mut clients = Clients::empty();
    let mut ui = test_ui();
    handle_key(&mut state, &mut clients, &mut ui, key(KeyCode::Enter)).unwrap();
    let open = state.thread_focus.clone();
    assert!(open.is_some());
    let p = panes(&mut state, &ui, 160).unwrap();
    assert!(p.thread_focused);
    assert_eq!(state.thread_focus, open);
    assert!(p.feed.lines.iter().any(|line| line.bar));
}

#[test]
fn the_split_keeps_every_post_when_the_right_pane_has_the_keys() {
    let mut state = fixtures::demo_state(Detail::Standard);
    state.layout = Layout::Auto;
    let mut ui = test_ui();
    frame(&mut state, &ui, 160, 40);
    assert!(state.select_post("devnull:p2", false));
    let preview = panes(&mut state, &ui, 160).expect("split");
    assert!(!preview.thread_focused);
    assert!(
        preview
            .thread
            .lines
            .iter()
            .any(|line| line.plain().contains("Hype")),
        "the reply is in the right pane before the keys move"
    );
    press(&mut state, &mut ui, KeyCode::Right);
    assert!(state.thread_focus.is_some());
    assert!(!matches!(
        state.focus,
        Some(crate::model::FeedId::Thread(_))
    ));
    let open = panes(&mut state, &ui, 160).expect("split");
    let feed: String = open.feed.lines.iter().map(|line| line.plain()).collect();
    assert!(feed.contains("Step 1"), "{feed}");
    assert!(feed.contains("ok devs"), "{feed}");
    press(&mut state, &mut ui, KeyCode::Left);
    assert!(state.thread_focus.is_none());
    let back = panes(&mut state, &ui, 160).expect("split");
    let feed: String = back.feed.lines.iter().map(|line| line.plain()).collect();
    assert!(
        feed.contains("Step 1") && feed.contains("ok devs"),
        "{feed}"
    );
}

#[test]
fn the_split_feed_lists_roots_and_keeps_the_selection_across_the_threshold() {
    let mut state = fixtures::demo_state(Detail::Standard);
    state.layout = Layout::Auto;
    let mut ui = test_ui();
    frame(&mut state, &ui, 100, 40);
    for _ in 0..12 {
        if state
            .selected_post()
            .is_some_and(|p| p.in_reply_to.is_some())
        {
            break;
        }
        press(&mut state, &mut ui, KeyCode::Char('j'));
    }
    let reply = state.selected_post().cloned().expect("a reply");
    assert!(reply.in_reply_to.is_some());
    frame(&mut state, &ui, 160, 40);
    assert!(ui.split.get());
    assert!(state.overview);
    let root = state.selected_post().cloned().expect("a selection");
    assert_ne!(root.id, reply.id);
    assert_eq!(root.conversation_id, reply.conversation_id);
    assert!(state
        .lay(&state.current_posts())
        .iter()
        .all(|r| r.depth == 0));
    press(&mut state, &mut ui, KeyCode::Char('j'));
    let next = state.selected_post_id();
    assert_ne!(next.as_deref(), Some(root.id.as_str()));
    frame(&mut state, &ui, 123, 40);
    assert!(!ui.split.get());
    assert!(!state.overview);
    assert_eq!(state.selected_post_id(), next);
}

#[test]
fn x_shows_and_hides_the_replies_in_place() {
    let mut state = fixtures::demo_state(Detail::Standard);
    state.layout = Layout::Auto;
    let mut ui = test_ui();
    frame(&mut state, &ui, 160, 40);
    let deep = |state: &State| {
        state
            .lay(&state.current_posts())
            .iter()
            .any(|r| r.depth > 0)
    };
    assert!(!deep(&state));
    assert!(state.select_post("devnull:p2", false));
    let id = state.selected_post_id();
    press(&mut state, &mut ui, KeyCode::Char('x'));
    frame(&mut state, &ui, 160, 40);
    assert_eq!(state.selected_post_id(), id);
    assert!(deep(&state), "x shows the replies inline");
    press(&mut state, &mut ui, KeyCode::Char('x'));
    frame(&mut state, &ui, 160, 40);
    assert_eq!(state.selected_post_id(), id);
    assert!(!deep(&state));
}

#[test]
fn an_open_thread_keeps_its_place_across_the_threshold() {
    let mut state = fixtures::demo_state(Detail::Standard);
    state.layout = Layout::Auto;
    let mut ui = test_ui();
    frame(&mut state, &ui, 160, 30);
    assert!(state.select_post("devnull:p2", false));
    press(&mut state, &mut ui, KeyCode::Enter);
    assert!(state.thread_focus.is_some());
    for _ in 0..3 {
        press(&mut state, &mut ui, KeyCode::Char('j'));
    }
    frame(&mut state, &ui, 160, 30);
    let id = state.selected_post_id();
    let key = state.selection_key();
    let buffer = frame(&mut state, &ui, 123, 30);
    assert!(!ui.split.get());
    assert_eq!(state.selected_post_id(), id);
    assert_eq!(*ui.scroll_key.borrow(), key);
    assert!(
        !bar_rows(&buffer, state.detail).is_empty(),
        "the selected reply is on screen"
    );
    frame(&mut state, &ui, 160, 30);
    assert!(ui.split.get());
    assert_eq!(state.selected_post_id(), id);
    assert_eq!(*ui.scroll_right_key.borrow(), key);
}

// MARK: input batches

fn pointer(kind: MouseEventKind, column: u16) -> MouseEvent {
    MouseEvent {
        kind,
        column,
        row: 20,
        modifiers: KeyModifiers::NONE,
    }
}

fn typed_key(ch: char) -> Event {
    Event::Key(key(KeyCode::Char(ch)))
}

/// Runs `events` through `coalesce` over one pane and records each press; `q` quits.
fn batch_of(events: Vec<Event>) -> (Vec<Press>, Batch) {
    let mut ran = Vec::new();
    let batch = coalesce(
        events,
        |_, _| Some(Zone::BodyLeft),
        |press| {
            let quits = matches!(&press, Press::Key(key) if key.code == KeyCode::Char('q'));
            ran.push(press);
            Ok(quits)
        },
    );
    (ran, batch.unwrap())
}

#[test]
fn a_pointer_flood_runs_only_its_last_move() {
    let columns = (1..=100).chain((1..=100).rev());
    let events = columns
        .map(|column| Event::Mouse(pointer(MouseEventKind::Moved, column)))
        .collect();
    let (ran, batch) = batch_of(events);
    assert_eq!(ran, [Press::Mouse(pointer(MouseEventKind::Moved, 1))]);
    assert_eq!(batch.moves_dropped, 199);
    assert_eq!(batch.processed, 1);
}

#[test]
fn each_same_direction_wheel_run_over_one_pane_is_one_step() {
    let mut events = vec![Event::Mouse(pointer(MouseEventKind::ScrollDown, 10)); 30];
    events.extend(vec![Event::Mouse(pointer(MouseEventKind::ScrollUp, 10)); 2]);
    let (ran, batch) = batch_of(events);
    assert_eq!(
        ran,
        [
            Press::Wheel(pointer(MouseEventKind::ScrollDown, 10), 30),
            Press::Wheel(pointer(MouseEventKind::ScrollUp, 10), -2),
        ]
    );
    assert_eq!(batch.scroll_net, 28);
    assert_eq!(batch.processed, 2);
}

/// The selected post after `events` arrive one by one, after they arrive as one batch, and the batch's steps.
fn wheel_paths(events: &[MouseEventKind]) -> (Option<String>, Option<String>, Vec<i32>) {
    let at = |kind: MouseEventKind| MouseEvent {
        kind,
        column: 20,
        row: 12,
        modifiers: KeyModifiers::NONE,
    };
    let mut single = fixtures::demo_state(Detail::Standard);
    let mut ui = test_ui();
    frame(&mut single, &ui, 104, 44);
    for kind in events {
        handle_mouse(&mut single, &mut Clients::empty(), &mut ui, at(*kind)).unwrap();
    }
    let mut batched = fixtures::demo_state(Detail::Standard);
    let mut ui = test_ui();
    frame(&mut batched, &ui, 104, 44);
    let batch = events.iter().map(|kind| Event::Mouse(at(*kind))).collect();
    let mut taken = Vec::new();
    coalesce(
        batch,
        |_, _| Some(Zone::BodyLeft),
        |press| match press {
            Press::Wheel(mouse, steps) => {
                taken.push(steps);
                wheel(
                    &mut batched,
                    &mut Clients::empty(),
                    &mut ui,
                    steps,
                    usize::from(mouse.column),
                    usize::from(mouse.row),
                )
            }
            _ => Ok(false),
        },
    )
    .unwrap();
    (single.selected_post_id(), batched.selected_post_id(), taken)
}

#[test]
fn a_batched_wheel_run_moves_as_far_as_its_single_notches() {
    let down = vec![MouseEventKind::ScrollDown; 20];
    let (single, batched, steps) = wheel_paths(&down);
    assert!(single.is_some());
    assert_eq!(batched, single);
    assert_eq!(steps, [20]);
    let mut mixed = vec![MouseEventKind::ScrollDown; 30];
    mixed.extend(vec![MouseEventKind::ScrollUp; 4]);
    let (single, batched, steps) = wheel_paths(&mixed);
    assert_eq!(single.as_deref(), Some("x:l1"));
    assert_eq!(batched, single);
    assert_eq!(steps, [30, -4]);
}

#[test]
fn keys_and_pastes_run_in_arrival_order() {
    let moved = pointer(MouseEventKind::Moved, 3);
    let events = vec![
        typed_key('j'),
        typed_key('k'),
        Event::Mouse(moved),
        Event::Mouse(pointer(MouseEventKind::Up(MouseButton::Left), 3)),
        Event::Mouse(moved),
        typed_key('l'),
        Event::Paste("hi".into()),
        typed_key('b'),
    ];
    let (ran, batch) = batch_of(events);
    assert_eq!(
        ran,
        [
            Press::Key(key(KeyCode::Char('j'))),
            Press::Key(key(KeyCode::Char('k'))),
            Press::Mouse(moved),
            Press::Key(key(KeyCode::Char('l'))),
            Press::Paste("hi".into()),
            Press::Key(key(KeyCode::Char('b'))),
        ]
    );
    assert_eq!(batch.moves_dropped, 1);
    assert!(!batch.quit);
}

#[test]
fn a_key_that_quits_stops_the_batch() {
    let events = vec![typed_key('j'), typed_key('q'), typed_key('k')];
    let (ran, batch) = batch_of(events);
    assert_eq!(
        ran,
        [
            Press::Key(key(KeyCode::Char('j'))),
            Press::Key(key(KeyCode::Char('q'))),
        ]
    );
    assert!(batch.quit);
}

#[test]
fn a_resize_burst_counts_once() {
    let events = (0..12).map(|i| Event::Resize(80 + i, 40)).collect();
    let (ran, batch) = batch_of(events);
    assert!(ran.is_empty());
    assert_eq!(batch.resizes, 1);
    assert_eq!(batch.processed, 1);
}

#[test]
fn hover_reports_turn_on_with_the_network_picker_and_off_when_it_closes() {
    let flags = [AtomicBool::new(false), AtomicBool::new(false)];
    let mut state = demo_state();
    let mut ui = test_ui();
    let sync = |ui: &Ui| {
        let mut out = Vec::new();
        report_modes(&mut out, [&flags[0], &flags[1]], true, hovers(&ui.mode)).unwrap();
        String::from_utf8(out).unwrap()
    };
    assert_eq!(sync(&ui), "\x1b[?1000h\x1b[?1002h\x1b[?1006h");
    press(&mut state, &mut ui, KeyCode::Char('A'));
    assert_eq!(sync(&ui), "\x1b[?1003h");
    assert_eq!(sync(&ui), "");
    press(&mut state, &mut ui, KeyCode::Esc);
    assert_eq!(sync(&ui), "\x1b[?1003l");
    assert_eq!(sync(&ui), "");

    press(&mut state, &mut ui, KeyCode::Char('A'));
    assert_eq!(sync(&ui), "\x1b[?1003h");
    let mut out = Vec::new();
    report_modes(&mut out, [&flags[0], &flags[1]], false, true).unwrap();
    assert!(String::from_utf8(out).unwrap().contains("\x1b[?1003l"));
    assert!(!flags[1].load(Ordering::Relaxed));
}

#[test]
fn an_idle_step_draws_nothing_until_the_refresh() {
    let recent = Some(Duration::from_secs(29));
    assert!(!should_draw(false, false, recent));
    assert!(should_draw(true, false, recent));
    assert!(should_draw(false, true, recent));
    assert!(should_draw(false, false, Some(REFRESH)));
    assert!(should_draw(false, false, None));
}

// MARK: focus and mouse

fn demo_state() -> State {
    State::boot(true, 15, &Config::default(), &Clients::empty(), None)
}

/// Draws one frame `width` columns wide, split when the layout says so, and returns it.
fn frame(state: &mut State, ui: &Ui, width: u16, height: u16) -> Buffer {
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, height)).unwrap();
    let split = panes(state, ui, usize::from(width));
    terminal
        .draw(|frame| draw(frame, state, ui, split.as_ref()))
        .unwrap();
    terminal.backend().buffer().clone()
}

const VIEWPORTS: [(u16, u16); 11] = [
    (24, 8),
    (32, 16),
    (40, 20),
    (43, 20),
    (44, 20),
    (60, 20),
    (80, 24),
    (100, 32),
    (123, 32),
    (124, 40),
    (180, 45),
];
const DETAILS: [Detail; 4] = [
    Detail::Plain,
    Detail::Minimal,
    Detail::Standard,
    Detail::Rich,
];

/**
What every viewport keeps to after a frame `width`×`height`: body lines fit the content width,
every target and dialog row the frame registered lies inside it, and the printed feed, without
color, fits the terminal.
*/
fn assert_inside(state: &State, ui: &Ui, width: u16, height: u16, what: &str) {
    let inner = render::inner_width_for(usize::from(width), state.detail);
    for line in screens::linked_body(state, inner, true).lines {
        let text = line.plain();
        assert!(text.width() <= inner, "{what}: {text:?}");
    }
    let hits = ui.hits.borrow();
    for target in &hits.targets {
        assert!(
            target.x + target.width <= width && target.y < height,
            "{what}: {target:?}"
        );
    }
    for (y, cols, _) in &hits.rows {
        assert!(
            cols.end <= usize::from(width) && *y < usize::from(height),
            "{what}: row {y} {cols:?}"
        );
    }
    let opts = crate::plain::PrintOpts {
        color: false,
        json: false,
        images: false,
        prompt: false,
        width: usize::from(width),
    };
    let (feed, footer) = crate::plain::printed(state, &opts, false);
    for line in feed.iter().chain(&footer) {
        assert!(
            line.width() <= usize::from(width),
            "{what} printed: {line:?}"
        );
    }
}

#[test]
fn f6_steps_through_the_zones_that_have_stops() {
    let mut state = fixtures::demo_state(Detail::Standard);
    let mut ui = test_ui();
    frame(&mut state, &ui, 100, 40);
    let mut zones: Vec<Zone> = tab_stops(&ui.hits.borrow().targets)
        .iter()
        .map(|t| t.zone)
        .collect();
    zones.dedup();
    assert!(zones.len() >= 2, "{zones:?}");
    let selected = state.selected_post_id();
    let mut seen = Vec::new();
    for _ in 0..zones.len() {
        press(&mut state, &mut ui, KeyCode::F(6));
        frame(&mut state, &ui, 100, 40);
        let zone = ui.focus.borrow().as_ref().map(|f| f.zone).unwrap();
        if zone == Zone::BodyLeft {
            assert_eq!(state.selected_post_id(), selected);
        }
        seen.push(zone);
    }
    let mut each = seen.clone();
    each.sort();
    each.dedup();
    assert_eq!(each, zones, "{seen:?}");
    handle_key(
        &mut state,
        &mut Clients::empty(),
        &mut ui,
        with(KeyCode::F(6), KeyModifiers::SHIFT),
    )
    .unwrap();
    let back = ui.focus.borrow().as_ref().map(|f| f.zone).unwrap();
    assert_eq!(back, seen[seen.len() - 2]);
    press(&mut state, &mut ui, KeyCode::F(18));
    let back = ui.focus.borrow().as_ref().map(|f| f.zone).unwrap();
    assert_eq!(back, seen[(seen.len() * 3 - 3) % seen.len()]);
}

#[test]
fn every_focus_range_ends_on_its_own_content() {
    let rail = |c: char| c.is_whitespace() || "│╎┊┆╵╷▍┃".contains(c);
    for detail in [Detail::Standard, Detail::Rich] {
        let mut state = fixtures::demo_state(detail);
        let mut ui = test_ui();
        for step in 0..10 {
            frame(&mut state, &ui, 100, 40);
            let body = screens::linked_body(&state, render::inner_width_for(100, detail), true);
            if let Some(span) = body.focus {
                let end = span.end.max(span.start + 1);
                let block: Vec<String> = body.lines[span.start..end]
                    .iter()
                    .map(|l| l.plain())
                    .collect();
                assert!(
                    !block[block.len() - 1].chars().all(rail),
                    "{detail:?} step {step} {:?}: {block:#?}",
                    state.selected_post_id()
                );
            }
            press(&mut state, &mut ui, KeyCode::Char('j'));
        }
    }
}

#[test]
fn every_viewport_and_detail_level_draws() {
    for (width, height) in VIEWPORTS {
        for detail in DETAILS {
            let mut state = fixtures::demo_state(detail);
            let ui = test_ui();
            frame(&mut state, &ui, width, height);
            assert_inside(
                &state,
                &ui,
                width,
                height,
                &format!("{width}x{height} {detail:?}"),
            );
        }
    }
}

#[test]
fn the_end_of_a_feed_draws_a_loading_card_once() {
    let mut state = fixtures::demo_state(Detail::Standard);
    state.demo = false;
    let id = state.current_feed_id().unwrap();
    state
        .feeds
        .get_mut(&id)
        .unwrap()
        .next
        .insert(Network::X, "cursor".into());
    let ui = test_ui();
    let rows = rows_of(&frame(&mut state, &ui, 80, 24));
    assert!(
        !rows.iter().any(|row| row.contains("loading")),
        "the first row does not fetch: {rows:#?}"
    );
    assert!(!state.show_page_card);

    let posts = state.current_posts();
    let selectable = crate::layout::selectable(&state.lay(&posts));
    assert!(state.select_row(*selectable.last().unwrap()));
    let rows = rows_of(&frame(&mut state, &ui, 80, 24));
    assert!(state.show_page_card);
    assert!(state.page_gate.is_none());
    let at = rows
        .iter()
        .position(|row| row.contains("loading…"))
        .unwrap_or_else(|| panic!("missing card: {rows:#?}"));
    assert!(rows[at.saturating_sub(1)].contains('╭'), "card: {rows:#?}");
    let hidden = screens::linked_body(&state, 80, false);
    assert!(!hidden
        .lines
        .iter()
        .any(|line| line.plain().contains("loading")));

    state.page_gate = Some((id, state.selected_post_id()));
    let rows = rows_of(&frame(&mut state, &ui, 80, 24));
    assert!(!state.show_page_card);
    assert!(!rows.iter().any(|row| row.contains("loading")));

    let posts = state.current_posts();
    let first = crate::layout::selectable(&state.lay(&posts))[0];
    assert!(state.select_row(first));
    frame(&mut state, &ui, 80, 24);
    assert!(state.page_gate.is_none());
}

fn rows_of(buffer: &Buffer) -> Vec<String> {
    (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect()
}

#[test]
fn below_the_minimum_size_only_the_size_notice_shows() {
    for (width, height) in [(23, 8), (60, 7)] {
        let mut state = fixtures::demo_state(Detail::Standard);
        let ui = test_ui();
        let rows = rows_of(&frame(&mut state, &ui, width, height));
        let shown: Vec<&String> = rows.iter().filter(|row| !row.trim().is_empty()).collect();
        assert_eq!(shown.len(), 1, "{width}x{height}: {rows:#?}");
        assert!(
            shown[0].contains("SoCLI needs 24×8"),
            "{width}x{height}: {rows:#?}"
        );
        assert!(ui.hits.borrow().targets.is_empty());
    }
    let rows = rows_of(&frame(
        &mut fixtures::demo_state(Detail::Standard),
        &test_ui(),
        60,
        7,
    ));
    assert_eq!(rows[3].trim(), "SoCLI needs 24×8 · q quits");
    for quit in [
        key(KeyCode::Char('q')),
        key(KeyCode::Esc),
        with(KeyCode::Char('c'), KeyModifiers::CONTROL),
    ] {
        assert!(quits_too_small(&Event::Key(quit)));
    }
    assert!(!quits_too_small(&Event::Key(key(KeyCode::Char('j')))));
    assert!(!quits_too_small(&Event::Resize(80, 24)));
}

/// A 32-column list keeps three wrapped lines and `Show more`. Opening the post shows every word, including one wrapped across rows.
#[test]
fn a_32_column_feed_shows_every_word_of_the_first_post() {
    for detail in DETAILS {
        let mut state = fixtures::demo_state(detail);
        let post = state.selected_post().unwrap();
        let text = post.text.clone();
        let id = post.id.clone();
        let closed = frame(&mut state, &test_ui(), 32, 40);
        let capped = block_text(&closed, detail, &text);
        assert!(capped.contains("Show more"), "{detail:?}: {capped:?}");
        assert!(
            !capped.contains("make"),
            "{detail:?}: the tail stays behind show more: {capped:?}"
        );
        state.opened.insert(id);
        let open = frame(&mut state, &test_ui(), 32, 40);
        let shown = block_text(&open, detail, &text);
        for word in text.split_whitespace() {
            assert!(shown.contains(word), "{detail:?}: {word:?} in {shown:?}");
        }
        assert!(shown.contains("Show less"), "{detail:?}: {shown:?}");
    }
}

/// The selected block's text from the first word's column, rows joined so a wrapped word stays whole.
fn block_text(buffer: &Buffer, detail: Detail, text: &str) -> String {
    let rows = rows_of(buffer);
    let block: Vec<Vec<char>> = bar_rows(buffer, detail)
        .into_iter()
        .map(|y| rows[usize::from(y)].chars().collect())
        .collect();
    let first = text.split_whitespace().next().unwrap_or("");
    let col = block
        .iter()
        .find_map(|row| {
            let row: String = row.iter().collect();
            row.find(first).map(|at| row[..at].chars().count())
        })
        .unwrap_or(0);
    block
        .iter()
        .map(|row| {
            let text: String = row[col.min(row.len())..].iter().collect();
            text.trim_end().to_string()
        })
        .collect::<Vec<_>>()
        .join("")
}

/// The screen rows of the selected block: the rows with the selection bar, `▍` or the plain `>`.
fn bar_rows(buffer: &Buffer, detail: Detail) -> Vec<u16> {
    let col = render::margin_for(usize::from(buffer.area.width), detail).saturating_sub(1) as u16;
    (0..buffer.area.height)
        .filter(|&y| matches!(buffer[(col, y)].symbol(), "▍" | ">"))
        .collect()
}

fn bar_row(buffer: &Buffer, detail: Detail) -> Option<u16> {
    bar_rows(buffer, detail).first().copied()
}

#[test]
fn d_cycles_the_level_for_the_session_and_keeps_the_reading_row() {
    let config = crate::config::path();
    let before = std::fs::metadata(&config)
        .and_then(|meta| meta.modified())
        .ok();
    let mut state = fixtures::demo_state(Detail::Standard);
    state.layout = Layout::Single;
    let mut ui = test_ui();
    for _ in 0..3 {
        press(&mut state, &mut ui, KeyCode::Char('j'));
    }
    let row = bar_row(&frame(&mut state, &ui, 80, 24), state.detail);
    let id = state.selected_post_id();
    assert!(row.is_some());
    for _ in 0..4 {
        press(&mut state, &mut ui, KeyCode::Char('d'));
        assert_eq!(
            state.status,
            format!("Detail: {} · demo, not saved", state.detail.word())
        );
        frame(&mut state, &ui, 80, 24);
        assert_eq!(state.selected_post_id(), id);
    }
    assert_eq!(state.detail, Detail::Standard);
    assert_eq!(bar_row(&frame(&mut state, &ui, 80, 24), state.detail), row);
    let after = std::fs::metadata(&config)
        .and_then(|meta| meta.modified())
        .ok();
    assert_eq!(before, after);
}

#[test]
fn a_resize_keeps_the_selected_post_on_its_row() {
    let mut state = fixtures::demo_state(Detail::Standard);
    state.layout = Layout::Single;
    let mut ui = test_ui();
    for _ in 0..2 {
        press(&mut state, &mut ui, KeyCode::Char('j'));
    }
    let id = state.selected_post_id();
    let row = bar_row(&frame(&mut state, &ui, 120, 40), state.detail).unwrap();
    for (width, height) in [(80, 24), (32, 16)] {
        let buffer = frame(&mut state, &ui, width, height);
        assert_eq!(state.selected_post_id(), id);
        let footer = if render::narrow(usize::from(width)) {
            2
        } else {
            3
        };
        let last = height - footer - 1;
        let span = screens::linked_body(
            &state,
            render::inner_width_for(usize::from(width), state.detail),
            true,
        )
        .focus
        .expect("a selected block");
        let len = span.end - span.start;
        let body = ui.viewport.get().1;
        let bars = bar_rows(&buffer, state.detail);
        assert_eq!(
            bars.len(),
            len.min(body),
            "{width}x{height}: the whole block shows"
        );
        assert!(bars[0] <= row, "{width}x{height}");
        assert!(
            usize::from(*bars.last().unwrap()) <= usize::from(last),
            "{width}x{height}"
        );
    }
}

#[test]
fn an_error_outlasts_any_wait_and_clears_on_the_next_press() {
    let mut state = fixtures::demo_state(Detail::Standard);
    let mut ui = test_ui();
    state.status.clear();
    state.fail("no link on this post");
    let now = Instant::now();
    for second in 0..=60 {
        assert!(!state.expire(now + Duration::from_secs(second)));
    }
    assert_eq!(state.status, "no link on this post");
    press(&mut state, &mut ui, KeyCode::Char('j'));
    assert_eq!(state.status, "");
    assert_eq!(state.status_error, None);

    state.fail("no link on this post");
    press(&mut state, &mut ui, KeyCode::Char('d'));
    assert_eq!(state.status, "Detail: rich · demo, not saved");
    assert_eq!(state.status_error, None);
}

#[test]
fn the_demo_applies_every_saved_change_and_writes_no_config() {
    let config = crate::config::path();
    let before = std::fs::metadata(&config)
        .and_then(|meta| meta.modified())
        .ok();
    let mut state = fixtures::demo_state(Detail::Standard);
    assert!(state.demo);
    let mut ui = test_ui();
    press(&mut state, &mut ui, KeyCode::Char(','));
    state.settings_row = settings::ROWS
        .iter()
        .position(|row| *row == settings::Row::Density)
        .unwrap();
    let density = state.density;
    press(&mut state, &mut ui, KeyCode::Char('l'));
    assert_ne!(state.density, density);
    assert_eq!(
        state.status,
        format!("ui.density = {} · demo, not saved", state.density.word())
    );
    press(&mut state, &mut ui, KeyCode::Esc);
    assert!(!state.settings);

    press(&mut state, &mut ui, KeyCode::Char('D'));
    press(&mut state, &mut ui, KeyCode::Char('j'));
    press(&mut state, &mut ui, KeyCode::Char('s'));
    assert!(matches!(ui.mode, Mode::Normal));
    assert_eq!(state.detail, Detail::Rich);
    assert_eq!(state.status, "Detail: rich · demo, not saved");

    theme_picker(&mut state, &mut ui);
    press(&mut state, &mut ui, KeyCode::Char('j'));
    press(&mut state, &mut ui, KeyCode::Enter);
    assert_eq!(
        state.status,
        format!("theme {} · demo, not saved", state.scheme.word())
    );

    press(&mut state, &mut ui, KeyCode::Char('F'));
    assert!(matches!(ui.mode, Mode::Filter { .. }));
    let n = state.on.len();
    press(&mut state, &mut ui, KeyCode::Enter);
    assert_eq!(
        state.status,
        format!("{n} networks in your feed · demo, not saved")
    );
    let after = std::fs::metadata(&config)
        .and_then(|meta| meta.modified())
        .ok();
    assert_eq!(before, after);
}

#[test]
fn the_detail_picker_describes_rich_and_choosing_applies_it() {
    let mut state = fixtures::demo_state(Detail::Standard);
    let rich = |state: &State| screens::detail_rows(state)[3].value.clone();
    assert_eq!(rich(&state), "Images off · ui.images");
    for (pictures, shown) in [
        ("kitty", "Images · kitty"),
        ("iTerm2", "Images · iTerm2"),
        ("sixel", "Images · sixel"),
        (
            "blocks in this terminal",
            "Images · blocks in this terminal",
        ),
    ] {
        state.pictures = Some(pictures);
        assert_eq!(rich(&state), shown);
    }
    assert_eq!(
        screens::detail_rows(&state)[2].value,
        "rails, cards and icon stats"
    );
    let mut ui = test_ui();
    press(&mut state, &mut ui, KeyCode::Char('D'));
    let rows = rows_of(&frame(&mut state, &ui, 80, 24));
    assert!(
        rows.iter().any(|row| row.contains("choose  close")),
        "{rows:#?}"
    );
    press(&mut state, &mut ui, KeyCode::Char('j'));
    press(&mut state, &mut ui, KeyCode::Enter);
    assert_eq!(state.detail, Detail::Rich);
    assert_eq!(state.status, "Detail: rich · demo, not saved");
}

#[test]
fn a_chosen_detail_level_and_theme_are_written_to_the_user_config() {
    let _guard = crate::config::lock_config_env();
    let dir = temp("prefs");
    let previous = std::env::var_os("SOCLI_CONFIG_DIR");
    std::env::set_var("SOCLI_CONFIG_DIR", &dir);
    struct Restore(Option<std::ffi::OsString>);
    impl Drop for Restore {
        fn drop(&mut self) {
            match self.0.take() {
                Some(value) => std::env::set_var("SOCLI_CONFIG_DIR", value),
                None => std::env::remove_var("SOCLI_CONFIG_DIR"),
            }
        }
    }
    let restore = Restore(previous);
    let mut state = State::boot(false, 15, &Config::default(), &Clients::empty(), None);
    let mut ui = test_ui();
    press(&mut state, &mut ui, KeyCode::Char('d'));
    assert_eq!(state.detail, Detail::Rich);
    assert_eq!(state.status, "Detail: rich · saved");
    press(&mut state, &mut ui, KeyCode::Char('T'));
    press(&mut state, &mut ui, KeyCode::Char('j'));
    press(&mut state, &mut ui, KeyCode::Enter);
    let theme = state.scheme.word();
    let config = crate::config::load().unwrap();
    assert_eq!(config.ui.detail.as_deref(), Some("rich"));
    assert_eq!(config.ui.theme.as_deref(), Some(theme.as_str()));
    drop(restore);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn plain_detail_without_color_draws_no_escape() {
    let mut state = fixtures::demo_state(Detail::Plain);
    state.forced_mono = true;
    state.retheme();
    let buffer = frame(&mut state, &test_ui(), 80, 24);
    assert!(buffer
        .content
        .iter()
        .all(|cell| !cell.symbol().contains('\x1b')));
}

#[test]
fn overlays_draw_in_short_viewports_and_after_a_resize() {
    let opens: [fn(&mut State, &mut Ui); 5] = [
        theme_picker,
        |state, ui| press(state, ui, KeyCode::Char('A')),
        |state, ui| press(state, ui, KeyCode::Char('?')),
        |state, ui| press(state, ui, KeyCode::Char(',')),
        |_, ui| {
            ui.mode = Mode::Compose(Box::new(composer_for(
                "hello 👩‍💻 日本語",
                Network::X,
                Overlay::None,
            )))
        },
    ];
    for open in opens {
        for height in [8, 10] {
            let mut state = demo_state();
            let mut ui = test_ui();
            open(&mut state, &mut ui);
            frame(&mut state, &ui, 60, height);
            frame(&mut state, &ui, 40, 12);
        }
    }
}

fn focused(ui: &Ui) -> Option<Target> {
    let stops = tab_stops(&ui.hits.borrow().targets);
    let focus = ui.focus.borrow().clone()?;
    focused_index(&stops, &focus).map(|i| stops[i].clone())
}

/// Puts the focus ring on the first tab stop `wanted` accepts on the last frame.
fn focus_on(ui: &Ui, wanted: impl Fn(&Target) -> bool) -> Target {
    let stops = tab_stops(&ui.hits.borrow().targets);
    let i = stops.iter().position(wanted).expect("no such tab stop");
    ui.focus.replace(Some(focus_of(&stops, i)));
    stops[i].clone()
}

fn target_where(ui: &Ui, wanted: impl Fn(&Target) -> bool) -> Target {
    ui.hits
        .borrow()
        .targets
        .iter()
        .find(|t| wanted(t))
        .cloned()
        .expect("no such target")
}

fn mouse(state: &mut State, ui: &mut Ui, kind: MouseEventKind, x: usize, y: usize) {
    let event = MouseEvent {
        kind,
        column: x as u16,
        row: y as u16,
        modifiers: KeyModifiers::NONE,
    };
    handle_mouse(state, &mut Clients::empty(), ui, event).unwrap();
}

fn click_at(state: &mut State, ui: &mut Ui, x: usize, y: usize) {
    mouse(state, ui, MouseEventKind::Down(MouseButton::Left), x, y);
}

fn click_on(state: &mut State, ui: &mut Ui, target: &Target) {
    click_at(state, ui, usize::from(target.x), usize::from(target.y));
}

fn dialog_row(ui: &Ui, pick: Pick) -> (usize, usize) {
    let hits = ui.hits.borrow();
    let (y, cols, _) = hits
        .rows
        .iter()
        .find(|(_, _, p)| *p == pick)
        .expect("no such dialog row");
    (cols.start + 2, *y)
}

#[test]
fn profile_share_targets_the_profile_even_with_a_post_selected() {
    for detail in DETAILS {
        let mut state = fixtures::demo_state(detail);
        state
            .open_user(
                Scope::One(Network::X),
                "river".into(),
                &mut Clients::empty(),
            )
            .unwrap();
        let mut ui = test_ui();
        press(&mut state, &mut ui, KeyCode::Char('j'));
        assert!(state.selected_post().is_some());
        frame(&mut state, &ui, 100, 40);
        let target = target_where(&ui, |target| matches!(target.act, Act::ShareProfile(..)));
        click_on(&mut state, &mut ui, &target);
        let Mode::Share(share) = &ui.mode else {
            panic!("profile sharing is open")
        };
        assert_eq!(share.post, None);
        assert_eq!(share.profile_url, "https://x.com/river");
    }
}

#[test]
fn the_overflow_marker_opens_the_hidden_tabs() {
    let mut state = State::boot(true, 15, &Config::default(), &Clients::empty(), None);
    state.reveal_shortcuts = true;
    let mut ui = test_ui();
    frame(&mut state, &ui, 32, 20);
    let marker = target_where(&ui, |target| {
        matches!(
            target.act,
            Act::Overflow(ref items)
                if items.iter().any(|item| matches!(item.act.as_ref(), Act::Tab(_)))
        )
    });
    click_on(&mut state, &mut ui, &marker);
    assert!(matches!(ui.mode, Mode::Overflow(_)));
    let text: String = frame(&mut state, &ui, 32, 20)
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect();
    assert!(text.contains("Discover"), "{text}");
    assert!(text.contains("Saved"), "{text}");
    click_at(&mut state, &mut ui, 0, 0);
    assert!(matches!(ui.mode, Mode::Normal));
    assert_eq!(state.merged_tab, MergedTab::Home);
    click_on(&mut state, &mut ui, &marker);
    press(&mut state, &mut ui, KeyCode::Char('j'));
    press(&mut state, &mut ui, KeyCode::Enter);
    assert!(matches!(ui.mode, Mode::Normal));
    assert_eq!(state.merged_tab, MergedTab::Saved);
}

#[test]
fn profile_views_draw_at_every_viewport_and_detail_level() {
    for (width, height) in VIEWPORTS {
        for detail in DETAILS {
            let mut state = fixtures::demo_state(detail);
            state
                .open_user(
                    Scope::One(Network::X),
                    "river".into(),
                    &mut Clients::empty(),
                )
                .unwrap();
            let mut ui = test_ui();
            let buffer = frame(&mut state, &ui, width, height);
            let text = buffer
                .content()
                .iter()
                .map(|cell| cell.symbol())
                .collect::<String>();
            if width >= 44 {
                assert!(text.contains("River Hale"), "{width}x{height} {detail:?}");
            }
            if ui.viewport.get().1 >= 6 && width >= 40 {
                assert!(
                    text.contains("Half-block"),
                    "the first post at {width}x{height} {detail:?}"
                );
            }
            if ui.viewport.get().1 >= 6 && width < 40 {
                let mut after = String::new();
                for _ in 0..40 {
                    press(&mut state, &mut ui, KeyCode::Char('j'));
                    after = frame(&mut state, &ui, width, height)
                        .content()
                        .iter()
                        .map(|cell| cell.symbol())
                        .collect();
                    if after.contains("Half-block") {
                        break;
                    }
                }
                assert!(
                    after.contains("Half-block"),
                    "the first post after j at {width}x{height} {detail:?}"
                );
            }
            assert_eq!(buffer.area.width, width);
            assert_eq!(buffer.area.height, height);
            assert_inside(
                &state,
                &ui,
                width,
                height,
                &format!("profile {width}x{height} {detail:?}"),
            );
        }
    }
}

#[test]
fn clicking_the_padding_of_a_repost_button_activates_that_post() {
    let mut state = fixtures::demo_state(Detail::Standard);
    let mut ui = test_ui();
    frame(&mut state, &ui, 104, 44);
    let target = target_where(&ui, |target| matches!(target.act, Act::Repost(_)));
    let Act::Repost(id) = &target.act else {
        unreachable!()
    };
    let before = state.posts[id].reposted;
    assert_eq!(target.width, 7);
    click_at(
        &mut state,
        &mut ui,
        usize::from(target.x + target.width - 1),
        usize::from(target.y),
    );
    assert_eq!(state.posts[id].reposted, !before);
    assert_eq!(state.selected_post_id().as_deref(), Some(id.as_str()));
}

#[test]
fn tab_from_no_focus_takes_the_first_header_target() {
    let mut state = demo_state();
    let mut ui = test_ui();
    frame(&mut state, &ui, 104, 44);
    press(&mut state, &mut ui, KeyCode::Tab);
    let first = focused(&ui).expect("a focus");
    assert_eq!(first.zone, Zone::Header);
    assert_eq!(first.act, Act::Crumb(0));
}

#[test]
fn tab_follows_zone_row_and_column_skips_hints_and_wraps() {
    let mut state = demo_state();
    let mut ui = test_ui();
    let mut seen = Vec::new();
    let mut first = None;
    for _ in 0..400 {
        frame(&mut state, &ui, 104, 44);
        press(&mut state, &mut ui, KeyCode::Tab);
        let target = focused(&ui).expect("a focus");
        let key = (target.zone, target.y, target.x, target.act.clone());
        if first.is_none() {
            first = Some(key);
        } else if first.as_ref() == Some(&key) && seen.len() > 3 {
            break;
        }
        seen.push(target);
    }
    let zones: Vec<Zone> = seen.iter().map(|t| t.zone).collect();
    assert!(zones.windows(2).all(|w| w[0] <= w[1]), "{zones:?}");
    for zone in [Zone::Header, Zone::Subbar, Zone::BodyLeft, Zone::Footer] {
        assert!(zones.contains(&zone), "{zone:?} {zones:?}");
    }
    assert!(seen.iter().all(|t| !matches!(t.act, Act::Key(_))));
    assert!(seen.iter().all(|t| t.tab));
    let header: Vec<&Target> = seen.iter().filter(|t| t.zone == Zone::Header).collect();
    assert!(header
        .windows(2)
        .all(|w| (w[0].y, w[0].x) < (w[1].y, w[1].x)));
    assert!(seen.iter().any(|target| target.act == Act::Compose));
    let last = seen.last().expect("a stop");
    assert_eq!(last.zone, Zone::Footer);
    match &last.act {
        Act::Compose => {}
        Act::Overflow(items) => assert!(
            items
                .iter()
                .any(|item| matches!(item.act.as_ref(), Act::Key(_))),
            "{items:?}"
        ),
        other => panic!("the ring ends on new post or the hidden hints, not {other:?}"),
    }
}

#[test]
fn shift_tab_from_no_focus_takes_the_last_target() {
    let mut state = demo_state();
    let mut ui = test_ui();
    frame(&mut state, &ui, 104, 44);
    press(&mut state, &mut ui, KeyCode::BackTab);
    let last = focused(&ui).expect("a focus");
    assert_eq!((last.zone, last.act), (Zone::Footer, Act::Compose));
    frame(&mut state, &ui, 104, 44);
    press(&mut state, &mut ui, KeyCode::Tab);
    assert_eq!(focused(&ui).map(|t| t.zone), Some(Zone::Header));
}

#[test]
fn enter_on_a_focused_mention_opens_that_users_posts() {
    let mut state = demo_state();
    let mut ui = test_ui();
    let mut found = false;
    for _ in 0..40 {
        frame(&mut state, &ui, 104, 44);
        found = ui
            .hits
            .borrow()
            .targets
            .iter()
            .any(|t| t.tab && matches!(t.act, Act::Mention { .. }));
        if found {
            break;
        }
        press(&mut state, &mut ui, KeyCode::Char('j'));
    }
    assert!(found, "no mention on screen");
    let target = focus_on(&ui, |t| matches!(t.act, Act::Mention { .. }));
    let Act::Mention { post, handle } = target.act.clone() else {
        unreachable!()
    };
    let net = net::split(&post).unwrap().0;
    press(&mut state, &mut ui, KeyCode::Enter);
    assert_eq!(state.section, Section::Search);
    assert_eq!(
        state.remote,
        Some(crate::model::FeedId::User(Scope::One(net), handle))
    );
    assert!(ui.focus.borrow().is_none());
}

#[test]
fn enter_on_a_focused_nav_section_switches_section() {
    let mut state = demo_state();
    let mut ui = test_ui();
    frame(&mut state, &ui, 104, 44);
    focus_on(&ui, |t| {
        t.zone == Zone::Footer && t.act == Act::Section(Section::Notifications)
    });
    press(&mut state, &mut ui, KeyCode::Enter);
    assert_eq!(state.section, Section::Notifications);
}

#[test]
fn j_clears_the_focus_and_moves_the_selection() {
    let mut state = demo_state();
    let mut ui = test_ui();
    frame(&mut state, &ui, 104, 44);
    let before = state.selected_post_id();
    press(&mut state, &mut ui, KeyCode::Tab);
    assert!(ui.focus.borrow().is_some());
    press(&mut state, &mut ui, KeyCode::Char('j'));
    assert!(ui.focus.borrow().is_none());
    assert_ne!(state.selected_post_id(), before);
}

#[test]
fn esc_with_a_focus_only_clears_it() {
    let mut state = demo_state();
    let mut ui = test_ui();
    frame(&mut state, &ui, 104, 44);
    press(&mut state, &mut ui, KeyCode::Char('j'));
    press(&mut state, &mut ui, KeyCode::Enter);
    assert!(state.thread_focus.is_some());
    frame(&mut state, &ui, 104, 44);
    let selected = state.selected_post_id();
    press(&mut state, &mut ui, KeyCode::Tab);
    press(&mut state, &mut ui, KeyCode::Esc);
    assert!(ui.focus.borrow().is_none());
    assert!(state.thread_focus.is_some());
    assert_eq!(state.selected_post_id(), selected);
}

#[test]
fn tab_in_the_keyed_pane_selects_the_post_it_lands_on() {
    let mut state = demo_state();
    let mut ui = test_ui();
    frame(&mut state, &ui, 104, 44);
    let first = state.selected_post_id().unwrap();
    let target = focus_on(&ui, |t| {
        t.zone == Zone::BodyLeft && t.act.post().is_some_and(|id| id != first)
    });
    let other = target.act.post().unwrap().to_string();
    let stops = tab_stops(&ui.hits.borrow().targets);
    let i = focused_index(&stops, ui.focus.borrow().as_ref().unwrap()).unwrap();
    ui.focus.replace(Some(focus_of(&stops, i - 1)));
    press(&mut state, &mut ui, KeyCode::Tab);
    assert_eq!(state.selected_post_id(), Some(other));
}

#[test]
fn tab_into_the_preview_only_moves_the_highlight_and_enter_moves_the_keys() {
    let mut state = demo_state();
    state.layout = Layout::Auto;
    let mut ui = test_ui();
    frame(&mut state, &ui, 160, 44);
    assert!(ui.split.get());
    let selected = state.selected_post_id();
    let key = state.selection_key();
    for _ in 0..200 {
        frame(&mut state, &ui, 160, 44);
        press(&mut state, &mut ui, KeyCode::Tab);
        if focused(&ui).is_some_and(|t| t.zone == Zone::BodyRight) {
            break;
        }
    }
    assert_eq!(focused(&ui).map(|t| t.zone), Some(Zone::BodyRight));
    let feed_selected = state.selected_post_id();
    for _ in 0..3 {
        frame(&mut state, &ui, 160, 44);
        press(&mut state, &mut ui, KeyCode::Tab);
    }
    assert!(state.thread_focus.is_none());
    assert_eq!(state.selection_key(), key);
    assert_eq!(state.selected_post_id(), feed_selected);
    assert!(selected.is_some());

    // The post under the focus ring may belong to a network with no like. Step to one that has.
    for _ in 0..12 {
        frame(&mut state, &ui, 160, 44);
        let ready = tab_stops(&ui.hits.borrow().targets)
            .iter()
            .any(|t| t.zone == Zone::BodyRight && matches!(t.act, Act::Like(_)));
        if ready {
            break;
        }
        press(&mut state, &mut ui, KeyCode::Char('k'));
    }
    let target = focus_on(&ui, |t| {
        t.zone == Zone::BodyRight && matches!(t.act, Act::Like(_))
    });
    let id = target.act.post().unwrap().to_string();
    let liked = state.posts[&id].liked;
    press(&mut state, &mut ui, KeyCode::Enter);
    assert!(state.thread_focus.is_some());
    assert_eq!(state.selected_post_id(), Some(id.clone()));
    assert_ne!(state.posts[&id].liked, liked);
}

#[test]
fn the_focus_ring_uses_underlines_in_every_theme() {
    let mut state = demo_state();
    state.scheme = Scheme::parse("black").unwrap();
    state.retheme();
    let mut ui = test_ui();
    frame(&mut state, &ui, 104, 44);
    press(&mut state, &mut ui, KeyCode::Tab);
    let buffer = frame(&mut state, &ui, 104, 44);
    let target = focused(&ui).unwrap();
    let cell = &buffer[(target.x, target.y)];
    assert_eq!(cell.fg, state.theme.fg(Kind::Blue, false));
    assert!(cell
        .modifier
        .contains(Modifier::UNDERLINED | Modifier::BOLD));

    for native in [true, false] {
        let mut state = demo_state();
        if native {
            state.scheme = Scheme::parse("native").unwrap();
        } else {
            state.color_mode = crate::theme::ColorMode::Mono;
        }
        state.retheme();
        assert!(state.theme.native || state.theme.mono);
        let mut ui = test_ui();
        frame(&mut state, &ui, 104, 44);
        press(&mut state, &mut ui, KeyCode::Tab);
        let buffer = frame(&mut state, &ui, 104, 44);
        let target = focused(&ui).unwrap();
        let cell = &buffer[(target.x, target.y)];
        assert!(
            cell.modifier
                .contains(Modifier::UNDERLINED | Modifier::BOLD),
            "native {native}"
        );
    }
}

#[test]
fn a_click_on_a_nav_section_switches_section() {
    let mut state = demo_state();
    let mut ui = test_ui();
    frame(&mut state, &ui, 104, 44);
    let target = target_where(&ui, |t| {
        t.zone == Zone::Footer && t.act == Act::Section(Section::Notifications)
    });
    click_on(&mut state, &mut ui, &target);
    assert_eq!(state.section, Section::Notifications);
    assert!(ui.focus.borrow().is_none());
}

#[test]
fn a_click_selects_a_post_and_a_second_click_opens_it() {
    let mut state = demo_state();
    let mut ui = test_ui();
    frame(&mut state, &ui, 104, 44);
    let selected = state.selected_index().unwrap();
    let (x, y) = {
        let hits = ui.hits.borrow();
        let pane = &hits.panes[0];
        let next = crate::layout::selectable(&state.lay(&state.current_posts()))
            .into_iter()
            .find(|row| *row > selected)
            .unwrap();
        let span = pane.spans[next];
        (
            pane.cols.end - 2,
            pane.rows.start + span.start - pane.offset,
        )
    };
    click_at(&mut state, &mut ui, x, y);
    assert_ne!(state.selected_index(), Some(selected));
    assert!(state.thread_focus.is_none());
    frame(&mut state, &ui, 104, 44);
    click_at(&mut state, &mut ui, x, y);
    assert!(state.thread_focus.is_some());
}

#[test]
fn a_click_on_a_link_card_row_resolves_to_its_link() {
    let mut state = demo_state();
    let ui = test_ui();
    frame(&mut state, &ui, 104, 44);
    let card = target_where(&ui, |t| !t.tab && matches!(t.act, Act::Link { .. }));
    let Act::Link { post, url } = &card.act else {
        unreachable!()
    };
    assert_eq!(*url, state.posts[post].links[0].expanded);
    let rows: Vec<Target> = ui
        .hits
        .borrow()
        .targets
        .iter()
        .filter(|t| t.act == card.act && t.x == card.x && t.width == card.width)
        .cloned()
        .collect();
    assert_eq!(rows.len(), 4);
    assert_eq!(rows.iter().filter(|t| t.tab).count(), 1);
    for row in &rows {
        let x = usize::from(row.x + row.width / 2);
        let hit = target_at(&ui, x, usize::from(row.y)).expect("a target");
        assert_eq!(hit.act, card.act);
    }
}

#[test]
fn a_click_on_a_hint_runs_its_key() {
    let mut state = demo_state();
    let mut ui = test_ui();
    frame(&mut state, &ui, 104, 44);
    let target = target_where(&ui, |t| t.act == Act::Key(KeyCode::Char('?')));
    assert!(!target.tab);
    click_on(&mut state, &mut ui, &target);
    assert!(matches!(ui.mode, Mode::Help));
}

#[test]
fn a_click_outside_a_picker_closes_it_and_the_theme_picker_restores() {
    let mut state = demo_state();
    let mut ui = test_ui();
    press(&mut state, &mut ui, KeyCode::Char('A'));
    frame(&mut state, &ui, 104, 44);
    click_at(&mut state, &mut ui, 0, 0);
    assert!(matches!(ui.mode, Mode::Normal));

    let before = state.scheme;
    ui.mode = PickKind::Theme.open(&state);
    press(&mut state, &mut ui, KeyCode::Char('j'));
    assert_ne!(state.scheme, before);
    frame(&mut state, &ui, 104, 44);
    click_at(&mut state, &mut ui, 0, 0);
    assert!(matches!(ui.mode, Mode::Normal));
    assert_eq!(state.scheme, before);
}

#[test]
fn a_click_on_a_picker_row_picks_it() {
    let mut state = demo_state();
    let mut ui = test_ui();
    press(&mut state, &mut ui, KeyCode::Char('A'));
    frame(&mut state, &ui, 104, 44);
    let (x, y) = dialog_row(&ui, Pick::Row(3));
    click_at(&mut state, &mut ui, x, y);
    assert!(matches!(ui.mode, Mode::Normal));
    assert_eq!(state.scope(), Scope::One(Network::Devnull));
}

#[test]
fn the_wheel_moves_the_selection_the_cursor_and_the_help() {
    let mut state = demo_state();
    let mut ui = test_ui();
    frame(&mut state, &ui, 104, 44);
    let before = state.selected_post_id();
    mouse(&mut state, &mut ui, MouseEventKind::ScrollDown, 20, 12);
    assert_ne!(state.selected_post_id(), before);
    frame(&mut state, &ui, 104, 44);
    mouse(&mut state, &mut ui, MouseEventKind::ScrollUp, 20, 12);
    assert_eq!(state.selected_post_id(), before);

    press(&mut state, &mut ui, KeyCode::Char('A'));
    frame(&mut state, &ui, 104, 44);
    mouse(&mut state, &mut ui, MouseEventKind::ScrollDown, 20, 12);
    assert_eq!(cursor_of(&ui), Some((PickKind::Network, 1)));
    press(&mut state, &mut ui, KeyCode::Esc);

    press(&mut state, &mut ui, KeyCode::Char('?'));
    frame(&mut state, &ui, 60, 16);
    assert!(ui.overflow.get() >= 3);
    mouse(&mut state, &mut ui, MouseEventKind::ScrollDown, 20, 8);
    assert_eq!(ui.scroll.get(), 3);
    assert!(matches!(ui.mode, Mode::Help));
}

#[test]
fn hovering_a_picker_row_moves_the_cursor() {
    let mut state = demo_state();
    let mut ui = test_ui();
    press(&mut state, &mut ui, KeyCode::Char('A'));
    frame(&mut state, &ui, 104, 44);
    let (x, y) = dialog_row(&ui, Pick::Row(2));
    mouse(&mut state, &mut ui, MouseEventKind::Moved, x, y);
    assert_eq!(cursor_of(&ui), Some((PickKind::Network, 2)));
    assert_eq!(state.scope(), Scope::All);
}

#[test]
fn settings_clicks_step_values_and_move_the_cursor() {
    let mut state = demo_state();
    state.open_settings();
    let ui = test_ui();
    frame(&mut state, &ui, 104, 44);
    let (y, hit) = ui
        .hits
        .borrow()
        .settings
        .iter()
        .copied()
        .find(|(_, hit)| hit.row == Row::Density)
        .unwrap();
    let (left, right) = hit.arrows.unwrap();
    let margin = ui.hits.borrow().margin;
    assert_eq!(
        settings_click(&mut state, &ui, margin + right, y),
        SettingsClick::Step(Row::Density, 1)
    );
    assert_eq!(
        settings_click(&mut state, &ui, margin + left, y),
        SettingsClick::Step(Row::Density, -1)
    );
    let theme = ui
        .hits
        .borrow()
        .settings
        .iter()
        .find(|(_, hit)| hit.row == Row::Theme)
        .map(|(y, _)| *y)
        .unwrap();
    assert_eq!(
        settings_click(&mut state, &ui, margin + 3, theme),
        SettingsClick::None
    );
    assert_eq!(settings::ROWS[state.settings_row], Row::Theme);
    assert_eq!(
        settings_click(&mut state, &ui, margin + 3, theme),
        SettingsClick::Press(KeyCode::Enter)
    );
    let rows = settings::ROWS.len();
    let mut ui = test_ui();
    state.settings_row = rows - 1;
    press(&mut state, &mut ui, KeyCode::Tab);
    assert_eq!(state.settings_row, 0);
    press(&mut state, &mut ui, KeyCode::BackTab);
    assert_eq!(settings::ROWS[state.settings_row], Row::Enter);
}

#[test]
fn the_accounts_page_signs_in_from_the_reader_without_saving_the_demo() {
    let mut state = demo_state();
    state.nets.retain(|info| info.net != Network::Threads);
    let on = state.on.clone();
    let mut ui = test_ui();
    state.open_settings();
    press(&mut state, &mut ui, KeyCode::Enter);
    assert_eq!(state.settings_page, SettingsPage::Accounts);
    assert_eq!(state.crumb(), "settings › accounts");
    assert!(state.settings);
    press(&mut state, &mut ui, KeyCode::Char(' '));
    assert!(
        state.status.contains("X · demo, not saved"),
        "{}",
        state.status
    );
    assert!(state.temporary_status());
    press(&mut state, &mut ui, KeyCode::Esc);
    assert_eq!(state.settings_page, SettingsPage::Main);
    assert!(state.settings);
    assert_eq!(settings::ROWS[state.settings_row], Row::Accounts);

    state.settings_row = settings::ROWS
        .iter()
        .position(|row| *row == Row::Net(Network::Threads))
        .unwrap();
    press(&mut state, &mut ui, KeyCode::Char(' '));
    assert!(
        state.status.contains("Threads · demo, not saved"),
        "{}",
        state.status
    );
    assert_eq!(state.on, on);
    press(&mut state, &mut ui, KeyCode::Enter);
    assert!(
        state.status.contains("Threads · demo, not saved"),
        "{}",
        state.status
    );
}

#[test]
fn tab_moves_the_cursor_in_the_filter_panel_and_the_share_dialog() {
    let mut state = demo_state();
    let mut ui = test_ui();
    press(&mut state, &mut ui, KeyCode::Char('F'));
    press(&mut state, &mut ui, KeyCode::Tab);
    assert!(matches!(ui.mode, Mode::Filter { cursor: 1, .. }));
    press(&mut state, &mut ui, KeyCode::BackTab);
    assert!(matches!(ui.mode, Mode::Filter { cursor: 0, .. }));
    press(&mut state, &mut ui, KeyCode::Esc);

    press(&mut state, &mut ui, KeyCode::Char('s'));
    press(&mut state, &mut ui, KeyCode::Tab);
    let Mode::Share(share) = &ui.mode else {
        panic!("the share dialog is open")
    };
    assert_eq!(share.row, 1);
}
#[test]
fn holding_control_or_command_reveals_shortcuts_without_changing_focus_or_selection() {
    let mut state = fixtures::demo_state(Detail::Standard);
    let mut ui = test_ui();
    frame(&mut state, &ui, 100, 40);
    press(&mut state, &mut ui, KeyCode::Tab);
    let focus = ui.focus.borrow().clone();
    let selected = state.selected_post_id();
    let modifier =
        |side, kind| KeyEvent::new_with_kind(KeyCode::Modifier(side), KeyModifiers::CONTROL, kind);
    let left = modifier(ModifierKeyCode::LeftControl, KeyEventKind::Press);
    handle_key(&mut state, &mut Clients::empty(), &mut ui, left).unwrap();
    assert!(state.reveal_shortcuts);
    assert_eq!(*ui.focus.borrow(), focus);
    assert_eq!(state.selected_post_id(), selected);
    let buffer = frame(&mut state, &ui, 100, 40);
    let rows = rows_of(&buffer);
    assert!(rows[3].contains("[1 Home]"));
    assert!(rows[38].contains("[n] new post"));
    for target in &ui.hits.borrow().targets {
        for x in target.x..target.x + target.width {
            let cell = &buffer[(x, target.y)];
            if !cell.symbol().trim().is_empty() {
                assert!(cell.modifier.contains(Modifier::UNDERLINED));
            }
        }
    }
    let right = modifier(ModifierKeyCode::RightSuper, KeyEventKind::Press);
    handle_key(&mut state, &mut Clients::empty(), &mut ui, right).unwrap();
    let released = modifier(ModifierKeyCode::LeftControl, KeyEventKind::Release);
    assert!(matches!(
        queued(Event::Key(released)),
        Some(Queued::Press(Press::Key(_)))
    ));
    handle_key(&mut state, &mut Clients::empty(), &mut ui, released).unwrap();
    assert!(state.reveal_shortcuts);
    handle_key(
        &mut state,
        &mut Clients::empty(),
        &mut ui,
        modifier(ModifierKeyCode::RightSuper, KeyEventKind::Release),
    )
    .unwrap();
    assert!(!state.reveal_shortcuts);
    let rows = rows_of(&frame(&mut state, &ui, 100, 40));
    assert!(!rows[3].contains("[1 Home]"));
    assert!(!rows[38].contains("[n]"));
    assert!(rows[39].trim_end().ends_with("help"));
    assert!(rows[39].contains("? help"));
    handle_key(&mut state, &mut Clients::empty(), &mut ui, left).unwrap();
    let Some(Queued::Press(Press::Key(lost))) = queued(Event::FocusLost) else {
        panic!("focus loss must clear disclosure");
    };
    handle_key(&mut state, &mut Clients::empty(), &mut ui, lost).unwrap();
    assert!(!state.reveal_shortcuts);
}

#[test]
fn palette_uses_the_detail_level_and_keeps_every_hit_inside_the_viewport() {
    for detail in DETAILS {
        for (width, height) in VIEWPORTS {
            let mut state = fixtures::demo_state(detail);
            let mut ui = test_ui();
            press(&mut state, &mut ui, KeyCode::Char(':'));
            let buffer = frame(&mut state, &ui, width, height);
            let simple = matches!(detail, Detail::Plain | Detail::Minimal);
            let hits = ui.hits.borrow();
            assert_eq!(
                hits.dialog.is_some(),
                !simple,
                "{detail:?} {width}x{height}"
            );
            for (row, cols, _) in &hits.rows {
                assert!(
                    *row < height as usize && cols.end <= width as usize,
                    "{detail:?} {width}x{height}: {row}, {cols:?}"
                );
            }
            if let Some((rows, cols)) = &hits.dialog {
                assert!(rows.end <= height as usize && cols.end <= width as usize);
                assert!(cols.start.abs_diff(width as usize - cols.end) <= 1);
                assert!(rows.start.abs_diff(height as usize - rows.end) <= 1);
            }
            if simple {
                assert!(!rows_of(&buffer).iter().any(|row| row.contains('╭')));
            }
        }
    }
    let mut state = fixtures::demo_state(Detail::Standard);
    let mut ui = test_ui();
    ui.mode = Mode::Command {
        input: Input::from("unrecognized"),
        pick: 0,
    };
    let rows = rows_of(&frame(&mut state, &ui, 40, 12));
    assert!(rows.iter().any(|row| row.contains("No matching commands")));
    press(&mut state, &mut ui, KeyCode::Esc);
    press(&mut state, &mut ui, KeyCode::Char('T'));
    assert!(matches!(
        ui.mode,
        Mode::Pick(Picker {
            kind: PickKind::Theme,
            ..
        })
    ));
}

#[test]
fn breadcrumb_clicks_return_to_the_parent_and_leave_settings() {
    let mut state = fixtures::demo_state(Detail::Standard);
    let mut ui = test_ui();
    press(&mut state, &mut ui, KeyCode::Enter);
    assert!(state.thread_focus.is_some());
    frame(&mut state, &ui, 100, 40);
    let current = target_where(&ui, |t| t.act == Act::Crumb(3));
    click_on(&mut state, &mut ui, &current);
    assert!(state.thread_focus.is_some());
    let parent = target_where(&ui, |t| t.act == Act::Crumb(2));
    click_on(&mut state, &mut ui, &parent);
    assert!(state.thread_focus.is_none());
    state.open_settings();
    frame(&mut state, &ui, 100, 40);
    let home = target_where(&ui, |t| t.act == Act::Crumb(0));
    click_on(&mut state, &mut ui, &home);
    assert!(!state.settings);
    assert_eq!(state.section, Section::Home);
}

#[test]
fn footer_suggests_the_focused_action_and_keeps_help_on_the_right() {
    let mut state = fixtures::demo_state(Detail::Standard);
    let ui = test_ui();
    frame(&mut state, &ui, 100, 40);
    focus_on(&ui, |t| matches!(t.act, Act::Link { .. }));
    let rows = rows_of(&frame(&mut state, &ui, 100, 40));
    assert!(rows[39].contains("open link"));
    assert!(rows[39].trim_end().ends_with("help"));
    assert!(!rows[39].contains("j/k"));
}

#[test]
fn standard_keeps_thread_rails_without_letter_avatars() {
    let mut state = fixtures::demo_state(Detail::Standard);
    let text = state
        .view(100, true)
        .lines
        .iter()
        .map(StyledLine::plain)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(!text.contains("│ N │") && !text.contains("│ P │"));
    assert!(text.contains('╎') && text.contains("├╌╌╌"));
    state.detail = Detail::Rich;
    let rich = state
        .view(100, true)
        .lines
        .iter()
        .map(StyledLine::plain)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(rich.contains("│ N │") && rich.contains("┤ P │"));
}

#[test]
fn refinement_gallery_covers_all_detail_levels_and_themes() {
    for detail in DETAILS {
        for light in [false, true] {
            for reveal in [false, true] {
                for palette in [false, true] {
                    let mut state = fixtures::demo_state(detail);
                    state.scheme = if light {
                        Scheme::parse("white").unwrap()
                    } else {
                        Scheme::parse("black").unwrap()
                    };
                    state.retheme();
                    state.reveal_shortcuts = reveal;
                    let mut ui = test_ui();
                    if palette {
                        ui.mode = Mode::Command {
                            input: Input::from("theme"),
                            pick: 0,
                        };
                    }
                    let buffer = frame(&mut state, &ui, 100, 40);
                    if let Ok(folder) = std::env::var("SOCLI_UI_SNAPSHOTS") {
                        let path = std::path::Path::new(&folder);
                        std::fs::create_dir_all(path).unwrap();
                        let cells: Vec<_> = buffer.content.iter().map(|cell| serde_json::json!({ "text": cell.symbol(), "fg": format!("{:?}", cell.fg), "bg": format!("{:?}", cell.bg), "modifiers": format!("{:?}", cell.modifier) })).collect();
                        let name = format!(
                            "{}-{}-{}-{}.json",
                            detail.word(),
                            if light { "light" } else { "dark" },
                            if reveal { "shift" } else { "quiet" },
                            if palette { "palette" } else { "feed" }
                        );
                        std::fs::write(
                            path.join(name),
                            serde_json::to_vec(
                                &serde_json::json!({"width":100,"height":40,"cells":cells}),
                            )
                            .unwrap(),
                        )
                        .unwrap();
                    }
                }
            }
        }
    }
}
#[test]
fn composer_actions_hide_keys_and_accept_mouse_clicks() {
    let mut state = fixtures::demo_state(Detail::Standard);
    let mut ui = test_ui();
    press(&mut state, &mut ui, KeyCode::Char('n'));
    let rows = rows_of(&frame(&mut state, &ui, 100, 40));
    assert!(rows.last().unwrap().contains("post  newline"));
    assert!(rows.last().unwrap().contains("cancel"));
    assert!(!rows.last().unwrap().contains("ctrl-j"));
    let newline = target_where(&ui, |t| {
        t.act == Act::Input(KeyCode::Char('j'), KeyModifiers::CONTROL)
    });
    click_on(&mut state, &mut ui, &newline);
    let Mode::Compose(composer) = &ui.mode else {
        panic!("composer remains open");
    };
    assert_eq!(composer.input.text(), "\n");
    state.reveal_shortcuts = true;
    let rows = rows_of(&frame(&mut state, &ui, 100, 40));
    assert!(rows.last().unwrap().contains("ctrl-j newline"));
}
#[test]
fn shift_preserves_uppercase_shortcuts_and_composer_input() {
    let mut state = fixtures::demo_state(Detail::Standard);
    let mut ui = test_ui();
    let mut clients = Clients::empty();
    handle_key(
        &mut state,
        &mut clients,
        &mut ui,
        KeyEvent::new(
            KeyCode::Modifier(ModifierKeyCode::LeftShift),
            KeyModifiers::SHIFT,
        ),
    )
    .unwrap();
    assert!(!state.reveal_shortcuts);
    handle_key(
        &mut state,
        &mut clients,
        &mut ui,
        with(KeyCode::Char('d'), KeyModifiers::SHIFT),
    )
    .unwrap();
    assert_eq!(cursor_of(&ui).map(|(kind, _)| kind), Some(PickKind::Detail));
    press(&mut state, &mut ui, KeyCode::Esc);
    handle_key(
        &mut state,
        &mut clients,
        &mut ui,
        with(KeyCode::Char('a'), KeyModifiers::SHIFT),
    )
    .unwrap();
    assert_eq!(
        cursor_of(&ui).map(|(kind, _)| kind),
        Some(PickKind::Network)
    );
    press(&mut state, &mut ui, KeyCode::Esc);
    press(&mut state, &mut ui, KeyCode::Char('n'));
    handle_key(
        &mut state,
        &mut clients,
        &mut ui,
        with(KeyCode::Char('a'), KeyModifiers::SHIFT),
    )
    .unwrap();
    handle_key(&mut state, &mut clients, &mut ui, key(KeyCode::Char('?'))).unwrap();
    ui.enhanced = true;
    handle_key(
        &mut state,
        &mut clients,
        &mut ui,
        with(KeyCode::Enter, KeyModifiers::SHIFT),
    )
    .unwrap();
    let Mode::Compose(composer) = &ui.mode else {
        panic!("Shift-Enter must not submit");
    };
    assert_eq!(composer.input.text(), "A?\n");
    assert!(!state.reveal_shortcuts);
}

#[test]
fn native_command_chords_do_not_type_activate_or_quit() {
    let mut state = fixtures::demo_state(Detail::Standard);
    let mut ui = test_ui();
    let selected = state.selected_post_id();
    for modifier in [KeyModifiers::SUPER, KeyModifiers::META] {
        for ch in ['c', 'v', 'n', 'q', 'D'] {
            assert!(!handle_key(
                &mut state,
                &mut Clients::empty(),
                &mut ui,
                with(KeyCode::Char(ch), modifier)
            )
            .unwrap());
            assert!(matches!(ui.mode, Mode::Normal));
            assert_eq!(state.selected_post_id(), selected);
        }
    }
    press(&mut state, &mut ui, KeyCode::Char('n'));
    for modifier in [KeyModifiers::SUPER, KeyModifiers::META] {
        handle_key(
            &mut state,
            &mut Clients::empty(),
            &mut ui,
            with(KeyCode::Char('c'), modifier),
        )
        .unwrap();
        assert!(!submits(with(KeyCode::Enter, modifier), Enter::Post, true));
        assert!(!breaks_line(
            with(KeyCode::Enter, modifier),
            Enter::Newline,
            true
        ));
    }
    let Mode::Compose(composer) = &ui.mode else {
        panic!("composer remains open");
    };
    assert!(composer.input.text().is_empty());
    ui.mode = Mode::Normal;
    assert!(handle_key(
        &mut state,
        &mut Clients::empty(),
        &mut ui,
        with(KeyCode::Char('c'), KeyModifiers::CONTROL)
    )
    .unwrap());
}

#[test]
fn ctrl_or_cmd_r_reloads_the_current_view_and_r_still_replies() {
    let mut state = fixtures::demo_state(Detail::Standard);
    let mut ui = test_ui();
    let mut clients = Clients::empty();

    fn reload(state: &mut State, clients: &mut Clients, ui: &mut Ui, key: KeyEvent) {
        state.status = "stale".into();
        state.status_error = None;
        state.status_expiry = None;
        assert!(!handle_key(state, clients, ui, key).unwrap());
        assert_eq!(
            state.status,
            "demo · already local",
            "{key:?} feed {:?} section {:?}",
            state.current_feed_id(),
            state.section
        );
    }

    handle_key(&mut state, &mut clients, &mut ui, key(KeyCode::Char('r'))).unwrap();
    assert_ne!(state.status, "demo · already local");
    assert!(matches!(ui.mode, Mode::Compose(_)));
    press(&mut state, &mut ui, KeyCode::Esc);
    assert!(matches!(ui.mode, Mode::Normal));

    handle_key(
        &mut state,
        &mut clients,
        &mut ui,
        with(KeyCode::Char('r'), KeyModifiers::ALT),
    )
    .unwrap();
    assert_ne!(state.status, "demo · already local");
    assert!(matches!(ui.mode, Mode::Compose(_)));
    press(&mut state, &mut ui, KeyCode::Esc);

    for modifiers in [
        KeyModifiers::CONTROL,
        KeyModifiers::SUPER,
        KeyModifiers::META,
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
        KeyModifiers::SUPER | KeyModifiers::SHIFT,
        KeyModifiers::CONTROL | KeyModifiers::ALT,
    ] {
        reload(
            &mut state,
            &mut clients,
            &mut ui,
            with(KeyCode::Char('r'), modifiers),
        );
        reload(
            &mut state,
            &mut clients,
            &mut ui,
            with(KeyCode::Char('R'), modifiers),
        );
        assert!(matches!(ui.mode, Mode::Normal));
    }

    press(&mut state, &mut ui, KeyCode::Char('n'));
    handle_key(&mut state, &mut clients, &mut ui, key(KeyCode::Char('h'))).unwrap();
    handle_key(&mut state, &mut clients, &mut ui, key(KeyCode::Char('i'))).unwrap();
    reload(
        &mut state,
        &mut clients,
        &mut ui,
        with(KeyCode::Char('r'), KeyModifiers::CONTROL),
    );
    reload(
        &mut state,
        &mut clients,
        &mut ui,
        with(KeyCode::Char('r'), KeyModifiers::SUPER),
    );
    let Mode::Compose(composer) = &ui.mode else {
        panic!("composer stays open");
    };
    assert_eq!(composer.input.text(), "hi");
    ui.mode = Mode::Normal;

    press(&mut state, &mut ui, KeyCode::Char(','));
    assert!(state.settings);
    let row = state.settings_row;
    reload(
        &mut state,
        &mut clients,
        &mut ui,
        with(KeyCode::Char('r'), KeyModifiers::CONTROL),
    );
    assert!(state.settings);
    assert_eq!(state.settings_row, row);
    press(&mut state, &mut ui, KeyCode::Esc);
    assert!(!state.settings);

    press(&mut state, &mut ui, KeyCode::Char(':'));
    handle_key(&mut state, &mut clients, &mut ui, key(KeyCode::Char('h'))).unwrap();
    reload(
        &mut state,
        &mut clients,
        &mut ui,
        with(KeyCode::Char('R'), KeyModifiers::SUPER),
    );
    let Mode::Command { input, .. } = &ui.mode else {
        panic!("command line stays open");
    };
    assert_eq!(input.text(), "h");
    press(&mut state, &mut ui, KeyCode::Esc);

    press(&mut state, &mut ui, KeyCode::Char('?'));
    reload(
        &mut state,
        &mut clients,
        &mut ui,
        with(KeyCode::Char('r'), KeyModifiers::CONTROL),
    );
    assert!(matches!(ui.mode, Mode::Help));
    press(&mut state, &mut ui, KeyCode::Esc);

    handle_key(
        &mut state,
        &mut clients,
        &mut ui,
        with(KeyCode::Char('a'), KeyModifiers::CONTROL),
    )
    .unwrap();
    handle_key(&mut state, &mut clients, &mut ui, key(KeyCode::Char('x'))).unwrap();
    reload(
        &mut state,
        &mut clients,
        &mut ui,
        with(KeyCode::Char('r'), KeyModifiers::CONTROL),
    );
    let Mode::Ask { input, .. } = &ui.mode else {
        panic!("ask stays open");
    };
    assert_eq!(input.text(), "x");
    press(&mut state, &mut ui, KeyCode::Esc);

    press(&mut state, &mut ui, KeyCode::Char('.'));
    reload(
        &mut state,
        &mut clients,
        &mut ui,
        with(KeyCode::Char('r'), KeyModifiers::CONTROL),
    );
    assert!(matches!(ui.mode, Mode::Inspect { .. }));
    press(&mut state, &mut ui, KeyCode::Esc);

    press(&mut state, &mut ui, KeyCode::Char('s'));
    reload(
        &mut state,
        &mut clients,
        &mut ui,
        with(KeyCode::Char('r'), KeyModifiers::SUPER),
    );
    assert!(matches!(ui.mode, Mode::Share(_)));
    press(&mut state, &mut ui, KeyCode::Esc);

    press(&mut state, &mut ui, KeyCode::Char('D'));
    let cursor = cursor_of(&ui);
    reload(
        &mut state,
        &mut clients,
        &mut ui,
        with(KeyCode::Char('r'), KeyModifiers::CONTROL),
    );
    assert_eq!(cursor_of(&ui), cursor);
    press(&mut state, &mut ui, KeyCode::Esc);

    press(&mut state, &mut ui, KeyCode::Char('F'));
    reload(
        &mut state,
        &mut clients,
        &mut ui,
        with(KeyCode::Char('r'), KeyModifiers::META),
    );
    assert!(matches!(ui.mode, Mode::Filter { .. }));
    press(&mut state, &mut ui, KeyCode::Esc);

    press(&mut state, &mut ui, KeyCode::Char('f'));
    handle_key(&mut state, &mut clients, &mut ui, key(KeyCode::Char('q'))).unwrap();
    state.status = "stale".into();
    handle_key(
        &mut state,
        &mut clients,
        &mut ui,
        with(KeyCode::Char('r'), KeyModifiers::META),
    )
    .unwrap();
    let Mode::Search(input) = &ui.mode else {
        panic!("search stays open");
    };
    assert_eq!(input.text(), "q");
    assert_eq!(state.status, "stale");
    press(&mut state, &mut ui, KeyCode::Enter);
    let remote = state.remote.clone();
    assert!(remote.is_some());
    reload(
        &mut state,
        &mut clients,
        &mut ui,
        with(KeyCode::Char('r'), KeyModifiers::CONTROL),
    );
    assert_eq!(state.section, Section::Search);
    assert_eq!(state.remote, remote);

    state
        .apply(Cmd::Section(Section::Notifications), &mut clients)
        .unwrap();
    reload(
        &mut state,
        &mut clients,
        &mut ui,
        with(KeyCode::Char('r'), KeyModifiers::CONTROL),
    );
    assert_eq!(state.section, Section::Notifications);
    assert!(matches!(ui.mode, Mode::Normal));

    state
        .open_user(Scope::One(Network::X), "river".into(), &mut clients)
        .unwrap();
    let remote = state.remote.clone();
    reload(
        &mut state,
        &mut clients,
        &mut ui,
        with(KeyCode::Char('r'), KeyModifiers::SUPER),
    );
    assert_eq!(state.section, Section::Search);
    assert_eq!(state.remote, remote);

    loop {
        let before = (state.section, state.focus.clone(), state.crumb());
        state.apply(Cmd::Back, &mut clients).unwrap();
        if (state.section, state.focus.clone(), state.crumb()) == before {
            break;
        }
    }
    assert_eq!(state.section, Section::Home);
    assert!(state.focus.is_none());
    state.apply(Cmd::Expand, &mut clients).unwrap();
    let focus = state.focus.clone();
    assert!(focus.is_some());
    reload(
        &mut state,
        &mut clients,
        &mut ui,
        with(KeyCode::Char('r'), KeyModifiers::CONTROL),
    );
    assert_eq!(state.focus, focus);
    assert!(matches!(ui.mode, Mode::Normal));

    handle_key(
        &mut state,
        &mut clients,
        &mut ui,
        KeyEvent::new(
            KeyCode::Modifier(ModifierKeyCode::LeftControl),
            KeyModifiers::CONTROL,
        ),
    )
    .unwrap();
    assert!(state.reveal_shortcuts);
    reload(
        &mut state,
        &mut clients,
        &mut ui,
        with(KeyCode::Char('r'), KeyModifiers::CONTROL),
    );
    assert!(state.reveal_shortcuts);
}

#[test]
fn control_chords_keep_their_actions_while_disclosure_is_held() {
    let mut state = fixtures::demo_state(Detail::Standard);
    let mut ui = test_ui();
    handle_key(
        &mut state,
        &mut Clients::empty(),
        &mut ui,
        KeyEvent::new(
            KeyCode::Modifier(ModifierKeyCode::LeftControl),
            KeyModifiers::CONTROL,
        ),
    )
    .unwrap();
    handle_key(
        &mut state,
        &mut Clients::empty(),
        &mut ui,
        with(KeyCode::Char('k'), KeyModifiers::CONTROL),
    )
    .unwrap();
    assert!(state.reveal_shortcuts);
    assert!(matches!(ui.mode, Mode::Command { .. }));
    let mut input = Input::new();
    edit(&mut input, key(KeyCode::Char('A')));
    edit(&mut input, with(KeyCode::Char('z'), KeyModifiers::CONTROL));
    assert_eq!(input.text(), "");
    edit(
        &mut input,
        with(
            KeyCode::Char('z'),
            KeyModifiers::CONTROL | KeyModifiers::SHIFT,
        ),
    );
    assert_eq!(input.text(), "A");
}

#[test]
fn picture_cells_keep_their_colors_and_never_receive_action_underlines() {
    let mut buffer = Buffer::empty(Rect::new(0, 0, 8, 4));
    for cell in &mut buffer.content {
        cell.set_symbol("▀");
        cell.set_fg(ratatui::style::Color::Rgb(17, 88, 143));
        cell.modifier.insert(Modifier::UNDERLINED);
    }
    let picture = Rect::new(1, 1, 5, 2);
    let overlay = Rect::new(4, 1, 2, 1);
    clear_picture_underlines(&mut buffer, &[picture], overlay);
    let target = Target {
        x: 1,
        y: 1,
        width: 5,
        act: Act::Media("demo".into()),
        zone: Zone::BodyLeft,
        tab: true,
    };
    highlight(
        &mut buffer,
        &target,
        &fixtures::demo_state(Detail::Rich).theme,
        &[picture],
    );
    for y in 1..3 {
        for x in 1..6 {
            let cell = &buffer[(x, y)];
            assert_eq!(cell.fg, ratatui::style::Color::Rgb(17, 88, 143));
            assert_eq!(cell.symbol(), "▀");
            assert_eq!(
                cell.modifier.contains(Modifier::UNDERLINED),
                overlay.contains(Position::new(x, y))
            );
        }
    }
    assert!(buffer[(0, 0)].modifier.contains(Modifier::UNDERLINED));
}

#[test]
fn rich_preview_slots_are_excluded_from_disclosure_in_the_actual_frame() {
    let mut state = fixtures::demo_state(Detail::Rich);
    state.preview_ratio = Some(0.5);
    state.graphics = true;
    state.reveal_shortcuts = true;
    let ui = test_ui();
    let buffer = frame(&mut state, &ui, 100, 40);
    let body = state.view_sized(96, true, Some(31));
    let mut checked = 0;
    for slot in body.previews.iter().filter(|slot| slot.y + slot.rows <= 31) {
        for y in slot.y..slot.y + slot.rows {
            for x in slot.x..slot.x + slot.cols {
                assert!(
                    !buffer[((x + 2) as u16, (y + 6) as u16)]
                        .modifier
                        .contains(Modifier::UNDERLINED),
                    "picture at {x},{y}"
                );
                checked += 1;
            }
        }
    }
    assert!(checked > 15);
}
#[test]
fn welcome_keeps_only_breadcrumbs_and_all_six_choices_at_every_size() {
    for (width, height) in [
        (24, 8),
        (32, 10),
        (40, 15),
        (40, 20),
        (60, 20),
        (80, 24),
        (100, 32),
        (180, 45),
    ] {
        let mut state = State::boot(false, 15, &Config::default(), &Clients::empty(), None);
        state.status.clear();
        let ui = test_ui();
        let buffer = frame(&mut state, &ui, width, height);
        let text = buffer
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(text.contains("SoCLI > login"), "{width}x{height}: {text}");
        assert!(!text.contains("Notifications"), "{text}");
        let targets = &ui.hits.borrow().targets;
        assert_eq!(
            targets
                .iter()
                .filter(|target| matches!(target.act, Act::SignIn(_) | Act::EnterDemo))
                .count(),
            6,
            "{width}x{height}"
        );
        assert!(
            targets
                .iter()
                .all(|target| matches!(target.act, Act::SignIn(_) | Act::EnterDemo)),
            "{targets:?}"
        );
        let demo = targets
            .iter()
            .find(|target| target.act == Act::EnterDemo)
            .unwrap();
        assert!(targets
            .iter()
            .filter(|target| matches!(target.act, Act::SignIn(_)))
            .all(|target| target.y > demo.y));
    }
}

#[test]
fn welcome_hover_and_keyboard_select_the_same_network_without_signing_in() {
    let mut state = State::boot(false, 15, &Config::default(), &Clients::empty(), None);
    let mut ui = test_ui();
    frame(&mut state, &ui, 100, 32);
    let target = target_where(&ui, |target| target.act == Act::SignIn(Network::Bluesky));
    mouse(
        &mut state,
        &mut ui,
        MouseEventKind::Moved,
        target.x.into(),
        target.y.into(),
    );
    assert_eq!(Network::ALL[state.gate_row], Network::Bluesky);
    press(&mut state, &mut ui, KeyCode::Up);
    assert_eq!(Network::ALL[state.gate_row], Network::X);
    press(&mut state, &mut ui, KeyCode::Up);
    assert_eq!(state.gate_row, onboard::CHOICES - 1);
    press(&mut state, &mut ui, KeyCode::Down);
    assert_eq!(Network::ALL[state.gate_row], Network::X);
    assert!(onboard::showing(&state));
}
#[test]
fn login_results_wait_for_continue_and_can_retry_or_cancel() {
    let mut attempts = 0;
    let mut outcomes = Vec::new();
    let completed = login_with_result(
        || {
            attempts += 1;
            if attempts == 1 {
                Err("rejected".into())
            } else {
                Ok(())
            }
        },
        |result| {
            outcomes.push(result.map_err(str::to_string));
            Ok(true)
        },
    )
    .unwrap();
    assert!(completed);
    assert_eq!(attempts, 2);
    assert_eq!(outcomes, [Err("rejected".into()), Ok(())]);
    let mut attempts = 0;
    assert!(!login_with_result(
        || {
            attempts += 1;
            Err("rejected".into())
        },
        |_| Ok(false)
    )
    .unwrap());
    assert_eq!(attempts, 1);
    assert!(!login_with_result(
        || Err("cancelled".into()),
        |_| panic!("an explicit cancellation returns to the reader")
    )
    .unwrap());
}

#[cfg(feature = "native")]
#[test]
fn a_terminal_guard_dropped_without_disarm_restores_the_terminal() {
    static RESTORED: AtomicBool = AtomicBool::new(false);
    fn mark(enhanced: bool) {
        RESTORED.store(enhanced, Ordering::SeqCst);
    }
    RESTORED.store(false, Ordering::SeqCst);
    {
        let _guard = TerminalGuard::armed_with(true, mark);
    }
    assert!(
        RESTORED.load(Ordering::SeqCst),
        "drop without disarm must restore"
    );
    RESTORED.store(false, Ordering::SeqCst);
    {
        let mut guard = TerminalGuard::armed_with(true, mark);
        guard.disarm();
    }
    assert!(
        !RESTORED.load(Ordering::SeqCst),
        "disarm must skip the second restore"
    );
}

#[cfg(feature = "native")]
#[test]
fn a_busy_clients_lock_keeps_the_key_and_sets_loading() {
    let mutex = Mutex::new(0);
    let _held = mutex.lock().unwrap();
    match next_clients_key(&mutex, 'l') {
        ClientsKey::Waiting { key, loading } => {
            assert_eq!(key, 'l');
            assert!(loading);
        }
        ClientsKey::Ready(_, _) => panic!("a busy lock must not handle the key"),
    };
}

#[cfg(feature = "native")]
#[test]
fn a_free_clients_lock_handles_the_key() {
    let mutex = Mutex::new(0);
    match next_clients_key(&mutex, 'l') {
        ClientsKey::Ready(key, guard) => {
            assert_eq!(key, 'l');
            assert_eq!(*guard, 0);
        }
        ClientsKey::Waiting { .. } => panic!("a free lock must handle the key"),
    };
}

#[cfg(feature = "native")]
#[test]
fn a_poisoned_clients_lock_still_handles_the_key() {
    let mutex = Mutex::new(0);
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _guard = mutex.lock().unwrap();
        panic!("poison the clients lock");
    }));
    match next_clients_key(&mutex, 'l') {
        ClientsKey::Ready(key, guard) => {
            assert_eq!(key, 'l');
            assert_eq!(*guard, 0);
        }
        ClientsKey::Waiting { .. } => panic!("poison is not a busy lock"),
    };
}
