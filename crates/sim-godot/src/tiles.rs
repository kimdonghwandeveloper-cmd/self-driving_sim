//! 배경 지도 타일 `SimTiles`(Godot 인터페이스 계약 v1 7-3).
//!
//! 받기·캐시·OSM 정책(동시 2개, 403·429 일시 중지, 실패 간격)은 `sim_core::picker::tiles::TileFetcher`가 하고,
//! 여기서는 이미 Godot에 보낸 타일을 기억해 같은 PNG를 다시 보내지 않는다(만료되면 다시 확인한다).

use std::collections::HashMap;
use std::sync::Arc;

use godot::prelude::*;
use sim_core::picker::tiles::{
    self, TileFetcher, TileId, TileOutcome, TileSettings, UreqTileTransport,
};

use crate::paths::repo_path;

/// 한 번에 요청할 화면 타일 수 상한(Bevy 판과 같음).
const MAX_VISIBLE_TILES: usize = 48;

/// 타일 하나의 결과 사전을 만든다(계약 7-3 `poll` 항목).
fn tile_dict(tile: TileId, status: &str, png: &PackedByteArray, error: &str) -> VarDictionary {
    let (min, max) = tile.view_rect();
    let rect = Rect2::new(
        Vector2::new(min.x, min.y),
        Vector2::new(max.x - min.x, max.y - min.y),
    );
    vdict! {
        "x" => i64::from(tile.x),
        "y" => i64::from(tile.y),
        "z" => i64::from(tile.z),
        "rect" => rect,
        "status" => status,
        "png" => png,
        "error" => error,
    }
}

/// 배경 지도 타일 받기. Godot `HTTPRequest`를 쓰지 않고 Rust가 받는다.
#[derive(GodotClass)]
#[class(init, base = RefCounted)]
pub struct SimTiles {
    /// `true`면 디스크 캐시만 쓴다. `SimTiles.new()` 직후 정한다(첫 `request_visible`에서 받기를 시작한다).
    #[var]
    offline: bool,
    fetcher: Option<TileFetcher>,
    /// Godot에 보낸 타일과 그 만료 시각(유닉스 초).
    delivered: HashMap<TileId, u64>,
}

#[godot_api]
impl SimTiles {
    /// 보이는 범위(지도 좌표, `min` 북서·`max` 남동)와 화면 1 px당 지도 m로 필요한 타일을 요청한다.
    /// 매 프레임 불러도 되며, 이미 보낸 타일은 만료 전까지 다시 요청하지 않는다.
    #[func]
    fn request_visible(&mut self, min: Vector2, max: Vector2, meters_per_px: f32) {
        let offline = self.offline;
        let fetcher = self.fetcher.get_or_insert_with(|| {
            let settings = TileSettings {
                network: !offline,
                cache_root: repo_path(tiles::TILE_CACHE_DIR),
                ..TileSettings::default()
            };
            TileFetcher::start(settings, Arc::new(UreqTileTransport))
        });
        let zoom = tiles::zoom_for_scale(meters_per_px);
        let (min, max) = (glam::Vec2::new(min.x, min.y), glam::Vec2::new(max.x, max.y));
        let now = tiles::now_s();
        let missing: Vec<TileId> = tiles::visible_tiles(min, max, zoom, MAX_VISIBLE_TILES)
            .into_iter()
            .filter(|tile| {
                self.delivered
                    .get(tile)
                    .is_none_or(|expires| *expires <= now)
            })
            .collect();
        fetcher.request(&missing);
    }

    /// 끝난 타일 결과(`{x, y, z, rect, status, png, error}`)를 꺼낸다. 기다리지 않는다.
    #[func]
    fn poll(&mut self) -> VarArray {
        let Some(fetcher) = self.fetcher.as_mut() else {
            return VarArray::new();
        };
        let mut results = VarArray::new();
        for (tile, outcome) in fetcher.poll() {
            let dict = match outcome {
                TileOutcome::Image { bytes, expires_at } => {
                    self.delivered.insert(tile, expires_at);
                    tile_dict(tile, "ok", &PackedByteArray::from(bytes), "")
                }
                TileOutcome::Failed(reason) => {
                    tile_dict(tile, "failed", &PackedByteArray::new(), &reason)
                }
            };
            results.push(&dict.to_variant());
        }
        results
    }

    /// 403·429로 멈춘 경우 다시 받을 유닉스 시각. 멈추지 않았으면 0.
    #[func]
    fn paused_until_s(&self) -> i64 {
        self.fetcher.as_ref().map_or(0, |fetcher| {
            let until = fetcher.paused_until_s();
            if until > tiles::now_s() {
                until as i64
            } else {
                0
            }
        })
    }

    /// Godot이 PNG를 읽지 못했을 때 부른다. 그 타일 캐시를 지우고 실패로 센다.
    #[func]
    fn report_bad(&mut self, x: i64, y: i64, z: i64) {
        let Some(tile) = tile_id(x, y, z) else {
            return;
        };
        self.delivered.remove(&tile);
        if let Some(fetcher) = self.fetcher.as_mut() {
            fetcher.report_bad(tile);
        }
    }

    /// Godot이 텍스처를 버렸을 때 부른다. 전달 기록만 지워 다음 요청에서 캐시로 다시 보낸다.
    #[func]
    fn forget_displayed(&mut self, x: i64, y: i64, z: i64) {
        if let Some(tile) = tile_id(x, y, z) {
            self.delivered.remove(&tile);
        }
    }
}

/// Godot 정수 세 개를 타일 번호로 바꾼다. 범위 밖이면 `None`.
fn tile_id(x: i64, y: i64, z: i64) -> Option<TileId> {
    Some(TileId {
        x: u32::try_from(x).ok()?,
        y: u32::try_from(y).ok()?,
        z: u8::try_from(z).ok()?,
    })
}
