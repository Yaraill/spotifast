//! The New releases page: every song of the latest releases from the
//! artists the account follows, newest release first, played like a
//! playlist.

use std::sync::Arc;

use egui::{CornerRadius, Sense, Vec2, pos2, vec2};

use crate::api::models::{Album, Episode, PlayableItem, Show, pick_image};
use crate::app::App;
use crate::i18n::gettext;
use crate::model::{Action, Loadable, Page, RowContext, WhatsNewFilter};
use crate::settings::{WhatsNewEvent, WhatsNewEventKind};
use crate::theme::{self, Icon};

use super::collection::{Table, remember_table_items, songs_and_duration, table, table_items_hit};
use super::widgets;

/// Spotify-style What's New feed combining releases from followed artists and
/// latest episodes from saved podcasts in reverse chronological order.
pub fn whats_new(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    let locale = app.locale;
    if let Some(marker) = app.latest_whats_new_marker()
        && app.has_unseen_whats_new()
    {
        app.actions.push(Action::MarkWhatsNewSeen(marker));
    }
    if app.home.new_releases.get().is_none() && !app.home.new_releases_loading {
        app.ensure_loaded(Page::WhatsNew);
    }

    ui.add_space(12.0);
    egui::Sides::new().show(
        ui,
        |ui| {
            theme::text(
                ui,
                gettext(locale, "What's New"),
                theme::bold(30.0),
                palette.text,
            );
        },
        |ui| {
            if theme::soft_button(
                ui,
                &palette,
                Some(Icon::Refresh),
                &gettext(locale, "Refresh"),
                false,
            )
            .clicked()
            {
                app.actions.push(Action::Reload(Page::WhatsNew));
            }
        },
    );
    ui.add_space(4.0);
    theme::text(
        ui,
        gettext(
            locale,
            "The latest releases from artists and podcasts you follow.",
        ),
        theme::regular(13.5),
        palette.secondary,
    );
    ui.add_space(12.0);

    let filter_labels: Vec<_> = WhatsNewFilter::ALL
        .iter()
        .map(|filter| (*filter, filter.label(locale)))
        .collect();
    let filter_options: Vec<(WhatsNewFilter, &str)> = filter_labels
        .iter()
        .map(|(filter, label)| (*filter, label.as_ref()))
        .collect();
    if let Some(filter) = widgets::chips(ui, &palette, &filter_options, app.home.whats_new_filter) {
        app.actions.push(Action::SetWhatsNewFilter(filter));
    }
    ui.add_space(14.0);

    enum FeedItem {
        Music {
            date: String,
            album: Album,
        },
        Podcast {
            date: String,
            show: Show,
            episode: Episode,
        },
        Activity {
            sort_key: String,
            event: WhatsNewEvent,
        },
    }

    impl FeedItem {
        fn date(&self) -> &str {
            match self {
                Self::Music { date, .. } | Self::Podcast { date, .. } => date,
                Self::Activity { sort_key, .. } => sort_key,
            }
        }
    }

    let mut feed: Vec<FeedItem> = Vec::new();
    let include_music = matches!(
        app.home.whats_new_filter,
        WhatsNewFilter::All | WhatsNewFilter::Music
    );
    let include_podcasts = matches!(
        app.home.whats_new_filter,
        WhatsNewFilter::All | WhatsNewFilter::Podcasts
    );
    let include_activity = matches!(
        app.home.whats_new_filter,
        WhatsNewFilter::All | WhatsNewFilter::Activity
    );
    if include_music && let Some(albums) = app.home.new_releases.get() {
        for album in albums {
            let date = album.release_date.clone().unwrap_or_default();
            feed.push(FeedItem::Music {
                date,
                album: album.clone(),
            });
        }
    }
    if include_podcasts {
        for (show, episodes) in &app.home.podcasts {
            if app.audiobook_shows.contains(&show.uri)
                || app.is_saved(&show.uri) == Some(false)
            {
                continue;
            }
            for episode in episodes.iter().take(2) {
                let date = episode
                    .release_date
                    .as_deref()
                    .and_then(|d| d.get(..10))
                    .unwrap_or("")
                    .to_owned();
                let mut ep = episode.clone();
                if ep.show.is_none() {
                    ep.show = Some(show.clone());
                }
                feed.push(FeedItem::Podcast {
                    date,
                    show: show.clone(),
                    episode: ep,
                });
            }
        }
    }
    if include_activity {
        for event in &app.settings.home.whats_new_events {
            let sort_key = event.timestamp.get(..10).unwrap_or("").to_owned();
            feed.push(FeedItem::Activity {
                sort_key,
                event: event.clone(),
            });
        }
    }
    feed.sort_by(|a, b| b.date().cmp(a.date()));

    if include_music && let Loadable::Failed(error) = &app.home.new_releases {
        let error = error.clone();
        widgets::error_row(ui, app, &error, Some(Page::WhatsNew));
        ui.add_space(10.0);
    }

    if feed.is_empty() {
        if app.home.new_releases_loading {
            widgets::loading_row(ui, &palette, locale);
        } else if app.home.whats_new_filter == WhatsNewFilter::Activity {
            widgets::empty_state(
                ui,
                &palette,
                Icon::Bell,
                &gettext(locale, "No activity yet"),
                &gettext(
                    locale,
                    "New followers and songs added to your playlists will show here.",
                ),
            );
        } else {
            widgets::empty_state(
                ui,
                &palette,
                Icon::Bell,
                &gettext(locale, "Nothing new yet"),
                &gettext(
                    locale,
                    "New music and podcast episodes from artists and shows you follow will show here.",
                ),
            );
        }
        return;
    }

    let art_px = app.art_px(64.0);
    let now = jiff::Timestamp::now();
    for item in &feed {
        match item {
            FeedItem::Music { date, album } => {
                let artists = crate::api::models::join_names(
                    album.artists.iter().map(|artist| artist.name.as_str()),
                );
                let date_label = if date.is_empty() {
                    String::new()
                } else {
                    crate::util::format_relative_date(locale, date, now)
                };
                let meta = match (artists.is_empty(), date_label.is_empty()) {
                    (false, false) => {
                        format!("{} • {artists} • {date_label}", album.kind_label(locale))
                    }
                    (false, true) => format!("{} • {artists}", album.kind_label(locale)),
                    (true, false) => format!("{} • {date_label}", album.kind_label(locale)),
                    (true, true) => album.kind_label(locale).into_owned(),
                };
                let context_here =
                    app.playing_context_uri().as_deref() == Some(album.uri.as_str());
                let playing_here = context_here && app.believed_playing();
                let action = whats_new_row(
                    ui,
                    app,
                    pick_image(&album.images, art_px),
                    Icon::Disc,
                    &album.name,
                    &meta,
                    playing_here,
                    context_here,
                    true,
                );
                if action.play {
                    if context_here {
                        app.actions.push(Action::TogglePlay);
                    } else {
                        app.actions.push(Action::PlayContext {
                            uri: album.uri.clone(),
                            offset_uri: None,
                            offset_index: None,
                        });
                    }
                } else if action.clicked && !album.id.is_empty() {
                    app.actions.push(Action::Open(Page::Album(album.id.clone())));
                }
            }
            FeedItem::Podcast { date, show, episode } => {
                let date_label = if date.is_empty() {
                    String::new()
                } else {
                    crate::util::format_relative_date(locale, date, now)
                };
                let dur = if let Some(resume) = episode.resume_ms()
                    && episode.duration_ms > resume
                {
                    gettext(locale, "{time} left").replace(
                        "{time}",
                        &crate::util::format_episode_ms(locale, episode.duration_ms - resume),
                    )
                } else if episode.duration_ms > 0 {
                    crate::util::format_episode_ms(locale, episode.duration_ms)
                } else {
                    String::new()
                };
                let mut parts = vec![show.name.clone()];
                if !date_label.is_empty() {
                    parts.push(date_label);
                }
                if !dur.is_empty() {
                    parts.push(dur);
                }
                let meta = parts.join(" • ");
                let playing_here =
                    app.now_playing().is_some_and(|np| np.playing && np.uri == episode.uri);
                let context_here = app.now_playing().is_some_and(|np| np.uri == episode.uri);
                let action = whats_new_row(
                    ui,
                    app,
                    pick_image(&episode.images, art_px).or_else(|| pick_image(&show.images, art_px)),
                    Icon::Mic,
                    &episode.name,
                    &meta,
                    playing_here,
                    context_here,
                    true,
                );
                if action.play {
                    if context_here {
                        app.actions.push(Action::TogglePlay);
                    } else {
                        app.actions.push(Action::PlayEpisode {
                            uri: episode.uri.clone(),
                            resume_ms: episode.resume_ms(),
                        });
                    }
                } else if action.clicked && !show.id.is_empty() {
                    app.actions.push(Action::Open(Page::Show(show.id.clone())));
                }
            }
            FeedItem::Activity { event, .. } => {
                let date_label = if event.timestamp.is_empty() {
                    String::new()
                } else {
                    crate::util::format_relative_date(locale, &event.timestamp, now)
                };
                match &event.kind {
                    WhatsNewEventKind::FollowersGained { delta, total } => {
                        let title = gettext(locale, "New followers (+{count})")
                            .replace("{count}", &delta.to_string());
                        let total_label = gettext(locale, "Total followers: {total}")
                            .replace("{total}", &total.to_string());
                        let subtitle = if date_label.is_empty() {
                            total_label
                        } else {
                            format!("{total_label} • {date_label}")
                        };
                        let _ = whats_new_row(
                            ui,
                            app,
                            None,
                            Icon::Users,
                            &title,
                            &subtitle,
                            false,
                            false,
                            false,
                        );
                    }
                    WhatsNewEventKind::PlaylistUpdated {
                        playlist_id,
                        playlist_name,
                        image_url,
                        delta_tracks,
                        total_tracks,
                    } => {
                        let uri = format!("spotify:playlist:{playlist_id}");
                        let context_here = app.playing_context_uri().as_deref() == Some(uri.as_str());
                        let playing_here = context_here && app.believed_playing();
                        let count_label = gettext(locale, "{count} new songs • {total} tracks")
                            .replace("{count}", &delta_tracks.to_string())
                            .replace("{total}", &total_tracks.to_string());
                        let subtitle = if date_label.is_empty() {
                            count_label
                        } else {
                            format!("{count_label} • {date_label}")
                        };
                        let action = whats_new_row(
                            ui,
                            app,
                            image_url.as_deref(),
                            Icon::ListMusic,
                            playlist_name,
                            &subtitle,
                            playing_here,
                            context_here,
                            true,
                        );
                        if action.play {
                            if context_here {
                                app.actions.push(Action::TogglePlay);
                            } else {
                                app.actions.push(Action::PlayContext {
                                    uri,
                                    offset_uri: None,
                                    offset_index: None,
                                });
                            }
                        } else if action.clicked {
                            app.actions
                                .push(Action::Open(Page::Playlist(playlist_id.clone())));
                        }
                    }
                }
            }
        }
        ui.add_space(6.0);
    }
}

struct WhatsNewRowAction {
    clicked: bool,
    play: bool,
}

#[allow(clippy::too_many_arguments)]
fn whats_new_row(
    ui: &mut egui::Ui,
    app: &mut App,
    image: Option<&str>,
    fallback: Icon,
    title: &str,
    subtitle: &str,
    playing_here: bool,
    context_here: bool,
    playable: bool,
) -> WhatsNewRowAction {
    let palette = app.palette;
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(vec2(width, 72.0), Sense::click());
    let mut play_clicked = false;
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
        ui.painter().rect_stroke(
            rect,
            CornerRadius::same(10),
            egui::Stroke::new(
                if context_here { 1.5 } else { 1.0 },
                if context_here {
                    palette.accent.gamma_multiply(0.8)
                } else {
                    palette.outline
                },
            ),
            egui::StrokeKind::Inside,
        );
        let cover = egui::Rect::from_min_size(
            pos2(rect.left() + 10.0, rect.center().y - 26.0),
            Vec2::splat(52.0),
        );
        widgets::paint_cover(
            ui,
            &palette,
            image,
            cover,
            6.0,
            fallback,
            Some(app.backend.art()),
        );
        let play_btn_rect = egui::Rect::from_center_size(
            pos2(rect.right() - 28.0, rect.center().y),
            Vec2::splat(36.0),
        );
        let text_right = if playable {
            play_btn_rect.left() - 10.0
        } else {
            rect.right() - 14.0
        };
        let text_rect = egui::Rect::from_min_max(
            pos2(cover.right() + 12.0, rect.top() + 8.0),
            pos2(text_right, rect.bottom() - 8.0),
        );
        crate::bidi::paint_line(
            &ui.painter().with_clip_rect(text_rect),
            text_rect.left(),
            text_rect.right(),
            rect.center().y - 10.0,
            title,
            theme::semibold(14.0),
            if context_here {
                palette.accent
            } else {
                palette.text
            },
        );
        crate::bidi::paint_line(
            &ui.painter().with_clip_rect(text_rect),
            text_rect.left(),
            text_rect.right(),
            rect.center().y + 11.0,
            subtitle,
            theme::regular(12.5),
            palette.secondary,
        );
        if playable {
            let mut child = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(play_btn_rect)
                    .layout(egui::Layout::centered_and_justified(
                        egui::Direction::LeftToRight,
                    )),
            );
            play_clicked = theme::circle_button(
                &mut child,
                if playing_here {
                    Icon::PauseFilled
                } else {
                    Icon::PlayFilled
                },
                36.0,
                if hovered || playing_here {
                    palette.accent
                } else {
                    palette.surface_active
                },
                palette.accent_hover,
                if hovered || playing_here {
                    palette.on_accent
                } else {
                    palette.text
                },
                &gettext(app.locale, if playing_here { "Pause" } else { "Play" }),
            )
            .clicked();
        }
    }
    theme::focus_ring(ui, &response);
    WhatsNewRowAction {
        clicked: response.clicked() && !play_clicked,
        play: play_clicked,
    }
}

pub fn page(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    let locale = app.locale;
    if app.home.new_release_tracks.get().is_none() && !app.home.new_release_tracks.is_loading() {
        app.ensure_loaded(Page::NewReleases);
    }
    ui.add_space(12.0);
    theme::text(
        ui,
        gettext(locale, "New releases"),
        theme::bold(30.0),
        palette.text,
    );
    ui.add_space(4.0);
    theme::text(
        ui,
        gettext(
            locale,
            "The latest releases from artists you follow, newest first.",
        ),
        theme::regular(13.5),
        palette.secondary,
    );
    ui.add_space(18.0);

    // A shelf that could not load leaves the page nothing to read.
    if let Loadable::Failed(error) = &app.home.new_releases
        && app.home.new_release_tracks.get().is_none()
    {
        let error = error.clone();
        widgets::error_row(ui, app, &error, Some(Page::NewReleases));
        return;
    }
    if app
        .home
        .new_releases
        .get()
        .is_some_and(|albums| albums.is_empty())
    {
        widgets::empty_state(
            ui,
            &palette,
            Icon::Music,
            &gettext(locale, "No new releases"),
            &gettext(
                locale,
                "New albums and singles from artists you follow will appear here.",
            ),
        );
        return;
    }
    let tracks = match &app.home.new_release_tracks {
        Loadable::Loaded(tracks) => tracks,
        Loadable::Loading | Loadable::NotLoaded => {
            widgets::loading_row(ui, &palette, locale);
            return;
        }
        Loadable::Failed(error) => {
            let error = error.clone();
            widgets::error_row(ui, app, &error, Some(Page::NewReleases));
            return;
        }
    };
    let count = u32::try_from(tracks.len()).unwrap_or(u32::MAX);
    let duration = tracks
        .iter()
        .map(|track| u64::from(track.duration_ms))
        .sum();
    let generation = app.home.new_release_tracks_generation;
    let names = app.user_names_revision;
    let items = match table_items_hit(app, &Page::NewReleases, generation, generation, names) {
        Some(items) => items,
        None => {
            let rows = app
                .home
                .new_release_tracks
                .get()
                .into_iter()
                .flatten()
                .map(|track| (PlayableItem::Track(track.clone()), None, None))
                .collect();
            remember_table_items(app, Page::NewReleases, generation, generation, names, rows)
        }
    };
    let uris: Arc<[String]> = items
        .iter()
        .map(|(item, _, _)| item.uri().to_string())
        .collect::<Vec<_>>()
        .into();

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 18.0;
        if ui
            .add_enabled_ui(!uris.is_empty(), |ui| {
                theme::circle_button(
                    ui,
                    Icon::PlayFilled,
                    56.0,
                    palette.accent,
                    palette.accent_hover,
                    palette.on_accent,
                    &gettext(locale, "Play"),
                )
            })
            .inner
            .clicked()
        {
            app.actions.push(Action::PlayUris {
                uris: uris.to_vec(),
                index: 0,
            });
        }
        let shuffle = app.playing_context_shuffle();
        if theme::icon_button(
            ui,
            Icon::Shuffle,
            26.0,
            if shuffle {
                palette.accent
            } else {
                palette.secondary
            },
            if shuffle {
                palette.accent_hover
            } else {
                palette.text
            },
            &if shuffle {
                gettext(locale, "Shuffle off")
            } else {
                gettext(locale, "Shuffle")
            },
        )
        .clicked()
        {
            app.actions.push(Action::SetShuffle(!shuffle));
        }
        theme::text(
            ui,
            songs_and_duration(locale, count, duration),
            theme::regular(13.5),
            palette.secondary,
        );
    });
    ui.add_space(12.0);

    table(
        app,
        ui,
        Table {
            items: &items,
            row_offset: 0,
            pagination: None,
            context: RowContext::Uris(Arc::clone(&uris)),
            show_album: true,
            show_cover: true,
            show_added: false,
            show_added_by: false,
            show_plays: false,
            page: Page::NewReleases,
            loading: false,
            error: None,
            can_load_more: false,
            filter: "",
            items_revision: generation,
            plays_shown_rows: false,
        },
    );
}
