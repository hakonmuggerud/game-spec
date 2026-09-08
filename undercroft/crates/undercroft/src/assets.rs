//! `GameDataAsset` and its loader. The prototype imported its tables as ES modules; here
//! `assets/data/` is loaded through the Bevy asset server so `config.ron` and the maps hot-reload
//! natively (the `dev` feature turns on the file watcher).
//!
//! Dispatch is by extension, so the root file has its own: `assets/data/game.gamedata.ron`. It is a
//! one-line marker; the loader reads `config.ron`, the other seven RON tables and the five map
//! texts *next to it* with [`LoadContext::read_asset_bytes`], which registers them as loader
//! dependencies — that is what makes a `config.ron` edit reload the asset. Registering the loader
//! for plain `ron` instead would hijack every `.ron` file in the project.

use bevy::asset::io::Reader;
use bevy::asset::{AssetLoader, LoadContext, LoadState, ReadAssetBytesError};
use bevy::prelude::*;
use bevy::reflect::TypePath;
use std::collections::BTreeMap;

use undercroft_data::{GameData, ZoneDef};
use undercroft_sim::creature::{self, ProfileError, Tuning};

use crate::state::GameMode;

/// The asset path of the data root, relative to the assets directory.
pub const DATA_ASSET_PATH: &str = "data/game.gamedata.ron";

/// Everything `assets/data/` holds, plus the creature tuning derived from it. The tuning lives in
/// the asset (rather than in a separate resource) so a hot reload can never leave the two out of
/// step (`creature::tuning(&config)`).
#[derive(Asset, TypePath, Debug, Clone)]
pub struct GameDataAsset {
    /// `MAPS` + `CFG` + the tables.
    pub data: GameData,
    /// `hunter.js:PROFILES` — built once from `config.ron`.
    pub tuning: Tuning,
}

impl GameDataAsset {
    /// Derive the tuning and wrap loaded data (used by the loader and the headless harness).
    pub fn from_data(data: GameData) -> Result<GameDataAsset, ProfileError> {
        let tuning = creature::tuning(&data.config)?;
        Ok(GameDataAsset { data, tuning })
    }
}

/// The handle keeping [`GameDataAsset`] alive. Inserted by [`start_load`] (or directly by the
/// headless harness).
#[derive(Resource, Debug, Clone)]
pub struct GameDataHandle(pub Handle<GameDataAsset>);

/// Anything that can go wrong loading the data root.
#[derive(Debug, thiserror::Error)]
pub enum GameDataLoadError {
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Read(#[from] ReadAssetBytesError),
    #[error("{0} is not UTF-8: {1}")]
    Utf8(String, std::string::FromUtf8Error),
    #[error("{0}")]
    Data(#[from] undercroft_data::DataError),
    #[error("zones.ron: {0}")]
    Zones(#[from] ron::error::SpannedError),
    #[error("creature tuning: {0}")]
    Tuning(#[from] ProfileError),
}

/// Reads `assets/data/*` into a [`GameDataAsset`].
#[derive(Default, TypePath)]
pub struct GameDataLoader;

impl AssetLoader for GameDataLoader {
    type Asset = GameDataAsset;
    type Settings = ();
    type Error = GameDataLoadError;

    async fn load(
        &self,
        reader: &mut dyn Reader,
        _settings: &(),
        ctx: &mut LoadContext<'_>,
    ) -> Result<GameDataAsset, GameDataLoadError> {
        // The marker file itself carries nothing; drain it so the reader is consumed.
        let mut marker = Vec::new();
        reader.read_to_end(&mut marker).await?;

        let dir = ctx
            .path()
            .path()
            .parent()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        let join = |rel: &str| {
            if dir.is_empty() {
                rel.to_string()
            } else {
                format!("{dir}/{rel}")
            }
        };

        fn to_text(name: &str, bytes: Vec<u8>) -> Result<String, GameDataLoadError> {
            String::from_utf8(bytes).map_err(|e| GameDataLoadError::Utf8(name.to_string(), e))
        }

        // `read_asset_bytes` records each path as a loader dependency, so editing any of them
        // reloads this asset when the file watcher is on.
        let mut files: BTreeMap<String, String> = BTreeMap::new();
        for stem in GameData::FILES {
            let name = format!("{stem}.ron");
            let bytes = ctx.read_asset_bytes(join(&name)).await?;
            files.insert(name.clone(), to_text(&name, bytes)?);
        }
        // Zone ids come from zones.ron, so the map list is never hard-coded here.
        let zones: Vec<ZoneDef> = ron::from_str(&files["zones.ron"])?;
        let mut maps = vec!["maps/hub.txt".to_string()];
        maps.extend(zones.iter().map(|z| format!("maps/{}.txt", z.id)));
        for name in maps {
            let bytes = ctx.read_asset_bytes(join(&name)).await?;
            files.insert(name.clone(), to_text(&name, bytes)?);
        }

        let mut read = |rel: &str| -> Result<String, String> {
            files
                .get(rel)
                .cloned()
                .ok_or_else(|| format!("{rel} was not read by the game data loader"))
        };
        GameDataAsset::from_data(GameData::from_reader(&mut read)?).map_err(Into::into)
    }

    fn extensions(&self) -> &[&str] {
        &["gamedata.ron"]
    }
}

/// The assets directory. Bevy looks next to `CARGO_MANIFEST_DIR` at dev time, which for the app
/// crate is `crates/undercroft/`; ours live one workspace up in `undercroft/assets/`. On the web
/// trunk copies the directory into `dist/assets`, so the page-relative "assets" is right.
pub fn asset_plugin() -> AssetPlugin {
    #[cfg(target_arch = "wasm32")]
    let file_path = "assets".to_string();
    #[cfg(not(target_arch = "wasm32"))]
    let file_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets").to_string();
    AssetPlugin {
        file_path,
        ..default()
    }
}

/// Kick the load off and remember the handle.
pub fn start_load(mut commands: Commands, server: Res<AssetServer>) {
    commands.insert_resource(GameDataHandle(server.load(DATA_ASSET_PATH)));
}

/// `Loading → Title` once the asset and its dependencies are in. A load failure is logged once and
/// the app stays in `Loading` (there is nothing to show without the data).
pub fn finish_load(
    server: Res<AssetServer>,
    handle: Res<GameDataHandle>,
    assets: Res<Assets<GameDataAsset>>,
    mut next: ResMut<NextState<GameMode>>,
    mut reported: Local<bool>,
) {
    match server.load_state(&handle.0) {
        LoadState::Failed(e) => {
            if !*reported {
                *reported = true;
                error!("could not load {DATA_ASSET_PATH}: {e}");
            }
        }
        _ => {
            if assets.get(&handle.0).is_some() {
                info!("game data loaded");
                next.set(GameMode::Title);
            }
        }
    }
}

/// Say so when the data hot-reloads; lanes that cache anything derived from it listen for the same
/// `AssetEvent`.
fn log_reload(mut events: MessageReader<AssetEvent<GameDataAsset>>) {
    for e in events.read() {
        if let AssetEvent::Modified { .. } = e {
            info!("game data reloaded");
        }
    }
}

/// The asset type, its loader and the `Loading → Title` handoff. Needs `AssetPlugin`, so it is part
/// of [`crate::UndercroftPlugin`] and not of [`crate::SkeletonPlugin`].
pub fn plugin(app: &mut App) {
    app.init_asset::<GameDataAsset>()
        .register_asset_loader(GameDataLoader)
        .add_systems(Startup, start_load)
        .add_systems(
            Update,
            (
                finish_load.run_if(in_state(GameMode::Loading)),
                log_reload.run_if(not(in_state(GameMode::Loading))),
            ),
        );
}
