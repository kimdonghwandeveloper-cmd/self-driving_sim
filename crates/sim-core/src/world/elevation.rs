//! 지상 코스의 DEM 표본을 지역 평면에서 보간한다.
use std::path::Path;

use glam::Vec2;
use serde::{Deserialize, Serialize};

use super::map::{LatLon, project};

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ElevationFile {
    pub origin: [f64; 2],
    pub north_west: [f64; 2],
    pub samples_m: Vec<Vec<f32>>,
}

/// 1초 간격 지형 고도 격자. 높이는 격자의 최저점 기준으로 반환한다.
#[derive(Debug, Clone)]
pub struct RoadElevation {
    north_west: Vec2,
    spacing: Vec2,
    samples: Vec<Vec<f32>>,
    datum: f32,
}

impl RoadElevation {
    /// 코스와 같은 이름의 .elevation.toml이 있으면 읽어 검증한다.
    pub fn load_for_course(course_path: &Path) -> Result<Option<Self>, String> {
        let path = course_path.with_extension("elevation.toml");
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(format!("{}: {error}", path.display())),
        };
        Self::parse(&text)
            .map(Some)
            .map_err(|error| format!("{}: {error}", path.display()))
    }

    /// 격자 원점과 유효한 고도 표본만 받아 지역 좌표로 변환한다.
    pub fn parse(text: &str) -> Result<Self, String> {
        let file = toml::from_str(text).map_err(|error| error.to_string())?;
        Self::from_grid(file)
    }

    /// 원본 표고 표본을 검증하고 주행용 격자를 구성한다.
    pub(crate) fn from_grid(file: ElevationFile) -> Result<Self, String> {
        let [lat, lon] = file.origin;
        let [north, west] = file.north_west;
        let width = file.samples_m.first().map_or(0, Vec::len);
        if !(2..=512).contains(&file.samples_m.len())
            || !(2..=512).contains(&width)
            || !(-85.0..=85.0).contains(&lat)
            || !(-85.0..=85.0).contains(&north)
            || !(-180.0..=180.0).contains(&lon)
            || !(-180.0..=180.0).contains(&west)
            || file
                .samples_m
                .iter()
                .any(|row| row.len() != width || row.iter().any(|h| !(-500.0..=9000.0).contains(h)))
        {
            return Err("invalid 1-arcsecond road elevation grid".to_owned());
        }
        let origin = LatLon { lat, lon };
        let north_west = project(
            origin,
            LatLon {
                lat: north,
                lon: west,
            },
        );
        let spacing = project(
            origin,
            LatLon {
                lat: lat - 1.0 / 3600.0,
                lon: lon + 1.0 / 3600.0,
            },
        );
        // 원본 1 m 단위 계단을 완화하도록 가까운 표본에 더 큰 가중치를 준다.
        let source = file.samples_m;
        let mut samples = source.clone();
        let last_row = source.len() - 1;
        let last_col = width - 1;
        for (row_index, row) in samples.iter_mut().enumerate() {
            for (col_index, height) in row.iter_mut().enumerate() {
                let north = row_index.saturating_sub(1);
                let south = (row_index + 1).min(last_row);
                let west = col_index.saturating_sub(1);
                let east = (col_index + 1).min(last_col);
                *height = (source[row_index][col_index] * 4.0
                    + source[north][col_index]
                    + source[south][col_index]
                    + source[row_index][west]
                    + source[row_index][east])
                    / 8.0;
            }
        }
        let datum = samples
            .iter()
            .flatten()
            .copied()
            .fold(f32::INFINITY, f32::min);
        Ok(Self {
            north_west,
            spacing,
            samples,
            datum,
        })
    }

    /// 지역 좌표의 높이를 쌍선형으로 보간한다. 격자 밖에서는 가장 가까운 가장자리를 쓴다.
    pub fn height(&self, point: Vec2) -> f32 {
        let cell = (point - self.north_west) / self.spacing;
        let row = cell.y.clamp(0.0, (self.samples.len() - 1) as f32);
        let col = cell.x.clamp(0.0, (self.samples[0].len() - 1) as f32);
        let r = (row as usize).min(self.samples.len() - 2);
        let c = (col as usize).min(self.samples[0].len() - 2);
        let fraction = col - c as f32;
        let north = self.samples[r][c] * (1.0 - fraction) + self.samples[r][c + 1] * fraction;
        let south =
            self.samples[r + 1][c] * (1.0 - fraction) + self.samples[r + 1][c + 1] * fraction;
        north + (south - north) * (row - r as f32) - self.datum
    }

    /// 주어진 방향의 높이 변화량을 6 m 구간에서 계산해 차량 경사로 사용한다.
    pub fn grade(&self, position: Vec2, direction: Vec2) -> f32 {
        let offset = direction * 3.0;
        (self.height(position + offset) - self.height(position - offset)) / 6.0
    }
}
