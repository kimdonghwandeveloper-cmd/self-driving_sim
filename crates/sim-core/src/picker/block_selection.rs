//! 클릭은 후보 조회일 뿐이며 블록과 방향을 명시적으로 확정한 경우에만 선택 상태가 바뀐다.
use crate::world::road_blocks::{BlockTraversal, RoadBlockModel};
use glam::Vec2;

/// 한 블록의 허용 진행 방향과 클릭 지점까지의 지도 좌표 거리.
#[derive(Debug, Clone, PartialEq)]
pub struct BlockCandidate {
    pub traversal: BlockTraversal,
    pub distance: f32,
}
/// 도로·방향을 명시적으로 선택한 출발·경유·도착 상태. 조회는 상태를 바꾸지 않는다.
pub struct BlockSelection {
    pub model: RoadBlockModel,
    pub start: Option<BlockTraversal>,
    pub vias: Vec<BlockTraversal>,
    pub goal: Option<BlockTraversal>,
}
impl BlockSelection {
    /// 고정 블록 모델에 빈 선택 슬롯을 만든다.
    pub fn new(model: RoadBlockModel) -> Self {
        Self {
            model,
            start: None,
            vias: Vec::new(),
            goal: None,
        }
    }
    /// 반경 안의 모든 허용 블록/방향 후보를 거리·ID·방향 순으로 반환한다.
    pub fn candidates(&self, at: Vec2, radius: f32) -> Vec<BlockCandidate> {
        if !at.is_finite() || !radius.is_finite() || radius <= 0. {
            return Vec::new();
        }
        let mut found = Vec::new();
        for b in &self.model.blocks {
            let distance = b
                .nodes
                .windows(2)
                .map(|p| {
                    let a = super::tiles::to_view(p[0].pos);
                    let delta = super::tiles::to_view(p[1].pos) - a;
                    let t = if delta.length_squared() > 0. {
                        (at - a).dot(delta) / delta.length_squared()
                    } else {
                        0.
                    };
                    at.distance(a + delta * t.clamp(0., 1.))
                })
                .fold(f32::INFINITY, f32::min);
            if distance <= radius {
                for &direction in &b.directions {
                    found.push(BlockCandidate {
                        traversal: BlockTraversal {
                            block_id: b.id.clone(),
                            direction,
                        },
                        distance,
                    });
                }
            }
        }
        found.sort_by(|a, b| {
            a.distance
                .total_cmp(&b.distance)
                .then_with(|| a.traversal.cmp(&b.traversal))
        });
        found
    }
    /// 존재와 허용 방향을 확인한 뒤 지정한 슬롯만 바꾼다.
    pub fn select(&mut self, role: &str, traversal: BlockTraversal) -> Result<(), String> {
        if !self
            .model
            .blocks
            .iter()
            .any(|b| b.id == traversal.block_id && b.directions.contains(&traversal.direction))
        {
            return Err("선택한 도로 또는 진행 방향이 없습니다".into());
        }
        match role {
            "start" => self.start = Some(traversal),
            "goal" => self.goal = Some(traversal),
            "via" if self.vias.len() < 32 => self.vias.push(traversal),
            _ => return Err("지원하지 않는 선택 역할 또는 경유 상한(32개)입니다".into()),
        }
        Ok(())
    }
    /// 출발과 도착을 요구하며 경유 순서를 그대로 돌려준다. 같은 한 블록의 양 끝 선택은 단일 통과다.
    pub fn ordered(&self) -> Result<Vec<BlockTraversal>, String> {
        let start = self.start.clone().ok_or("출발 도로를 확정하세요")?;
        let goal = self.goal.clone().ok_or("도착 도로를 확정하세요")?;
        if self.vias.is_empty() && start == goal {
            return Ok(vec![start]);
        }
        Ok(std::iter::once(start)
            .chain(self.vias.clone())
            .chain(std::iter::once(goal))
            .collect())
    }
    /// 경유 항목을 한 칸씩 옮기고 범위 밖 편집은 원래 목록을 유지한다.
    pub fn move_via(&mut self, index: usize, offset: isize) -> Result<(), String> {
        let target = index
            .checked_add_signed(offset)
            .ok_or("경유 순서 범위 밖입니다")?;
        if ![-1, 1].contains(&offset) || index >= self.vias.len() || target >= self.vias.len() {
            return Err("경유 순서 범위 밖입니다".into());
        }
        self.vias.swap(index, target);
        Ok(())
    }
    /// 명시한 경유 항목 하나만 삭제한다.
    pub fn remove_via(&mut self, index: usize) -> Result<(), String> {
        if index >= self.vias.len() {
            return Err("경유 항목이 없습니다".into());
        }
        self.vias.remove(index);
        Ok(())
    }
    /// 모든 선택 슬롯을 비운다.
    pub fn clear(&mut self) {
        self.start = None;
        self.goal = None;
        self.vias.clear();
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::road_blocks::BlockDirection;
    use crate::world::{
        osm_graph::{fixtures, parse_overpass_graph},
        road_blocks::build_road_blocks,
        route_plan::RoadGraphConfig,
    };

    /// 가까운 평행 도로와 일방통행을 실제 OSM 파서로 구성한다.
    fn fixture() -> BlockSelection {
        let json = fixtures::overpass_json(&[
            (
                10,
                "service",
                &[("width", "6"), ("name", "first")],
                vec![(1, 0., 0.), (2, 60., 0.)],
            ),
            (
                20,
                "service",
                &[("width", "6"), ("name", "parallel")],
                vec![(3, 0., 8.), (4, 60., 8.)],
            ),
            (
                30,
                "service",
                &[("width", "6"), ("oneway", "yes")],
                vec![(2, 60., 0.), (5, 120., 0.)],
            ),
        ]);
        BlockSelection::new(
            build_road_blocks(
                &parse_overpass_graph(&json).unwrap(),
                &[],
                &RoadGraphConfig::default(),
            )
            .unwrap(),
        )
    }
    /// 조회만으로 가까운 도로를 확정하지 않고 평행 도로와 양방향 후보를 모두 돌려준다.
    #[test]
    fn candidates_do_not_select_and_keep_parallel_directions() {
        let s = fixture();
        let at = super::super::tiles::to_view(fixtures::offset(25., 4.));
        let found = s.candidates(at, 20.);
        assert_eq!(found.len(), 4);
        assert!(s.start.is_none() && s.goal.is_none());
        assert_eq!(found, s.candidates(at, 20.));
        assert!(s.candidates(at, 0.).is_empty());
        assert!(s.candidates(glam::Vec2::NAN, 20.).is_empty());
    }
    /// 일방통행 역방향은 후보와 명시 선택 모두에서 거부하며 기존 선택을 보존한다.
    #[test]
    fn forbidden_direction_is_rejected_transactionally() {
        let mut s = fixture();
        let b = s
            .model
            .blocks
            .iter()
            .find(|b| b.source_edges[0].way_id == 30)
            .unwrap();
        let good = BlockTraversal {
            block_id: b.id.clone(),
            direction: b.directions[0],
        };
        s.select("start", good.clone()).unwrap();
        let mut wrong = good.clone();
        wrong.direction = match good.direction {
            BlockDirection::Forward => BlockDirection::Backward,
            BlockDirection::Backward => BlockDirection::Forward,
        };
        assert!(s.select("start", wrong).is_err());
        assert_eq!(s.start, Some(good));
    }
    /// 끝점 교체 시 경유 목록을 유지하며 경유 순서 편집·삭제가 명시 순서에 반영된다.
    #[test]
    fn ordered_roles_and_via_edits_preserve_other_slots() {
        let mut s = fixture();
        let all = s.candidates(super::super::tiles::to_view(fixtures::offset(25., 4.)), 20.);
        let a = all[0].traversal.clone();
        let b = all[1].traversal.clone();
        s.select("goal", b.clone()).unwrap();
        assert!(s.ordered().is_err());
        s.select("start", a.clone()).unwrap();
        s.select("via", a.clone()).unwrap();
        s.select("via", b.clone()).unwrap();
        s.move_via(0, 1).unwrap();
        assert_eq!(s.vias, vec![b.clone(), a.clone()]);
        assert!(s.move_via(0, -1).is_err());
        s.remove_via(1).unwrap();
        s.select("start", b.clone()).unwrap();
        assert_eq!(s.ordered().unwrap(), vec![b.clone(), b.clone(), b]);
        assert!(s.select("invalid", a).is_err());
        s.clear();
        assert!(s.ordered().is_err());
    }
}
