//! A grid over the plan of the elements a rule looks among, so finding the
//! ones near a box does not mean testing them all.

use super::engine::Prepared;
use crate::Aabb;

/// Most cells along each side of the grid.
const MAX_CELLS: f32 = 256.0;
/// Elements covering more cells than this are tested for every query.
const MAX_CELLS_PER_ITEM: usize = 256;

pub(super) struct BoxIndex {
    /// The elements, as indices into the prepared list.
    items: Vec<usize>,
    min: [f32; 2],
    cell: f32,
    size: [usize; 2],
    /// Per cell, positions in `items`.
    cells: Vec<Vec<u32>>,
    /// Positions of elements too large for the grid.
    large: Vec<u32>,
}

impl BoxIndex {
    pub(super) fn new(prepared: &[Prepared], items: &[usize]) -> Self {
        let bounds = items
            .iter()
            .fold(Aabb::EMPTY, |acc, &i| acc.union(&prepared[i].bounds));
        let extent = (bounds.max - bounds.min).max(glam::Vec3::ZERO);
        let cell = (extent.x.max(extent.y) / MAX_CELLS).max(0.5);
        let size = [
            ((extent.x / cell) as usize + 1).max(1),
            ((extent.y / cell) as usize + 1).max(1),
        ];
        let mut index = Self {
            items: items.to_vec(),
            min: [bounds.min.x, bounds.min.y],
            cell,
            size,
            cells: vec![
                Vec::new();
                if items.is_empty() {
                    0
                } else {
                    size[0] * size[1]
                }
            ],
            large: Vec::new(),
        };
        for (position, &i) in items.iter().enumerate() {
            let [x0, x1, y0, y1] = index.range(&prepared[i].bounds);
            if (x1 - x0 + 1) * (y1 - y0 + 1) > MAX_CELLS_PER_ITEM {
                index.large.push(position as u32);
                continue;
            }
            for y in y0..=y1 {
                for x in x0..=x1 {
                    index.cells[y * size[0] + x].push(position as u32);
                }
            }
        }
        index
    }

    /// Cells covered by a box, clamped to the grid: [x0, x1, y0, y1].
    fn range(&self, b: &Aabb) -> [usize; 4] {
        let at = |v: f32, min: f32, n: usize| {
            (((v - min) / self.cell).floor().max(0.0) as usize).min(n - 1)
        };
        [
            at(b.min.x, self.min[0], self.size[0]),
            at(b.max.x, self.min[0], self.size[0]),
            at(b.min.y, self.min[1], self.size[1]),
            at(b.max.y, self.min[1], self.size[1]),
        ]
    }

    /// The elements that may meet `b`, in the order they were given. Their
    /// boxes still have to be tested.
    pub(super) fn near(&self, b: &Aabb) -> Vec<usize> {
        if self.items.is_empty() {
            return Vec::new();
        }
        let [x0, x1, y0, y1] = self.range(b);
        let mut found = self.large.clone();
        for y in y0..=y1 {
            for x in x0..=x1 {
                found.extend_from_slice(&self.cells[y * self.size[0] + x]);
            }
        }
        found.sort_unstable();
        found.dedup();
        found.into_iter().map(|p| self.items[p as usize]).collect()
    }
}
