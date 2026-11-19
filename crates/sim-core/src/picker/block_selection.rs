//! 클릭은 후보 조회일 뿐이며 블록과 방향을 명시적으로 확정한 경우에만 선택 상태가 바뀐다.
use crate::world::block_route::plan_block_route;
use crate::world::road_blocks::{BlockTraversal, RoadBlockModel};
use glam::Vec2;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

/// 확정에 실패한 원인과 지도에서 강조할 원본 두 블록 및 선택 번호.
#[derive(Debug, Clone, PartialEq)]
pub struct SelectionFailure {
    pub code: &'static str,
    pub message: String,
    pub from: Option<BlockTraversal>,
    pub to: Option<BlockTraversal>,
    pub index: usize,
}
impl SelectionFailure {
    /// 사용자용 설명과 두 끝 블록을 원래 선택 순서대로 보존한다.
    fn at(code: &'static str, message: String, selected: &[BlockTraversal], index: usize) -> Self {
        Self {
            code,
            message,
            from: index.checked_sub(1).and_then(|i| selected.get(i)).cloned(),
            to: selected.get(index).cloned(),
            index,
        }
    }
}
/// 길이 제한과 무관하게 회전 제한이 적용된 방향 그래프의 도달 여부를 검사한다.
fn reachable(
    adj: &BTreeMap<BlockTraversal, Vec<BlockTraversal>>,
    from: &BlockTraversal,
    to: &BlockTraversal,
) -> bool {
    let mut seen = BTreeSet::from([from.clone()]);
    let mut queue = VecDeque::from([from.clone()]);
    while let Some(at) = queue.pop_front() {
        if &at == to {
            return true;
        }
        for next in adj.get(&at).into_iter().flatten() {
            if seen.insert(next.clone()) {
                queue.push_back(next.clone());
            }
        }
    }
    false
}

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
    /// 확정 직전에 전체 순서를 검사하고 실패하면 기존 선택과 미리보기 원본을 유지한다.
    pub fn select_checked(
        &mut self,
        role: &str,
        t: BlockTraversal,
        limit: f64,
    ) -> Result<(), SelectionFailure> {
        let old = (self.start.clone(), self.vias.clone(), self.goal.clone());
        let code = if self
            .model
            .blocks
            .iter()
            .any(|b| b.id == t.block_id && !b.directions.contains(&t.direction))
        {
            "direction"
        } else {
            "selection"
        };
        let result = self
            .select(role, t.clone())
            .map_err(|message| SelectionFailure::at(code, message, std::slice::from_ref(&t), 0))
            .and_then(|()| self.check(limit));
        if result.is_err() {
            (self.start, self.vias, self.goal) = old;
        }
        result
    }
    /// 경유 순서 변경과 삭제도 연결 검사를 통과할 때만 적용한다.
    pub fn edit_checked(
        &mut self,
        index: usize,
        offset: isize,
        limit: f64,
    ) -> Result<(), SelectionFailure> {
        let old = self.vias.clone();
        let result = if offset == 0 {
            self.remove_via(index)
        } else {
            self.move_via(index, offset)
        };
        let result = result
            .map_err(|message| SelectionFailure::at("selection", message, &[], 0))
            .and_then(|()| self.check(limit));
        if result.is_err() {
            self.vias = old;
        }
        result
    }
    /// 미완성 슬롯도 현재 출발·경유·도착 순서로 검사하되 한 블록의 출발/도착은 축약한다.
    pub fn check(&self, limit: f64) -> Result<(), SelectionFailure> {
        let mut selected: Vec<_> = self
            .start
            .iter()
            .chain(&self.vias)
            .chain(self.goal.iter())
            .cloned()
            .collect();
        if self.vias.is_empty() && self.start.is_some() && self.start == self.goal {
            selected.truncate(1);
        }
        if selected.is_empty() {
            return Ok(());
        }
        for (i, t) in selected.iter().enumerate() {
            let Some(b) = self.model.blocks.iter().find(|b| b.id == t.block_id) else {
                return Err(SelectionFailure::at(
                    "connection",
                    "선택 도로가 현재 지도에 없습니다. 지역을 다시 여세요.".into(),
                    &selected,
                    i,
                ));
            };
            if !b.directions.contains(&t.direction) {
                return Err(SelectionFailure::at(
                    "direction",
                    "방향 오류: 이 도로에서 허용되지 않는 주행 방향입니다.".into(),
                    &selected,
                    i,
                ));
            }
            if i > 0 && selected[i - 1] == *t {
                return Err(SelectionFailure::at(
                    "duplicate",
                    format!(
                        "중복 선택: {}번째와 {}번째가 같은 도로·방향입니다. 기존 선택은 유지했습니다.",
                        i,
                        i + 1
                    ),
                    &selected,
                    i,
                ));
            }
        }
        let mut adj: BTreeMap<BlockTraversal, Vec<BlockTraversal>> = BTreeMap::new();
        for c in &self.model.connections {
            adj.entry(c.from.clone()).or_default().push(c.to.clone());
        }
        for (i, pair) in selected.windows(2).enumerate() {
            if reachable(&adj, &pair[0], &pair[1]) {
                continue;
            }
            let alternatives = |t: &BlockTraversal| {
                self.model
                    .blocks
                    .iter()
                    .find(|b| b.id == t.block_id)
                    .unwrap()
                    .directions
                    .iter()
                    .map(|&direction| BlockTraversal {
                        block_id: t.block_id.clone(),
                        direction,
                    })
                    .collect::<Vec<_>>()
            };
            let direction = alternatives(&pair[0])
                .iter()
                .any(|a| alternatives(&pair[1]).iter().any(|b| reachable(&adj, a, b)));
            let (code, hint) = if direction {
                (
                    "direction",
                    "방향 오류: 강조한 도로의 반대 방향 후보로 연결할 수 있습니다. 화살표를 확인해 다시 선택하세요.",
                )
            } else {
                (
                    "connection",
                    "연결 오류: 현재 지도에서 두 도로 사이의 허용 연결을 찾을 수 없습니다. 연결 노드·회전 제한·지역 경계를 확인하세요.",
                )
            };
            return Err(SelectionFailure::at(
                code,
                format!(
                    "{}번째 → {}번째 선택: {} 기존 선택은 유지했습니다.",
                    i + 1,
                    i + 2,
                    hint
                ),
                &selected,
                i + 1,
            ));
        }
        plan_block_route(&self.model, &selected, limit).map(|_| ()).map_err(|e| {
            let i = e.selection_index.unwrap_or(0);
            SelectionFailure::at("distance", format!("거리 초과: 누적 경로가 최대 {:.0} m를 넘습니다. 경유를 줄이거나 도착을 가까이 선택하세요.", limit), &selected, i)
        })
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
    /// 선택 순간 연결·방향·거리 오류를 구분하고 기존 슬롯을 보존한다.
    #[test]
    fn checked_selection_reports_connection_direction_distance_and_preserves_slots() {
        let mut s = fixture();
        let traversal = |way| {
            let b = s
                .model
                .blocks
                .iter()
                .find(|b| b.source_edges[0].way_id == way)
                .unwrap();
            BlockTraversal {
                block_id: b.id.clone(),
                direction: BlockDirection::Forward,
            }
        };
        let a = traversal(10);
        let disconnected = traversal(20);
        let mut end = traversal(30);
        s.select_checked("start", a.clone(), 1000.).unwrap();
        let err = s.select_checked("goal", disconnected, 1000.).unwrap_err();
        assert_eq!(err.code, "connection");
        assert_eq!(err.from, Some(a.clone()));
        assert!(s.goal.is_none());
        let err = s.select_checked("goal", end.clone(), 80.).unwrap_err();
        assert_eq!(err.code, "distance");
        assert!(s.goal.is_none());
        // 출발 방향을 바꾸면 같은 목표에 도달할 수 있는지 검사한다.
        let mut reverse = a.clone();
        reverse.direction = BlockDirection::Backward;
        s.select_checked("start", reverse, 1000.).unwrap();
        let err = s.select_checked("goal", end.clone(), 1000.).unwrap_err();
        assert_eq!(err.code, "direction");
        s.select_checked("start", a.clone(), 1000.).unwrap();
        s.select_checked("goal", end.clone(), 1000.).unwrap();
        let err = s.select_checked("via", a.clone(), 1000.).unwrap_err();
        assert_eq!(err.code, "duplicate");
        assert!(s.vias.is_empty());
        assert_eq!(s.goal, Some(end.clone()));
        end.direction = BlockDirection::Backward;
        assert_eq!(
            s.select_checked("goal", end, 1000.).unwrap_err().code,
            "direction"
        );
    }
    /// 같은 블록의 출발·도착 확정과 도착 먼저 선택을 지원한다.
    #[test]
    fn checked_single_block_and_goal_first_are_supported() {
        let mut s = fixture();
        let b = &s.model.blocks[0];
        let t = BlockTraversal {
            block_id: b.id.clone(),
            direction: b.directions[0],
        };
        s.select_checked("goal", t.clone(), 1000.).unwrap();
        s.select_checked("start", t, 1000.).unwrap();
        assert_eq!(s.ordered().unwrap().len(), 1);
    }
    /// 차단된 회전은 방향을 무시해 허용하지 않고 연결 오류로 보고한다.
    #[test]
    fn checked_selection_respects_blocked_turn_connections() {
        let mut s = fixture();
        let a = s
            .model
            .blocks
            .iter()
            .find(|b| b.source_edges[0].way_id == 10)
            .unwrap();
        let a = BlockTraversal {
            block_id: a.id.clone(),
            direction: BlockDirection::Forward,
        };
        let b = s
            .model
            .blocks
            .iter()
            .find(|b| b.source_edges[0].way_id == 30)
            .unwrap();
        let b = BlockTraversal {
            block_id: b.id.clone(),
            direction: BlockDirection::Forward,
        };
        s.model.connections.clear();
        s.select_checked("start", a, 1000.).unwrap();
        let e = s.select_checked("goal", b, 1000.).unwrap_err();
        assert_eq!(e.code, "connection");
        assert_eq!(e.index, 1);
        assert!(s.goal.is_none());
    }
    /// 세 블록의 누적 거리와 경유 재배치 실패를 검사하며 기존 순서를 유지한다.
    #[test]
    fn checked_edits_and_cumulative_distance_preserve_route() {
        let json = fixtures::overpass_json(&[
            (
                10,
                "service",
                &[("width", "6"), ("oneway", "yes")],
                vec![(1, 0., 0.), (2, 60., 0.)],
            ),
            (
                20,
                "service",
                &[("width", "7"), ("oneway", "yes")],
                vec![(2, 60., 0.), (3, 120., 0.)],
            ),
            (
                30,
                "service",
                &[("width", "8"), ("oneway", "yes")],
                vec![(3, 120., 0.), (4, 180., 0.)],
            ),
        ]);
        let model = build_road_blocks(
            &parse_overpass_graph(&json).unwrap(),
            &[],
            &RoadGraphConfig::default(),
        )
        .unwrap();
        let t = |way| {
            let b = model
                .blocks
                .iter()
                .find(|b| b.source_edges[0].way_id == way)
                .unwrap();
            BlockTraversal {
                block_id: b.id.clone(),
                direction: BlockDirection::Forward,
            }
        };
        let (a, b, c) = (t(10), t(20), t(30));
        let mut s = BlockSelection::new(model);
        s.select_checked("start", a, 1000.).unwrap();
        s.select_checked("via", b.clone(), 1000.).unwrap();
        assert_eq!(
            s.select_checked("goal", c.clone(), 150.).unwrap_err().code,
            "distance"
        );
        s.select_checked("via", c, 1000.).unwrap();
        let before = s.vias.clone();
        assert!(s.edit_checked(0, 1, 1000.).is_err());
        assert_eq!(s.vias, before);
        s.edit_checked(0, 0, 1000.).unwrap();
        assert_eq!(s.vias.len(), 1);
        assert_ne!(s.vias[0], b);
    }
}
