//! Pack large maps around their actual hubs. The small neighborhood view still uses
//! the spring layout. A full map must not run an all-pairs simulation on every rebuild.

use crate::sare::viz;

pub(super) struct Placement {
    pub positions: Vec<(f32, f32)>,
    // Local index of the hub which owns each disk in the large-map layout.
    pub hubs: Vec<usize>,
}

pub(super) fn place(count: usize, links: &[(usize, usize)], iterations: usize) -> Placement {
    if count <= 256 {
        return Placement {
            positions: viz::layout(count, links, iterations),
            hubs: Vec::new(),
        };
    }
    let mut neighbors = vec![Vec::new(); count];
    for &(a, b) in links {
        neighbors[a].push(b);
        neighbors[b].push(a);
    }
    let mut ranked: Vec<_> = (0..count).collect();
    ranked.sort_unstable_by_key(|&i| (std::cmp::Reverse(neighbors[i].len()), i));

    let mut used = vec![false; count];
    let mut groups = Vec::new();
    for hub in ranked {
        if used[hub] {
            continue;
        }
        used[hub] = true;
        let mut group = vec![hub];
        for &other in &neighbors[hub] {
            if !used[other] {
                used[other] = true;
                group.push(other);
            }
        }
        groups.push(group);
    }
    // Each hub owns a non-overlapping disk. A golden-angle spiral gives even
    // spacing inside large stars. Shelf packing also handles isolated nodes.
    groups.sort_by_key(|g| std::cmp::Reverse(g.len()));
    let radius = |n: usize| 12.0 * ((n - 1) as f32).sqrt() + 9.0;
    let width = (groups
        .iter()
        .map(|g| (2.0 * radius(g.len())).powi(2))
        .sum::<f32>()
        * 1.65)
        .sqrt();
    let mut hubs = vec![0; count];
    let mut positions = vec![(0.0, 0.0); count];
    let (mut x, mut y, mut row) = (0.0, 0.0, 0.0_f32);
    for group in groups {
        let r = radius(group.len());
        if x > 0.0 && x + 2.0 * r > width {
            x = 0.0;
            y += row;
            row = 0.0;
        }
        let hub = group[0];
        for (i, node) in group.into_iter().enumerate() {
            hubs[node] = hub;
            let distance = 12.0 * (i as f32).sqrt();
            let angle = i as f32 * 2.399_963_1;
            let (sin, cos) = angle.sin_cos();
            positions[node] = (x + r + distance * cos, y + r + distance * sin);
        }
        x += 2.0 * r;
        row = row.max(2.0 * r);
    }
    Placement { positions, hubs }
}

#[cfg(test)]
mod tests {
    use super::place;
    use std::collections::HashSet;

    #[test]
    fn large_star_parallel_edges_and_isolates_have_distinct_positions() {
        let mut links: Vec<_> = (1..20_000).map(|i| (0, i)).collect();
        links.extend([(0, 0), (1, 0), (0, 1)]);
        let placement = place(25_000, &links, 320);
        assert!(placement.hubs[..20_000].iter().all(|&hub| hub == 0));
        let positions = placement.positions;
        let distinct: HashSet<_> = positions
            .iter()
            .map(|(x, y)| (x.to_bits(), y.to_bits()))
            .collect();
        assert_eq!(distinct.len(), 25_000);
        assert_eq!(positions, place(25_000, &links, 320).positions);
    }
}
