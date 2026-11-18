use super::tests::headless_app;
use super::*;

fn smart_app() -> App {
    let mut app = headless_app();
    crate::demo::populate(&mut app);
    app.remote = None;
    app.selected_device = None;
    app.local_ready = true;
    app.local.connected = true;
    app.local.playback = Playback::Playing;
    app.local.track = Some(crate::player::LocalTrack {
        uri: "spotify:track:old".into(),
        title: "Old song".into(),
        duration_ms: 240_000,
        ..Default::default()
    });
    app.local.position_ms = 42_000;
    app.local.position_at = None;
    app.shuffle_wanted = true;
    app.smart_wanted = true;
    app
}

fn pending_smart(app: &mut App, count: usize) -> (u64, String) {
    let tracks: Vec<String> = (1..=count).map(|n| format!("spotify:track:p{n}")).collect();
    app.start_play(PlayRequest::tracks(tracks).starting_at_index(0), false);
    (
        app.smart_generation,
        app.smart_pending.clone().expect("recommendations pending"),
    )
}

#[test]
fn smart_shuffle_late_answer_must_not_undo_next() {
    let mut app = smart_app();
    let (generation, key) = pending_smart(&mut app, 6);
    app.backend.take_player_commands();
    app.apply(Action::Next, &egui::Context::default());
    app.backend.take_player_commands();
    app.on_smart_shuffle(generation, key, Ok(vec!["spotify:track:r1".into()]));
    let commands = app.backend.take_player_commands();
    app.backend.shutdown();
    assert!(
        !commands.iter().any(|c| matches!(c, PlayerCommand::Load(_))),
        "late recommendations restarted playback after Next: {commands:?}"
    );
}

#[test]
fn smart_shuffle_late_answer_must_not_undo_pause() {
    let mut app = smart_app();
    let (generation, key) = pending_smart(&mut app, 6);
    app.apply(Action::TogglePlay, &egui::Context::default());
    app.local.playback = Playback::Paused;
    app.backend.take_player_commands();
    app.on_smart_shuffle(generation, key, Ok(vec!["spotify:track:r1".into()]));
    let commands = app.backend.take_player_commands();
    app.backend.shutdown();
    assert!(
        !commands
            .iter()
            .any(|c| matches!(c, PlayerCommand::Load(load) if load.play)),
        "late recommendations resumed playback after Pause: {commands:?}"
    );
}

#[test]
fn smart_shuffle_off_must_remove_woven_recommendations() {
    let mut app = smart_app();
    let (generation, key) = pending_smart(&mut app, 6);
    app.on_smart_shuffle(generation, key, Ok(vec!["spotify:track:r1".into()]));
    assert!(
        app.local_list
            .as_ref()
            .unwrap()
            .iter()
            .any(|u| u == "spotify:track:r1")
    );
    app.set_shuffle_mode(ShuffleMode::Off);
    let leftover = app
        .local_list
        .as_ref()
        .unwrap()
        .iter()
        .any(|u| u == "spotify:track:r1");
    app.backend.shutdown();
    assert!(
        !leftover,
        "turning Smart Shuffle off leaves its recommendation in the play order"
    );
}

#[test]
fn smart_shuffle_must_keep_all_450_original_songs() {
    let mut app = smart_app();
    let (generation, key) = pending_smart(&mut app, 450);
    app.backend.take_player_commands();
    let recommendations = (1..=150).map(|n| format!("spotify:track:r{n}")).collect();
    app.on_smart_shuffle(generation, key, Ok(recommendations));
    let commands = app.backend.take_player_commands();
    let load = commands
        .iter()
        .find_map(|c| match c {
            PlayerCommand::Load(load) => Some(load),
            _ => None,
        })
        .unwrap();
    let own_count = load
        .uris
        .iter()
        .filter(|u| u.starts_with("spotify:track:p"))
        .count();
    let total = load.uris.len();
    app.backend.shutdown();
    assert_eq!(
        own_count, 450,
        "adding recommendations cut original songs; sent {total} total tracks"
    );
}

#[test]
fn smart_shuffle_must_show_chosen_song_before_recommendations() {
    let mut app = smart_app();
    let chosen = app.context_track_uris("spotify:playlist:pl1").unwrap()[0].clone();
    app.start_play(
        PlayRequest::context("spotify:playlist:pl1").starting_at_uri(&chosen),
        false,
    );
    let shown = app.now_playing().unwrap().uri;
    app.backend.shutdown();
    assert_eq!(
        shown, chosen,
        "the old song stays shown until the recommendations answer"
    );
}

#[test]
fn smart_shuffle_enabling_while_paused_must_work_when_resumed() {
    let mut app = smart_app();
    app.local.playback = Playback::Paused;
    app.smart_wanted = false;
    app.assumed_context = Some(AssumedContext {
        uri: "spotify:playlist:pl1".into(),
        shuffle: Some(true),
        at: Instant::now(),
    });
    app.set_shuffle_mode(ShuffleMode::Smart);
    app.apply(Action::TogglePlay, &egui::Context::default());
    let pending = app.smart_pending.clone();
    app.backend.shutdown();
    assert!(
        pending.is_some(),
        "Smart is selected while paused, but resuming never fetches suggestions"
    );
}

fn woven_app() -> App {
    let mut app = smart_app();
    let (generation, key) = pending_smart(&mut app, 6);
    app.on_smart_shuffle(
        generation,
        key,
        Ok(vec!["spotify:track:r1".into(), "spotify:track:r2".into()]),
    );
    app.local.track.as_mut().unwrap().uri = "spotify:track:p1".into();
    app.backend.take_player_commands();
    app
}

#[test]
fn smart_shuffle_off_while_paused_defers_restore_until_resume() {
    let mut app = woven_app();
    app.local.playback = Playback::Paused;
    app.optimistic_playing = None;
    app.set_shuffle_mode(ShuffleMode::Off);
    assert!(app.smart_restore.is_some());
    assert!(
        !app.backend
            .take_player_commands()
            .iter()
            .any(|c| matches!(c, PlayerCommand::Load(_)))
    );
    app.apply(Action::TogglePlay, &egui::Context::default());
    let commands = app.backend.take_player_commands();
    let load = commands
        .iter()
        .find_map(|c| match c {
            PlayerCommand::Load(load) => Some(load),
            _ => None,
        })
        .unwrap();
    assert!(load.play);
    assert_eq!(load.position_ms, 42_000);
    assert!(load.uris.iter().all(|u| !u.starts_with("spotify:track:r")));
    assert!(!commands.iter().any(|c| matches!(c, PlayerCommand::Toggle)));
    app.backend.shutdown();
}

#[test]
fn smart_shuffle_plain_mode_restores_original_songs_and_keeps_manual_queue() {
    let mut app = woven_app();
    app.manual_queue = vec!["spotify:track:r2".into(), "spotify:track:manual".into()];
    let manual = app.manual_queue.clone();
    app.set_shuffle_mode(ShuffleMode::On);
    assert_eq!(app.shuffle_mode(), ShuffleMode::On);
    assert_eq!(app.manual_queue, manual);
    assert!(
        app.local_list
            .as_ref()
            .unwrap()
            .iter()
            .all(|u| !u.starts_with("spotify:track:r"))
    );
    let commands = app.backend.take_player_commands();
    assert!(
        commands
            .iter()
            .any(|c| matches!(c, PlayerCommand::Shuffle(true)))
    );
    assert!(
        !commands
            .iter()
            .any(|c| matches!(c, PlayerCommand::ClearQueue))
    );
    app.backend.shutdown();
}

#[test]
fn smart_shuffle_off_finishes_current_recommendation_without_future_suggestions() {
    let mut app = woven_app();
    app.local.track.as_mut().unwrap().uri = "spotify:track:r1".into();
    app.set_shuffle_mode(ShuffleMode::Off);
    let list = app.local_list.as_ref().unwrap();
    assert_eq!(list[0], "spotify:track:r1");
    assert_eq!(list.len(), 7);
    assert!(!list.iter().any(|u| u == "spotify:track:r2"));
    let load = app
        .backend
        .take_player_commands()
        .into_iter()
        .find_map(|c| match c {
            PlayerCommand::Load(load) => Some(load),
            _ => None,
        })
        .unwrap();
    assert_eq!(load.offset_uri.as_deref(), Some("spotify:track:r1"));
    assert_eq!(load.position_ms, 42_000);
    app.backend.shutdown();
}

#[test]
fn smart_shuffle_off_keeps_manually_queued_occurrence_of_recommendation() {
    let mut app = woven_app();
    app.manual_queue = vec!["spotify:track:r2".into()];
    app.queue = Loadable::Loaded(crate::api::models::Queue {
        currently_playing: None,
        queue: ["spotify:track:r2", "spotify:track:r1", "spotify:track:p2"]
            .into_iter()
            .map(|uri| {
                PlayableItem::Track(Track {
                    uri: uri.into(),
                    ..Default::default()
                })
            })
            .collect(),
    });
    app.set_shuffle_mode(ShuffleMode::Off);
    let shown: Vec<&str> = app
        .queue
        .get()
        .unwrap()
        .queue
        .iter()
        .map(PlayableItem::uri)
        .collect();
    assert_eq!(shown, ["spotify:track:r2", "spotify:track:p2"]);
    assert_eq!(app.manual_queue, ["spotify:track:r2"]);
    app.backend.shutdown();
}

#[test]
fn smart_shuffle_incomplete_playlist_keeps_full_native_context() {
    let mut app = smart_app();
    let page = app.playlist_pages.get_mut("pl1").unwrap();
    page.items.next_offset = Some(page.items.items.len() as u32);
    page.items.total = Some(1_000);
    app.start_play(PlayRequest::context("spotify:playlist:pl1"), false);
    assert!(app.smart_pending.is_none());
    // Incomplete contexts play natively without discarding the user's Smart Shuffle toggle.
    assert_eq!(app.shuffle_mode(), ShuffleMode::Smart);
    let commands = app.backend.take_player_commands();
    let load = commands
        .iter()
        .find_map(|c| match c {
            PlayerCommand::Load(load) => Some(load),
            _ => None,
        })
        .unwrap();
    assert_eq!(load.context_uri.as_deref(), Some("spotify:playlist:pl1"));
    assert!(load.uris.is_empty());
    app.backend.shutdown();
}

#[test]
fn smart_shuffle_reweave_and_restore_keep_filtered_source() {
    let mut app = smart_app();
    let rows = app.context_track_uris("spotify:playlist:pl1").unwrap();
    let subset = rows[..4].to_vec();
    app.local_list = Some(subset.clone());
    app.local.track.as_mut().unwrap().uri = subset[0].clone();
    app.smart_wanted = false;
    app.assumed_context = Some(AssumedContext {
        uri: "spotify:playlist:pl1".into(),
        shuffle: Some(true),
        at: Instant::now(),
    });
    app.set_shuffle_mode(ShuffleMode::Smart);
    assert_eq!(app.smart_tracks.len(), 4);
    let generation = app.smart_generation;
    let key = app.smart_pending.clone().unwrap();
    app.on_smart_shuffle(generation, key, Ok(vec!["spotify:track:r1".into()]));
    app.set_shuffle_mode(ShuffleMode::Off);
    assert_eq!(app.local_list, Some(subset));
    assert_eq!(
        app.playing_context_uri().as_deref(),
        Some("spotify:playlist:pl1")
    );
    app.backend.shutdown();
}

#[test]
fn smart_shuffle_seek_invalidates_pending_answer() {
    let mut app = smart_app();
    let (generation, key) = pending_smart(&mut app, 6);
    app.apply(Action::Seek(90_000), &egui::Context::default());
    app.backend.take_player_commands();
    app.on_smart_shuffle(generation, key, Ok(vec!["spotify:track:r1".into()]));
    assert!(
        !app.backend
            .take_player_commands()
            .iter()
            .any(|c| matches!(c, PlayerCommand::Load(_)))
    );
    app.backend.shutdown();
}

#[test]
fn smart_shuffle_pause_while_previous_song_is_paused_never_resumes_it() {
    let mut app = smart_app();
    app.local.playback = Playback::Paused;
    let chosen = app.context_track_uris("spotify:playlist:pl1").unwrap()[0].clone();
    app.start_play(
        PlayRequest::context("spotify:playlist:pl1").starting_at_uri(chosen),
        false,
    );
    app.backend.take_player_commands();
    app.apply(Action::TogglePlay, &egui::Context::default());
    assert!(app.smart_pending.is_none());
    assert!(app.backend.take_player_commands().is_empty());
    app.backend.shutdown();
}

#[test]
fn smart_shuffle_automatic_advance_cannot_rewind_reweave() {
    let mut app = smart_app();
    let rows = app.context_track_uris("spotify:playlist:pl1").unwrap();
    app.local.track.as_mut().unwrap().uri = rows[0].clone();
    app.assumed_context = Some(AssumedContext {
        uri: "spotify:playlist:pl1".into(),
        shuffle: Some(true),
        at: Instant::now(),
    });
    app.reweave_playing_for_smart_shuffle();
    let generation = app.smart_generation;
    let key = app.smart_pending.clone().unwrap();
    app.local.track.as_mut().unwrap().uri = rows[1].clone();
    app.backend.take_player_commands();
    app.on_smart_shuffle(generation, key, Ok(vec!["spotify:track:r1".into()]));
    assert!(app.backend.take_player_commands().is_empty());
    app.backend.shutdown();
}

#[test]
fn smart_shuffle_local_long_list_keeps_every_original_song() {
    let mut app = smart_app();
    let (generation, key) = pending_smart(&mut app, 600);
    app.on_smart_shuffle(generation, key, Ok(vec!["spotify:track:r1".into()]));
    let list = app.local_list.as_ref().unwrap();
    assert_eq!(
        list.iter()
            .filter(|u| u.starts_with("spotify:track:p"))
            .count(),
        600
    );
    assert_eq!(list.len(), 601);
    app.backend.shutdown();
}

#[test]
fn smart_shuffle_soloist_budgets_suggestions_without_cutting_originals() {
    let mut app = smart_app();
    crate::demo::populate(&mut app);
    app.local = LocalState::default();
    app.local_ready = false;
    app.local_playback = LocalPlayback::Unavailable;
    app.settings.soloist_enabled = true;
    app.soloist_device_id = Some("soloist-1".into());
    app.soloist_status = crate::soloist::Status::Ready {
        name: "soloist".into(),
        version: "test".into(),
        expires: u64::MAX,
    };
    app.selected_device = Some("soloist-1".into());
    let (generation, key) = pending_smart(&mut app, 450);
    app.backend.take_remote_play_requests();
    app.on_smart_shuffle(
        generation,
        key,
        Ok((1..=150).map(|n| format!("spotify:track:r{n}")).collect()),
    );
    let requests = app.backend.take_remote_play_requests();
    let [
        ApiRequest::ShufflePlay {
            play,
            shuffle: false,
            device_id: Some(id),
        },
    ] = requests.as_slice()
    else {
        panic!("expected one ordered start: {requests:?}")
    };
    assert_eq!(id, "soloist-1");
    assert_eq!(play.uris.len(), 500);
    assert_eq!(
        play.uris
            .iter()
            .filter(|u| u.starts_with("spotify:track:p"))
            .count(),
        450
    );
    assert!(app.local_list.is_none());
    app.backend.shutdown();
}

#[test]
fn smart_shuffle_device_change_cancels_old_answer_even_after_returning() {
    let mut app = smart_app();
    let (generation, key) = pending_smart(&mut app, 6);
    app.apply(
        Action::Transfer("speaker".into()),
        &egui::Context::default(),
    );
    app.selected_device = None;
    app.backend.take_player_commands();
    app.on_smart_shuffle(generation, key, Ok(vec!["spotify:track:r1".into()]));
    assert!(app.smart_pending.is_none());
    assert!(
        !app.backend
            .take_player_commands()
            .iter()
            .any(|c| matches!(c, PlayerCommand::Load(_)))
    );
    app.backend.shutdown();
}

#[test]
fn smart_shuffle_paused_restore_uses_latest_seek_and_mode() {
    let mut app = woven_app();
    app.local.playback = Playback::Paused;
    app.optimistic_playing = None;
    app.set_shuffle_mode(ShuffleMode::Off);
    app.set_shuffle_mode(ShuffleMode::On);
    assert!(app.smart_restore.is_some());
    app.apply(Action::Seek(90_000), &egui::Context::default());
    app.backend.take_player_commands();
    app.apply(Action::TogglePlay, &egui::Context::default());
    let load = app
        .backend
        .take_player_commands()
        .into_iter()
        .find_map(|c| match c {
            PlayerCommand::Load(load) => Some(load),
            _ => None,
        })
        .unwrap();
    assert_eq!(load.position_ms, 90_000);
    assert_eq!(app.shuffle_mode(), ShuffleMode::On);
    assert!(load.uris.iter().all(|u| !u.starts_with("spotify:track:r")));
    app.backend.shutdown();
}

#[test]
fn smart_shuffle_paused_restore_never_rewinds_a_later_current_song() {
    let mut app = woven_app();
    app.local.playback = Playback::Paused;
    app.optimistic_playing = None;
    app.set_shuffle_mode(ShuffleMode::Off);
    app.local.track.as_mut().unwrap().uri = "spotify:track:p4".into();
    app.local.position_ms = 12_000;
    app.backend.take_player_commands();
    app.apply(Action::TogglePlay, &egui::Context::default());
    let load = app
        .backend
        .take_player_commands()
        .into_iter()
        .find_map(|c| match c {
            PlayerCommand::Load(load) => Some(load),
            _ => None,
        })
        .unwrap();
    assert_eq!(load.offset_uri.as_deref(), Some("spotify:track:p4"));
    assert_eq!(load.position_ms, 12_000);
    app.backend.shutdown();
}

fn soloist_webapi_source() -> App {
    let mut app = smart_app();
    crate::demo::populate(&mut app);
    app.local = LocalState::default();
    app.local_ready = false;
    app.local_playback = LocalPlayback::Unavailable;
    app.settings.soloist_enabled = true;
    app.soloist_device_id = Some("soloist-1".into());
    app.soloist_status = crate::soloist::Status::Ready {
        name: "soloist".into(),
        version: "test".into(),
        expires: u64::MAX,
    };
    app.selected_device = Some("soloist-1".into());
    let current = app.context_track_uris("spotify:playlist:pl1").unwrap()[0].clone();
    let remote = app.remote.as_mut().unwrap();
    remote.state.is_playing = true;
    remote.state.item = Some(PlayableItem::Track(Track {
        uri: current,
        duration_ms: 200_000,
        ..Default::default()
    }));
    remote.state.context = Some(crate::api::models::Context {
        uri: "spotify:webapi".into(),
        kind: String::new(),
    });
    remote.state.device.as_mut().unwrap().id = Some("soloist-1".into());
    remote.received_at = Instant::now();
    app.assumed_context = Some(AssumedContext {
        uri: "spotify:playlist:pl1".into(),
        shuffle: Some(true),
        at: Instant::now() - ASSUMED_CONTEXT_HOLD - Duration::from_secs(1),
    });
    app.shuffle_wanted = true;
    app.smart_wanted = false;
    app
}

#[test]
fn smart_shuffle_webapi_context_keeps_source_across_repeated_activation() {
    let mut app = soloist_webapi_source();
    assert_eq!(
        app.playing_context_uri().as_deref(),
        Some("spotify:playlist:pl1")
    );
    app.set_shuffle_mode(ShuffleMode::Smart);
    assert_eq!(app.smart_pending.as_deref(), Some("spotify:playlist:pl1"));
    let generation = app.smart_generation;
    app.on_smart_shuffle(
        generation,
        "spotify:playlist:pl1".into(),
        Ok(vec!["spotify:track:r1".into()]),
    );
    app.assumed_context.as_mut().unwrap().at =
        Instant::now() - ASSUMED_CONTEXT_HOLD - Duration::from_secs(1);
    app.set_shuffle_mode(ShuffleMode::Off);
    app.assumed_context.as_mut().unwrap().at =
        Instant::now() - ASSUMED_CONTEXT_HOLD - Duration::from_secs(1);
    app.set_shuffle_mode(ShuffleMode::On);
    app.set_shuffle_mode(ShuffleMode::Smart);
    assert_eq!(app.shuffle_mode(), ShuffleMode::Smart);
    assert_eq!(app.smart_pending.as_deref(), Some("spotify:playlist:pl1"));
    assert_eq!(app.smart_context.as_deref(), Some("spotify:playlist:pl1"));
    app.backend.shutdown();
}

#[test]
fn smart_shuffle_real_context_change_still_replaces_assumed_source() {
    let mut app = soloist_webapi_source();
    app.remote
        .as_mut()
        .unwrap()
        .state
        .context
        .as_mut()
        .unwrap()
        .uri = "spotify:playlist:another".into();
    assert_eq!(
        app.playing_context_uri().as_deref(),
        Some("spotify:playlist:another")
    );
    app.backend.shutdown();
}

#[test]
fn smart_shuffle_webapi_marker_alone_is_not_a_collection() {
    let mut app = soloist_webapi_source();
    app.assumed_context = None;
    assert_eq!(app.playing_context_uri(), None);
    app.backend.shutdown();
}
