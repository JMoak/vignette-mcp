use crate::lattice::{SPAN, Seat};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
    Top,
    Bottom,
}

pub fn grid(count: usize) -> Vec<Seat> {
    if count == 0 {
        return Vec::new();
    }
    let columns = (count as f64).sqrt().ceil().min(4.0) as usize;
    let rows = count.div_ceil(columns);
    (0..count).map(|at| cell(columns, at % columns, rows, at / columns)).collect()
}

pub fn lead_and_stack(count: usize, side: Side) -> Vec<Seat> {
    let rest = count.saturating_sub(1);
    if count == 0 {
        return Vec::new();
    }
    if rest == 0 {
        return vec![Seat { x: 0, y: 0, w: SPAN, h: SPAN }];
    }
    let stacked_in = if rest <= 4 { 1 } else { 2 };
    let deep = rest.div_ceil(stacked_in);
    let lead = Seat { x: 0, y: 0, w: 8, h: SPAN };
    let stack = (0..rest).map(|at| {
        let within = cell(stacked_in, at % stacked_in, deep, at / stacked_in);
        Seat { x: 8 + within.x / 3, y: within.y, w: within.w / 3, h: within.h }
    });
    std::iter::once(lead).chain(stack).map(|seat| facing(seat, side)).collect()
}

fn cell(columns: usize, column: usize, rows: usize, row: usize) -> Seat {
    let across = |at: usize| (at * SPAN as usize / columns) as i32;
    let down = |at: usize| (at * SPAN as usize / rows) as i32;
    Seat {
        x: across(column),
        y: down(row),
        w: across(column + 1) - across(column),
        h: down(row + 1) - down(row),
    }
}

fn facing(seat: Seat, side: Side) -> Seat {
    let turned = Seat { x: seat.y, y: seat.x, w: seat.h, h: seat.w };
    match side {
        Side::Left => seat,
        Side::Right => Seat { x: SPAN - seat.x - seat.w, ..seat },
        Side::Top => turned,
        Side::Bottom => Seat { y: SPAN - turned.y - turned.h, ..turned },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apart(seats: &[Seat]) -> bool {
        seats.iter().enumerate().all(|(at, one)| seats[at + 1..].iter().all(|other| !one.overlaps(*other)))
    }

    #[test]
    fn a_grid_keeps_every_pane_on_the_stage_and_apart() {
        for count in 1..=16 {
            let seats = grid(count);
            assert_eq!(seats.len(), count);
            assert!(apart(&seats), "{count}");
            assert!(seats.iter().all(|seat| seat.fitted().is_ok()), "{count}");
        }
    }

    #[test]
    fn the_lead_takes_two_thirds_on_the_side_it_is_given() {
        for count in 2..=9 {
            for side in [Side::Left, Side::Right, Side::Top, Side::Bottom] {
                let seats = lead_and_stack(count, side);
                assert!(apart(&seats), "{count} {side:?}");
                assert!(seats.iter().all(|seat| seat.fitted().is_ok()), "{count} {side:?}");
            }
        }
        assert_eq!(lead_and_stack(3, Side::Right)[0], Seat { x: 4, y: 0, w: 8, h: 12 });
        assert_eq!(lead_and_stack(3, Side::Top)[0], Seat { x: 0, y: 0, w: 12, h: 8 });
    }
}
