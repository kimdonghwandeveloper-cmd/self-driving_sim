//! 기어 키 동시 입력 판정.

use crate::vehicle::Gear;

/// 기어 키 처리 순서(P, R, N, D).
const GEARS: [Gear; 4] = [Gear::Park, Gear::Reverse, Gear::Neutral, Gear::Drive];

/// 이번 프레임의 기어 키 판정 결과.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GearSelection {
    None,
    Requested(Gear),
    /// 서로 다른 기어 키가 동시에 눌려 요청을 거부함.
    Conflict(Vec<Gear>),
}

/// 새로 눌린 기어 키와 눌려 있는 기어 키로 요청을 판정한다.
///
/// 새로 눌린 키가 없으면 요청 없음. 새로 눌린 키를 포함해 서로 다른 기어가 둘 이상
/// 눌려 있으면 동시 요청으로 거부한다.
pub fn resolve_gear_selection(just_pressed: &[Gear], held: &[Gear]) -> GearSelection {
    if just_pressed.is_empty() {
        return GearSelection::None;
    }
    let active: Vec<Gear> = GEARS
        .into_iter()
        .filter(|gear| just_pressed.contains(gear) || held.contains(gear))
        .collect();
    match active.as_slice() {
        [gear] => GearSelection::Requested(*gear),
        _ => GearSelection::Conflict(active),
    }
}

/// 기어 키 처리 순서 목록을 반환한다.
pub fn gears() -> [Gear; 4] {
    GEARS
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 기어 키 하나만 새로 누르면 해당 기어를 요청하는지 확인한다.
    #[test]
    fn single_gear_press_is_requested() {
        assert_eq!(
            resolve_gear_selection(&[Gear::Drive], &[Gear::Drive]),
            GearSelection::Requested(Gear::Drive)
        );
    }

    /// 기어 키를 누르고만 있고 새로 누르지 않으면 요청이 없는지 확인한다.
    #[test]
    fn no_new_press_means_no_request() {
        assert_eq!(
            resolve_gear_selection(&[], &[Gear::Drive]),
            GearSelection::None
        );
    }

    /// 서로 다른 기어 키를 같은 프레임에 누르면 충돌로 거부하는지 확인한다.
    #[test]
    fn simultaneous_gear_presses_conflict() {
        assert_eq!(
            resolve_gear_selection(&[Gear::Drive, Gear::Reverse], &[]),
            GearSelection::Conflict(vec![Gear::Reverse, Gear::Drive])
        );
    }

    /// 다른 기어 키를 누른 채 새 기어 키를 누르면 충돌로 거부하는지 확인한다.
    #[test]
    fn new_press_while_other_gear_held_conflicts() {
        assert_eq!(
            resolve_gear_selection(&[Gear::Reverse], &[Gear::Drive, Gear::Reverse]),
            GearSelection::Conflict(vec![Gear::Reverse, Gear::Drive])
        );
    }
}
