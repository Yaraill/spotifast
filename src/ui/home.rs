//! The Home page.

use std::sync::Arc;

use egui::{CornerRadius, Rect, Sense, Vec2, pos2, vec2};

use crate::api::models::{Album, Episode, PlayableItem, Playlist, Show, pick_image};
use crate::app::App;
use crate::i18n::{gettext, pgettext};
use crate::model::{Action, DragHomeShelf, HomeFilter, Loadable, Page, RowContext, TopPeriod};
use crate::settings::{BentoWidgetMode, HomeShelfId, normalized_shelf_order};
use crate::theme::{self, Icon};

use super::widgets::{self, TrackRow};

pub fn show(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    if !app.network_reachable {
        let space = ((ui.available_height() - 160.0) / 2.0).max(48.0);
        ui.add_space(space);
        ui.vertical_centered(|ui| {
            theme::icon(ui, Icon::WifiOff, 54.0, palette.dim);
            ui.add_space(16.0);
            theme::text(
                ui,
                gettext(app.locale, "You're offline"),
                theme::bold(28.0),
                palette.text,
            );
            ui.add_space(8.0);
            theme::text(
                ui,
                gettext(
                    app.locale,
                    "Spotifast will reconnect automatically when your connection returns.",
                ),
                theme::regular(14.0),
                palette.secondary,
            );
        });
        return;
    }
    ui.add_space(6.0);
    app.ensure_home_daylist_fresh();
    let greeting = crate::util::greeting(app.locale);
    ui.horizontal(|ui| {
        theme::text(ui, greeting.as_ref(), theme::bold(30.0), palette.text);
        if app.home.filter == HomeFilter::All {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                home_layout_controls(app, ui);
            });
        }
    });
    ui.add_space(10.0);
    let topbar_chips = ui
        .ctx()
        .data(|data| data.get_temp::<bool>(egui::Id::new("home-topbar-chips-shown")))
        .unwrap_or(false);
    if !topbar_chips {
        home_filter_row(app, ui);
        ui.add_space(10.0);
    } else {
        ui.add_space(2.0);
    }

    match app.home.filter {
        HomeFilter::All | HomeFilter::Music => {
            bento_hero(app, ui);
            ui.add_space(16.0);
            render_ordered_shelves(app, ui);
        }
        HomeFilter::Podcasts => podcasts_filter_view(app, ui),
        HomeFilter::Top => top_filter_view(app, ui),
    }
}

fn home_layout_controls(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    let locale = app.locale;
    let locked = app.settings.home.layout_locked;
    if locked {
        let tip = gettext(locale, "Customize home");
        if theme::icon_button(ui, Icon::Lock, 15.0, palette.secondary, palette.text, &tip).clicked()
        {
            app.actions.push(Action::SetHomeLayoutLocked(false));
        }
        return;
    }

    ui.spacing_mut().item_spacing.x = 8.0;
    if theme::accent_chip(ui, &palette, &gettext(locale, "Lock layout"), true).clicked() {
        app.actions.push(Action::SetHomeLayoutLocked(true));
    }
    if normalized_shelf_order(&app.settings.home.shelf_order) != HomeShelfId::ALL
        && theme::link(
            ui,
            &*gettext(locale, "Reset"),
            theme::medium(12.5),
            palette.secondary,
        )
        .clicked()
    {
        app.actions.push(Action::ResetHomeShelfOrder);
    }
}

fn shelf_title_key(id: HomeShelfId) -> &'static str {
    match id {
        HomeShelfId::MadeForYou => "Made for you",
        HomeShelfId::TopMixes => "Your top mixes",
        HomeShelfId::NewReleases => "New releases",
        HomeShelfId::RecentlyPlayed => "Recently played",
        HomeShelfId::Podcasts => "Your podcasts",
        HomeShelfId::TopArtists => "Your top artists",
        HomeShelfId::TopSongs => "Your top songs",
        HomeShelfId::Recommendations => "Recommended for you",
    }
}

fn shelf_is_visible(app: &App, id: HomeShelfId) -> bool {
    match id {
        HomeShelfId::MadeForYou => {
            app.settings.home.made_for_you.visible
                && !matches!(&app.home.made_for_you, Loadable::Loaded(v) if v.is_empty())
        }
        HomeShelfId::TopMixes => {
            app.settings.home.made_for_you.visible
                && app
                    .home
                    .top_mixes
                    .get()
                    .is_some_and(|playlists| !playlists.is_empty())
        }
        HomeShelfId::NewReleases => {
            !matches!(&app.home.new_releases, Loadable::Loaded(v) if v.is_empty())
        }
        HomeShelfId::RecentlyPlayed => {
            !matches!(&app.home.recently_played, Loadable::Loaded(v) if v.is_empty())
        }
        HomeShelfId::Podcasts => {
            app.home.filter == HomeFilter::All
                && !podcast_episodes(
                    &app.home.podcasts,
                    |show| {
                        app.audiobook_shows.contains(&show.uri)
                            || app.is_saved(&show.uri) == Some(false)
                    },
                    jiff::Zoned::now().date(),
                )
                .is_empty()
        }
        HomeShelfId::TopArtists => {
            !matches!(&app.home.top_artists, Loadable::Loaded(v) if v.is_empty())
        }
        HomeShelfId::TopSongs => {
            !matches!(&app.home.top_tracks, Loadable::Loaded(v) if v.is_empty())
        }
        HomeShelfId::Recommendations => {
            app.settings.home.recommendations.visible
                && !matches!(&app.home.recommendations, Loadable::Loaded(v) if v.is_empty())
        }
    }
}

fn reorder_target_index(from: usize, desired_gap: usize) -> Option<usize> {
    let to_index = if desired_gap > from {
        desired_gap.saturating_sub(1)
    } else {
        desired_gap
    };
    (to_index != from).then_some(to_index)
}

fn shelf_drop_slot(slots: &[(usize, f32, f32)], pointer_y: f32) -> Option<(usize, f32)> {
    let first = slots.first()?;
    if pointer_y <= (first.1 + first.2) * 0.5 {
        return Some((first.0, first.1));
    }
    for window in slots.windows(2) {
        let next = window[1];
        let next_mid = (next.1 + next.2) * 0.5;
        if pointer_y <= next_mid {
            return Some((next.0, next.1));
        }
    }
    let last = slots.last()?;
    Some((last.0 + 1, last.2))
}

fn draw_home_shelf(app: &mut App, ui: &mut egui::Ui, id: HomeShelfId) {
    match id {
        HomeShelfId::MadeForYou => made_for_you(app, ui),
        HomeShelfId::TopMixes => top_mixes(app, ui),
        HomeShelfId::NewReleases => new_releases(app, ui),
        HomeShelfId::RecentlyPlayed => recently_played(app, ui),
        HomeShelfId::Podcasts => podcasts(app, ui),
        HomeShelfId::TopArtists => top_artists(app, ui),
        HomeShelfId::TopSongs => top_tracks(app, ui),
        HomeShelfId::Recommendations => recommendations(app, ui),
    }
}

fn shelf_handle_strip(
    app: &mut App,
    ui: &mut egui::Ui,
    visible: &[(usize, HomeShelfId)],
    vis_idx: usize,
) {
    let palette = app.palette;
    let locale = app.locale;
    let (_, id) = visible[vis_idx];
    let title_key = shelf_title_key(id);
    let label = gettext(locale, title_key);
    let strip_w = ui.available_width();
    egui::Frame::new()
        .fill(palette.surface.gamma_multiply(0.75))
        .stroke(egui::Stroke::new(1.0, palette.outline.gamma_multiply(0.8)))
        .corner_radius(CornerRadius::same(8))
        .inner_margin(egui::Margin::symmetric(8, 3))
        .show(ui, |ui| {
            ui.set_width((strip_w - 16.0).max(160.0));
            ui.horizontal(|ui| {
                let drag_resp = ui
                    .horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        theme::icon(ui, Icon::GripVertical, 13.0, palette.secondary);
                        theme::text(ui, label.as_ref(), theme::semibold(11.5), palette.secondary);
                    })
                    .response
                    .interact(Sense::click_and_drag())
                    .on_hover_cursor(egui::CursorIcon::Grab)
                    .on_hover_text(gettext(locale, "Drag to reorder"));
                if drag_resp.drag_started_by(egui::PointerButton::Primary) {
                    egui::DragAndDrop::set_payload(
                        ui.ctx(),
                        DragHomeShelf {
                            id,
                            title_hint: title_key,
                        },
                    );
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    if vis_idx + 1 < visible.len()
                        && theme::icon_button(
                            ui,
                            Icon::ChevronDown,
                            13.0,
                            palette.secondary,
                            palette.text,
                            &gettext(locale, "Move down"),
                        )
                        .clicked()
                    {
                        app.actions.push(Action::ReorderHomeShelf {
                            id,
                            to_index: visible[vis_idx + 1].0,
                        });
                    }
                    if vis_idx > 0
                        && theme::icon_button(
                            ui,
                            Icon::ChevronUp,
                            13.0,
                            palette.secondary,
                            palette.text,
                            &gettext(locale, "Move up"),
                        )
                        .clicked()
                    {
                        app.actions.push(Action::ReorderHomeShelf {
                            id,
                            to_index: visible[vis_idx - 1].0,
                        });
                    }
                });
            });
        });
    ui.add_space(4.0);
}

fn render_ordered_shelves(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    let order = normalized_shelf_order(&app.settings.home.shelf_order);
    let visible: Vec<(usize, HomeShelfId)> = order
        .iter()
        .enumerate()
        .filter(|&(_, &id)| shelf_is_visible(app, id))
        .map(|(pos, &id)| (pos, id))
        .collect();
    let can_reorder = !app.settings.home.layout_locked && app.home.filter == HomeFilter::All;
    let mut slots: Vec<(usize, f32, f32)> = Vec::with_capacity(visible.len());
    let drag_active =
        can_reorder && egui::DragAndDrop::has_payload_of_type::<DragHomeShelf>(ui.ctx());
    if drag_active {
        widgets::scroll_during_drag(ui);
    }

    for (vis_idx, &(full_pos, id)) in visible.iter().enumerate() {
        let block = ui.vertical(|ui| {
            if can_reorder {
                shelf_handle_strip(app, ui, &visible, vis_idx);
            }
            draw_home_shelf(app, ui, id);
        });
        slots.push((full_pos, block.response.rect.top(), block.response.rect.bottom()));
    }

    if drag_active
        && let Some(pointer) = ui.ctx().pointer_latest_pos()
        && let Some((desired_gap, line_y)) = shelf_drop_slot(&slots, pointer.y)
    {
        let rect = ui.max_rect();
        ui.painter().hline(
            rect.x_range(),
            line_y,
            egui::Stroke::new(2.5, palette.accent),
        );
        if ui.input(|input| input.pointer.button_released(egui::PointerButton::Primary))
            && let Some(drag) = egui::DragAndDrop::take_payload::<DragHomeShelf>(ui.ctx())
            && let Some(from) = order.iter().position(|&known| known == drag.id)
            && let Some(to_index) = reorder_target_index(from, desired_gap)
        {
            app.actions.push(Action::ReorderHomeShelf {
                id: drag.id,
                to_index,
            });
        }
    }
}

fn home_filter_row(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    let locale = app.locale;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
        for filter in HomeFilter::ALL {
            let selected = app.home.filter == filter;
            if theme::accent_chip(ui, &palette, &filter.label(locale), selected).clicked()
                && !selected
            {
                app.actions.push(Action::SetHomeFilter(filter));
            }
        }
    });
}

struct Tile {
    image: Option<String>,
    name: String,
    page: Page,
    uri: Option<String>,
    liked: bool,
    owned_playlist: Option<Playlist>,
    /// A song played on its own, which the tile plays and whose menu it
    /// offers, rather than a playlist, album or artist.
    song: Option<crate::api::models::Track>,
}

/// The height of a quick-access tile, and so the side of its cover.
const TILE_HEIGHT: f32 = 60.0;
/// How many quick-access tiles Home shows, as Spotify does.
const TILES: usize = 8;

/// What was played most recently, as Spotify's Home shows it: the playlists,
/// albums, artists and podcasts songs were played from, Liked Songs, and
/// songs played on their own, newest first (#942). Until Spotify's play
/// history arrives, or when it holds fewer, Liked Songs and the first
/// playlists in the Library fill the rest.
fn tile_liked(app: &App) -> Tile {
    let collection = app
        .user
        .as_ref()
        .map(|user| format!("spotify:user:{}:collection", user.id));
    Tile {
        image: None,
        name: gettext(app.locale, "Liked Songs").into_owned(),
        page: Page::LikedSongs,
        uri: collection,
        liked: true,
        owned_playlist: None,
        song: None,
    }
}

fn tile_playlist(app: &App, playlist: &Playlist, tile_art: u32) -> Tile {
    Tile {
        image: pick_image(&playlist.images, tile_art).map(str::to_string),
        name: playlist.name.clone(),
        page: Page::Playlist(playlist.id.clone()),
        uri: Some(playlist.uri.clone()),
        liked: false,
        owned_playlist: app
            .user_id()
            .is_some_and(|id| playlist.owned_by(id))
            .then(|| playlist.clone()),
        song: None,
    }
}

fn tile_album(album: &Album, tile_art: u32) -> Tile {
    Tile {
        image: pick_image(&album.images, tile_art).map(str::to_string),
        name: album.name.clone(),
        page: Page::Album(album.id.clone()),
        uri: Some(album.uri.clone()),
        liked: false,
        owned_playlist: None,
        song: None,
    }
}

fn tile_artist(artist: &crate::api::models::Artist, tile_art: u32) -> Tile {
    Tile {
        image: pick_image(&artist.images, tile_art).map(str::to_string),
        name: artist.name.clone(),
        page: Page::Artist(artist.id.clone()),
        uri: Some(artist.uri.clone()),
        liked: false,
        owned_playlist: None,
        song: None,
    }
}

fn tile_from_play(app: &App, play: &crate::api::models::PlayHistory, tile_art: u32) -> Option<Tile> {
    let track = &play.track;
    let context = play.context.as_ref().map(|context| context.uri.as_str());
    match context.map(|uri| (uri, uri.rsplit(':').next().unwrap_or(""))) {
        Some((uri, _)) if uri.ends_with(":collection") => Some(tile_liked(app)),
        Some((_, id)) if context.is_some_and(|u| u.starts_with("spotify:playlist:")) => app
            .known_playlist(id)
            .map(|playlist| tile_playlist(app, playlist, tile_art)),
        Some((uri, id)) if uri.starts_with("spotify:album:") => app
            .known_album(id)
            .or(track.album.as_ref().filter(|album| album.id == id))
            .map(|album| Tile {
                image: pick_image(&album.images, tile_art).map(str::to_string),
                name: album.name.clone(),
                page: Page::Album(album.id.clone()),
                uri: Some(uri.to_string()),
                liked: false,
                owned_playlist: None,
                song: None,
            }),
        Some((uri, id)) if uri.starts_with("spotify:artist:") => {
            app.known_artist(id).map(|artist| Tile {
                image: pick_image(&artist.images, tile_art).map(str::to_string),
                name: artist.name.clone(),
                page: Page::Artist(artist.id.clone()),
                uri: Some(uri.to_string()),
                liked: false,
                owned_playlist: None,
                song: None,
            })
        }
        Some((uri, id)) if uri.starts_with("spotify:show:") => app.known_show(id).map(|show| Tile {
            image: pick_image(&show.images, tile_art).map(str::to_string),
            name: show.name.clone(),
            page: Page::Show(show.id.clone()),
            uri: Some(uri.to_string()),
            liked: false,
            owned_playlist: None,
            song: None,
        }),
        // A song played on its own, or from somewhere Home cannot name.
        _ => track
            .album
            .as_ref()
            .filter(|album| !album.id.is_empty())
            .map(|album| Tile {
                image: track.image(tile_art).map(str::to_string),
                name: track.name.clone(),
                page: Page::Album(album.id.clone()),
                uri: Some(track.uri.clone()),
                liked: false,
                owned_playlist: None,
                song: Some(track.clone()),
            }),
    }
}

fn quick_access_tiles(app: &App) -> Vec<Tile> {
    let tile_art = app.art_px(TILE_HEIGHT);
    let mut tiles: Vec<Tile> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for play in app.home.recently_played.get().into_iter().flatten() {
        if tiles.len() == TILES {
            break;
        }
        if let Some(tile) = tile_from_play(app, play, tile_art)
            && seen.insert(tile.page.clone())
        {
            tiles.push(tile);
        }
    }
    let library = app.library.playlists.get().into_iter().flatten();
    for tile in
        std::iter::once(tile_liked(app)).chain(library.map(|p| tile_playlist(app, p, tile_art)))
    {
        if tiles.len() == TILES {
            break;
        }
        if seen.insert(tile.page.clone()) {
            tiles.push(tile);
        }
    }
    tiles
}

const EXTRA_TILES: usize = 12;

fn extra_quick_access_tiles(app: &App, primary: &[Tile]) -> Vec<Tile> {
    let tile_art = app.art_px(TILE_HEIGHT);
    let mut tiles: Vec<Tile> = Vec::new();
    let mut seen: std::collections::HashSet<Page> =
        primary.iter().map(|tile| tile.page.clone()).collect();

    for play in app.home.recently_played.get().into_iter().flatten() {
        if tiles.len() == EXTRA_TILES {
            return tiles;
        }
        if let Some(tile) = tile_from_play(app, play, tile_art)
            && seen.insert(tile.page.clone())
        {
            tiles.push(tile);
        }
    }
    for playlist in app.library.playlists.get().into_iter().flatten() {
        if tiles.len() == EXTRA_TILES {
            return tiles;
        }
        let tile = tile_playlist(app, playlist, tile_art);
        if seen.insert(tile.page.clone()) {
            tiles.push(tile);
        }
    }
    for saved in &app.library.albums.items {
        if tiles.len() == EXTRA_TILES {
            return tiles;
        }
        let tile = tile_album(&saved.album, tile_art);
        if seen.insert(tile.page.clone()) {
            tiles.push(tile);
        }
    }
    for artist in app
        .home
        .top_artists
        .get()
        .into_iter()
        .flatten()
        .chain(app.library.artists.items.iter())
    {
        if tiles.len() == EXTRA_TILES {
            return tiles;
        }
        let tile = tile_artist(artist, tile_art);
        if seen.insert(tile.page.clone()) {
            tiles.push(tile);
        }
    }
    tiles
}

fn bento_hero(app: &mut App, ui: &mut egui::Ui) {
    let _ = bento_hero_rects(app, ui);
}

fn bento_hero_rects(app: &mut App, ui: &mut egui::Ui) -> Option<(Rect, Rect)> {
    let available = ui.available_width();
    let tiles = quick_access_tiles(app);
    let seed = resolve_live_radio(app);
    if let Some(seed) = &seed
        && app.is_connected()
        && app.network_reachable
    {
        app.load_radio(&seed.uri);
    }
    if available < 680.0 {
        quick_access(app, ui);
        if let Some(seed) = seed {
            ui.add_space(12.0);
            let _ = live_radio_card(app, ui, available, 0.0, seed);
        }
        ui.add_space(12.0);
        let _ = bento_widget_card(app, ui, available, 0.0, &tiles);
        return None;
    }

    let gap = 12.0;
    let Some(seed) = seed else {
        quick_access_grid(app, ui, &tiles, TILES, None, TILE_HEIGHT);
        ui.add_space(12.0);
        let _ = bento_widget_card(app, ui, available, 0.0, &tiles);
        return None;
    };
    let radio_w = ((available - gap) * 0.40).clamp(270.0, 420.0);
    let right_w = (available - radio_w - gap).max(260.0);
    let quick_cols = if right_w >= 500.0 { 4 } else { 2 };
    let quick_rows = (tiles.len().min(quick_cols * 2)).div_ceil(quick_cols);
    let row_gap = ui.spacing().item_spacing.y;
    let quick_h =
        (TILE_HEIGHT * quick_rows as f32) + (quick_rows.saturating_sub(1) as f32 * row_gap);
    let widget_h = 258.0;
    let min_radio_h = quick_h + 12.0 + widget_h;

    let rects = ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = gap;
        let radio_rect = ui
            .allocate_ui_with_layout(
                vec2(radio_w, 0.0),
                egui::Layout::top_down(egui::Align::Min),
                |ui| live_radio_card(app, ui, radio_w, min_radio_h, seed).rect,
            )
            .inner;
        let widget_rect = ui
            .allocate_ui_with_layout(
                vec2(right_w, 0.0),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    ui.scope(|ui| {
                        ui.spacing_mut().item_spacing.y = row_gap;
                        quick_access_grid(
                            app,
                            ui,
                            &tiles,
                            quick_cols * 2,
                            Some(quick_cols),
                            TILE_HEIGHT,
                        );
                    });
                    ui.add_space(12.0);
                    bento_widget_card(app, ui, right_w, widget_h, &tiles).rect
                },
            )
            .inner;
        (radio_rect, widget_rect)
    });
    Some(rects.inner)
}

struct RadioNextRow {
    title: String,
    subtitle: String,
    uri: String,
    image: Option<String>,
    episode_resume_ms: Option<u32>,
    queue_index: Option<usize>,
}

struct LiveRadioSeed {
    uri: String,
    station_name: String,
    subtitle: String,
    tags: Vec<String>,
    image: Option<String>,
    track: Option<crate::api::models::Track>,
    up_next: Vec<RadioNextRow>,
}

const LIVE_RADIO_UP_NEXT_LIMIT: usize = 4;

fn push_unique_radio_row(
    rows: &mut Vec<RadioNextRow>,
    seen: &mut std::collections::HashSet<String>,
    seed_uri: &str,
    row: RadioNextRow,
) {
    if rows.len() >= LIVE_RADIO_UP_NEXT_LIMIT
        || row.uri.is_empty()
        || row.uri == seed_uri
        || !seen.insert(row.uri.clone())
    {
        return;
    }
    rows.push(row);
}

fn resolve_live_radio(app: &App) -> Option<LiveRadioSeed> {
    let art_px = app.art_px(72.0);
    let row_art = app.art_px(36.0);
    let locale = app.locale;

    let fallback_track = || {
        app.home
            .recently_played
            .get()
            .and_then(|list| list.first().map(|p| p.track.clone()))
            .or_else(|| {
                app.home
                    .top_tracks
                    .get()
                    .and_then(|list| list.first().cloned())
            })
            .or_else(|| {
                app.home
                    .recommendations
                    .get()
                    .and_then(|list| list.first().cloned())
            })
    };

    let (uri, station_name, subtitle, image, track) = if let Some(now) = app
        .now_playing()
        .filter(|now| !now.is_episode && !now.uri.is_empty())
    {
        let uri = now.uri.clone();
        let name = app
            .radio_name(&uri)
            .unwrap_or_else(|| gettext(locale, "{track} Radio").replace("{track}", &now.title));
        let matched_track = app
            .home
            .recently_played
            .get()
            .into_iter()
            .flatten()
            .map(|play| &play.track)
            .chain(app.home.top_tracks.get().into_iter().flatten())
            .chain(app.home.recommendations.get().into_iter().flatten())
            .find(|track| track.uri == uri)
            .cloned();
        let img = now.art_url.clone().or_else(|| {
            app.radio_images(&uri)
                .first()
                .map(|image| image.url.clone())
        });
        (uri, name, now.subtitle, img, matched_track)
    } else {
        let track = fallback_track()?;
        let uri = track.uri.clone();
        let name = app
            .radio_name(&uri)
            .unwrap_or_else(|| gettext(locale, "{track} Radio").replace("{track}", &track.name));
        let img = track.image(art_px).map(str::to_string);
        let subtitle = track.artist_names();
        (uri, name, subtitle, img, Some(track))
    };

    let mut tags = Vec::new();
    let artist_id = track
        .as_ref()
        .and_then(|t| t.artists.first())
        .and_then(|a| a.id.clone())
        .or_else(|| {
            app.now_playing()
                .and_then(|n| n.artists.first().and_then(|a| a.id.clone()))
        });
    if let Some(id) = artist_id.as_deref()
        && let Some(artist) = app.artist_pages.get(id).and_then(|p| p.artist.get())
    {
        for genre in artist.genres.iter().take(2) {
            if !genre.is_empty() {
                tags.push(genre.clone());
            }
        }
    }
    if let Some(year) = track
        .as_ref()
        .and_then(|t| t.album.as_ref())
        .and_then(|a| a.year())
        .filter(|y| !y.is_empty())
    {
        tags.push(year.to_string());
    }
    if track.as_ref().is_some_and(|t| t.explicit) {
        tags.push(gettext(locale, "Explicit").into_owned());
    }

    let mut up_next = Vec::with_capacity(LIVE_RADIO_UP_NEXT_LIMIT);
    let mut seen = std::collections::HashSet::new();
    if let Some(queue) = app.queue.get() {
        for (queue_idx, item) in queue.queue.iter().enumerate() {
            let resume_ms = match item {
                PlayableItem::Episode(ep) => ep.resume_ms(),
                PlayableItem::Track(_) => None,
            };
            push_unique_radio_row(
                &mut up_next,
                &mut seen,
                &uri,
                RadioNextRow {
                    title: item.name().to_string(),
                    subtitle: item.subtitle(),
                    uri: item.uri().to_string(),
                    image: item.image(row_art).map(str::to_string),
                    episode_resume_ms: resume_ms,
                    queue_index: Some(queue_idx),
                },
            );
        }
    }
    if let Some(songs) = app.radio_pages.get(&uri).and_then(|page| page.songs.get()) {
        for track in songs {
            push_unique_radio_row(
                &mut up_next,
                &mut seen,
                &uri,
                RadioNextRow {
                    title: track.name.clone(),
                    subtitle: track.artist_names(),
                    uri: track.uri.clone(),
                    image: track.image(row_art).map(str::to_string),
                    episode_resume_ms: None,
                    queue_index: None,
                },
            );
        }
    }
    for track in app
        .home
        .recommendations
        .get()
        .into_iter()
        .flatten()
        .chain(app.home.top_tracks.get().into_iter().flatten())
        .chain(
            app.home
                .recently_played
                .get()
                .into_iter()
                .flatten()
                .map(|p| &p.track),
        )
    {
        push_unique_radio_row(
            &mut up_next,
            &mut seen,
            &uri,
            RadioNextRow {
                title: track.name.clone(),
                subtitle: track.artist_names(),
                uri: track.uri.clone(),
                image: track.image(row_art).map(str::to_string),
                episode_resume_ms: None,
                queue_index: None,
            },
        );
    }

    Some(LiveRadioSeed {
        uri,
        station_name,
        subtitle,
        tags,
        image,
        track,
        up_next,
    })
}

fn live_radio_card(
    app: &mut App,
    ui: &mut egui::Ui,
    width: f32,
    min_height: f32,
    seed: LiveRadioSeed,
) -> egui::Response {
    let palette = app.palette;
    let locale = app.locale;
    let pct = app.settings.tint_intensity.percent();
    let target_fill = if pct == 0 {
        palette.surface
    } else {
        match app.resolved_now_playing_tint() {
            Some(tint) => theme::Palette::blend_color(
                palette.surface,
                tint,
                if palette.dark {
                    theme::tint_curve(pct, 0.22, 0.36, 0.40)
                } else {
                    theme::tint_curve(pct, 0.12, 0.18, 0.22)
                },
            ),
            None => palette.surface,
        }
    };
    let fill = theme::ease_color(
        ui.ctx(),
        egui::Id::new(("live-radio-fill", palette.dark)),
        target_fill,
        theme::ART_EASE_SECONDS,
    );
    let border = if pct > 0
        && let Some(tint) = app.resolved_now_playing_tint()
    {
        theme::Palette::blend_color(
            palette.outline,
            tint,
            theme::tint_curve(pct, 0.10, 0.20, 0.26),
        )
    } else {
        palette.outline
    };

    let station = crate::util::station_uri(&seed.uri);
    let active_station = station
        .as_deref()
        .is_some_and(|st| app.playing_context_uri().as_deref() == Some(st))
        || app.active_home_radio_seed.as_deref() == Some(seed.uri.as_str())
        || app
            .now_playing()
            .is_some_and(|now| !now.is_episode && now.uri == seed.uri);
    let playing_here = active_station && app.believed_playing();

    egui::Frame::new()
        .fill(fill)
        .stroke(egui::Stroke::new(1.0, border))
        .corner_radius(CornerRadius::same(theme::FLOATING_RADIUS))
        .inner_margin(egui::Margin::same(16))
        .show(ui, |ui| {
            ui.set_width((width - 34.0).max(200.0));
            ui.spacing_mut().item_spacing.y = 0.0;
            if min_height > 34.0 {
                ui.set_min_height((min_height - 34.0).max(160.0));
            }
            ui.allocate_ui_with_layout(
                vec2(ui.available_width(), 18.0),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    let (dot_rect, _) = ui.allocate_exact_size(vec2(8.0, 14.0), Sense::hover());
                    ui.painter().circle_filled(
                        dot_rect.center(),
                        3.5,
                        if playing_here {
                            palette.accent
                        } else {
                            palette.accent.gamma_multiply(0.8)
                        },
                    );
                    theme::text(
                        ui,
                        &*pgettext(locale, "home", "Live radio"),
                        theme::bold(11.5),
                        palette.accent,
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if theme::link(
                            ui,
                            &*gettext(locale, "Go to song radio"),
                            theme::medium(11.5),
                            palette.secondary,
                        )
                        .clicked()
                        {
                            if let Some(track) = &seed.track {
                                app.actions.push(Action::OpenSongRadio {
                                    uri: seed.uri.clone(),
                                    track: Box::new(track.clone()),
                                });
                            } else {
                                app.actions
                                    .push(Action::Open(Page::Radio(seed.uri.clone())));
                            }
                        }
                    });
                },
            );
            ui.add_space(10.0);

            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 12.0;
                let (cover, _) = ui.allocate_exact_size(Vec2::splat(68.0), Sense::hover());
                widgets::paint_cover(
                    ui,
                    &palette,
                    seed.image.as_deref(),
                    cover,
                    10.0,
                    Icon::Radio,
                    Some(app.backend.art()),
                );
                let play_size = 40.0;
                let text_w = (ui.available_width() - play_size - 12.0).max(60.0);
                ui.allocate_ui_with_layout(
                    vec2(text_w, 68.0),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.add_space(10.0);
                        let (title_rect, title_resp) =
                            ui.allocate_exact_size(vec2(text_w, 24.0), Sense::hover());
                        crate::bidi::paint_line(
                            &ui.painter().with_clip_rect(title_rect),
                            title_rect.left(),
                            title_rect.right(),
                            title_rect.center().y,
                            &seed.station_name,
                            theme::bold(17.0),
                            palette.text,
                        );
                        title_resp.on_hover_text(&seed.station_name);
                        ui.add_space(4.0);
                        if !seed.subtitle.is_empty() {
                            let (sub_rect, _) =
                                ui.allocate_exact_size(vec2(text_w, 16.0), Sense::hover());
                            crate::bidi::paint_line(
                                &ui.painter().with_clip_rect(sub_rect),
                                sub_rect.left(),
                                sub_rect.right(),
                                sub_rect.center().y,
                                &seed.subtitle,
                                theme::regular(12.5),
                                palette.secondary,
                            );
                        }
                    },
                );
                if theme::circle_button(
                    ui,
                    if playing_here {
                        Icon::PauseFilled
                    } else {
                        Icon::PlayFilled
                    },
                    play_size,
                    palette.accent,
                    palette.accent_hover,
                    palette.on_accent,
                    &gettext(locale, if playing_here { "Pause" } else { "Play" }),
                )
                .clicked()
                {
                    if active_station {
                        app.actions.push(Action::TogglePlay);
                    } else {
                        app.actions.push(Action::PlayHomeRadio {
                            seed: seed.uri.clone(),
                            start_uri: None,
                        });
                    }
                }
            });
            ui.add_space(10.0);

            ui.allocate_ui_with_layout(
                vec2(ui.available_width(), 22.0),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    for tag in &seed.tags {
                        paint_meta_pill(ui, &palette, tag);
                    }
                },
            );

            if !seed.up_next.is_empty() {
                ui.add_space(8.0);
                let (sep_rect, _) =
                    ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
                ui.painter().hline(
                    sep_rect.x_range(),
                    sep_rect.center().y,
                    egui::Stroke::new(1.0, palette.outline),
                );
                ui.add_space(8.0);
                ui.allocate_ui_with_layout(
                    vec2(ui.available_width(), 16.0),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        theme::text(
                            ui,
                            &*gettext(locale, "Next up"),
                            theme::bold(11.5),
                            palette.secondary,
                        );
                    },
                );
                ui.add_space(7.0);
                for (index, row) in seed.up_next.iter().enumerate() {
                    let row_w = ui.available_width();
                    let (row_rect, row_resp) =
                        ui.allocate_exact_size(vec2(row_w, 45.5), Sense::click());
                    if ui.is_rect_visible(row_rect) {
                        if row_resp.hovered() {
                            ui.painter().rect_filled(
                                row_rect,
                                CornerRadius::same(8),
                                palette.surface_hover.gamma_multiply(0.75),
                            );
                        }
                        let thumb = Rect::from_min_size(
                            pos2(row_rect.left() + 4.0, row_rect.center().y - 16.0),
                            Vec2::splat(32.0),
                        );
                        widgets::paint_cover(
                            ui,
                            &palette,
                            row.image.as_deref(),
                            thumb,
                            6.0,
                            Icon::Music,
                            Some(app.backend.art()),
                        );
                        let play_icon_w = if row_resp.hovered() { 24.0 } else { 8.0 };
                        let tr = Rect::from_min_max(
                            pos2(thumb.right() + 10.0, row_rect.top()),
                            pos2(row_rect.right() - play_icon_w, row_rect.bottom()),
                        );
                        crate::bidi::paint_line(
                            &ui.painter().with_clip_rect(tr),
                            tr.left(),
                            tr.right(),
                            row_rect.center().y - 7.0,
                            &row.title,
                            theme::medium(12.5),
                            palette.text,
                        );
                        if !row.subtitle.is_empty() {
                            crate::bidi::paint_line(
                                &ui.painter().with_clip_rect(tr),
                                tr.left(),
                                tr.right(),
                                row_rect.center().y + 7.5,
                                &row.subtitle,
                                theme::regular(11.0),
                                palette.secondary,
                            );
                        }
                        if row_resp.hovered() {
                            Icon::PlayFilled.image(palette.accent, 13.0).paint_at(
                                ui,
                                Rect::from_center_size(
                                    pos2(row_rect.right() - 12.0, row_rect.center().y),
                                    Vec2::splat(13.0),
                                ),
                            );
                        }
                    }
                    theme::focus_ring(ui, &row_resp);
                    if row_resp.clicked() {
                        if let Some(queue_idx) = row.queue_index {
                            app.actions.push(Action::PlayFromRow {
                                context: RowContext::Queue,
                                uri: row.uri.clone(),
                                index: queue_idx as u32,
                            });
                        } else if let Some(resume_ms) = row.episode_resume_ms {
                            app.actions.push(Action::PlayEpisode {
                                uri: row.uri.clone(),
                                resume_ms: Some(resume_ms),
                            });
                        } else if crate::util::uri_kind(&row.uri) == Some("episode") {
                            app.actions.push(Action::PlayEpisode {
                                uri: row.uri.clone(),
                                resume_ms: None,
                            });
                        } else {
                            app.actions.push(Action::PlayHomeRadio {
                                seed: seed.uri.clone(),
                                start_uri: Some(row.uri.clone()),
                            });
                        }
                    }
                    if index + 1 < seed.up_next.len() {
                        ui.add_space(4.0);
                    }
                }
            }
        })
        .response
}

fn tile_podcast_progress(podcasts: &[(Show, Vec<Episode>)], uri: Option<&str>) -> Option<f32> {
    let uri = uri?;
    for (show, episodes) in podcasts {
        if show.uri == uri
            && let Some(episode) = episodes.iter().find(|ep| ep.resume_ms().is_some())
            && let Some(resume_ms) = episode.resume_ms()
            && episode.duration_ms > 0
        {
            return Some((resume_ms as f32 / episode.duration_ms as f32).clamp(0.04, 0.99));
        }
        if let Some(episode) = episodes.iter().find(|ep| ep.uri == uri)
            && let Some(resume_ms) = episode.resume_ms()
            && episode.duration_ms > 0
        {
            return Some((resume_ms as f32 / episode.duration_ms as f32).clamp(0.04, 0.99));
        }
    }
    None
}

fn paint_meta_pill(ui: &mut egui::Ui, palette: &theme::Palette, label: &str) {
    let font = theme::medium(11.0);
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_owned(), font, palette.secondary);
    let w = galley.size().x + 14.0;
    let h = 20.0;
    let (rect, _) = ui.allocate_exact_size(vec2(w, h), Sense::hover());
    if ui.is_rect_visible(rect) {
        ui.painter().rect_filled(
            rect,
            CornerRadius::same(10),
            palette.surface_hover.gamma_multiply(0.7),
        );
        ui.painter().rect_stroke(
            rect,
            CornerRadius::same(10),
            egui::Stroke::new(1.0, palette.outline.gamma_multiply(0.6)),
            egui::StrokeKind::Inside,
        );
        ui.painter().galley(
            pos2(rect.left() + 7.0, rect.center().y - galley.size().y / 2.0),
            galley,
            palette.secondary,
        );
    }
}

fn bento_widget_icon(mode: BentoWidgetMode) -> Icon {
    match mode {
        BentoWidgetMode::PeakAndFriends => Icon::TrendingUp,
        BentoWidgetMode::Pulse => Icon::Sparkles,
        BentoWidgetMode::CurrentArtist => Icon::Radio,
        BentoWidgetMode::ExtraQuickAccess => Icon::Music,
    }
}

fn bento_cycle_mode(current: BentoWidgetMode, step: isize) -> BentoWidgetMode {
    let count = BentoWidgetMode::ALL.len() as isize;
    let next = (current.index() as isize + step).rem_euclid(count) as usize;
    BentoWidgetMode::ALL[next]
}

fn bento_widget_card(
    app: &mut App,
    ui: &mut egui::Ui,
    width: f32,
    min_height: f32,
    primary_tiles: &[Tile],
) -> egui::Response {
    let palette = app.palette;
    let locale = app.locale;
    let mode = app.settings.home.bento_widget;
    let inner_w = (width - 30.0).max(200.0);

    egui::Frame::new()
        .fill(palette.surface)
        .stroke(egui::Stroke::new(1.0, palette.outline))
        .corner_radius(CornerRadius::same(theme::FLOATING_RADIUS))
        .inner_margin(egui::Margin::same(14))
        .show(ui, |ui| {
            ui.set_width(inner_w);
            ui.spacing_mut().item_spacing.y = 0.0;
            let inner_h = if min_height > 30.0 {
                (min_height - 30.0).max(150.0)
            } else {
                228.0
            };
            ui.set_min_height(inner_h);
            ui.allocate_ui_with_layout(
                vec2(inner_w, 20.0),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                theme::icon(ui, bento_widget_icon(mode), 15.0, palette.accent);
                theme::text(
                    ui,
                    mode.title(locale).as_ref(),
                    theme::bold(14.0),
                    palette.text,
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let dots_resp = ui
                        .horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 4.0;
                            for candidate in BentoWidgetMode::ALL {
                                let active = candidate == mode;
                                let (rect, resp) =
                                    ui.allocate_exact_size(vec2(14.0, 14.0), Sense::click());
                                if ui.is_rect_visible(rect) {
                                    let dot_color = if active {
                                        palette.accent
                                    } else if resp.hovered() {
                                        palette.text
                                    } else {
                                        palette.secondary.gamma_multiply(0.45)
                                    };
                                    ui.painter().circle_filled(
                                        rect.center(),
                                        if active { 4.0 } else { 3.0 },
                                        dot_color,
                                    );
                                }
                                let resp = resp
                                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                                    .on_hover_text(candidate.title(locale));
                                if resp.clicked() && !active {
                                    app.actions.push(Action::SetHomeBentoWidget(candidate));
                                }
                            }
                        })
                        .response;
                    if dots_resp.hovered() {
                        let dy = ui.input(|i| i.smooth_scroll_delta.y);
                        if dy.abs() >= 6.0 {
                            let step = if dy > 0.0 { -1 } else { 1 };
                            app.actions
                                .push(Action::SetHomeBentoWidget(bento_cycle_mode(mode, step)));
                        }
                    }
                });
            });
            ui.add_space(10.0);
            let body_h = (inner_h - 30.0).max(120.0);
            ui.allocate_ui_with_layout(
                vec2(inner_w, body_h),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    match mode {
                        BentoWidgetMode::PeakAndFriends => {
                            bento_peak_and_friends(app, ui, inner_w);
                        }
                        BentoWidgetMode::Pulse => bento_pulse(app, ui),
                        BentoWidgetMode::CurrentArtist => bento_current_artist(app, ui),
                        BentoWidgetMode::ExtraQuickAccess => {
                            bento_extra_quick_access(app, ui, primary_tiles, inner_w);
                        }
                    }
                },
            );
        })
        .response
}

fn shuffle_uris_copy(mut uris: Vec<String>) -> Vec<String> {
    let mut seed = jiff::Timestamp::now().as_nanosecond() as u64;
    if seed == 0 {
        seed = 0x9e37_79b9_7f4a_7c15;
    }
    for i in (1..uris.len()).rev() {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        let j = (seed as usize) % (i + 1);
        uris.swap(i, j);
    }
    uris
}

fn bento_peak_and_friends(app: &mut App, ui: &mut egui::Ui, width: f32) {
    app.ensure_home_friends();
    let palette = app.palette;
    let locale = app.locale;
    let two_cols = width >= 330.0;
    let col_gap = 12.0;
    let left_w = if two_cols {
        ((width - col_gap) * 0.52).max(130.0)
    } else {
        width
    };
    let right_w = if two_cols {
        (width - left_w - col_gap).max(120.0)
    } else {
        width
    };

    let render_top_songs = |app: &mut App, ui: &mut egui::Ui, w: f32| {
        ui.allocate_ui_with_layout(
            vec2(w, 26.0),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                if theme::link(
                    ui,
                    &*gettext(locale, "Your top songs"),
                    theme::semibold(12.0),
                    palette.secondary,
                )
                .clicked()
                {
                    app.actions.push(Action::Open(Page::TopSongs));
                }
                if let Some(tracks) = app.home.top_tracks.get().filter(|t| !t.is_empty()) {
                    let uris: Vec<String> = tracks.iter().map(|t| t.uri.clone()).collect();
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if theme::icon_button(
                            ui,
                            Icon::Shuffle,
                            13.0,
                            palette.secondary,
                            palette.accent,
                            &gettext(locale, "Shuffle"),
                        )
                        .clicked()
                        {
                            app.actions.push(Action::PlayUris {
                                uris: shuffle_uris_copy(uris),
                                index: 0,
                            });
                        }
                    });
                }
            },
        );
        ui.add_space(6.0);
        let tracks: Vec<_> = app
            .home
            .top_tracks
            .get()
            .map_or(&[][..], Vec::as_slice)
            .iter()
            .take(4)
            .cloned()
            .collect();
        if tracks.is_empty() {
            widgets::loading_row(ui, &palette, locale);
            return;
        }
        let all_uris: Vec<String> = app
            .home
            .top_tracks
            .get()
            .map_or(Vec::new(), |t| t.iter().map(|tr| tr.uri.clone()).collect());
        let art_px = app.art_px(32.0);
        for (idx, track) in tracks.iter().enumerate() {
            let plays = app.track_play_count(&track.uri);
            let (rect, resp) = ui.allocate_exact_size(vec2(w, 38.5), Sense::click());
            if ui.is_rect_visible(rect) {
                if resp.hovered() {
                    ui.painter().rect_filled(
                        rect,
                        CornerRadius::same(8),
                        palette.surface_hover.gamma_multiply(0.75),
                    );
                }
                let rank_str = format!("{}", idx + 1);
                let rank_rect =
                    Rect::from_min_size(pos2(rect.left() + 4.0, rect.top()), vec2(16.0, 38.5));
                crate::bidi::paint_line(
                    ui.painter(),
                    rank_rect.left(),
                    rank_rect.right(),
                    rank_rect.center().y,
                    &rank_str,
                    theme::bold(12.0),
                    palette.accent,
                );
                let thumb = Rect::from_min_size(
                    pos2(rank_rect.right() + 2.0, rect.center().y - 15.0),
                    Vec2::splat(30.0),
                );
                widgets::paint_cover(
                    ui,
                    &palette,
                    track.image(art_px),
                    thumb,
                    6.0,
                    Icon::Music,
                    Some(app.backend.art()),
                );
                let badge_text = (plays > 0).then(|| format!("{}x", plays));
                let badge_w = if badge_text.is_some() { 34.0 } else { 4.0 };
                let text_rect = Rect::from_min_max(
                    pos2(thumb.right() + 8.0, rect.top()),
                    pos2(rect.right() - badge_w, rect.bottom()),
                );
                crate::bidi::paint_line(
                    &ui.painter().with_clip_rect(text_rect),
                    text_rect.left(),
                    text_rect.right(),
                    rect.center().y - 7.0,
                    &track.name,
                    theme::medium(12.0),
                    palette.text,
                );
                crate::bidi::paint_line(
                    &ui.painter().with_clip_rect(text_rect),
                    text_rect.left(),
                    text_rect.right(),
                    rect.center().y + 7.0,
                    &track.artist_names(),
                    theme::regular(11.0),
                    palette.secondary,
                );
                if let Some(badge) = &badge_text {
                    crate::bidi::paint_line(
                        ui.painter(),
                        rect.right() - 32.0,
                        rect.right() - 4.0,
                        rect.center().y,
                        badge,
                        theme::semibold(10.5),
                        palette.secondary,
                    );
                }
            }
            if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                app.actions.push(Action::PlayUris {
                    uris: all_uris.clone(),
                    index: idx as u32,
                });
            }
            if idx + 1 < tracks.len() {
                ui.add_space(4.0);
            }
        }
    };

    let render_friends_or_releases = |app: &mut App, ui: &mut egui::Ui, w: f32| {
        let friends: Vec<_> = app
            .friends
            .get()
            .map_or(&[][..], Vec::as_slice)
            .iter()
            .take(4)
            .cloned()
            .collect();
        if !friends.is_empty() {
            ui.allocate_ui_with_layout(
                vec2(w, 26.0),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    if theme::link(
                        ui,
                        &*gettext(locale, "Listening activity"),
                        theme::semibold(12.0),
                        palette.secondary,
                    )
                    .clicked()
                    {
                        app.actions.push(Action::ToggleFriendsPanel);
                    }
                },
            );
            ui.add_space(6.0);
            for (idx, friend) in friends.iter().enumerate() {
                let (rect, resp) = ui.allocate_exact_size(vec2(w, 38.5), Sense::click());
                if ui.is_rect_visible(rect) {
                    if resp.hovered() {
                        ui.painter().rect_filled(
                            rect,
                            CornerRadius::same(8),
                            palette.surface_hover.gamma_multiply(0.75),
                        );
                    }
                    let avatar = Rect::from_min_size(
                        pos2(rect.left() + 4.0, rect.center().y - 15.0),
                        Vec2::splat(30.0),
                    );
                    widgets::paint_cover(
                        ui,
                        &palette,
                        friend.image_url.as_deref(),
                        avatar,
                        15.0,
                        Icon::User,
                        Some(app.backend.art()),
                    );
                    if friend.playing {
                        let dot_pos = avatar.right_top() + vec2(-3.0, 3.0);
                        ui.painter().circle_filled(dot_pos, 5.0, palette.surface);
                        ui.painter()
                            .circle_filled(dot_pos, 3.5, egui::Color32::from_rgb(66, 133, 244));
                    }
                    let tr = Rect::from_min_max(
                        pos2(avatar.right() + 8.0, rect.top()),
                        pos2(rect.right() - 4.0, rect.bottom()),
                    );
                    crate::bidi::paint_line(
                        &ui.painter().with_clip_rect(tr),
                        tr.left(),
                        tr.right(),
                        rect.center().y - 7.0,
                        &friend.user_name,
                        theme::medium(12.0),
                        palette.text,
                    );
                    let sub = if friend.artist_name.is_empty() {
                        friend.track_name.clone()
                    } else {
                        format!("{} • {}", friend.track_name, friend.artist_name)
                    };
                    crate::bidi::paint_line(
                        &ui.painter().with_clip_rect(tr),
                        tr.left(),
                        tr.right(),
                        rect.center().y + 7.0,
                        &sub,
                        theme::regular(11.0),
                        palette.secondary,
                    );
                }
                let hint = gettext(locale, "Play {song}").replace("{song}", &friend.track_name);
                if resp
                    .clone()
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .on_hover_text(hint)
                    .clicked()
                    && !friend.track_uri.is_empty()
                {
                    app.actions.push(Action::PlayUris {
                        uris: vec![friend.track_uri.clone()],
                        index: 0,
                    });
                }
                if let Some(user_id) = crate::util::uri_id(&friend.user_uri) {
                    let user_id = user_id.to_string();
                    egui::Popup::context_menu(&resp)
                        .id(ui.make_persistent_id(("home-friend-menu", &friend.user_uri)))
                        .frame(widgets::menu_frame(&palette))
                        .show(|ui| {
                            if widgets::menu_item(
                                ui,
                                &palette,
                                Some(Icon::User),
                                &gettext(locale, "Profile"),
                            ) {
                                app.actions.push(Action::Open(Page::UserProfile(user_id)));
                            }
                        });
                }
                if idx + 1 < friends.len() {
                    ui.add_space(4.0);
                }
            }
            return;
        }

        ui.allocate_ui_with_layout(
            vec2(w, 26.0),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                theme::text(
                    ui,
                    &*gettext(locale, "New releases"),
                    theme::semibold(12.0),
                    palette.secondary,
                );
            },
        );
        ui.add_space(6.0);
        let releases: Vec<_> = app
            .home
            .new_releases
            .get()
            .map_or(&[][..], Vec::as_slice)
            .iter()
            .take(4)
            .cloned()
            .collect();
        if releases.is_empty() {
            widgets::loading_row(ui, &palette, locale);
            return;
        }
        let art_px = app.art_px(32.0);
        for (idx, album) in releases.iter().enumerate() {
            let (rect, resp) = ui.allocate_exact_size(vec2(w, 38.5), Sense::click());
            if ui.is_rect_visible(rect) {
                if resp.hovered() {
                    ui.painter().rect_filled(
                        rect,
                        CornerRadius::same(8),
                        palette.surface_hover.gamma_multiply(0.75),
                    );
                }
                let thumb = Rect::from_min_size(
                    pos2(rect.left() + 4.0, rect.center().y - 15.0),
                    Vec2::splat(30.0),
                );
                widgets::paint_cover(
                    ui,
                    &palette,
                    pick_image(&album.images, art_px),
                    thumb,
                    6.0,
                    Icon::Music,
                    Some(app.backend.art()),
                );
                let tr = Rect::from_min_max(
                    pos2(thumb.right() + 8.0, rect.top()),
                    pos2(rect.right() - 4.0, rect.bottom()),
                );
                crate::bidi::paint_line(
                    &ui.painter().with_clip_rect(tr),
                    tr.left(),
                    tr.right(),
                    rect.center().y - 7.0,
                    &album.name,
                    theme::medium(12.0),
                    palette.text,
                );
                let album_artists: String = album
                    .artists
                    .iter()
                    .map(|a| a.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                crate::bidi::paint_line(
                    &ui.painter().with_clip_rect(tr),
                    tr.left(),
                    tr.right(),
                    rect.center().y + 7.0,
                    &album_artists,
                    theme::regular(11.0),
                    palette.secondary,
                );
            }
            if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                app.actions.push(Action::Open(Page::Album(album.id.clone())));
            }
            if idx + 1 < releases.len() {
                ui.add_space(4.0);
            }
        }
    };

    if two_cols {
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = col_gap;
            ui.allocate_ui_with_layout(
                vec2(left_w, 0.0),
                egui::Layout::top_down(egui::Align::Min),
                |ui| render_top_songs(app, ui, left_w),
            );
            ui.allocate_ui_with_layout(
                vec2(right_w, 0.0),
                egui::Layout::top_down(egui::Align::Min),
                |ui| render_friends_or_releases(app, ui, right_w),
            );
        });
    } else {
        render_top_songs(app, ui, left_w);
        ui.add_space(10.0);
        render_friends_or_releases(app, ui, right_w);
    }
}

fn bento_pulse(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    let locale = app.locale;
    let today = jiff::Zoned::now().date();
    let (today_count, today_ms) = app.today_play_stats(today);
    let pulse_range = app.settings.home.pulse_range;
    let (second_label, second_count, second_detail, second_tooltip) = match pulse_range {
        crate::settings::PulseRange::ThisWeek => {
            let (week_count, week_ms) = app.week_play_stats(today);
            (
                gettext(locale, "This week"),
                week_count,
                if week_ms > 0 {
                    crate::util::format_total_ms(locale, week_ms)
                } else {
                    gettext(locale, "Plays").into_owned()
                },
                gettext(locale, "Monday to Sunday. Click to show all time."),
            )
        }
        crate::settings::PulseRange::AllTime => (
            gettext(locale, "All time (this device)"),
            app.plays.plays().len(),
            gettext(locale, "Plays").into_owned(),
            gettext(locale, "Click to show this week."),
        ),
    };
    let w = ui.available_width();
    let tile_w = ((w - 10.0) / 2.0).max(110.0);

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        let stats = [
            (
                gettext(locale, "Today (all songs)"),
                format!("{}", today_count),
                if today_ms > 0 {
                    crate::util::format_total_ms(locale, today_ms)
                } else {
                    gettext(locale, "Plays").into_owned()
                },
                None,
            ),
            (
                second_label,
                format!("{}", second_count),
                second_detail,
                Some(second_tooltip),
            ),
        ];
        for (label, value, detail, toggle_hint) in stats {
            let clickable = toggle_hint.is_some();
            let (rect, resp) = ui.allocate_exact_size(
                vec2(tile_w, 64.0),
                if clickable {
                    Sense::click()
                } else {
                    Sense::hover()
                },
            );
            if ui.is_rect_visible(rect) {
                let bg_alpha = if clickable && resp.hovered() { 0.85 } else { 0.5 };
                ui.painter().rect_filled(
                    rect,
                    CornerRadius::same(10),
                    palette.surface_hover.gamma_multiply(bg_alpha),
                );
                ui.painter().rect_stroke(
                    rect,
                    CornerRadius::same(10),
                    egui::Stroke::new(
                        1.0,
                        if clickable && resp.hovered() {
                            palette.accent.gamma_multiply(0.7)
                        } else {
                            palette.outline.gamma_multiply(0.6)
                        },
                    ),
                    egui::StrokeKind::Inside,
                );
                let left_x = rect.left() + 12.0;
                let right_x = rect.right() - if clickable { 26.0 } else { 12.0 };
                let label_clip = Rect::from_min_max(
                    pos2(left_x, rect.top()),
                    pos2(right_x.max(left_x + 20.0), rect.top() + 30.0),
                );
                crate::bidi::paint_line(
                    &ui.painter().with_clip_rect(label_clip),
                    label_clip.left(),
                    label_clip.right(),
                    rect.top() + 18.0,
                    label.as_ref(),
                    theme::medium(11.5),
                    palette.secondary,
                );
                if clickable {
                    let active_week = pulse_range == crate::settings::PulseRange::ThisWeek;
                    let d1_pos = pos2(rect.right() - 18.0, rect.top() + 18.0);
                    let d2_pos = pos2(rect.right() - 11.0, rect.top() + 18.0);
                    ui.painter().circle_filled(
                        d1_pos,
                        if active_week { 2.8 } else { 2.0 },
                        if active_week {
                            palette.accent
                        } else {
                            palette.secondary.gamma_multiply(0.45)
                        },
                    );
                    ui.painter().circle_filled(
                        d2_pos,
                        if active_week { 2.0 } else { 2.8 },
                        if active_week {
                            palette.secondary.gamma_multiply(0.45)
                        } else {
                            palette.accent
                        },
                    );
                }
                let val_galley =
                    ui.painter()
                        .layout_no_wrap(value, theme::bold(20.0), palette.accent);
                let val_w = val_galley.size().x;
                let val_y = rect.top() + 43.0 - val_galley.size().y * 0.5;
                ui.painter()
                    .galley(pos2(left_x, val_y), val_galley, palette.accent);
                let detail_left = left_x + val_w + 6.0;
                if detail_left < rect.right() - 12.0 {
                    let detail_clip = Rect::from_min_max(
                        pos2(detail_left, rect.top() + 28.0),
                        pos2(rect.right() - 10.0, rect.bottom()),
                    );
                    crate::bidi::paint_line(
                        &ui.painter().with_clip_rect(detail_clip),
                        detail_clip.left(),
                        detail_clip.right(),
                        rect.top() + 44.0,
                        &detail,
                        theme::regular(11.5),
                        palette.secondary,
                    );
                }
            }
            if let Some(hint) = toggle_hint {
                let resp = resp
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .on_hover_text(hint.as_ref());
                if resp.clicked() {
                    app.actions
                        .push(Action::SetHomePulseRange(pulse_range.toggled()));
                }
            }
        }
    });

    ui.add_space(10.0);
    let cols = if w >= 360.0 { 3 } else { 2 };
    let col_gap = 8.0;
    let cell_w = ((w - col_gap * (cols as f32 - 1.0)) / cols as f32).max(96.0);
    let top_artists: Vec<_> = app
        .home
        .top_artists
        .get()
        .map_or(&[][..], Vec::as_slice)
        .iter()
        .take(cols)
        .cloned()
        .collect();
    if !top_artists.is_empty() {
        let art_px = app.art_px(32.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = col_gap;
            for (rank, top_artist) in top_artists.into_iter().enumerate() {
                app.ensure_artist_preview(&top_artist.id);
                let sub = app
                    .artist_pages
                    .get(&top_artist.id)
                    .and_then(|p| p.artist.get())
                    .and_then(|a| a.genres.first().cloned())
                    .or_else(|| top_artist.genres.first().cloned())
                    .unwrap_or_else(|| gettext(locale, "Top artists").into_owned());
                let (rect, resp) = ui.allocate_exact_size(vec2(cell_w, 57.0), Sense::click());
                if ui.is_rect_visible(rect) {
                    ui.painter().rect_filled(
                        rect,
                        CornerRadius::same(8),
                        palette
                            .surface_hover
                            .gamma_multiply(if resp.hovered() { 0.85 } else { 0.35 }),
                    );
                    let avatar = Rect::from_min_size(
                        pos2(rect.left() + 8.0, rect.center().y - 16.0),
                        Vec2::splat(32.0),
                    );
                    widgets::paint_cover(
                        ui,
                        &palette,
                        pick_image(&top_artist.images, art_px),
                        avatar,
                        16.0,
                        Icon::User,
                        Some(app.backend.art()),
                    );
                    let tr = Rect::from_min_max(
                        pos2(avatar.right() + 8.0, rect.top() + 4.0),
                        pos2(rect.right() - 6.0, rect.bottom() - 4.0),
                    );
                    let ranked_name = format!("#{} {}", rank + 1, top_artist.name);
                    crate::bidi::paint_line(
                        &ui.painter().with_clip_rect(tr),
                        tr.left(),
                        tr.right(),
                        rect.center().y - 7.5,
                        &ranked_name,
                        theme::bold(12.5),
                        palette.text,
                    );
                    crate::bidi::paint_line(
                        &ui.painter().with_clip_rect(tr),
                        tr.left(),
                        tr.right(),
                        rect.center().y + 7.5,
                        &sub,
                        theme::regular(11.0),
                        palette.secondary,
                    );
                }
                if resp
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .on_hover_text(&top_artist.name)
                    .clicked()
                {
                    app.actions
                        .push(Action::Open(Page::Artist(top_artist.id.clone())));
                }
            }
        });
        ui.add_space(10.0);
    }

    let top_tracks: Vec<_> = app
        .home
        .top_tracks
        .get()
        .map_or(&[][..], Vec::as_slice)
        .iter()
        .take(cols)
        .cloned()
        .collect();
    if !top_tracks.is_empty() {
        let all_uris: Vec<String> = app
            .home
            .top_tracks
            .get()
            .map_or(Vec::new(), |t| t.iter().map(|tr| tr.uri.clone()).collect());
        let art_px = app.art_px(32.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = col_gap;
            for (idx, track) in top_tracks.iter().enumerate() {
                let plays = app.track_play_count(&track.uri);
                let (rect, resp) = ui.allocate_exact_size(vec2(cell_w, 57.0), Sense::click());
                if ui.is_rect_visible(rect) {
                    ui.painter().rect_filled(
                        rect,
                        CornerRadius::same(8),
                        palette
                            .surface_hover
                            .gamma_multiply(if resp.hovered() { 0.85 } else { 0.35 }),
                    );
                    let thumb = Rect::from_min_size(
                        pos2(rect.left() + 8.0, rect.center().y - 16.0),
                        Vec2::splat(32.0),
                    );
                    widgets::paint_cover(
                        ui,
                        &palette,
                        track.image(art_px),
                        thumb,
                        6.0,
                        Icon::Music,
                        Some(app.backend.art()),
                    );
                    let badge = (plays > 0).then(|| format!("{}x", plays));
                    let badge_w = if badge.is_some() { 26.0 } else { 6.0 };
                    let tr = Rect::from_min_max(
                        pos2(thumb.right() + 8.0, rect.top() + 4.0),
                        pos2(rect.right() - badge_w, rect.bottom() - 4.0),
                    );
                    let ranked_song = format!("#{} {}", idx + 1, track.name);
                    crate::bidi::paint_line(
                        &ui.painter().with_clip_rect(tr),
                        tr.left(),
                        tr.right(),
                        rect.center().y - 7.5,
                        &ranked_song,
                        theme::medium(12.5),
                        palette.text,
                    );
                    crate::bidi::paint_line(
                        &ui.painter().with_clip_rect(tr),
                        tr.left(),
                        tr.right(),
                        rect.center().y + 7.5,
                        &track.artist_names(),
                        theme::regular(11.0),
                        palette.secondary,
                    );
                    if let Some(badge) = &badge {
                        crate::bidi::paint_line(
                            ui.painter(),
                            rect.right() - 25.0,
                            rect.right() - 5.0,
                            rect.center().y,
                            badge,
                            theme::semibold(10.5),
                            palette.accent,
                        );
                    }
                }
                if resp
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .on_hover_text(format!("{} — {}", track.name, track.artist_names()))
                    .clicked()
                {
                    app.actions.push(Action::PlayUris {
                        uris: all_uris.clone(),
                        index: idx as u32,
                    });
                }
            }
        });
    }
}

fn bento_current_artist(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    let locale = app.locale;
    let target = app
        .now_playing()
        .filter(|now| !now.is_episode)
        .and_then(|now| {
            now.artists
                .first()
                .and_then(|a| a.id.clone().map(|id| (id, a.name.clone())))
        })
        .or_else(|| {
            app.home
                .recently_played
                .get()
                .and_then(|r| r.first())
                .and_then(|p| p.track.artists.first())
                .and_then(|a| a.id.clone().map(|id| (id, a.name.clone())))
        })
        .or_else(|| {
            app.home
                .top_artists
                .get()
                .and_then(|a| a.first())
                .map(|a| (a.id.clone(), a.name.clone()))
        });

    if let Some((artist_id, artist_name)) = target {
        app.ensure_artist_preview(&artist_id);
        let artist_obj = app
            .artist_pages
            .get(&artist_id)
            .and_then(|p| p.artist.get())
            .or_else(|| app.known_artist(&artist_id))
            .cloned();
        let top_tracks: Vec<_> = app
            .artist_pages
            .get(&artist_id)
            .and_then(|p| p.top_tracks.get())
            .map_or(&[][..], Vec::as_slice)
            .iter()
            .take(3)
            .cloned()
            .collect();

        ui.allocate_ui_with_layout(
            vec2(ui.available_width(), 36.0),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                let art_px = app.art_px(32.0);
                let (avatar, _) = ui.allocate_exact_size(Vec2::splat(32.0), Sense::hover());
                widgets::paint_cover(
                    ui,
                    &palette,
                    artist_obj.as_ref().and_then(|a| pick_image(&a.images, art_px)),
                    avatar,
                    16.0,
                    Icon::User,
                    Some(app.backend.art()),
                );
                let display_name = artist_obj
                    .as_ref()
                    .map_or(artist_name.as_str(), |a| a.name.as_str());
                if theme::link(ui, display_name, theme::bold(15.0), palette.text).clicked() {
                    app.actions.push(Action::Open(Page::Artist(artist_id.clone())));
                }
            },
        );
        ui.add_space(12.0);
        if top_tracks.is_empty() {
            widgets::loading_row(ui, &palette, locale);
        } else {
            let uris: Vec<String> = top_tracks.iter().map(|t| t.uri.clone()).collect();
            let art_px = app.art_px(32.0);
            for (idx, track) in top_tracks.iter().enumerate() {
                let (rect, resp) =
                    ui.allocate_exact_size(vec2(ui.available_width(), 46.0), Sense::click());
                if ui.is_rect_visible(rect) {
                    if resp.hovered() {
                        ui.painter().rect_filled(
                            rect,
                            CornerRadius::same(8),
                            palette.surface_hover.gamma_multiply(0.75),
                        );
                    }
                    let thumb = Rect::from_min_size(
                        pos2(rect.left() + 6.0, rect.center().y - 16.0),
                        Vec2::splat(32.0),
                    );
                    widgets::paint_cover(
                        ui,
                        &palette,
                        track.image(art_px),
                        thumb,
                        6.0,
                        Icon::Music,
                        Some(app.backend.art()),
                    );
                    let dur = (track.duration_ms > 0)
                        .then(|| crate::util::format_duration_ms(track.duration_ms));
                    let right_pad = if dur.is_some() { 48.0 } else { 8.0 };
                    let tr = Rect::from_min_max(
                        pos2(thumb.right() + 10.0, rect.top()),
                        pos2(rect.right() - right_pad, rect.bottom()),
                    );
                    crate::bidi::paint_line(
                        &ui.painter().with_clip_rect(tr),
                        tr.left(),
                        tr.right(),
                        rect.center().y,
                        &track.name,
                        theme::medium(13.0),
                        palette.text,
                    );
                    if let Some(dur) = &dur {
                        ui.painter().text(
                            pos2(rect.right() - 8.0, rect.center().y),
                            egui::Align2::RIGHT_CENTER,
                            dur,
                            theme::regular(11.5),
                            palette.secondary,
                        );
                    }
                }
                if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    app.actions.push(Action::PlayUris {
                        uris: uris.clone(),
                        index: idx as u32,
                    });
                }
                if idx + 1 < top_tracks.len() {
                    ui.add_space(6.0);
                }
            }
        }
    }
}

fn bento_extra_quick_access(
    app: &mut App,
    ui: &mut egui::Ui,
    primary_tiles: &[Tile],
    width: f32,
) {
    let extras = extra_quick_access_tiles(app, primary_tiles);
    if extras.is_empty() {
        theme::text(
            ui,
            &*gettext(app.locale, "Nothing new yet"),
            theme::regular(12.5),
            app.palette.secondary,
        );
        return;
    }
    let cols = if width >= 440.0 { 4 } else { 2 };
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = 12.0;
        quick_access_grid(app, ui, &extras, cols * 3, Some(cols), 58.0);
    });
}

fn quick_access(app: &mut App, ui: &mut egui::Ui) {
    let tiles = quick_access_tiles(app);
    quick_access_grid(app, ui, &tiles, TILES, None, TILE_HEIGHT);
}

fn quick_access_grid(
    app: &mut App,
    ui: &mut egui::Ui,
    tiles: &[Tile],
    max_tiles: usize,
    fixed_columns: Option<usize>,
    tile_height: f32,
) {
    let palette = app.palette;
    let count = tiles.len().min(max_tiles);
    if count == 0 {
        return;
    }
    let available = ui.available_width();
    let columns =
        fixed_columns.unwrap_or_else(|| ((available / 300.0).floor() as usize).clamp(2, 4));
    let gap = 10.0;
    let row_gap = ui.spacing().item_spacing.y;
    let tile_width = (available - gap * (columns as f32 - 1.0)) / columns as f32;
    let rows = count.div_ceil(columns);
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        for row in 0..rows {
            ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            for column in 0..columns {
                let index = row * columns + column;
                if index >= count {
                    break;
                }
                let Some(Tile {
                    image,
                    name,
                    page,
                    uri,
                    liked,
                    owned_playlist,
                    song,
                }) = tiles.get(index)
                else {
                    break;
                };
                let context_here = uri
                    .as_deref()
                    .is_some_and(|u| app.playing_context_uri().as_deref() == Some(u));
                let playing_here = context_here && app.believed_playing();
                let (rect, response) =
                    ui.allocate_exact_size(vec2(tile_width, tile_height), Sense::click());
                if ui.is_rect_visible(rect) {
                    let hovered = ui.rect_contains_pointer(rect);
                    let fill = if hovered {
                        palette.surface_hover
                    } else if context_here {
                        theme::Palette::blend_color(
                            palette.surface,
                            palette.accent,
                            if palette.dark { 0.14 } else { 0.08 },
                        )
                    } else {
                        palette.surface
                    };
                    ui.painter().rect_filled(rect, CornerRadius::same(10), fill);
                    let stroke_color = if context_here {
                        palette.accent.gamma_multiply(0.85)
                    } else {
                        palette.outline
                    };
                    ui.painter().rect_stroke(
                        rect,
                        CornerRadius::same(10),
                        egui::Stroke::new(if context_here { 1.5 } else { 1.0 }, stroke_color),
                        egui::StrokeKind::Inside,
                    );
                    let cover = Rect::from_min_size(rect.min, Vec2::splat(tile_height));
                    if *liked {
                        super::sidebar::liked_cover(ui, cover, 10.0);
                    } else {
                        widgets::paint_cover(
                            ui,
                            &palette,
                            image.as_deref(),
                            cover,
                            10.0,
                            Icon::Music,
                            Some(app.backend.art()),
                        );
                    }
                    let compact_tile = tile_height < 52.0;
                    let btn_size = if compact_tile { 32.0 } else { 40.0 };
                    let play_room = if hovered && uri.is_some() {
                        btn_size + 12.0
                    } else if context_here {
                        34.0
                    } else {
                        10.0
                    };
                    let text_rect = Rect::from_min_max(
                        pos2(cover.right() + if compact_tile { 8.0 } else { 12.0 }, rect.top()),
                        pos2(rect.right() - play_room, rect.bottom()),
                    );
                    crate::bidi::paint_line(
                        &ui.painter().with_clip_rect(text_rect),
                        text_rect.left(),
                        text_rect.right(),
                        rect.center().y,
                        name,
                        theme::bold(if compact_tile { 12.5 } else { 14.0 }),
                        palette.text,
                    );
                    if let Some(progress) =
                        tile_podcast_progress(&app.home.podcasts, uri.as_deref())
                    {
                        let bar = Rect::from_min_max(
                            pos2(cover.right() + 10.0, rect.bottom() - 5.0),
                            pos2(rect.right() - 10.0, rect.bottom() - 2.0),
                        );
                        ui.painter()
                            .rect_filled(bar, CornerRadius::same(2), palette.outline);
                        let filled = Rect::from_min_max(
                            bar.min,
                            pos2(bar.left() + bar.width() * progress, bar.bottom()),
                        );
                        ui.painter()
                            .rect_filled(filled, CornerRadius::same(2), palette.accent);
                    }
                    if hovered && let Some(uri) = uri {
                        let button = Rect::from_center_size(
                            pos2(rect.right() - (btn_size * 0.5 + 8.0), rect.center().y),
                            Vec2::splat(btn_size),
                        );
                        let mut child =
                            ui.new_child(egui::UiBuilder::new().max_rect(button).layout(
                                egui::Layout::centered_and_justified(egui::Direction::LeftToRight),
                            ));
                        if theme::circle_button(
                            &mut child,
                            if playing_here {
                                Icon::PauseFilled
                            } else {
                                Icon::PlayFilled
                            },
                            btn_size,
                            palette.accent,
                            palette.accent_hover,
                            palette.on_accent,
                            &gettext(app.locale, if playing_here { "Pause" } else { "Play" }),
                        )
                        .clicked()
                        {
                            if context_here {
                                app.actions.push(Action::TogglePlay);
                            } else if song.is_some() {
                                app.actions.push(Action::PlayUris {
                                    uris: vec![uri.clone()],
                                    index: 0,
                                });
                            } else {
                                app.actions.push(Action::PlayContext {
                                    uri: uri.clone(),
                                    offset_uri: None,
                                    offset_index: None,
                                });
                            }
                        }
                    } else if context_here {
                        let eq_rect = Rect::from_center_size(
                            pos2(rect.right() - 18.0, rect.center().y),
                            vec2(14.0, 14.0),
                        );
                        theme::paint_eq_bars(ui, eq_rect, palette.accent, playing_here);
                    }
                }
                if response.clicked() {
                    app.actions.push(Action::Open(page.clone()));
                }
                if !liked && let Some(uri) = uri {
                    egui::Popup::context_menu(&response)
                        .id(ui.make_persistent_id(("quick-access-menu", tile_height as u32, uri)))
                        .frame(widgets::menu_frame(&palette))
                        .show(|ui| match song {
                            Some(track) => widgets::item_menu(
                                ui,
                                app,
                                &PlayableItem::Track(track.clone()),
                                None,
                                None,
                            ),
                            None => widgets::context_menu_items(
                                ui,
                                app,
                                uri,
                                name,
                                owned_playlist.as_ref(),
                            ),
                        });
                }
            }
        });
            if row + 1 < rows && row_gap > 0.0 {
                ui.add_space(row_gap);
            }
        }
    });
}

/// Renders session Home cards or approximate fallback cards in the same shelf.
fn made_for_you(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    let (playlists, loading, failure) = match &app.home.made_for_you {
        Loadable::Loaded(playlists) => (playlists.clone(), false, None),
        Loadable::Loading | Loadable::NotLoaded => (Vec::new(), true, None),
        Loadable::Failed(error) => (Vec::new(), false, Some(error.clone())),
    };
    if playlists.is_empty() && !loading && failure.is_none() {
        return;
    }
    widgets::shelf(
        ui,
        &palette,
        app.locale,
        "made-for-you",
        &gettext(app.locale, "Made for you"),
        |ui| {
            if playlists.is_empty() && loading {
                widgets::skeleton_cards(ui, &palette, app.locale, 7, false);
            } else if playlists.is_empty()
                && let Some(message) = &failure
            {
                widgets::error_row(ui, app, message, Some(Page::Home));
            }
            playlist_cards(app, ui, "home-made_for_you-menu", &playlists);
        },
    );
}

fn top_mixes(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    let Some(playlists) = app
        .home
        .top_mixes
        .get()
        .filter(|playlists| !playlists.is_empty())
        .cloned()
    else {
        return;
    };
    widgets::shelf(
        ui,
        &palette,
        app.locale,
        "top-mixes",
        &gettext(app.locale, "Your top mixes"),
        |ui| playlist_cards(app, ui, "home-top_mixes-menu", &playlists),
    );
}

/// The latest releases from followed artists. A card opens its release;
/// the title opens the page that plays all their songs, newest first.
fn new_releases(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    let (albums, loading, failure) = match &app.home.new_releases {
        Loadable::Loaded(albums) => (albums.clone(), false, None),
        Loadable::Loading | Loadable::NotLoaded => (Vec::new(), true, None),
        Loadable::Failed(error) => (Vec::new(), false, Some(error.clone())),
    };
    if albums.is_empty() && !loading && failure.is_none() {
        return;
    }
    let open = widgets::linked_shelf(
        ui,
        &palette,
        app.locale,
        "new-releases",
        &gettext(app.locale, "New releases"),
        |ui| {
            if albums.is_empty() && loading {
                widgets::loading_row(ui, &palette, app.locale);
            } else if albums.is_empty()
                && let Some(message) = &failure
            {
                widgets::error_row(ui, app, message, Some(Page::Home));
            }
            for album in &albums {
                release_card(app, ui, album);
            }
        },
    );
    if open {
        app.actions.push(Action::Open(Page::NewReleases));
    }
}

fn release_card(app: &mut App, ui: &mut egui::Ui, album: &Album) {
    let palette = app.palette;
    let artists =
        crate::api::models::join_names(album.artists.iter().map(|artist| artist.name.as_str()));
    let subtitle = if artists.is_empty() {
        album.kind_label(app.locale).into_owned()
    } else {
        format!("{} • {artists}", album.kind_label(app.locale))
    };
    let context_here = app.playing_context_uri().as_deref() == Some(album.uri.as_str());
    let playing_here = context_here && app.believed_playing();
    let card_art = app.art_px(widgets::CARD_ART);
    let card = widgets::card(
        ui,
        app,
        pick_image(&album.images, card_art),
        &album.name,
        &subtitle,
        widgets::CardCover::square(playing_here),
    );
    if card.play {
        if context_here {
            app.actions.push(Action::TogglePlay);
        } else {
            app.actions.push(Action::PlayContext {
                uri: album.uri.clone(),
                offset_uri: None,
                offset_index: None,
            });
        }
    }
    if card.clicked {
        app.actions
            .push(Action::Open(Page::Album(album.id.clone())));
    }
    egui::Popup::context_menu(&card.response)
        .id(ui.make_persistent_id(("home-new-releases-menu", &album.uri)))
        .frame(widgets::menu_frame(&palette))
        .show(|ui| {
            widgets::context_menu_items(ui, app, &album.uri, &album.name, None);
        });
}

fn playlist_cards(app: &mut App, ui: &mut egui::Ui, menu: &str, playlists: &[Playlist]) {
    let palette = app.palette;
    for playlist in playlists {
        let base_subtitle = playlist
            .description
            .as_deref()
            .map(crate::util::strip_html)
            .filter(|d| !d.is_empty())
            .unwrap_or_else(|| {
                // Translators: {owner} is the name of the playlist's owner.
                gettext(app.locale, "By {owner}").replace("{owner}", playlist.owner_name())
            });
        let subtitle = if playlist.id == crate::made_for_you::DAYLIST_ID
            && !base_subtitle.to_ascii_lowercase().starts_with("daylist")
        {
            format!("daylist • {base_subtitle}")
        } else {
            base_subtitle
        };
        let context_here = app.playing_context_uri().as_deref() == Some(playlist.uri.as_str());
        let playing_here = context_here && app.believed_playing();
        let card_art = app.art_px(widgets::CARD_ART);
        let card = widgets::card(
            ui,
            app,
            pick_image(&playlist.images, card_art),
            &playlist.name,
            &subtitle,
            widgets::CardCover::square(playing_here),
        );
        if card.play {
            if context_here {
                app.actions.push(Action::TogglePlay);
            } else {
                app.actions.push(Action::PlayContext {
                    uri: playlist.uri.clone(),
                    offset_uri: None,
                    offset_index: None,
                });
            }
        }
        if card.clicked {
            app.actions
                .push(Action::Open(Page::Playlist(playlist.id.clone())));
        }
        egui::Popup::context_menu(&card.response)
            .id(ui.make_persistent_id((menu, &playlist.uri)))
            .frame(widgets::menu_frame(&palette))
            .show(|ui| {
                let owned = app.user_id().is_some_and(|id| playlist.owned_by(id));
                widgets::context_menu_items(
                    ui,
                    app,
                    &playlist.uri,
                    &playlist.name,
                    owned.then_some(playlist),
                );
            });
    }
}

fn recently_played(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    let history = match app.home.recently_played.clone() {
        Loadable::Loaded(history) => history,
        Loadable::Loading | Loadable::NotLoaded => {
            widgets::shelf(
                ui,
                &palette,
                app.locale,
                "recent",
                &gettext(app.locale, "Recently played"),
                |ui| widgets::skeleton_cards(ui, &palette, app.locale, 7, false),
            );
            return;
        }
        Loadable::Failed(message) => {
            widgets::shelf(
                ui,
                &palette,
                app.locale,
                "recent",
                &gettext(app.locale, "Recently played"),
                |ui| {
                    widgets::error_row(ui, app, &message, Some(Page::Home));
                },
            );
            return;
        }
    };
    let mut seen = std::collections::HashSet::new();
    let tracks: Vec<_> = history
        .into_iter()
        .filter(|entry| {
            entry
                .track
                .id
                .as_ref()
                .is_some_and(|id| seen.insert(id.clone()))
        })
        .take(16)
        .collect();
    if tracks.is_empty() {
        return;
    }
    widgets::shelf(
        ui,
        &palette,
        app.locale,
        "recent",
        &gettext(app.locale, "Recently played"),
        |ui| {
            for entry in &tracks {
                let track = &entry.track;
                let card_art = app.art_px(widgets::CARD_ART);
                let card = widgets::card(
                    ui,
                    app,
                    track.image(card_art),
                    &track.name,
                    &track.artist_names(),
                    widgets::CardCover::square(false),
                );
                if card.play {
                    app.actions.push(Action::PlayUris {
                        uris: vec![track.uri.clone()],
                        index: 0,
                    });
                }
                if card.clicked
                    && let Some(album) = &track.album
                    && !album.id.is_empty()
                {
                    app.actions
                        .push(Action::Open(Page::Album(album.id.clone())));
                }
                egui::Popup::context_menu(&card.response)
                    .id(ui.make_persistent_id(("home-recently_played-menu", &track.uri)))
                    .frame(widgets::menu_frame(&palette))
                    .show(|ui| {
                        widgets::item_menu(
                            ui,
                            app,
                            &PlayableItem::Track(track.clone()),
                            None,
                            None,
                        );
                    });
            }
        },
    );
}

/// Why an episode is on the podcast shelf.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EpisodeReason {
    /// Started and not finished, with this much left.
    Continue { left_ms: u32 },
    /// The show's newest episode, recently released and not yet started.
    New,
}

/// How many days after its release an unplayed episode still counts as new.
const NEW_EPISODE_DAYS: i64 = 30;
/// The podcast shelf's card limit, as for Recently played.
const PODCAST_CARDS: usize = 16;

/// The episodes on the podcast shelf: those to continue first, then each
/// show's newest unstarted episode from the last month, each group newest
/// first. `skip` leaves out shows that are audiobooks or no longer saved.
pub(crate) fn podcast_episodes(
    podcasts: &[(Show, Vec<Episode>)],
    skip: impl Fn(&Show) -> bool,
    today: jiff::civil::Date,
) -> Vec<(Show, Episode, EpisodeReason)> {
    let oldest_new = today
        .checked_sub(jiff::Span::new().days(NEW_EPISODE_DAYS))
        .unwrap_or(today);
    let released = |episode: &Episode| {
        episode
            .release_date
            .as_deref()
            .and_then(|date| date.get(..10))
            .and_then(|date| date.parse::<jiff::civil::Date>().ok())
    };
    let mut continuing = Vec::new();
    let mut new = Vec::new();
    for (show, episodes) in podcasts {
        if skip(show) {
            continue;
        }
        for episode in episodes {
            if let Some(resume) = &episode.resume_point
                && !resume.fully_played
                && resume.resume_position_ms > 0
            {
                let left_ms = episode
                    .duration_ms
                    .saturating_sub(resume.resume_position_ms);
                continuing.push((show, episode, EpisodeReason::Continue { left_ms }));
            }
        }
        // Spotify lists a show's episodes newest first.
        if let Some(newest) = episodes.first()
            && newest
                .resume_point
                .as_ref()
                .is_none_or(|resume| !resume.fully_played && resume.resume_position_ms == 0)
            && released(newest).is_some_and(|date| date >= oldest_new)
        {
            new.push((show, newest, EpisodeReason::New));
        }
    }
    for group in [&mut continuing, &mut new] {
        group.sort_by_key(|(_, episode, _)| std::cmp::Reverse(released(episode)));
    }
    let mut seen = std::collections::HashSet::new();
    continuing
        .into_iter()
        .chain(new)
        .filter(|(_, episode, _)| !episode.uri.is_empty() && seen.insert(&episode.uri))
        .take(PODCAST_CARDS)
        .map(|(show, episode, reason)| {
            let mut episode = episode.clone();
            // A show's episode list leaves out the show; menus need it.
            if episode.show.is_none() {
                episode.show = Some(show.clone());
            }
            (show.clone(), episode, reason)
        })
        .collect()
}

fn podcasts(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    let episodes = podcast_episodes(
        &app.home.podcasts,
        |show| app.audiobook_shows.contains(&show.uri) || app.is_saved(&show.uri) == Some(false),
        jiff::Zoned::now().date(),
    );
    if episodes.is_empty() {
        return;
    }
    widgets::shelf(
        ui,
        &palette,
        app.locale,
        "podcasts",
        &gettext(app.locale, "Your podcasts"),
        |ui| {
            for (show, episode, reason) in &episodes {
                let subtitle = match reason {
                    EpisodeReason::Continue { left_ms } => {
                        // Translators: {time} is the time left in an episode, such as 12 min; {show} is the podcast's name.
                        gettext(app.locale, "{time} left • {show}")
                            .replace(
                                "{time}",
                                &crate::util::format_episode_ms(app.locale, *left_ms),
                            )
                            .replace("{show}", &show.name)
                    }
                    EpisodeReason::New => {
                        // Translators: {show} is the podcast's name.
                        gettext(app.locale, "New • {show}").replace("{show}", &show.name)
                    }
                };
                let card_art = app.art_px(widgets::CARD_ART);
                let card = widgets::card(
                    ui,
                    app,
                    pick_image(&episode.images, card_art)
                        .or_else(|| pick_image(&show.images, card_art)),
                    &episode.name,
                    &subtitle,
                    widgets::CardCover::square(false),
                );
                if card.play {
                    app.actions.push(Action::PlayEpisode {
                        uri: episode.uri.clone(),
                        resume_ms: episode.resume_ms(),
                    });
                }
                paint_episode_progress(ui, &palette, &card.response, episode);
                if card.clicked && !show.id.is_empty() {
                    app.actions.push(Action::Open(Page::Show(show.id.clone())));
                }
                egui::Popup::context_menu(&card.response)
                    .id(ui.make_persistent_id(("home-podcasts-menu", &episode.uri)))
                    .frame(widgets::menu_frame(&palette))
                    .show(|ui| {
                        widgets::item_menu(
                            ui,
                            app,
                            &PlayableItem::Episode(episode.clone()),
                            None,
                            None,
                        );
                    });
            }
        },
    );
}

fn top_artists(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    let artists = match app.home.top_artists.clone() {
        Loadable::Loaded(artists) => artists,
        Loadable::Loading | Loadable::NotLoaded => {
            widgets::shelf(
                ui,
                &palette,
                app.locale,
                "top-artists",
                &gettext(app.locale, "Your top artists"),
                |ui| widgets::skeleton_cards(ui, &palette, app.locale, 7, true),
            );
            return;
        }
        Loadable::Failed(message) => {
            widgets::shelf(
                ui,
                &palette,
                app.locale,
                "top-artists",
                &gettext(app.locale, "Your top artists"),
                |ui| {
                    widgets::error_row(ui, app, &message, Some(Page::Home));
                },
            );
            return;
        }
    };
    if artists.is_empty() {
        return;
    }
    widgets::shelf(
        ui,
        &palette,
        app.locale,
        "top-artists",
        &gettext(app.locale, "Your top artists"),
        |ui| {
            for artist in &artists {
                let context_here =
                    app.playing_context_uri().as_deref() == Some(artist.uri.as_str());
                let playing_here = context_here && app.believed_playing();
                let card_art = app.art_px(widgets::CARD_ART);
                let card = widgets::card(
                    ui,
                    app,
                    pick_image(&artist.images, card_art),
                    &artist.name,
                    &gettext(app.locale, "Artist"),
                    widgets::CardCover::portrait(playing_here),
                );
                if card.play {
                    if context_here {
                        app.actions.push(Action::TogglePlay);
                    } else {
                        app.actions.push(Action::PlayContext {
                            uri: artist.uri.clone(),
                            offset_uri: None,
                            offset_index: None,
                        });
                    }
                }
                if card.clicked {
                    app.actions
                        .push(Action::Open(Page::Artist(artist.id.clone())));
                }
                egui::Popup::context_menu(&card.response)
                    .id(ui.make_persistent_id(("home-top_artists-menu", &artist.uri)))
                    .frame(widgets::menu_frame(&palette))
                    .show(|ui| {
                        widgets::context_menu_items(ui, app, &artist.uri, &artist.name, None);
                    });
            }
        },
    );
}

fn track_list(
    app: &mut App,
    ui: &mut egui::Ui,
    title: &str,
    tracks: Loadable<Vec<crate::api::models::Track>>,
    limit: usize,
    title_page: Option<Page>,
    more_label: Option<&str>,
) {
    let palette = app.palette;
    let tracks = match tracks {
        Loadable::Loaded(tracks) => tracks,
        Loadable::Loading | Loadable::NotLoaded => {
            if let Some(page) = title_page {
                if theme::link(ui, title, theme::bold(17.0), palette.text).clicked() {
                    app.actions.push(Action::Open(page));
                }
            } else {
                theme::section_title(ui, &palette, title);
            }
            widgets::skeleton_rows(ui, &palette, app.locale, 4);
            ui.add_space(12.0);
            return;
        }
        Loadable::Failed(message) => {
            theme::section_title(ui, &palette, title);
            widgets::error_row(ui, app, &message, Some(title_page.unwrap_or(Page::Home)));
            ui.add_space(12.0);
            return;
        }
    };
    if tracks.is_empty() {
        return;
    }
    if let Some(page) = title_page {
        if theme::link(ui, title, theme::bold(17.0), palette.text).clicked() {
            app.actions.push(Action::Open(page));
        }
    } else {
        theme::section_title(ui, &palette, title);
    }
    ui.add_space(4.0);
    let uris: Arc<[String]> = tracks
        .iter()
        .map(|track| track.uri.clone())
        .collect::<Vec<_>>()
        .into();
    let context = RowContext::Uris(Arc::clone(&uris));
    for (index, track) in tracks.iter().take(limit).enumerate() {
        let item = PlayableItem::Track(track.clone());
        widgets::track_row(
            ui,
            app,
            TrackRow {
                index,
                number: None,
                item: &item,
                context: &context,
                show_cover: !app.settings.tracklist_compact,
                show_album: true,
                added_at: None,
                added_by: None,
                show_added_by: false,
                show_plays: false,
                compact: false,
                thin: app.settings.tracklist_compact,
                shift: 0.0,
                picked: false,
                picked_songs: &[],
                picked_positions: &[],
                position: None,
            },
        );
    }
    if let Some(label) = more_label
        && tracks.len() > limit
        && theme::link(ui, label, theme::semibold(14.0), palette.secondary).clicked()
    {
        app.actions.push(Action::Open(Page::TopSongs));
    }
    ui.add_space(16.0);
}

fn top_tracks(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    let title = gettext(app.locale, "Your top songs");
    let more = gettext(app.locale, "Show more top songs");
    let tracks = app.home.top_tracks.clone();

    if theme::link(ui, &*title, theme::bold(17.0), palette.text).clicked() {
        app.actions.push(Action::Open(Page::TopSongs));
    }
    ui.add_space(4.0);

    let labels: Vec<_> = TopPeriod::ALL
        .iter()
        .map(|p| (*p, p.label(app.locale)))
        .collect();
    let options: Vec<(TopPeriod, &str)> = labels
        .iter()
        .map(|(period, label)| (*period, label.as_ref()))
        .collect();
    if let Some(period) = widgets::chips(ui, &palette, &options, app.home.top_period) {
        app.actions.push(Action::SetTopPeriod(period));
    }
    ui.add_space(8.0);

    let tracks = match tracks {
        Loadable::Loaded(tracks) => tracks,
        Loadable::Loading | Loadable::NotLoaded => {
            widgets::loading_row(ui, &palette, app.locale);
            ui.add_space(12.0);
            return;
        }
        Loadable::Failed(message) => {
            widgets::error_row(ui, app, &message, Some(Page::TopSongs));
            ui.add_space(12.0);
            return;
        }
    };

    let uris: Arc<[String]> = tracks
        .iter()
        .map(|track| track.uri.clone())
        .collect::<Vec<_>>()
        .into();
    super::collection::top_songs_actions(ui, app, &tracks);
    let context = RowContext::Uris(Arc::clone(&uris));
    for (index, track) in tracks.iter().take(10).enumerate() {
        let item = PlayableItem::Track(track.clone());
        widgets::top_track_row(
            ui,
            app,
            TrackRow {
                index,
                number: None,
                item: &item,
                context: &context,
                show_cover: !app.settings.tracklist_compact,
                show_album: true,
                added_at: None,
                added_by: None,
                show_added_by: false,
                show_plays: false,
                compact: false,
                thin: app.settings.tracklist_compact,
                shift: 0.0,
                picked: false,
                picked_songs: &[],
                picked_positions: &[],
                position: None,
            },
        );
    }
    if tracks.len() > 10
        && theme::link(ui, &*more, theme::semibold(14.0), palette.secondary).clicked()
    {
        app.actions.push(Action::Open(Page::TopSongs));
    }
    ui.add_space(16.0);
}

fn recommendations(app: &mut App, ui: &mut egui::Ui) {
    let tracks = app.home.recommendations.clone();
    let title = gettext(app.locale, "Recommended for you");
    track_list(app, ui, &title, tracks, 20, None, None);
}

fn paint_episode_progress(
    ui: &egui::Ui,
    palette: &theme::Palette,
    response: &egui::Response,
    episode: &Episode,
) {
    let Some(resume_ms) = episode.resume_ms() else {
        return;
    };
    if episode.duration_ms == 0 {
        return;
    }
    let frac = (resume_ms as f32 / episode.duration_ms as f32).clamp(0.03, 0.99);
    const PAD: f32 = 12.0;
    let side = (response.rect.width() - PAD * 2.0).max(1.0);
    let cover = Rect::from_min_size(response.rect.min + Vec2::splat(PAD), Vec2::splat(side));
    if !ui.is_rect_visible(cover) {
        return;
    }
    let bar = Rect::from_min_max(
        pos2(cover.left() + 8.0, cover.bottom() - 8.0),
        pos2(cover.right() - 8.0, cover.bottom() - 4.0),
    );
    ui.painter().rect_filled(
        bar,
        CornerRadius::same(2),
        palette.window.gamma_multiply(0.75),
    );
    let filled = Rect::from_min_max(bar.min, pos2(bar.left() + bar.width() * frac, bar.bottom()));
    ui.painter()
        .rect_filled(filled, CornerRadius::same(2), palette.accent);
}

fn podcasts_filter_view(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    let locale = app.locale;
    podcasts(app, ui);
    let shows: Vec<Show> = app
        .library
        .shows
        .items
        .iter()
        .map(|saved| &saved.show)
        .filter(|show| !app.audiobook_shows.contains(&show.uri))
        .cloned()
        .collect();
    if !shows.is_empty() {
        ui.add_space(4.0);
        theme::text(
            ui,
            &*gettext(locale, "Podcasts"),
            theme::bold(17.0),
            palette.text,
        );
        ui.add_space(8.0);
        let card_art = app.art_px(widgets::CARD_ART);
        widgets::grid(ui, |ui| {
            for show in &shows {
                let context_here = app.playing_context_uri().as_deref() == Some(show.uri.as_str());
                let playing_here = context_here && app.believed_playing();
                let card = widgets::card(
                    ui,
                    app,
                    pick_image(&show.images, card_art),
                    &show.name,
                    &show.publisher,
                    widgets::CardCover::square(playing_here),
                );
                if card.play {
                    if context_here {
                        app.actions.push(Action::TogglePlay);
                    } else {
                        app.actions.push(Action::PlayContext {
                            uri: show.uri.clone(),
                            offset_uri: None,
                            offset_index: None,
                        });
                    }
                }
                if card.clicked && !show.id.is_empty() {
                    app.actions.push(Action::Open(Page::Show(show.id.clone())));
                }
                egui::Popup::context_menu(&card.response)
                    .id(ui.make_persistent_id(("home-podcast-show-menu", &show.uri)))
                    .frame(widgets::menu_frame(&palette))
                    .show(|ui| {
                        widgets::context_menu_items(ui, app, &show.uri, &show.name, None);
                    });
            }
        });
    } else if app.home.podcasts.is_empty() {
        widgets::empty_state(
            ui,
            &palette,
            Icon::Radio,
            &gettext(locale, "Podcasts"),
            &gettext(locale, "Nothing here yet."),
        );
    }
}

fn top_filter_view(app: &mut App, ui: &mut egui::Ui) {
    top_artists(app, ui);
    top_tracks(app, ui);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::models::ResumePoint;

    fn show(id: &str) -> Show {
        Show {
            id: id.into(),
            uri: format!("spotify:show:{id}"),
            name: id.to_uppercase(),
            ..Show::default()
        }
    }

    fn episode(id: &str, date: &str, played: Option<u32>, finished: bool) -> Episode {
        Episode {
            id: id.into(),
            uri: format!("spotify:episode:{id}"),
            duration_ms: 3_600_000,
            release_date: Some(date.into()),
            resume_point: Some(ResumePoint {
                fully_played: finished,
                resume_position_ms: played.unwrap_or(0),
            }),
            ..Episode::default()
        }
    }

    fn ids(shelf: &[(Show, Episode, EpisodeReason)]) -> Vec<&str> {
        shelf
            .iter()
            .map(|(_, episode, _)| episode.id.as_str())
            .collect()
    }

    /// A tile's cover is 60 points. Spotify's 64 pixel rendition would be
    /// blurry even at 100%, so the tile takes the 300 pixel one and scales
    /// it down; at 300% the 300 pixel one still has a quarter more pixels
    /// than the tile shows.
    #[cfg(feature = "demo")]
    #[test]
    fn quick_access_tiles_ask_for_art_the_display_can_show() {
        let root = std::env::temp_dir().join(format!("spotifast-tiles-{}", std::process::id()));
        let mut app = App::new(
            &crate::backend::Waker::default(),
            crate::paths::AppDirs {
                config: root.join("config"),
                state: root.join("state"),
                cache: root.join("cache"),
            },
            crate::settings::Settings::default(),
            crate::app::AppOptions {
                media_controls: false,
                restore_sign_in: false,
                tray: false,
                autostart: false,
            },
        );
        crate::demo::populate(&mut app);
        // Only the Library's playlists, so the second tile is the first one.
        app.home.recently_played = Loadable::Loaded(Vec::new());
        let first = app.library.playlists.get().unwrap()[0].images.clone();
        let width = |url: Option<&str>| {
            first
                .iter()
                .find(|image| Some(image.url.as_str()) == url)
                .and_then(|image| image.width)
        };
        app.pixels_per_point = 1.0;
        assert_eq!(
            width(quick_access_tiles(&app)[1].image.as_deref()),
            Some(300)
        );
        app.pixels_per_point = 3.0;
        assert_eq!(
            width(quick_access_tiles(&app)[1].image.as_deref()),
            Some(300)
        );
        let _ = std::fs::remove_dir_all(root);
    }

    /// Home's tiles follow what was played, newest first: the playlist,
    /// album or Liked Songs a song came from, or the song itself, each once.
    /// The Library's playlists fill what history leaves, and stand in until
    /// it arrives (#942).
    #[cfg(feature = "demo")]
    #[test]
    fn quick_access_tiles_follow_what_was_played() {
        let root =
            std::env::temp_dir().join(format!("spotifast-recent-tiles-{}", std::process::id()));
        let mut app = App::new(
            &crate::backend::Waker::default(),
            crate::paths::AppDirs {
                config: root.join("config"),
                state: root.join("state"),
                cache: root.join("cache"),
            },
            crate::settings::Settings::default(),
            crate::app::AppOptions {
                media_controls: false,
                restore_sign_in: false,
                tray: false,
                autostart: false,
            },
        );
        crate::demo::populate(&mut app);
        let history = app.home.recently_played.get().unwrap().clone();
        let tiles = quick_access_tiles(&app);
        assert_eq!(tiles.len(), TILES);
        assert_eq!(
            tiles[0].page,
            Page::Playlist("pl1".into()),
            "played from it"
        );
        let song = &history[2].track;
        assert_eq!(
            tiles[1].uri.as_deref(),
            Some(song.uri.as_str()),
            "played on its own"
        );
        assert!(tiles[1].song.is_some());
        let pages: Vec<_> = tiles.iter().map(|tile| tile.page.clone()).collect();
        assert_eq!(
            pages
                .iter()
                .filter(|page| **page == Page::LikedSongs)
                .count(),
            1
        );
        let unique: std::collections::HashSet<_> = pages.iter().collect();
        assert_eq!(unique.len(), pages.len(), "each once");

        app.home.recently_played = Loadable::Loading;
        let tiles = quick_access_tiles(&app);
        assert_eq!(
            tiles[0].page,
            Page::LikedSongs,
            "the Library until history arrives"
        );
        let first = app.library.playlists.get().unwrap()[0].id.clone();
        assert_eq!(tiles[1].page, Page::Playlist(first));
        assert_eq!(tiles.len(), TILES);
        app.backend.shutdown();
        let _ = std::fs::remove_dir_all(root);
    }

    fn today() -> jiff::civil::Date {
        "2026-09-23".parse().unwrap()
    }

    #[test]
    fn episodes_to_continue_come_before_new_ones() {
        let podcasts = vec![
            (
                show("a"),
                vec![
                    episode("a-new", "2026-09-20", None, false),
                    episode("a-started", "2026-09-01", Some(600_000), false),
                    episode("a-done", "2026-08-25", Some(0), true),
                ],
            ),
            (
                show("b"),
                vec![
                    episode("b-started", "2026-09-10", Some(60_000), false),
                    episode("b-old", "2026-09-03", None, false),
                ],
            ),
            (show("c"), vec![episode("c-new", "2026-09-22", None, false)]),
        ];
        let shelf = podcast_episodes(&podcasts, |_| false, today());
        assert_eq!(ids(&shelf), ["b-started", "a-started", "c-new", "a-new"]);
        assert_eq!(shelf[1].2, EpisodeReason::Continue { left_ms: 3_000_000 });
        assert_eq!(shelf[2].2, EpisodeReason::New);
        assert_eq!(
            shelf[0].1.show.as_ref().map(|show| show.id.as_str()),
            Some("b"),
            "the episode menu can go to its podcast"
        );
    }

    #[test]
    fn only_a_recent_unstarted_newest_episode_is_new() {
        let podcasts = vec![
            (show("old"), vec![episode("old", "2026-07-01", None, false)]),
            (
                show("finished"),
                vec![episode("finished", "2026-09-20", Some(0), true)],
            ),
            (
                show("undated"),
                vec![episode("undated", "2026", None, false)],
            ),
            (
                show("second"),
                vec![
                    episode("second-done", "2026-09-21", Some(0), true),
                    episode("second-new", "2026-09-20", None, false),
                ],
            ),
        ];
        assert!(podcast_episodes(&podcasts, |_| false, today()).is_empty());
    }

    #[test]
    fn skipped_shows_leave_the_shelf_and_it_stays_bounded() {
        let podcasts: Vec<_> = (0..20)
            .map(|index| {
                let id = format!("s{index}");
                let started = episode(&format!("{id}-e"), "2026-09-01", Some(1), false);
                (show(&id), vec![started])
            })
            .collect();
        let shelf = podcast_episodes(&podcasts, |show| show.id == "s0", today());
        assert_eq!(shelf.len(), PODCAST_CARDS);
        assert!(!ids(&shelf).contains(&"s0-e"));
    }

    #[test]
    fn tile_podcast_progress_reads_show_and_episode_uris() {
        let ep_started = episode("ep1", "2026-09-10", Some(900_000), false);
        let ep_tiny = episode("ep2", "2026-09-11", Some(10_000), false);
        let ep_done = episode("ep3", "2026-09-12", Some(3_599_000), true);
        let podcasts = vec![(
            show("s1"),
            vec![ep_started.clone(), ep_tiny.clone(), ep_done.clone()],
        )];

        let show_ratio = tile_podcast_progress(&podcasts, Some("spotify:show:s1")).unwrap();
        assert!((show_ratio - 0.25).abs() < 0.001, "got {show_ratio}");

        // Small positive resume position clamps up to 4% visibility floor.
        let tiny_ratio = tile_podcast_progress(&podcasts, Some(&ep_tiny.uri)).unwrap();
        assert_eq!(tiny_ratio, 0.04);

        // Finished episodes return None from resume_ms() so no progress bar is drawn.
        assert_eq!(tile_podcast_progress(&podcasts, Some(&ep_done.uri)), None);
        assert_eq!(
            tile_podcast_progress(&podcasts, Some("spotify:playlist:other")),
            None
        );
        assert_eq!(tile_podcast_progress(&podcasts, None), None);
    }

    #[cfg(feature = "demo")]
    #[test]
    fn resolve_live_radio_prefers_fallback_and_caps_up_next() {
        let root =
            std::env::temp_dir().join(format!("spotifast-live-radio-{}", std::process::id()));
        let mut app = App::new(
            &crate::backend::Waker::default(),
            crate::paths::AppDirs {
                config: root.join("config"),
                state: root.join("state"),
                cache: root.join("cache"),
            },
            crate::settings::Settings::default(),
            crate::app::AppOptions {
                media_controls: false,
                restore_sign_in: false,
                tray: false,
                autostart: false,
            },
        );
        crate::demo::populate(&mut app);
        let seed = resolve_live_radio(&app).expect("demo home yields a live radio seed");
        assert!(!seed.uri.is_empty());
        assert!(!seed.station_name.is_empty());
        assert!(seed.up_next.len() <= LIVE_RADIO_UP_NEXT_LIMIT);
        assert!(seed.up_next.iter().all(|row| row.uri != seed.uri));
        let primary = quick_access_tiles(&app);
        let extras = extra_quick_access_tiles(&app, &primary);
        assert!(!extras.is_empty());
        assert!(
            extras
                .iter()
                .all(|tile| primary.iter().all(|seen| seen.page != tile.page))
        );
        app.backend.shutdown();
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn shelf_reorder_and_bento_mode_helpers_behave() {
        assert_eq!(reorder_target_index(0, 2), Some(1));
        assert_eq!(reorder_target_index(2, 1), Some(1));
        assert_eq!(reorder_target_index(0, 0), None);
        assert_eq!(reorder_target_index(0, 1), None);

        let start = BentoWidgetMode::PeakAndFriends;
        let next = bento_cycle_mode(start, 1);
        assert_eq!(next, BentoWidgetMode::Pulse);
        assert_eq!(bento_cycle_mode(start, -1), BentoWidgetMode::ExtraQuickAccess);
        assert_eq!(bento_cycle_mode(next, -1), start);
    }

    #[cfg(feature = "demo")]
    #[test]
    fn bento_hero_aligns_bottoms_across_all_modes_and_widths() {
        let root = std::env::temp_dir().join(format!(
            "spotifast-bento-align-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let mut app = App::new(
            &crate::backend::Waker::default(),
            crate::paths::AppDirs {
                config: root.join("config"),
                state: root.join("state"),
                cache: root.join("cache"),
            },
            crate::settings::Settings::default(),
            crate::app::AppOptions {
                media_controls: false,
                restore_sign_in: false,
                tray: false,
                autostart: false,
            },
        );
        crate::demo::populate(&mut app);

        let ctx = egui::Context::default();
        theme::install(&ctx);

        let mut failures = Vec::new();
        for width in [740.0_f32, 940.0, 1200.0] {
            for mode in BentoWidgetMode::ALL {
                for range in [
                    crate::settings::PulseRange::ThisWeek,
                    crate::settings::PulseRange::AllTime,
                ] {
                    app.settings.home.bento_widget = mode;
                    app.settings.home.pulse_range = range;
                    let mut measured = None;
                    let raw = egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            vec2(width + 60.0, 800.0),
                        )),
                        ..Default::default()
                    };
                    let mut out = ctx.run_ui(raw, |ui| {
                        ui.spacing_mut().item_spacing = vec2(8.0, 6.0);
                        ui.allocate_ui_with_layout(
                            vec2(width, 600.0),
                            egui::Layout::top_down(egui::Align::Min),
                            |ui| {
                                measured = bento_hero_rects(&mut app, ui);
                            },
                        );
                    });
                    out.textures_delta.clear();
                    let (radio_rect, widget_rect) =
                        measured.expect("two-column bento hero renders");
                    let diff = (radio_rect.bottom() - widget_rect.bottom()).abs();
                    if diff > 0.5 {
                        failures.push(format!(
                            "width={width} mode={mode:?} range={range:?}: radio={:?} widget={:?} diff={diff}",
                            radio_rect, widget_rect
                        ));
                    }
                }
            }
        }
        assert!(
            failures.is_empty(),
            "bento bottom alignment mismatches:\n{}",
            failures.join("\n")
        );
        app.backend.shutdown();
        let _ = std::fs::remove_dir_all(root);
    }
}
