//! 기준 시야(`61.jpg`)를 만드는 최소 실내 형상: 창 양옆의 두꺼운 내장재, 하단 창 경계, 회색 선반.
//!
//! 사진을 참고해 단순 도형으로 만든 **임시 형상**이며 실측 치수가 아니다. 창 중앙은 비워 3D 코스가
//! 보이게 하고, 위쪽 창 경계는 사진에 보이지 않으므로 상단 프레임을 만들지 않는다.
//! 전방 구성을 차체 중심 기준으로 180° 돌려 후방에도 같은 창 주변을 둔다(양방향 외형 가정, 미확인).
//! 충돌·운동 계산에는 쓰지 않는다.

use bevy::prelude::*;

use crate::vehicle::VEHICLE_FOOTPRINT;

/// 차체 높이(m). 외관 박스와 같다. 실차 치수 미확인 임시값.
pub const BODY_HEIGHT_M: f32 = 2.2;
/// 차체 앞·뒤 끝에서 창(유리면)까지 안쪽 거리(m).
const GLASS_INSET_M: f32 = 0.1;
/// 하단 창 경계·선반 윗면 높이(m).
pub const SILL_HEIGHT_M: f32 = 1.38;
/// 선반 깊이(m, 유리면에서 실내 쪽).
const SHELF_DEPTH_M: f32 = 0.6;
/// 선반 두께(m).
const SHELF_THICKNESS_M: f32 = 0.12;
/// 선반 폭(m). 차체 폭보다 약간 좁다.
const SHELF_WIDTH_M: f32 = VEHICLE_FOOTPRINT.width_m - 0.14;
/// 창 양옆 내장재의 안쪽 모서리(x 절댓값, 높이; m). 아래에서 위로 갈수록 바깥으로 기운다.
const PILLAR_INNER_BOTTOM: Vec2 = Vec2::new(0.53, SILL_HEIGHT_M - 0.05);
const PILLAR_INNER_TOP: Vec2 = Vec2::new(0.76, 2.12);
/// 창 양옆 내장재 두께(바깥 방향)·깊이(m).
const PILLAR_THICKNESS_M: f32 = 0.14;
const PILLAR_DEPTH_M: f32 = 0.4;
/// 내장재 앞쪽(실내 쪽) 밝은 회색 덧판의 x 범위(절댓값)·높이 범위·유리면에서의 깊이 범위(m).
const TRIM_X_M: (f32, f32) = (0.72, 0.88);
const TRIM_Y_M: (f32, f32) = (0.95, 1.85);
const TRIM_Z_M: (f32, f32) = (0.05, 0.35);
/// 선반 위 원형 개구부 3개의 x 위치·반지름(m)·유리면에서의 거리(m).
const VENT_X_M: [f32; 3] = [-0.3, 0.0, 0.3];
const VENT_RADIUS_M: f32 = 0.035;
const VENT_SETBACK_M: f32 = 0.08;
/// 선반 양쪽 원형 스피커 덮개의 x 위치(절댓값)·반지름(m)·유리면에서의 거리(m).
const SPEAKER_X_M: f32 = 0.62;
const SPEAKER_RADIUS_M: f32 = 0.08;
const SPEAKER_SETBACK_M: f32 = 0.4;
/// 좌우 옆창 아래 벽의 윗면 높이·두께(m).
const SIDE_SILL_HEIGHT_M: f32 = 1.15;
const SIDE_SILL_THICKNESS_M: f32 = 0.05;
/// 실내 바닥 윗면 높이·두께(m). 후방·측면 시점에서 지면이 차체 안으로 비치지 않게 한다.
const FLOOR_HEIGHT_M: f32 = 0.35;
const FLOOR_THICKNESS_M: f32 = 0.05;
/// 선반 아래 끝벽 두께(m).
const END_WALL_THICKNESS_M: f32 = 0.05;

/// 실내 부품의 재질 구분.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartMaterial {
    /// 창 양옆의 어두운 내장재.
    Pillar,
    /// 내장재 앞의 밝은 회색 덧판.
    Trim,
    /// 회색 선반.
    Shelf,
    /// 선반 위 볼록한 판.
    ShelfPanel,
    /// 검은 하단 창 경계.
    Seal,
    /// 원형 개구부·스피커 덮개.
    Opening,
    /// 옆창 아래 벽.
    SideWall,
    /// 실내 바닥.
    Floor,
}

impl PartMaterial {
    /// 재질의 기본 색을 반환한다(사진 색감을 단순화한 값).
    fn color(self) -> Color {
        match self {
            PartMaterial::Pillar => Color::srgb(0.10, 0.10, 0.11),
            PartMaterial::Trim => Color::srgb(0.42, 0.42, 0.42),
            PartMaterial::Shelf => Color::srgb(0.40, 0.40, 0.40),
            PartMaterial::ShelfPanel => Color::srgb(0.46, 0.46, 0.45),
            PartMaterial::Seal => Color::srgb(0.03, 0.03, 0.03),
            PartMaterial::Opening => Color::srgb(0.06, 0.06, 0.07),
            PartMaterial::SideWall => Color::srgb(0.30, 0.30, 0.31),
            PartMaterial::Floor => Color::srgb(0.12, 0.12, 0.13),
        }
    }
}

/// 부품 도형. 크기는 m 단위다.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PartShape {
    /// 직육면체(폭, 높이, 깊이).
    Box(Vec3),
    /// 수직축 원판(반지름, 높이).
    Disc { radius: f32, height: f32 },
}

impl PartShape {
    /// 도형의 로컬 반 크기(축 정렬 경계 상자)를 반환한다.
    pub fn half_extents(self) -> Vec3 {
        match self {
            PartShape::Box(size) => size / 2.0,
            PartShape::Disc { radius, height } => Vec3::new(radius, height / 2.0, radius),
        }
    }
}

/// 차량 로컬 좌표(후륜축 중심 원점)에 놓인 실내 부품 하나.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct InteriorPart {
    pub shape: PartShape,
    pub material: PartMaterial,
    pub transform: Transform,
}

/// 차체 중심의 차량 로컬 z(m)를 반환한다.
fn body_center_z() -> f32 {
    -VEHICLE_FOOTPRINT.rear_axle_to_center_m
}

/// 차체 중심 기준 유리면의 z(m)를 반환한다(전방 끝 기준, 음수).
fn glass_z_from_center() -> f32 {
    -(VEHICLE_FOOTPRINT.length_m / 2.0 - GLASS_INSET_M)
}

/// 전방 유리면의 차량 로컬 z(m)를 반환한다.
pub fn front_glass_z() -> f32 {
    body_center_z() + glass_z_from_center()
}

/// x-y 평면에서 `inner_bottom`→`inner_top` 모서리를 안쪽 면으로 하는 기운 막대를 만든다.
///
/// `side`는 -1(왼쪽) 또는 +1(오른쪽)이며 두께는 바깥 방향으로 쌓는다. `z`는 막대 중심 z다.
fn slanted_bar(
    inner_bottom: Vec2,
    inner_top: Vec2,
    thickness: f32,
    depth: f32,
    z: f32,
    side: f32,
) -> (PartShape, Transform) {
    let bottom = Vec2::new(inner_bottom.x * side, inner_bottom.y);
    let top = Vec2::new(inner_top.x * side, inner_top.y);
    let along = (top - bottom).normalize();
    // 막대 방향에 수직이면서 차체 바깥쪽을 향하는 단위 벡터.
    let outward = if side < 0.0 {
        Vec2::new(-along.y, along.x)
    } else {
        Vec2::new(along.y, -along.x)
    };
    let center = (bottom + top) / 2.0 + outward * thickness / 2.0;
    let angle = (-along.x).atan2(along.y);
    (
        PartShape::Box(Vec3::new(thickness, (top - bottom).length(), depth)),
        Transform::from_xyz(center.x, center.y, z).with_rotation(Quat::from_rotation_z(angle)),
    )
}

/// 차체 중심 기준(전방 -Z)으로 한쪽 끝 창 주변 부품을 만든다.
fn window_end_parts() -> Vec<InteriorPart> {
    let glass = glass_z_from_center();
    let mut parts = Vec::new();
    let mut push = |shape, material, transform| {
        parts.push(InteriorPart {
            shape,
            material,
            transform,
        });
    };

    // 선반 아래 바닥까지 막는 끝벽. 반대쪽 시점에서 선반 밑으로 지면이 비치지 않게 한다.
    push(
        PartShape::Box(Vec3::new(
            SHELF_WIDTH_M,
            SILL_HEIGHT_M - FLOOR_HEIGHT_M,
            END_WALL_THICKNESS_M,
        )),
        PartMaterial::SideWall,
        Transform::from_xyz(
            0.0,
            (SILL_HEIGHT_M + FLOOR_HEIGHT_M) / 2.0,
            glass + END_WALL_THICKNESS_M / 2.0,
        ),
    );
    // 선반과 그 위의 볼록한 판, 앞쪽 끝의 검은 창 경계.
    push(
        PartShape::Box(Vec3::new(SHELF_WIDTH_M, SHELF_THICKNESS_M, SHELF_DEPTH_M)),
        PartMaterial::Shelf,
        Transform::from_xyz(
            0.0,
            SILL_HEIGHT_M - SHELF_THICKNESS_M / 2.0,
            glass + SHELF_DEPTH_M / 2.0,
        ),
    );
    push(
        PartShape::Box(Vec3::new(0.8, 0.02, 0.25)),
        PartMaterial::ShelfPanel,
        Transform::from_xyz(0.0, SILL_HEIGHT_M + 0.01, glass + 0.4),
    );
    push(
        PartShape::Box(Vec3::new(SHELF_WIDTH_M, 0.04, 0.03)),
        PartMaterial::Seal,
        Transform::from_xyz(0.0, SILL_HEIGHT_M + 0.01, glass + 0.015),
    );
    for x in VENT_X_M {
        push(
            PartShape::Disc {
                radius: VENT_RADIUS_M,
                height: 0.01,
            },
            PartMaterial::Opening,
            Transform::from_xyz(x, SILL_HEIGHT_M + 0.005, glass + VENT_SETBACK_M),
        );
    }
    for side in [-1.0, 1.0] {
        push(
            PartShape::Disc {
                radius: SPEAKER_RADIUS_M,
                height: 0.01,
            },
            PartMaterial::Opening,
            Transform::from_xyz(
                SPEAKER_X_M * side,
                SILL_HEIGHT_M + 0.005,
                glass + SPEAKER_SETBACK_M,
            ),
        );

        // 창 양옆의 기운 어두운 내장재와 그 앞의 밝은 덧판.
        let (shape, transform) = slanted_bar(
            PILLAR_INNER_BOTTOM,
            PILLAR_INNER_TOP,
            PILLAR_THICKNESS_M,
            PILLAR_DEPTH_M,
            glass + PILLAR_DEPTH_M / 2.0,
            side,
        );
        push(shape, PartMaterial::Pillar, transform);
        let trim_size = Vec3::new(
            TRIM_X_M.1 - TRIM_X_M.0,
            TRIM_Y_M.1 - TRIM_Y_M.0,
            TRIM_Z_M.1 - TRIM_Z_M.0,
        );
        push(
            PartShape::Box(trim_size),
            PartMaterial::Trim,
            Transform::from_xyz(
                (TRIM_X_M.0 + TRIM_X_M.1) / 2.0 * side,
                (TRIM_Y_M.0 + TRIM_Y_M.1) / 2.0,
                glass + (TRIM_Z_M.0 + TRIM_Z_M.1) / 2.0,
            ),
        );
    }
    parts
}

/// 좌우 옆창 아래 벽과 실내 바닥(차체 중심 기준)을 만든다. 둘 다 앞뒤 유리면 사이 전체를 덮는다.
fn side_wall_parts() -> Vec<InteriorPart> {
    let length = -2.0 * glass_z_from_center();
    let floor = InteriorPart {
        shape: PartShape::Box(Vec3::new(
            VEHICLE_FOOTPRINT.width_m - 2.0 * SIDE_SILL_THICKNESS_M,
            FLOOR_THICKNESS_M,
            length,
        )),
        material: PartMaterial::Floor,
        transform: Transform::from_xyz(0.0, FLOOR_HEIGHT_M - FLOOR_THICKNESS_M / 2.0, 0.0),
    };
    [-1.0, 1.0]
        .into_iter()
        .map(|side| InteriorPart {
            shape: PartShape::Box(Vec3::new(SIDE_SILL_THICKNESS_M, SIDE_SILL_HEIGHT_M, length)),
            material: PartMaterial::SideWall,
            transform: Transform::from_xyz(
                (VEHICLE_FOOTPRINT.width_m / 2.0 - SIDE_SILL_THICKNESS_M) * side,
                SIDE_SILL_HEIGHT_M / 2.0,
                0.0,
            ),
        })
        .chain(std::iter::once(floor))
        .collect()
}

/// 앞뒤 창 주변, 옆벽, 바닥 부품 전체를 차량 로컬 좌표로 반환한다.
pub fn interior_parts() -> Vec<InteriorPart> {
    let to_vehicle = Transform::from_xyz(0.0, 0.0, body_center_z());
    let to_rear = to_vehicle.with_rotation(Quat::from_rotation_y(std::f32::consts::PI));
    let front = window_end_parts();
    let mut parts = Vec::with_capacity(front.len() * 2 + 2);
    for frame in [to_vehicle, to_rear] {
        parts.extend(front.iter().map(|part| InteriorPart {
            transform: frame.mul_transform(part.transform),
            ..*part
        }));
    }
    parts.extend(side_wall_parts().into_iter().map(|part| InteriorPart {
        transform: to_vehicle.mul_transform(part.transform),
        ..part
    }));
    parts
}

/// 실내 부품을 `parent`(차량 루트)의 자식으로 만든다. 기본 렌더 레이어라 1인칭 카메라에 보인다.
pub fn spawn_interior(
    parent: &mut ChildSpawnerCommands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) {
    for part in interior_parts() {
        let mesh = match part.shape {
            PartShape::Box(size) => meshes.add(Cuboid::from_size(size)),
            PartShape::Disc { radius, height } => meshes.add(Cylinder::new(radius, height)),
        };
        let material = materials.add(StandardMaterial {
            base_color: part.material.color(),
            perceptual_roughness: 0.95,
            ..default()
        });
        parent.spawn((Mesh3d(mesh), MeshMaterial3d(material), part.transform));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::camera::views::{OperatorView, ViewDirection, local_camera_transform};

    /// 부품의 경계 상자 꼭짓점 8개를 차량 로컬 좌표로 반환한다.
    fn corners(part: &InteriorPart) -> Vec<Vec3> {
        let half = part.shape.half_extents();
        let mut out = Vec::new();
        for sx in [-1.0, 1.0] {
            for sy in [-1.0, 1.0] {
                for sz in [-1.0, 1.0] {
                    out.push(part.transform.transform_point(half * Vec3::new(sx, sy, sz)));
                }
            }
        }
        out
    }

    /// 광선이 부품의 경계 상자와 만나는지(슬랩 방식) 확인한다.
    fn ray_hits(part: &InteriorPart, origin: Vec3, direction: Vec3, max_distance: f32) -> bool {
        let inverse = part.transform.compute_affine().inverse();
        let local_origin = inverse.transform_point3(origin);
        let local_dir = inverse.transform_vector3(direction);
        let half = part.shape.half_extents();
        let (mut near, mut far) = (0.0_f32, max_distance);
        for axis in 0..3 {
            if local_dir[axis].abs() < 1e-9 {
                if local_origin[axis].abs() > half[axis] {
                    return false;
                }
                continue;
            }
            let a = (-half[axis] - local_origin[axis]) / local_dir[axis];
            let b = (half[axis] - local_origin[axis]) / local_dir[axis];
            near = near.max(a.min(b));
            far = far.min(a.max(b));
            if near > far {
                return false;
            }
        }
        true
    }

    /// 모든 실내 부품이 외관 차체 박스 안에 들어가는지 확인한다(외부 카메라에서 튀어나오지 않음).
    #[test]
    fn parts_stay_inside_body() {
        let front = front_glass_z() - GLASS_INSET_M;
        let rear = front + VEHICLE_FOOTPRINT.length_m;
        let half_width = VEHICLE_FOOTPRINT.width_m / 2.0;
        for part in interior_parts() {
            for corner in corners(&part) {
                assert!(
                    corner.x.abs() <= half_width + 1e-4
                        && (-1e-4..=BODY_HEIGHT_M + 1e-4).contains(&corner.y)
                        && (front - 1e-4..=rear + 1e-4).contains(&corner.z),
                    "{part:?} corner {corner:?} outside body"
                );
            }
        }
    }

    /// 기본 눈 위치가 어떤 부품 안에도 있지 않고, 네 방향 시선 중앙이 부품에 가리지 않는지 확인한다.
    #[test]
    fn default_view_center_is_open() {
        let view = OperatorView::default();
        let parts = interior_parts();
        for direction in ViewDirection::ALL {
            let camera = local_camera_transform(&view, direction);
            let forward = *camera.forward();
            for part in &parts {
                assert!(
                    !ray_hits(part, camera.translation, forward, 10.0),
                    "{direction:?} view center blocked by {part:?}"
                );
            }
        }
    }

    /// 전방 시점의 화면 아래쪽 가장자리 시선은 선반에, 창 아래 경계 바로 위 시선은 창 밖으로 향하는지 확인한다.
    #[test]
    fn front_view_shows_shelf_below_window() {
        let view = OperatorView::default();
        let camera = local_camera_transform(&view, ViewDirection::Front);
        let half_fov = view.vertical_fov_deg.to_radians() / 2.0;
        let bottom = camera.rotation * Quat::from_rotation_x(-half_fov * 0.98) * Vec3::NEG_Z;
        let shelves: Vec<_> = interior_parts()
            .into_iter()
            .filter(|part| {
                part.material == PartMaterial::Shelf
                    && part.transform.translation.z < camera.translation.z
            })
            .collect();
        assert!(
            shelves
                .iter()
                .any(|part| ray_hits(part, camera.translation, bottom, 3.0)),
            "bottom edge of the front view should land on the shelf"
        );
    }
}
