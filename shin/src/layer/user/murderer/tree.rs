//! The family tree shared by the Check screen (the testimonies) and the Select screen (naming the culprits): the
//! original's table at `0x710017224b`, 46 entries of (kind, column, row, data), and the cursor's 2D pick.

use glam::{Vec2, vec2};

/// A connecting line tile: grid cell, tile in `line` (4 x 4 tiles of 32 x 36) and colour
pub struct LineTile {
    pub cell: (u8, u8),
    pub tile: (u8, u8),
    pub color: u32,
}

/// A person in the tree, or the "Confirm" button (`id` [`CONFIRM`])
pub struct Node {
    pub id: u8,
    pub cell: (u8, u8),
    /// The correct answer: Rudolf, Kyrie and Battler (bit 6)
    pub culprit: bool,
    /// Murdered: drawn red and skipped in the Check screen (bit 7)
    pub victim: bool,
}

pub const CONFIRM: u8 = 63;

/// The line colours, 0xAARRGGBB (`0x710016ee30`; index 0 is not drawn)
const RED: u32 = 0xffa0143c;
const BLUE: u32 = 0xff3c16b0;

const fn line(col: u8, row: u8, u: u8, v: u8, color: u32) -> LineTile {
    LineTile {
        cell: (col, row),
        tile: (u, v),
        color,
    }
}

/// Entries 0-27
pub const LINES: [LineTile; 28] = [
    line(3, 1, 0, 3, RED),
    line(7, 1, 1, 1, RED),
    line(11, 1, 1, 1, RED),
    line(3, 2, 2, 2, RED),
    line(3, 3, 2, 2, RED),
    line(3, 4, 2, 2, RED),
    line(3, 5, 2, 3, RED),
    line(7, 5, 1, 1, RED),
    line(11, 5, 1, 1, RED),
    line(3, 6, 2, 2, RED),
    line(3, 7, 2, 2, RED),
    line(3, 8, 2, 2, RED),
    line(3, 9, 2, 3, RED),
    line(7, 9, 1, 1, RED),
    line(11, 9, 1, 1, RED),
    line(3, 10, 2, 2, RED),
    line(3, 11, 2, 2, RED),
    line(3, 12, 2, 2, RED),
    line(3, 13, 2, 1, RED),
    line(7, 13, 1, 1, RED),
    line(8, 13, 1, 1, RED),
    line(9, 13, 1, 1, RED),
    line(10, 13, 1, 1, RED),
    line(11, 13, 1, 1, RED),
    line(1, 15, 2, 2, BLUE),
    line(3, 17, 1, 1, BLUE),
    line(7, 17, 1, 1, BLUE),
    line(11, 17, 1, 1, BLUE),
];

const fn node(id: u8, col: u8, row: u8, culprit: bool, victim: bool) -> Node {
    Node {
        id,
        cell: (col, row),
        culprit,
        victim,
    }
}

/// Entries 28-45, in table order (which is also the order ties are broken in)
pub const NODES: [Node; 18] = [
    node(0, 5, 1, false, false),
    node(1, 9, 1, false, false),
    node(2, 13, 1, false, false),
    node(3, 5, 5, false, true),
    node(4, 9, 5, false, true),
    node(5, 13, 5, false, false),
    node(6, 5, 9, true, true),
    node(7, 9, 9, true, true),
    node(8, 13, 9, true, false),
    node(9, 5, 13, false, true),
    node(10, 13, 13, false, false),
    node(11, 1, 5, false, false),
    node(12, 1, 13, false, true),
    node(13, 1, 17, false, false),
    node(14, 5, 17, false, false),
    node(15, 9, 17, false, false),
    node(16, 13, 17, false, false),
    node(CONFIRM, 7, 21, false, false),
];

/// Portrait texture by person id (`0x71001db1e0`)
pub const PORTRAITS: [&str; 17] = [
    "kla0", "nat0", "jes0", "eva0", "hid0", "geo0", "rud0", "kir0", "but0", "ros0", "mar1", "nan0",
    "gen0", "sha0", "kan0", "goh0", "kum0",
];

/// The Check screen's bust archive by person id (`0x71001db970`): `/chars/m_<name>.txa`; victims have none
pub const BUSTS: [Option<&str>; 17] = [
    Some("kla"),
    Some("nat"),
    Some("jes"),
    None,
    None,
    Some("geo"),
    None,
    None,
    Some("but"),
    None,
    Some("mar"),
    Some("nan"),
    None,
    Some("sha"),
    Some("kan"),
    Some("goh"),
    Some("kum"),
];

pub fn cell(node: &Node) -> Vec2 {
    vec2(node.cell.0 as f32, node.cell.1 as f32)
}

/// The cursor's move (`0x7100096740`, `0x7100099cf0`): among `candidates` (indices into `cells`, the current one
/// excluded by the caller), the one with the lowest |D| / cos^4, where D runs from `target` to the candidate and
/// cos is the angle between D and `direction`; candidates behind or beside the direction are skipped and ties keep
/// the first. `target` is the current cell plus the direction (the Select screen stretches it 7x sideways on the
/// Confirm button).
pub fn pick(
    cells: &[Vec2],
    candidates: impl IntoIterator<Item = usize>,
    target: Vec2,
    direction: Vec2,
) -> Option<usize> {
    let direction = direction.normalize();
    let mut best = None;
    let mut best_score = f32::MAX;
    for index in candidates {
        let d = cells[index] - target;
        let length = d.length();
        let cos = direction.dot(d / length);
        if cos > f32::EPSILON {
            let score = length / cos.powi(4);
            if score < best_score {
                best_score = score;
                best = Some(index);
            }
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(index: usize) -> &'static str {
        match NODES[index].id {
            CONFIRM => "Confirm",
            id => &PORTRAITS[id as usize][..3],
        }
    }

    /// The Select screen's moves from each node: up, down, left, right
    fn select_moves(from: usize) -> [Option<&'static str>; 4] {
        let cells = NODES.iter().map(cell).collect::<Vec<_>>();
        let stretch = if NODES[from].id == CONFIRM { 7.0 } else { 1.0 };
        [
            vec2(0.0, -1.0),
            vec2(0.0, 1.0),
            vec2(-1.0, 0.0),
            vec2(1.0, 0.0),
        ]
        .map(|d| {
            let target = cells[from] + vec2(d.x * stretch, d.y);
            pick(&cells, (0..NODES.len()).filter(|&i| i != from), target, d).map(name)
        })
    }

    #[test]
    fn select_moves_match_the_original() {
        // a few rows of the neighbour graph worked out from the original's formula and table
        assert_eq!(
            select_moves(0),
            [None, Some("eva"), Some("nan"), Some("nat")]
        );
        assert_eq!(
            select_moves(6),
            [Some("eva"), Some("ros"), Some("nan"), Some("kir")]
        );
        assert_eq!(
            select_moves(13),
            [Some("gen"), Some("Confirm"), None, Some("kan")]
        );
        // Kanon and Gohda tie for up from Confirm; Kanon comes first in the table
        assert_eq!(select_moves(17), [Some("kan"), None, None, None]);
    }

    #[test]
    fn check_moves_skip_victims() {
        let cells = NODES.iter().map(cell).collect::<Vec<_>>();
        let from = 0; // Krauss
        let candidates = (0..17).filter(|&i| i != from && !NODES[i].victim);
        let down = pick(
            &cells,
            candidates,
            cells[from] + vec2(0.0, 1.0),
            vec2(0.0, 1.0),
        );
        assert_eq!(down.map(name), Some("kan"));
    }
}
