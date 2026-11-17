//! Public user profile page: avatar hero, active/recent listening card,
//! public playlists grid, and recently played artists shelf.

use std::borrow::Cow;

use crate::app::App;
use crate::i18n::{gettext, ngettext};
use crate::model::{Action, Loadable, Page};
use crate::session_reads::{FriendActivity, FullUserProfile};
use crate::theme::{self, Icon};
use crate::util;

use super::collection::{Hero, HeroImages, hero};
use super::widgets;

/// Renders a Spotify user's public profile (`Page::UserProfile`).
pub fn show(app: &mut App, ui: &mut egui::Ui, id: &str) {
    if !app.user_pages.contains_key(id) {
        app.ensure_loaded(Page::UserProfile(id.to_string()));
    }
    let Some(page) = app.user_pages.remove(id) else {
        return;
    };
    let palette = app.palette;
    let locale = app.locale;
    let preview = preview_identity(app, id);
    let activity = friend_activity(app, id);
    let is_self = app.user_id() == Some(id);

    match &page.profile {
        Loadable::Loaded(profile) => {
            profile_hero(app, ui, profile, is_self);
            profile_actions(app, ui, profile);
            if let Some(friend) = activity.as_ref() {
                listening_banner(app, ui, friend);
                ui.add_space(16.0);
            }
            if !profile.public_playlists.is_empty() {
                theme::section_title(ui, &palette, &gettext(locale, "Public playlists"));
                ui.add_space(6.0);
                let owner_fallback = profile.name.as_deref().unwrap_or(profile.id.as_str());
                widgets::grid(ui, |ui| {
                    for playlist in &profile.public_playlists {
                        let context_here =
                            app.playing_context_uri().as_deref() == Some(playlist.uri.as_str());
                        let playing_here = context_here && app.believed_playing();
                        let subtitle = playlist
                            .followers_count
                            .filter(|&n| n > 0)
                            .map(|count| {
                                ngettext(
                                    locale,
                                    "{count} save",
                                    "{count} saves",
                                    u32::try_from(count).unwrap_or(u32::MAX),
                                )
                                .replace("{count}", &util::format_count(count))
                            })
                            .unwrap_or_else(|| {
                                playlist
                                    .owner_name
                                    .as_deref()
                                    .unwrap_or(owner_fallback)
                                    .to_string()
                            });
                        let card = widgets::card(
                            ui,
                            app,
                            playlist.image_url.as_deref(),
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
                        if card.clicked && !playlist.id.is_empty() {
                            app.actions
                                .push(Action::Open(Page::Playlist(playlist.id.clone())));
                        }
                        egui::Popup::context_menu(&card.response)
                            .id(ui.make_persistent_id(("profile-playlist-menu", &playlist.uri)))
                            .frame(widgets::menu_frame(&palette))
                            .show(|ui| {
                                widgets::context_menu_items(
                                    ui,
                                    app,
                                    &playlist.uri,
                                    &playlist.name,
                                    None,
                                );
                            });
                    }
                });
                ui.add_space(16.0);
            } else if profile.recent_artists.is_empty() {
                widgets::empty_state(
                    ui,
                    &palette,
                    Icon::User,
                    &gettext(locale, "No public playlists"),
                    &gettext(locale, "This profile has no public playlists yet."),
                );
            }

            if !profile.recent_artists.is_empty() {
                let artist_label = gettext(locale, "Artist");
                let title = gettext(locale, "Recently played artists");
                widgets::shelf(
                    ui,
                    &palette,
                    locale,
                    &format!("profile-artists-{}", profile.id),
                    &title,
                    |ui| {
                        for artist in &profile.recent_artists {
                            let context_here =
                                app.playing_context_uri().as_deref() == Some(artist.uri.as_str());
                            let playing_here = context_here && app.believed_playing();
                            let card = widgets::card(
                                ui,
                                app,
                                artist.image_url.as_deref(),
                                &artist.name,
                                &artist_label,
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
                            if card.clicked && !artist.id.is_empty() {
                                app.actions
                                    .push(Action::Open(Page::Artist(artist.id.clone())));
                            }
                            egui::Popup::context_menu(&card.response)
                                .id(ui.make_persistent_id(("profile-artist-menu", &artist.uri)))
                                .frame(widgets::menu_frame(&palette))
                                .show(|ui| {
                                    widgets::context_menu_items(
                                        ui,
                                        app,
                                        &artist.uri,
                                        &artist.name,
                                        None,
                                    );
                                });
                        }
                    },
                );
            }
        }
        Loadable::Loading | Loadable::NotLoaded => {
            if let Some((name, image_url)) = preview {
                let placeholder = FullUserProfile {
                    id: id.to_string(),
                    uri: format!("spotify:user:{id}"),
                    name: Some(name),
                    image_url,
                    ..Default::default()
                };
                profile_hero(app, ui, &placeholder, is_self);
            } else {
                ui.add_space(24.0);
            }
            if !app.network_reachable {
                widgets::empty_state(
                    ui,
                    &palette,
                    Icon::WifiOff,
                    &gettext(locale, "You're offline"),
                    &gettext(
                        locale,
                        "Spotifast will reconnect automatically when your connection returns.",
                    ),
                );
            } else {
                widgets::skeleton_rows(ui, &palette, locale, 5);
            }
        }
        Loadable::Failed(error) => {
            if let Some((name, image_url)) = preview {
                let placeholder = FullUserProfile {
                    id: id.to_string(),
                    uri: format!("spotify:user:{id}"),
                    name: Some(name),
                    image_url,
                    ..Default::default()
                };
                profile_hero(app, ui, &placeholder, is_self);
            }
            let error = error.clone();
            widgets::error_row(ui, app, &error, Some(Page::UserProfile(id.to_string())));
        }
    }

    app.user_pages.insert(id.to_string(), page);
}

fn preview_identity(app: &App, id: &str) -> Option<(String, Option<String>)> {
    if let Some(user) = &app.user
        && user.id == id
    {
        let name = user
            .display_name
            .as_deref()
            .filter(|name| !name.is_empty())
            .unwrap_or(&user.id)
            .to_string();
        let image = user.images.first().map(|img| img.url.clone());
        return Some((name, image));
    }
    if let Some(friends) = app.friends.get()
        && let Some(friend) = friends
            .iter()
            .find(|friend| util::uri_id(&friend.user_uri) == Some(id))
    {
        return Some((friend.user_name.clone(), friend.image_url.clone()));
    }
    if let Some(Some(name)) = app.user_names.get(id) {
        return Some((name.clone(), None));
    }
    None
}

fn friend_activity(app: &App, id: &str) -> Option<FriendActivity> {
    app.friends.get().and_then(|friends| {
        friends
            .iter()
            .find(|friend| util::uri_id(&friend.user_uri) == Some(id))
            .cloned()
    })
}

fn profile_hero(app: &mut App, ui: &mut egui::Ui, profile: &FullUserProfile, is_self: bool) {
    let locale = app.locale;
    let title = profile
        .name
        .as_deref()
        .filter(|name| !name.is_empty())
        .unwrap_or(&profile.id);
    let image = profile.image_url.as_deref();
    let mut byline: Vec<(String, Option<Page>)> = Vec::new();
    if is_self {
        byline.push((gettext(locale, "You").into_owned(), None));
    }
    if let Some(followers) = profile.followers_count {
        byline.push((
            ngettext(
                locale,
                "{count} follower",
                "{count} followers",
                u32::try_from(followers).unwrap_or(u32::MAX),
            )
            .replace("{count}", &util::format_count(followers)),
            None,
        ));
    }
    if let Some(following) = profile.following_count {
        byline.push((
            gettext(locale, "{count} following")
                .replace("{count}", &util::format_count(following)),
            None,
        ));
    }
    if !profile.public_playlists.is_empty() {
        let count = profile.public_playlists.len() as u64;
        byline.push((
            ngettext(
                locale,
                "{count} public playlist",
                "{count} public playlists",
                u32::try_from(count).unwrap_or(u32::MAX),
            )
            .replace("{count}", &util::format_count(count)),
            None,
        ));
    }
    hero(
        app,
        ui,
        Hero {
            images: HeroImages {
                image,
                enlarged: image,
                previous: None,
                thumbnail: image,
                align_thumbnail: false,
            },
            liked: false,
            kind: Cow::from(gettext(locale, "Profile").into_owned()),
            title,
            description: None,
            byline,
            byline_tooltip: None,
            avatar: None,
            round: true,
        },
    );
}

fn profile_actions(app: &mut App, ui: &mut egui::Ui, profile: &FullUserProfile) {
    let palette = app.palette;
    let locale = app.locale;
    ui.add_space(16.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 12.0;
        let refresh = theme::soft_button(
            ui,
            &palette,
            Some(Icon::Refresh),
            &gettext(locale, "Refresh"),
            false,
        );
        if refresh.clicked() {
            app.actions
                .push(Action::Reload(Page::UserProfile(profile.id.clone())));
        }
        if !profile.id.is_empty() {
            let copy = theme::soft_button(
                ui,
                &palette,
                Some(Icon::ExternalLink),
                &gettext(locale, "Copy link"),
                false,
            );
            if copy.clicked() {
                ui.ctx()
                    .copy_text(format!("https://open.spotify.com/user/{}", profile.id));
                app.toast(gettext(locale, "Link copied"));
            }
        }
    });
    ui.add_space(20.0);
}

fn listening_banner(app: &mut App, ui: &mut egui::Ui, friend: &FriendActivity) {
    let palette = app.palette;
    let locale = app.locale;
    let status = if friend.playing {
        gettext(locale, "Listening now")
    } else {
        gettext(locale, "Recently listened")
    };
    egui::Frame::new()
        .fill(palette.surface)
        .stroke(egui::Stroke::new(1.0, palette.outline))
        .corner_radius(egui::CornerRadius::same(10))
        .inner_margin(egui::Margin::symmetric(14, 10))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 12.0;
                let (dot_rect, _) =
                    ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                let dot_color = if friend.playing {
                    palette.accent
                } else {
                    palette.secondary
                };
                ui.painter()
                    .circle_filled(dot_rect.center(), 4.0, dot_color);

                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    theme::text(ui, &*status, theme::semibold(11.5), dot_color);
                    let line = if friend.artist_name.is_empty() {
                        friend.track_name.clone()
                    } else {
                        format!("{} — {}", friend.track_name, friend.artist_name)
                    };
                    if !friend.track_uri.is_empty() {
                        let resp = theme::link(ui, &line, theme::semibold(14.0), palette.text)
                            .on_hover_text(
                                gettext(locale, "Play {song}").replace("{song}", &friend.track_name),
                            );
                        if resp.clicked() {
                            app.actions.push(Action::PlayUris {
                                uris: vec![friend.track_uri.clone()],
                                index: 0,
                            });
                        }
                    } else {
                        theme::text(ui, &line, theme::semibold(14.0), palette.text);
                    }
                    if let (Some(context_uri), Some(context_name)) =
                        (&friend.context_uri, &friend.context_name)
                        && !context_name.is_empty()
                        && let Some(link) = crate::link::parse(context_uri)
                    {
                        if theme::link(
                            ui,
                            context_name,
                            theme::regular(12.5),
                            palette.secondary,
                        )
                        .clicked()
                        {
                            app.actions.push(Action::OpenLink(link));
                        }
                    }
                });
            });
        });
}
