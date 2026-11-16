//! TheAudioDB and Fanart.tv (issue #12, SPEC 7 "Providers"): artist images, backgrounds, logos
//! and biographies, covers for albums without one, and disc art. Both are optional and off until
//! the user enters their own key (Settings → Metadata). A key is checked with one request before
//! it is kept, lives in the OS keystore (for this session only when there is none), and never
//! reaches a log or the UI. Without keys, or with music information limited to file tags,
//! nothing is sent to either service.

use std::sync::Arc;

use huehuecoyotl_library::{jobs, music};
use huehuecoyotl_metadata::MetaError;
use huehuecoyotl_metadata::fanarttv::FanartTv;
use huehuecoyotl_metadata::music::{ArtistExtras, MusicProvider};
use huehuecoyotl_metadata::theaudiodb::TheAudioDb;
use huehuecoyotl_scanner::music::Cover;
use huehuecoyotl_vfs::VPath;
use serde_json::json;

use crate::{Core, Event, Inner, secrets};

/// The optional providers, in the order their images are preferred (Fanart.tv's are made for
/// display at size; TheAudioDB adds the biographies).
pub const PROVIDERS: &[&str] = &["fanarttv", "theaudiodb"];

fn credential(name: &str) -> String {
    format!("music-key:{name}")
}

pub(crate) fn display_name(name: &str) -> &'static str {
    match name {
        "fanarttv" => "Fanart.tv",
        _ => "TheAudioDB",
    }
}

/// A provider's state in Settings: `none`, `keystore`, `session` (the keystore could not keep
/// the key; it works until the app closes) or `rejected` (the service no longer accepts it).
#[derive(Default)]
pub(crate) struct Extras {
    providers: Vec<(&'static str, Arc<dyn MusicProvider>)>,
    states: Vec<(&'static str, &'static str)>,
    /// Tests: other servers for (Fanart.tv, TheAudioDB), with no pause between requests.
    bases: Option<(String, String)>,
    /// Counts key changes: a lookup still running with the old keys must not stand in for the
    /// one the new key needs (jobs are de-duplicated against running ones too).
    generation: u64,
}

impl Extras {
    fn set(&mut self, name: &'static str, p: Option<Arc<dyn MusicProvider>>, state: &'static str) {
        self.providers.retain(|(n, _)| *n != name);
        if let Some(p) = p {
            self.providers.push((name, p));
            self.providers
                .sort_by_key(|(n, _)| PROVIDERS.iter().position(|x| x == n));
        }
        self.states.retain(|(n, _)| *n != name);
        self.states.push((name, state));
    }
    fn state(&self, name: &str) -> &'static str {
        self.states
            .iter()
            .find(|(n, _)| *n == name)
            .map_or("none", |(_, s)| s)
    }
}

fn static_name(name: &str) -> Option<&'static str> {
    PROVIDERS.iter().copied().find(|n| *n == name)
}

fn make(inner: &Inner, name: &str, key: &str) -> Result<Arc<dyn MusicProvider>, MetaError> {
    let v = env!("CARGO_PKG_VERSION");
    let bases = inner.extras.read().unwrap().bases.clone();
    let fast = std::time::Duration::from_millis(1);
    Ok(match (name, bases) {
        ("fanarttv", None) => Arc::new(FanartTv::new(v, key, &inner.language)?),
        ("fanarttv", Some((f, _))) => {
            Arc::new(FanartTv::with_base(v, key, &inner.language, &f, fast)?)
        }
        (_, None) => Arc::new(TheAudioDb::new(v, key)?),
        (_, Some((_, t))) => Arc::new(TheAudioDb::with_base(v, key, &t, fast)?),
    })
}

impl Inner {
    pub(crate) fn extras(&self) -> Vec<Arc<dyn MusicProvider>> {
        self.extras
            .read()
            .unwrap()
            .providers
            .iter()
            .map(|(_, p)| p.clone())
            .collect()
    }

    /// At start-up: the stored keys (from the keystore; none with the keystore off).
    pub(crate) async fn load_music_keys(self: &Arc<Self>) {
        for &name in PROVIDERS {
            let r = credential(name);
            let key = tokio::task::spawn_blocking(move || secrets::load_password(&r))
                .await
                .unwrap_or_default();
            if key.trim().is_empty() {
                continue;
            }
            match make(self, name, &key) {
                Ok(p) => self.extras.write().unwrap().set(name, Some(p), "keystore"),
                Err(e) => tracing::warn!(provider = name, "provider not available: {e}"),
            }
        }
    }

    /// A provider rejected its key during a lookup: it stops, and Settings says so.
    fn extra_rejected(&self, name: &'static str) {
        tracing::warn!(provider = name, "the service rejected the stored key");
        self.extras.write().unwrap().set(name, None, "rejected");
        self.sink.post(keys_event(self, 0, ""));
    }
}

fn keys_event(inner: &Inner, request_id: u64, message: &str) -> Event {
    let x = inner.extras.read().unwrap();
    let mut v = serde_json::Map::new();
    for &name in PROVIDERS {
        v.insert(name.into(), json!(x.state(name)));
    }
    v.insert("message".into(), json!(message));
    Event::MusicKeys {
        request_id,
        json: serde_json::Value::Object(v).to_string(),
    }
}

/// Queue lookups of these artists (none: every artist not looked up yet), when an optional
/// provider has a key and music information may go online.
pub(crate) async fn enqueue_artists(inner: &Arc<Inner>, artists: &[i64]) {
    if inner.extras().is_empty() {
        return;
    }
    let artists = artists.to_vec();
    let generation = inner.extras.read().unwrap().generation;
    let queued = inner
        .lib
        .write(move |c| {
            if !music::online(c)? {
                return Ok(false);
            }
            let ids = if artists.is_empty() {
                music::artists_due(c)?
            } else {
                artists
            };
            for id in &ids {
                jobs::enqueue(
                    c,
                    "music_artist",
                    &json!({ "item_id": id }).to_string(),
                    jobs::priority::NORMAL,
                    Some(&format!("music_artist:{id}:{generation}")),
                )?;
            }
            Ok(!ids.is_empty())
        })
        .await;
    if matches!(queued, Ok(true)) {
        inner.wake.notify_waiters();
    }
}

fn db(e: huehuecoyotl_library::DbError) -> MetaError {
    MetaError::Other(e.to_string())
}

/// Write an image as it is (logos and disc art keep their transparency), at most 16 MiB.
async fn write_raw(inner: &Arc<Inner>, item: i64, role: &'static str, data: bytes::Bytes) {
    if data.len() > 16 << 20 {
        return;
    }
    let dir = inner.data_dir.join("artwork").join(item.to_string());
    let made = tokio::task::spawn_blocking(move || -> Result<(String, u32, u32), String> {
        let (ext, w, h) = huehuecoyotl_metadata::artwork::inspect(&data)?;
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let file = dir.join(format!("{role}.{ext}"));
        std::fs::write(&file, &data).map_err(|e| e.to_string())?;
        Ok((VPath::new(file.to_string_lossy()).to_string(), w, h))
    })
    .await;
    match made {
        Ok(Ok((local, w, h))) => {
            let _ = inner
                .lib
                .write(move |c| {
                    huehuecoyotl_library::catalog::set_artwork_tier(c, item, role, &local, w, h)
                })
                .await;
        }
        Ok(Err(e)) => tracing::warn!(item, role, "image could not be decoded: {e}"),
        Err(_) => {}
    }
}

/// A background, scaled to at most 1920 px.
async fn write_backdrop(inner: &Arc<Inner>, item: i64, data: bytes::Bytes) {
    let dir = inner.data_dir.join("artwork").join(item.to_string());
    let made = tokio::task::spawn_blocking(move || -> Result<(String, u32, u32), String> {
        let (jpg, w, h) = huehuecoyotl_metadata::artwork::scale_to_jpeg(&data, 1920)?;
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let file = dir.join("backdrop.jpg");
        std::fs::write(&file, jpg).map_err(|e| e.to_string())?;
        Ok((VPath::new(file.to_string_lossy()).to_string(), w, h))
    })
    .await;
    if let Ok(Ok((local, w, h))) = made {
        let _ = inner
            .lib
            .write(move |c| {
                huehuecoyotl_library::catalog::set_artwork_tier(c, item, "backdrop", &local, w, h)
            })
            .await;
    }
}

/// The first provider that has the image delivers it.
async fn fetch(ps: &[Arc<dyn MusicProvider>], url: &str) -> Option<bytes::Bytes> {
    for p in ps {
        if let Ok(b) = p.image(url).await {
            return Some(b);
        }
    }
    None
}

/// Look an artist up at every provider with a key: image, background, logo and biography, and
/// covers and disc art for the artist's albums.
pub(crate) async fn lookup_artist(inner: &Arc<Inner>, artist: i64) -> Result<(), MetaError> {
    let Some(t) = inner
        .lib
        .read(move |c| {
            Ok(if music::online(c)? {
                music::artist_target(c, artist)?
            } else {
                None
            })
        })
        .await
        .map_err(db)?
    else {
        return Ok(());
    };
    let named: Vec<(&'static str, Arc<dyn MusicProvider>)> =
        inner.extras.read().unwrap().providers.clone();
    if named.is_empty() {
        return Ok(());
    }
    let ps: Vec<Arc<dyn MusicProvider>> = named.iter().map(|(_, p)| p.clone()).collect();
    let mut all = ArtistExtras::default();
    for (name, p) in &named {
        match p.artist_extras(&t.mbid).await {
            Ok(Some(x)) => all.merge(x),
            Ok(None) | Err(MetaError::NotFound) => {}
            Err(MetaError::Auth) => inner.extra_rejected(name),
            Err(e) => return Err(e),
        }
    }
    if let Some(url) = &all.image
        && let Some(b) = fetch(&ps, url).await
    {
        crate::worker::write_cover(inner, &inner.local, artist, Cover::Embedded(b.to_vec())).await;
    }
    if let Some(url) = &all.background
        && let Some(b) = fetch(&ps, url).await
    {
        write_backdrop(inner, artist, b).await;
    }
    if let Some(url) = &all.logo
        && let Some(b) = fetch(&ps, url).await
    {
        write_raw(inner, artist, "logo", b).await;
    }
    // Albums: the art that came with the artist, else one lookup per album still without a
    // cover (only covers and disc art the album lacks; never over a cover it has).
    for a in &t.albums {
        let Some(group) = &a.group else { continue };
        let mut art = all
            .albums
            .iter()
            .find(|(g, _)| g == group)
            .map(|(_, x)| x.clone())
            .unwrap_or_default();
        if !a.has_cover && art.cover.is_none() {
            for p in &ps {
                if let Ok(Some(x)) = p.album_art(group).await {
                    art.cover = art.cover.or(x.cover);
                    art.disc = art.disc.or(x.disc);
                    if art.cover.is_some() {
                        break;
                    }
                }
            }
        }
        if !a.has_cover
            && let Some(url) = &art.cover
            && let Some(b) = fetch(&ps, url).await
        {
            crate::worker::write_cover(inner, &inner.local, a.id, Cover::Embedded(b.to_vec()))
                .await;
            inner.sink.post(Event::ItemChanged { item_id: a.id });
        }
        if !a.has_disc
            && let Some(url) = &art.disc
            && let Some(b) = fetch(&ps, url).await
        {
            write_raw(inner, a.id, "disc", b).await;
        }
    }
    let bio = all.biography(&inner.language).map(str::to_string);
    inner
        .lib
        .write(move |c| music::set_artist_looked_up(c, artist, bio.as_deref()))
        .await
        .map_err(db)?;
    inner.sink.post(Event::ItemChanged { item_id: artist });
    Ok(())
}

impl Core {
    /// Use these optional providers instead of the stored keys' (tests).
    pub fn set_extra_provider(&self, name: &str, p: Option<Arc<dyn MusicProvider>>) {
        if let Some(n) = static_name(name) {
            let state = if p.is_some() { "keystore" } else { "none" };
            self.inner.extras.write().unwrap().set(n, p, state);
        }
    }

    /// Tests: talk to these servers instead of Fanart.tv and TheAudioDB.
    pub fn set_extra_bases(&self, fanarttv: &str, theaudiodb: &str) {
        self.inner.extras.write().unwrap().bases = Some((fanarttv.into(), theaudiodb.into()));
    }

    /// Which optional music providers have a key (never the key itself).
    pub fn music_keys(&self, request_id: u64) {
        self.spawn(request_id, move |inner| async move {
            Ok(keys_event(&inner, request_id, ""))
        });
    }

    /// Enter (or with "" remove) the key of an optional music provider. The key is checked with
    /// one request first; a rejected key is reported now and not kept.
    pub fn set_music_key(&self, request_id: u64, provider: &str, key: &str) {
        let (provider, key) = (provider.to_string(), key.trim().to_string());
        self.spawn(request_id, move |inner| async move {
            let Some(name) = static_name(&provider) else {
                return Err(crate::CoreError::new(
                    crate::ErrorKind::Unsupported,
                    format!("unknown provider {provider}"),
                ));
            };
            let shown = display_name(name);
            let r = credential(name);
            if key.is_empty() {
                let _ = tokio::task::spawn_blocking(move || secrets::delete_password(&r)).await;
                inner.extras.write().unwrap().set(name, None, "none");
                return Ok(keys_event(&inner, request_id, ""));
            }
            let p = make(&inner, name, &key)
                .map_err(|e| crate::CoreError::new(crate::ErrorKind::Internal, e.to_string()))?;
            let message = match p.validate().await {
                Ok(()) => None,
                Err(MetaError::Auth) => Some(format!("{shown} did not accept this key.")),
                Err(MetaError::RateLimited(_)) => Some(format!(
                    "{shown} is busy and could not check the key. Try again in a minute."
                )),
                Err(_) => Some(format!(
                    "{shown} could not be reached to check the key. Try again when you are online."
                )),
            };
            if let Some(m) = message {
                return Ok(keys_event(&inner, request_id, &m));
            }
            let stored = tokio::task::spawn_blocking(move || secrets::store_password(&r, &key))
                .await
                .unwrap_or_else(|_| Err("internal error".into()));
            let (state, message) = match stored {
                Ok(()) => ("keystore", String::new()),
                Err(warn) => ("session", warn),
            };
            {
                let mut x = inner.extras.write().unwrap();
                x.set(name, Some(p), state);
                x.generation += 1;
            }
            // A new key: every artist again, so it can add what it has.
            let _ = inner.lib.write(|c| music::reset_artists(c)).await;
            enqueue_artists(&inner, &[]).await;
            Ok(keys_event(&inner, request_id, &message))
        });
    }
}
