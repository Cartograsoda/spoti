use std::{future::Future, time::Duration};

use anyhow::{Context as _, Result};
use rspotify::{clients::BaseClient, model::Market};

use crate::state::{Album, ArtistId, Context, Track, TTL_CACHE_DURATION};

use super::WebApiClient;

pub(super) const PARTIAL_ARTIST_TTL: Duration = Duration::from_secs(300);

pub(super) struct ArtistContext {
    pub context: Context,
    pub ttl: Duration,
}

pub(super) async fn load(
    client: &WebApiClient,
    artist_id: ArtistId<'_>,
    albums: impl Future<Output = Result<Vec<Album>>>,
    on_update: impl Fn(&Context),
) -> Result<ArtistContext> {
    let artist = client
        .artist(artist_id.as_ref())
        .await
        .context("get artist")?
        .into();
    let albums = albums.await.context("get artist's albums")?;
    let mut context = Context::Artist {
        artist,
        albums,
        top_tracks: Vec::new(),
        related_artists: Vec::new(),
    };
    // Publish usable data before restricted endpoints can wait through a cooldown.
    on_update(&context);
    let mut complete = true;
    #[allow(deprecated)]
    let top_tracks = client
        .artist_top_tracks(artist_id.as_ref(), Some(Market::FromToken))
        .await;
    let rate_limited = top_tracks.as_ref().err().is_some_and(|err| {
        WebApiClient::fallback_status(err) == Some(reqwest::StatusCode::TOO_MANY_REQUESTS)
    });
    match top_tracks {
        Ok(tracks) => {
            if let Context::Artist { top_tracks, .. } = &mut context {
                *top_tracks = tracks
                    .into_iter()
                    .filter_map(Track::try_from_full_track)
                    .collect();
            }
            on_update(&context);
        }
        Err(err) => {
            complete = false;
            tracing::warn!("Artist top tracks unavailable; keeping albums: {err:#}");
        }
    }

    // Do not add another restricted request to a client already being throttled.
    if !rate_limited {
        #[allow(deprecated)]
        match client.artist_related_artists(artist_id).await {
            Ok(artists) => {
                if let Context::Artist {
                    related_artists, ..
                } = &mut context
                {
                    *related_artists = artists.into_iter().map(Into::into).collect();
                }
            }
            Err(err) => {
                complete = false;
                tracing::warn!("Related artists unavailable; keeping artist page: {err:#}");
            }
        }
    }

    Ok(ArtistContext {
        context,
        ttl: if complete {
            *TTL_CACHE_DURATION
        } else {
            PARTIAL_ARTIST_TTL
        },
    })
}

#[cfg(test)]
mod tests {
    use super::{load, PARTIAL_ARTIST_TTL};
    use crate::{
        client::WebApiClient,
        state::{Album, AlbumId, ArtistId, Context, TTL_CACHE_DURATION},
    };
    use rspotify::{clients::BaseClient, AuthCodePkceSpotify, Config, Token};
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    use wiremock::{
        matchers::{method, path},
        Mock, MockServer, ResponseTemplate,
    };

    const ARTIST_ID: &str = "0LcJLqbBmaGUft1e9Mm8HV";

    fn artist_json() -> serde_json::Value {
        serde_json::json!({
            "id": ARTIST_ID, "name": "ABBA", "type": "artist",
            "uri": format!("spotify:artist:{ARTIST_ID}"), "href": "",
            "external_urls": {}, "followers": {"href": null, "total": 1},
            "genres": [], "images": [], "popularity": 1
        })
    }

    fn album() -> Album {
        Album {
            id: AlbumId::from_id("6C4mld4qfPk7d3g4dGqnMl").unwrap(),
            release_date: "1976".to_string(),
            name: "Arrival".to_string(),
            artists: Vec::new(),
            typ: None,
            added_at: 0,
        }
    }

    async fn client(server: &MockServer) -> WebApiClient {
        let client = AuthCodePkceSpotify::with_config(
            rspotify::Credentials::new_pkce("client-id"),
            rspotify::OAuth::default(),
            Config {
                api_base_url: format!("{}/v1", server.uri()),
                ..Default::default()
            },
        );
        *client.get_token().lock().await.unwrap() = Some(Token {
            access_token: "test".to_string(),
            expires_at: Some(chrono::Utc::now() + chrono::Duration::hours(1)),
            ..Default::default()
        });
        Mock::given(method("GET"))
            .and(path(format!("/v1/artists/{ARTIST_ID}")))
            .respond_with(ResponseTemplate::new(200).set_body_json(artist_json()))
            .expect(1)
            .mount(server)
            .await;
        WebApiClient::new(client, None)
    }

    #[tokio::test]
    async fn publishes_albums_before_throttled_top_tracks_and_skips_related() {
        let server = MockServer::start().await;
        let client = client(&server).await;
        let published = Arc::new(AtomicBool::new(false));
        let observed = published.clone();
        Mock::given(method("GET"))
            .and(path(format!("/v1/artists/{ARTIST_ID}/top-tracks")))
            .respond_with(move |_: &wiremock::Request| {
                assert!(observed.load(Ordering::SeqCst));
                ResponseTemplate::new(429).insert_header("retry-after", "30")
            })
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path(format!("/v1/artists/{ARTIST_ID}/related-artists")))
            .respond_with(ResponseTemplate::new(200))
            .expect(0)
            .mount(&server)
            .await;
        let loaded = load(
            &client,
            ArtistId::from_id(ARTIST_ID).unwrap(),
            async { Ok(vec![album()]) },
            |context| {
                assert!(matches!(context, Context::Artist { albums, .. } if albums.len() == 1));
                published.store(true, Ordering::SeqCst);
            },
        )
        .await
        .unwrap();
        assert_eq!(loaded.ttl, PARTIAL_ARTIST_TTL);
        assert!(
            matches!(loaded.context, Context::Artist { albums, top_tracks, .. } if albums.len() == 1 && top_tracks.is_empty())
        );
    }

    #[tokio::test]
    async fn successful_artist_load_keeps_normal_cache_lifetime() {
        let server = MockServer::start().await;
        let client = client(&server).await;
        Mock::given(method("GET"))
            .and(path(format!("/v1/artists/{ARTIST_ID}/top-tracks")))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"tracks": []})),
            )
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path(format!("/v1/artists/{ARTIST_ID}/related-artists")))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"artists": [artist_json()]})),
            )
            .expect(1)
            .mount(&server)
            .await;
        let loaded = load(
            &client,
            ArtistId::from_id(ARTIST_ID).unwrap(),
            async { Ok(vec![album()]) },
            |_| {},
        )
        .await
        .unwrap();
        assert_eq!(loaded.ttl, *TTL_CACHE_DURATION);
        assert!(
            matches!(loaded.context, Context::Artist { albums, related_artists, .. } if albums.len() == 1 && related_artists.len() == 1)
        );
    }

    #[tokio::test]
    async fn failed_albums_do_not_start_optional_requests() {
        let server = MockServer::start().await;
        let client = client(&server).await;
        let result = load(
            &client,
            ArtistId::from_id(ARTIST_ID).unwrap(),
            async { anyhow::bail!("albums unavailable") },
            |_| panic!("no usable page yet"),
        )
        .await;
        assert!(result.is_err());
        assert_eq!(server.received_requests().await.unwrap().len(), 1);
    }
}
