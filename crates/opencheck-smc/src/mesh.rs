// This file is from Open BIM Suite by the OpenAEC Foundation
// (https://github.com/OpenAEC-Foundation/open-bim-suite, src/loaders/smc/mesh.rs,
// revision eb4142ee12ea28f796c07f475a24a1d0c66aa8f0), licensed under the GNU Lesser General Public
// License, version 3 or later: see ../licenses/open-bim-suite-LICENSE and
// ../licenses/open-bim-suite-NOTICE. OpenCheck's changes are marked "OpenCheck:".
// The file is the upstream one run through rustfmt, with its lints left as
// they are, so a newer revision can be taken in by copying it again.
#![allow(clippy::all, dead_code)]

//! The shapes of a coordination file as triangles.
//!
//! An element's shapes stand in its coordinate system, which stands in its storey's, and so on
//! up to the project: a shape's place in the world is the product of that chain. A shape is
//! faces as loops of points, a profile drawn along a line, several shapes as one, one shape cut
//! off by a plane (what lies on the side its normal points to goes, the cut is closed) or cut
//! by another shape (drawn uncut), or another shape's form at its own place (the file says at its
//! end which shape is whose; the form comes without where that other shape stands). The parts of
//! a compound shape stand in that shape's frame. Lengths in the file are millimetres; triangles come in metres.

use super::stream::{Data, Elem, GeoKind, V};
#[cfg(test)]
use glam::DVec4;
use glam::{DMat4, DVec2, DVec3};
use std::collections::HashMap;

/// Triangles, flat shaded: the corners of each face once, with the face's normal.
#[derive(Default)]
pub struct Mesh {
    /// OpenCheck: f64 (upstream f32), so georeferenced coordinates keep their precision.
    pub positions: Vec<f64>,
    pub normals: Vec<f32>,
    pub colors: Vec<[u8; 4]>,
    pub indices: Vec<u32>,
}

impl Mesh {
    pub fn triangles(&self) -> usize {
        self.indices.len() / 3
    }

    pub fn clear(&mut self) {
        self.positions.clear();
        self.normals.clear();
        self.colors.clear();
        self.indices.clear();
    }

    /// The corners of a face; gives the number of the first.
    fn corners(&mut self, points: &[DVec3], normal: DVec3, color: [u8; 4]) -> u32 {
        let first = (self.positions.len() / 3) as u32;
        for p in points {
            // OpenCheck: kept as f64 (upstream casts to f32)
            self.positions
                .extend([p.x * 0.001, p.y * 0.001, p.z * 0.001]);
            self.normals
                .extend([normal.x as f32, normal.y as f32, normal.z as f32]);
            self.colors.push(color);
        }
        first
    }
}

const GREY: [u8; 4] = [184, 184, 189, 255];
/// How deep shapes of shapes are followed.
const DEEPEST: u32 = 12;

/// The shape whose form each mapped shape takes, as the relations at the end of the file say.
pub fn sources(d: &Data) -> HashMap<u32, u32> {
    let mut source = HashMap::new();
    for rec in d
        .recs
        .iter()
        .filter(|r| &*d.classes[r.class as usize].short == "IsMappedBy")
    {
        let V::Geo(from) = d.field(rec, "Item", "parent") else {
            continue;
        };
        for item in d.list(d.field(rec, "Property", "value")) {
            if let V::Geo(mapped) = item {
                source.insert(*mapped, from);
            }
        }
    }
    source
}

/// Makes the triangles of shapes. One per thread: what it works out of the file (where
/// coordinate systems stand, how heavy shapes are) it keeps for itself.
pub struct Shapes<'a> {
    d: &'a Data,
    world: HashMap<u32, DMat4>,
    /// The shape whose form a mapped shape takes ([`sources`]).
    source: &'a HashMap<u32, u32>,
    /// How many triangles a shape comes to, about, once worked out.
    heavy: HashMap<u32, u32>,
    /// Shapes left out: mapped ones without a form, and shapes cut by another, drawn uncut.
    pub formless: usize,
    pub uncut: usize,
}

impl<'a> Shapes<'a> {
    pub fn new(d: &'a Data, source: &'a HashMap<u32, u32>) -> Self {
        Shapes {
            d,
            world: HashMap::new(),
            source,
            heavy: HashMap::new(),
            formless: 0,
            uncut: 0,
        }
    }

    /// About how many triangles an element's shapes come to, without making them: enough to
    /// choose which models fit.
    pub fn weight(&mut self, e: &Elem) -> usize {
        let d = self.d;
        d.list(e.children)
            .iter()
            .map(|child| {
                if let V::Geo(k) = *child {
                    self.weigh(k, 0) as usize
                } else {
                    0
                }
            })
            .sum()
    }

    fn weigh(&mut self, k: u32, depth: u32) -> u32 {
        if depth > DEEPEST {
            return 0;
        }
        if let Some(known) = self.heavy.get(&k) {
            return *known;
        }
        let d = self.d;
        let g = &d.geos[k as usize];
        // a face of n corners comes to n - 2 triangles
        let faces = |slots: [V; 5]| match (slots[1], slots[2]) {
            (V::I32s(sizes), per_face) => {
                let corners: i64 = d.i32s[sizes as usize].iter().map(|n| *n as i64).sum();
                let count = if let V::I32s(c) = per_face {
                    d.i32s[c as usize].len()
                } else {
                    d.i32s[sizes as usize].len()
                };
                (corners - 2 * count as i64).max(0) as u32
            }
            _ => 0,
        };
        let weight = match g.kind {
            GeoKind::Brep => faces(g.slots),
            GeoKind::Multi if matches!(g.slots[0], V::F32s(n) if !d.f32s[n as usize].is_empty()) => {
                faces(g.slots)
            }
            GeoKind::Multi => d
                .list(g.slots[4])
                .iter()
                .map(|item| {
                    if let V::Geo(inner) = *item {
                        self.weigh(inner, depth + 1)
                    } else {
                        0
                    }
                })
                .sum(),
            GeoKind::Mapped => self
                .source
                .get(&k)
                .copied()
                .map_or(0, |from| self.weigh(from, depth + 1)),
            GeoKind::Extrusion => {
                // two ends and a side per corner of the profile
                let corners = match g.slots[0] {
                    V::Rec(p) => {
                        let profile = &d.recs[p as usize];
                        let outline = d.list(d.any_field(profile, "outerCurve")).len();
                        if outline > 0 {
                            outline
                        } else {
                            parametric(
                                None,
                                &vec![1.0; d.list(d.any_field(profile, "parameters")).len()],
                            )
                            .iter()
                            .map(Vec::len)
                            .sum()
                        }
                    }
                    _ => 0,
                };
                corners as u32 * 4
            }
            GeoKind::Csg => {
                if let V::Geo(first) = g.slots[0] {
                    self.weigh(first, depth + 1)
                } else {
                    0
                }
            }
            GeoKind::Halfspace | GeoKind::Other => 0,
        };
        self.heavy.insert(k, weight);
        weight
    }

    fn matrix(&self, v: V) -> DMat4 {
        match v {
            V::Mat(k) => DMat4::from_cols_array(&self.d.mats[k as usize]).transpose(),
            _ => DMat4::IDENTITY,
        }
    }

    /// Where a coordinate system (or a shape taken as one) stands in the world.
    fn place(&mut self, v: V, depth: u32) -> DMat4 {
        if depth > 64 {
            return DMat4::IDENTITY;
        }
        match v {
            V::Csys(k) => {
                if let Some(m) = self.world.get(&k) {
                    return *m;
                }
                let (matrix, origin) = self.d.csys[k as usize];
                let m = self.place(origin, depth + 1) * self.matrix(matrix);
                self.world.insert(k, m);
                m
            }
            V::Geo(k) => {
                let g = &self.d.geos[k as usize];
                let (origin, matrix) = (g.origin, g.matrix);
                self.place(origin, depth + 1) * self.matrix(matrix)
            }
            _ => DMat4::IDENTITY,
        }
    }

    /// Where a part of a compound shape stands in that shape's frame: by its own matrix when
    /// it names no system to stand in, else as the two stand in the world.
    fn beside(&mut self, whole: u32, part: u32) -> DMat4 {
        let g = &self.d.geos[part as usize];
        match g.origin {
            V::Csys(_) | V::Geo(_) => {
                self.place(V::Geo(whole), 0).inverse() * self.place(V::Geo(part), 0)
            }
            _ => self.matrix(g.matrix),
        }
    }

    /// For checking the reader against the file: a compound shape's own faces, and its parts
    /// put together, both in the shape's own frame.
    #[cfg(test)]
    pub fn own_and_parts(&mut self, k: u32) -> Option<(Mesh, Mesh)> {
        let g = &self.d.geos[k as usize];
        let slots = g.slots;
        if g.kind != GeoKind::Multi
            || !matches!(slots[0], V::F32s(n) if !self.d.f32s[n as usize].is_empty())
            || self.d.list(slots[4]).is_empty()
        {
            return None;
        }
        let (mut own, mut parts) = (Mesh::default(), Mesh::default());
        self.faces(slots, DMat4::IDENTITY, GREY, &mut own);
        for item in self.d.list(slots[4]) {
            if let V::Geo(inner) = *item {
                let there = self.beside(k, inner);
                self.shape(inner, there, GREY, 1, &mut parts);
            }
        }
        Some((own, parts))
    }

    /// The triangles of an element's shapes.
    pub fn element(&mut self, e: &Elem, out: &mut Mesh) {
        for child in self.d.list(e.children) {
            if let V::Geo(k) = *child {
                let at = self.place(V::Geo(k), 0);
                self.shape(k, at, GREY, 0, out);
            }
        }
    }

    /// A shape with its own frame at `at`.
    fn shape(&mut self, k: u32, at: DMat4, color: [u8; 4], depth: u32, out: &mut Mesh) {
        if depth > DEEPEST {
            return;
        }
        let g = &self.d.geos[k as usize];
        let (kind, slots, own_matrix) = (g.kind, g.slots, g.matrix);
        let color = match g.color {
            V::Color(c) => c.to_be_bytes(),
            _ => color,
        };
        let part = |shapes: &mut Self, part: u32| at * shapes.beside(k, part);
        match kind {
            GeoKind::Brep => self.faces(slots, at, color, out),
            GeoKind::Multi => {
                if matches!(slots[0], V::F32s(n) if !self.d.f32s[n as usize].is_empty()) {
                    self.faces(slots, at, color, out);
                } else {
                    for item in self.d.list(slots[4]) {
                        if let V::Geo(inner) = *item {
                            let there = part(self, inner);
                            self.shape(inner, there, color, depth + 1, out);
                        }
                    }
                }
            }
            GeoKind::Mapped => match self.source.get(&k).copied() {
                Some(from) => {
                    // the form alone: where the other shape stands itself does not come along
                    self.shape(from, at, color, depth + 1, out);
                }
                None => self.formless += 1,
            },
            GeoKind::Extrusion => self.extrusion(slots, at, color, out),
            GeoKind::Csg => {
                let V::Geo(first) = slots[0] else { return };
                // What a cut is made of does not stand in the cut's own frame but beside it, in
                // the frame the cut itself stands in: every shape of such a tree repeats the one
                // matrix, and the plane is written in that same frame.
                let around = at * self.matrix(own_matrix).inverse();
                let inside = match self.d.geos[first as usize].origin {
                    V::Csys(_) | V::Geo(_) => self.matrix(own_matrix) * self.beside(k, first),
                    _ => self.matrix(self.d.geos[first as usize].matrix),
                };
                // holes through a plate: holes in its profile
                if self.holes_through(k, around, color, depth, out) {
                    return;
                }
                // What is taken away, as convex parts: what lies on the normal's side of a plane
                // (within its outline, when it has one), or another shape: whole when it is
                // convex, a profile drawn along a line in convex pieces when it is not.
                let second = match slots[1] {
                    V::Geo(second) => Some(second),
                    _ => None,
                };
                let there = |shapes: &mut Self, second: u32| {
                    let other = &shapes.d.geos[second as usize];
                    match other.origin {
                        V::Csys(_) | V::Geo(_) => {
                            shapes.matrix(own_matrix) * shapes.beside(k, second)
                        }
                        _ => shapes.matrix(other.matrix),
                    }
                };
                let regions: Option<Vec<Vec<(DVec3, DVec3)>>> = match (second, slots[3]) {
                    (Some(second), V::Int(0))
                        if self.d.geos[second as usize].kind == GeoKind::Halfspace =>
                    {
                        let half = &self.d.geos[second as usize];
                        let point = |v: V| {
                            if let V::Pt(p) = v {
                                Some(DVec3::from(self.d.points[p as usize].map(f64::from)))
                            } else {
                                None
                            }
                        };
                        let outline: Vec<DVec3> = self
                            .d
                            .list(half.slots[3])
                            .iter()
                            .filter_map(|v| point(*v))
                            .collect();
                        point(half.slots[0])
                            .and_then(|n| n.try_normalize())
                            .zip(point(half.slots[1]))
                            .map(|(normal, through)| vec![within(normal, through, &outline)])
                    }
                    (Some(second), V::Int(0)) => {
                        let place = there(self, second);
                        let mut hole = Mesh::default();
                        self.shape(second, place, color, depth + 1, &mut hole);
                        match convex(&hole) {
                            Some(region) => Some(vec![region]),
                            None => self.pieces(second, place),
                        }
                    }
                    // joined: both are drawn
                    (Some(second), V::Int(1)) => {
                        let place = there(self, second);
                        self.shape(first, around * inside, color, depth + 1, out);
                        self.shape(second, around * place, color, depth + 1, out);
                        return;
                    }
                    _ => None,
                };
                match regions {
                    Some(regions) if !regions.is_empty() => {
                        let mut whole = Mesh::default();
                        self.shape(first, inside, color, depth + 1, &mut whole);
                        less_all(&whole, &regions, around, out);
                    }
                    // crossed, or cut by a shape that cannot be taken in convex parts: drawn uncut
                    _ => {
                        self.uncut += 1;
                        self.shape(first, around * inside, color, depth + 1, out);
                    }
                }
            }
            GeoKind::Halfspace | GeoKind::Other => {}
        }
    }

    /// Faces as loops of points: `slots` are the points, the size of each loop and the loops of
    /// each face (the first its outline, the others holes).
    fn faces(&mut self, slots: [V; 5], at: DMat4, color: [u8; 4], out: &mut Mesh) {
        let (V::F32s(v), V::I32s(s)) = (slots[0], slots[1]) else {
            return;
        };
        let (vertices, sizes) = (&self.d.f32s[v as usize], &self.d.i32s[s as usize]);
        let per_face: &[i32] = match slots[2] {
            V::I32s(c) => &self.d.i32s[c as usize],
            _ => &[],
        };
        let point = |i: usize| {
            vertices
                .get(i * 3..i * 3 + 3)
                .map(|p| at.transform_point3(DVec3::new(p[0] as f64, p[1] as f64, p[2] as f64)))
        };
        // a mirrored placement turns every face inside out: its corners are taken the other way
        let mirrored = at.determinant() < 0.0;
        let (mut next, mut loop_at) = (0usize, 0usize);
        let mut face = 0;
        let mut loops: Vec<Vec<DVec3>> = Vec::new();
        while loop_at < sizes.len() {
            let count = per_face.get(face).copied().unwrap_or(1).max(1) as usize;
            loops.clear();
            for size in sizes[loop_at..].iter().take(count) {
                let size = (*size).max(0) as usize;
                let mut corners: Vec<DVec3> = (next..next + size).filter_map(point).collect();
                if mirrored {
                    corners.reverse();
                }
                loops.push(corners);
                next += size;
            }
            loop_at += count;
            face += 1;
            face_loops(&loops, color, out);
        }
    }

    /// A profile drawn along a line: `slots` are the profile and the two ends of the line.
    fn extrusion(&mut self, slots: [V; 5], at: DMat4, color: [u8; 4], out: &mut Mesh) {
        if let Some((loops, own, from, to)) = self.profile(slots) {
            prism(&loops, own, from, to, at, color, out);
        }
    }

    /// A profile drawn along a line with shapes cut out of it one after the other (a plate with
    /// its holes): the shapes that are profiles drawn the same way and go right through it
    /// become holes in its profile, the others are cut out as convex parts. False when the cut
    /// is not of that kind.
    fn holes_through(
        &mut self,
        k: u32,
        around: DMat4,
        color: [u8; 4],
        depth: u32,
        out: &mut Mesh,
    ) -> bool {
        let d = self.d;
        let loose = |g: u32| !matches!(d.geos[g as usize].origin, V::Csys(_) | V::Geo(_));
        let mut holes = Vec::new();
        let mut node = k;
        loop {
            let g = &d.geos[node as usize];
            match (g.kind, g.slots[0], g.slots[1], g.slots[3]) {
                (GeoKind::Csg, V::Geo(first), V::Geo(second), V::Int(0))
                    if d.geos[second as usize].kind != GeoKind::Halfspace
                        && loose(first)
                        && loose(second) =>
                {
                    holes.push(second);
                    node = first;
                }
                _ => break,
            }
        }
        let base = &d.geos[node as usize];
        if holes.is_empty() || base.kind != GeoKind::Extrusion {
            return false;
        }
        let Some((mut loops, own, from, to)) = self.profile(base.slots) else {
            return false;
        };
        let inside = self.matrix(base.matrix);
        // the plate's own frame: its profile in x and y, its line along z
        let to_plate = (inside * DMat4::from_translation(from) * own).inverse();
        let axis = own.inverse().transform_vector3(to - from);
        if axis.truncate().length() > 1e-6 * axis.length() || axis.z.abs() < 1e-6 {
            return false;
        }
        let (low, high) = (axis.z.min(0.0), axis.z.max(0.0));
        let mut left = Vec::new();
        for hole in holes {
            match self.hole_in(hole, to_plate, low, high, &loops) {
                Some(outline) => loops.push(outline),
                None => left.push(hole),
            }
        }
        if left.is_empty() {
            prism(&loops, own, from, to, around * inside, color, out);
            return true;
        }
        // the rest cut out one after the other
        let mut whole = Mesh::default();
        prism(&loops, own, from, to, inside, color, &mut whole);
        let mut regions = Vec::new();
        for hole in left {
            let place = self.matrix(d.geos[hole as usize].matrix);
            let mut solid = Mesh::default();
            self.shape(hole, place, color, depth + 1, &mut solid);
            match convex(&solid)
                .map(|region| vec![region])
                .or_else(|| self.pieces(hole, place))
            {
                Some(parts) => regions.extend(parts),
                None => self.uncut += 1,
            }
        }
        less_all(&whole, &regions, around, out);
        true
    }

    /// The outline a shape leaves in a plate's profile (`to_plate` takes the cut's frame to the
    /// plate's, whose line runs along z from `low` to `high`), when it is a profile without holes
    /// drawn the same way, right through the plate, and lies within the plate's outline apart
    /// from its other holes.
    fn hole_in(
        &self,
        hole: u32,
        to_plate: DMat4,
        low: f64,
        high: f64,
        loops: &[Vec<DVec2>],
    ) -> Option<Vec<DVec2>> {
        let g = &self.d.geos[hole as usize];
        if g.kind != GeoKind::Extrusion {
            return None;
        }
        let (rings, own, from, to) = self.profile(g.slots)?;
        let [ring] = &rings[..] else { return None };
        let there = to_plate * self.matrix(g.matrix);
        let along = there.transform_vector3(to - from);
        if along.truncate().length() > 1e-4 * along.length() {
            return None;
        }
        let at: Vec<DVec3> = ring
            .iter()
            .map(|p| there.transform_point3(own.transform_point3(DVec3::new(p.x, p.y, 0.0)) + from))
            .collect();
        let z = at[0].z;
        if at.iter().any(|p| (p.z - z).abs() > 0.1)
            || z.min(z + along.z) > low + 0.1
            || z.max(z + along.z) < high - 0.1
        {
            return None;
        }
        let outline: Vec<DVec2> = at.iter().map(|p| p.truncate()).collect();
        let area = (0..outline.len())
            .map(|i| outline[i].perp_dot(outline[(i + 1) % outline.len()]))
            .sum::<f64>()
            / 2.0;
        if area.abs() < 1e-3 {
            return None;
        }
        // within the outline, clear of its edges, and clear of the other holes
        let (lo, hi) = bounds2(&outline);
        let outer = &loops[0];
        if !outline.iter().all(|p| inside2(outer, *p)) || crosses(outer, &outline) {
            return None;
        }
        for other in &loops[1..] {
            let (olo, ohi) = bounds2(other);
            if lo.x <= ohi.x && olo.x <= hi.x && lo.y <= ohi.y && olo.y <= hi.y {
                return None;
            }
        }
        Some(outline)
    }

    /// The convex parts of a shape to cut out (each as the planes that bound it, the frame
    /// `at` stands in, millimetres): the pieces of its profile, when it is a profile drawn
    /// along a line.
    fn pieces(&mut self, k: u32, at: DMat4) -> Option<Vec<Vec<(DVec3, DVec3)>>> {
        let g = &self.d.geos[k as usize];
        if g.kind != GeoKind::Extrusion {
            return None;
        }
        let (loops, own, from, to) = self.profile(g.slots)?;
        let mut regions = Vec::new();
        for piece in convex_pieces(&loops) {
            let mut solid = Mesh::default();
            prism(&[piece], own, from, to, at, GREY, &mut solid);
            regions.push(convex(&solid)?);
        }
        Some(regions)
    }

    /// A profile's loops (the outline first, then its holes), how it stands on the line, and
    /// the line's two ends.
    fn profile(&self, slots: [V; 5]) -> Option<(Vec<Vec<DVec2>>, DMat4, DVec3, DVec3)> {
        let d = self.d;
        let V::Rec(p) = slots[0] else { return None };
        let profile = &d.recs[p as usize];
        let ends: Vec<DVec3> = d
            .list(slots[1])
            .iter()
            .filter_map(|v| {
                if let V::Pt(k) = v {
                    Some(DVec3::from(d.points[*k as usize].map(f64::from)))
                } else {
                    None
                }
            })
            .collect();
        let [from, to] = ends[..] else { return None };
        let flat = |v: V| -> Vec<DVec2> {
            d.list(v)
                .iter()
                .filter_map(|v| {
                    if let V::Pt(k) = v {
                        Some(DVec2::new(
                            d.points[*k as usize][0] as f64,
                            d.points[*k as usize][1] as f64,
                        ))
                    } else {
                        None
                    }
                })
                .collect()
        };
        let mut loops: Vec<Vec<DVec2>> = Vec::new();
        let outline = flat(d.any_field(profile, "outerCurve"));
        if !outline.is_empty() {
            loops.push(outline);
            loops.extend(
                d.list(d.any_field(profile, "innerCurves"))
                    .iter()
                    .map(|v| flat(*v))
                    .filter(|l| l.len() >= 3),
            );
        } else {
            let numbers: Vec<f64> = d
                .list(d.any_field(profile, "parameters"))
                .iter()
                .map(|v| if let V::Num(x) = v { *x as f64 } else { 0.0 })
                .collect();
            // the kind of profile the measures belong to, as the file numbers it
            let kind = match d.any_field(profile, "type") {
                V::Rec(t) => match d.any_field(&d.recs[t as usize], "ordinal") {
                    V::Int(kind) => Some(kind as i64),
                    _ => None,
                },
                _ => None,
            };
            loops = parametric(kind, &numbers);
        }
        // a loop that ends where it starts says its first point twice
        for l in &mut loops {
            if l.len() > 3
                && l.first()
                    .zip(l.last())
                    .is_some_and(|(a, b)| a.distance(*b) < 1e-6)
            {
                l.pop();
            }
        }
        if loops.first().is_none_or(|l| l.len() < 3) {
            return None;
        }
        Some((
            loops,
            self.matrix(d.any_field(profile, "transform")),
            from,
            to,
        ))
    }
}

/// A profile (loops in its own plane) drawn from one end of a line to the other: `own` stands
/// it on the line, `at` places the whole.
fn prism(
    loops: &[Vec<DVec2>],
    own: DMat4,
    from: DVec3,
    to: DVec3,
    at: DMat4,
    color: [u8; 4],
    out: &mut Mesh,
) {
    {
        let lift = |p: DVec2, end: DVec3| {
            at.transform_point3(own.transform_point3(DVec3::new(p.x, p.y, 0.0)) + end)
        };
        let (mut bottom, mut top): (Vec<Vec<DVec3>>, Vec<Vec<DVec3>>) = (
            loops
                .iter()
                .map(|l| l.iter().map(|p| lift(*p, from)).collect())
                .collect(),
            loops
                .iter()
                .map(|l| l.iter().map(|p| lift(*p, to)).collect())
                .collect(),
        );
        // every face looks out of the solid, so a cut can tell material from hollow: seen from
        // the far end the outline runs counter-clockwise and the holes clockwise, whichever way
        // the file drew them and however the shape is placed
        let along = top[0][0] - bottom[0][0];
        for (k, (low, high)) in bottom.iter_mut().zip(top.iter_mut()).enumerate() {
            if (newell(low).dot(along) > 0.0) != (k == 0) {
                low.reverse();
                high.reverse();
            }
        }
        polygon(
            &bottom
                .iter()
                .map(|l| l.iter().rev().copied().collect())
                .collect::<Vec<_>>(),
            color,
            out,
        );
        polygon(&top, color, out);
        for (low, high) in bottom.iter().zip(&top) {
            for i in 0..low.len() {
                let j = (i + 1) % low.len();
                polygon(&[vec![low[i], low[j], high[j], high[i]]], color, out);
            }
        }
    }
}

/// The corners of the box around flat points.
fn bounds2(points: &[DVec2]) -> (DVec2, DVec2) {
    points.iter().fold(
        (DVec2::splat(f64::MAX), DVec2::splat(f64::MIN)),
        |(lo, hi), p| (lo.min(*p), hi.max(*p)),
    )
}

/// Whether a point lies within a flat outline.
fn inside2(outline: &[DVec2], p: DVec2) -> bool {
    let mut inside = false;
    for i in 0..outline.len() {
        let (a, b) = (outline[i], outline[(i + 1) % outline.len()]);
        if (a.y > p.y) != (b.y > p.y) && p.x < a.x + (b.x - a.x) * (p.y - a.y) / (b.y - a.y) {
            inside = !inside;
        }
    }
    inside
}

/// Whether a side of one flat outline crosses or touches a side of another.
fn crosses(one: &[DVec2], other: &[DVec2]) -> bool {
    let side = |a: DVec2, b: DVec2, p: DVec2| (b - a).perp_dot(p - a);
    (0..one.len()).any(|i| {
        let (a, b) = (one[i], one[(i + 1) % one.len()]);
        (0..other.len()).any(|j| {
            let (c, d) = (other[j], other[(j + 1) % other.len()]);
            side(a, b, c) * side(a, b, d) <= 0.0 && side(c, d, a) * side(c, d, b) <= 0.0
        })
    })
}

/// A flat outline with holes in convex pieces: its triangles, joined two by two for as long as
/// what they make stays convex.
fn convex_pieces(loops: &[Vec<DVec2>]) -> Vec<Vec<DVec2>> {
    let points: Vec<DVec2> = loops.iter().flatten().copied().collect();
    let flat: Vec<f64> = points.iter().flat_map(|p| [p.x, p.y]).collect();
    let mut holes = Vec::new();
    let mut at = 0;
    for l in &loops[..loops.len().saturating_sub(1)] {
        at += l.len();
        holes.push(at);
    }
    let Ok(triangles) = earcutr::earcut(&flat, &holes, 2) else {
        return Vec::new();
    };
    // every piece counter-clockwise
    let mut pieces: Vec<Vec<usize>> = triangles
        .chunks_exact(3)
        .map(|t| {
            if (points[t[1]] - points[t[0]]).perp_dot(points[t[2]] - points[t[0]]) >= 0.0 {
                vec![t[0], t[1], t[2]]
            } else {
                vec![t[0], t[2], t[1]]
            }
        })
        .collect();
    let bulges = |piece: &[usize]| {
        (0..piece.len()).any(|i| {
            let (a, b, c) = (
                points[piece[i]],
                points[piece[(i + 1) % piece.len()]],
                points[piece[(i + 2) % piece.len()]],
            );
            (b - a).perp_dot(c - b) < -1e-9 * (1.0 + (b - a).length() * (c - b).length())
        })
    };
    // each side two triangles share once, joining them where what they make stays convex
    let mut owner: HashMap<(usize, usize), usize> = HashMap::new();
    for (i, piece) in pieces.iter().enumerate() {
        for s in 0..piece.len() {
            owner.insert((piece[s], piece[(s + 1) % piece.len()]), i);
        }
    }
    let shared: Vec<(usize, usize)> = owner
        .keys()
        .filter(|(a, b)| a < b && owner.contains_key(&(*b, *a)))
        .copied()
        .collect();
    let mut alive = vec![true; pieces.len()];
    for (a, b) in shared {
        let (Some(&i), Some(&j)) = (owner.get(&(a, b)), owner.get(&(b, a))) else {
            continue;
        };
        if i == j {
            continue;
        }
        let (first, second) = (&pieces[i], &pieces[j]);
        let Some(s) =
            (0..first.len()).find(|&s| first[s] == a && first[(s + 1) % first.len()] == b)
        else {
            continue;
        };
        let Some(t) =
            (0..second.len()).find(|&t| second[t] == b && second[(t + 1) % second.len()] == a)
        else {
            continue;
        };
        // the first from b round to a, then the second from after a round to before b
        let mut merged: Vec<usize> = (0..first.len())
            .map(|k| first[(s + 1 + k) % first.len()])
            .collect();
        merged.extend((2..second.len()).map(|k| second[(t + k) % second.len()]));
        if bulges(&merged) {
            continue;
        }
        owner.remove(&(a, b));
        owner.remove(&(b, a));
        for k in 0..merged.len() {
            owner.insert((merged[k], merged[(k + 1) % merged.len()]), i);
        }
        pieces[i] = merged;
        alive[j] = false;
    }
    let pieces: Vec<Vec<usize>> = pieces
        .into_iter()
        .zip(alive)
        .filter(|(_, alive)| *alive)
        .map(|(piece, _)| piece)
        .collect();
    pieces
        .into_iter()
        .map(|piece| piece.into_iter().map(|k| points[k]).collect())
        .collect()
}

/// The outline of a profile given by its measures. The file numbers the kinds of profile: an
/// I shape (1), a T (4), an angle (5), a channel (6), a rectangle (7), a hollow rectangle (8), a
/// circle (10) and a tube (11), each with its measures in the order the open building-model
/// standard lists them, around the middle of its bounding box. Two more are beams with ledges
/// written as a T: one with a ledge of its own height on either side (16, the thickness of the
/// wide part left and right) and one with a ledge on one side only (17). Where the kind is not
/// known the number of measures tells it; what does not fit is drawn as the rectangle around it.
fn parametric(kind: Option<i64>, n: &[f64]) -> Vec<Vec<DVec2>> {
    let rectangle = |x: f64, y: f64| {
        vec![
            DVec2::new(-x / 2.0, -y / 2.0),
            DVec2::new(x / 2.0, -y / 2.0),
            DVec2::new(x / 2.0, y / 2.0),
            DVec2::new(-x / 2.0, y / 2.0),
        ]
    };
    let circle = |r: f64| -> Vec<DVec2> {
        (0..24)
            .map(|k| DVec2::from_angle(k as f64 * std::f64::consts::TAU / 24.0) * r)
            .collect()
    };
    let outline = |xy: &[(f64, f64)]| {
        vec![
            xy.iter()
                .map(|&(x, y)| DVec2::new(x, y))
                .collect::<Vec<_>>(),
        ]
    };
    let get = |k: usize| n.get(k).copied().unwrap_or(0.0);
    match (kind, n.len()) {
        (_, 0) => Vec::new(),
        // a tube: its radius and the thickness of its wall
        (Some(11), _) if get(1) > 0.0 && get(1) < get(0) => {
            vec![circle(get(0)), circle(get(0) - get(1))]
        }
        (Some(10 | 11), _) | (None, 1) => vec![circle(get(0))],
        // an angle: its depth, its width and its thickness; the legs along the left and the bottom
        (Some(5), _) if get(2) > 0.0 && get(2) < get(0).min(get(1)) => {
            let (h, b, t) = (get(0) / 2.0, get(1) / 2.0, get(2));
            outline(&[
                (-b, -h),
                (b, -h),
                (b, -h + t),
                (-b + t, -h + t),
                (-b + t, h),
                (-b, h),
            ])
        }
        // a channel: its depth, the width of its flanges, its web and the flanges' thickness
        (Some(6), _)
            if get(2) > 0.0 && get(2) < get(1) && get(3) > 0.0 && get(3) * 2.0 < get(0) =>
        {
            let (h, b, web, flange) = (get(0) / 2.0, get(1) / 2.0, get(2), get(3));
            outline(&[
                (-b, -h),
                (b, -h),
                (b, -h + flange),
                (-b + web, -h + flange),
                (-b + web, h - flange),
                (b, h - flange),
                (b, h),
                (-b, h),
            ])
        }
        // a T: its depth, the width of its flange, its web and the flange's thickness; the flange on top
        (Some(4), _) if get(2) > 0.0 && get(2) < get(1) && get(3) > 0.0 && get(3) < get(0) => {
            let (h, b, web, flange) = (get(0) / 2.0, get(1) / 2.0, get(2) / 2.0, get(3));
            outline(&[
                (-web, -h),
                (web, -h),
                (web, h - flange),
                (b, h - flange),
                (b, h),
                (-b, h),
                (-b, h - flange),
                (-web, h - flange),
            ])
        }
        (Some(8) | None, 5) if get(2) > 0.0 && get(2) * 2.0 < get(0).min(get(1)) => vec![
            rectangle(get(0), get(1)),
            rectangle(get(0) - 2.0 * get(2), get(1) - 2.0 * get(2)),
        ],
        (Some(1) | None, 7) if get(2) > 0.0 && get(3) > 0.0 => {
            let (b, h, web, flange) = (get(0) / 2.0, get(1) / 2.0, get(2) / 2.0, get(3));
            outline(&[
                (-b, -h),
                (b, -h),
                (b, -h + flange),
                (web, -h + flange),
                (web, h - flange),
                (b, h - flange),
                (b, h),
                (-b, h),
                (-b, h - flange),
                (-web, h - flange),
                (-web, -h + flange),
                (-b, -h + flange),
            ])
        }
        // a T whose wide part is thicker on one side than on the other
        (Some(16), _)
            if get(2) > 0.0
                && get(2) < get(1)
                && get(3) > 0.0
                && get(3) < get(0)
                && get(4) > 0.0
                && get(4) < get(0) =>
        {
            let (h, b, web, left, right) =
                (get(0) / 2.0, get(1) / 2.0, get(2) / 2.0, get(3), get(4));
            outline(&[
                (-web, -h),
                (web, -h),
                (web, h - right),
                (b, h - right),
                (b, h),
                (-b, h),
                (-b, h - left),
                (-web, h - left),
            ])
        }
        // a T with its web against the left side: one ledge, on the right
        (Some(17), _) if get(2) > 0.0 && get(2) < get(1) && get(3) > 0.0 && get(3) < get(0) => {
            let (h, b, web, flange) = (get(0) / 2.0, get(1) / 2.0, get(2), get(3));
            outline(&[
                (-b, -h),
                (-b + web, -h),
                (-b + web, h - flange),
                (b, h - flange),
                (b, h),
                (-b, h),
            ])
        }
        // these give their depth before their width
        (Some(4 | 5 | 6 | 16 | 17), _) | (None, 9..) => vec![rectangle(get(1), get(0))],
        _ => vec![rectangle(get(0), get(1))],
    }
}

/// Newell's normal of a loop of points, as long as twice its area: it looks at whoever sees the
/// loop run counter-clockwise.
fn newell(outline: &[DVec3]) -> DVec3 {
    let mut normal = DVec3::ZERO;
    for (i, a) in outline.iter().enumerate() {
        let b = outline[(i + 1) % outline.len()];
        normal += DVec3::new(
            (a.y - b.y) * (a.z + b.z),
            (a.z - b.z) * (a.x + b.x),
            (a.x - b.x) * (a.y + b.y),
        );
    }
    normal
}

/// The region a plane takes away: the side its normal points to, and when the plane has an
/// outline (points on it), only what lies within that outline, seen along the normal.
fn within(normal: DVec3, through: DVec3, outline: &[DVec3]) -> Vec<(DVec3, DVec3)> {
    let mut region = vec![(normal, through)];
    if outline.len() >= 3 {
        let middle = outline.iter().sum::<DVec3>() / outline.len() as f64;
        for (i, a) in outline.iter().enumerate() {
            let b = outline[(i + 1) % outline.len()];
            let Some(mut inward) = (b - *a).cross(normal).try_normalize() else {
                continue;
            };
            if (middle - *a).dot(inward) < 0.0 {
                inward = -inward;
            }
            region.push((inward, *a));
        }
    }
    region
}

/// The region a closed, convex shape fills (its own frame, metres), as the planes of its faces
/// with their normals turned inward; nothing when it bulges inward anywhere or has too many
/// faces to cut by.
fn convex(shape: &Mesh) -> Option<Vec<(DVec3, DVec3)>> {
    let corner = |i: u32| {
        DVec3::new(
            shape.positions[i as usize * 3] as f64,
            shape.positions[i as usize * 3 + 1] as f64,
            shape.positions[i as usize * 3 + 2] as f64,
        ) * 1000.0
    };
    let from = corner(*shape.indices.first()?);
    let mut volume = 0.0;
    let mut faces: Vec<(DVec3, DVec3)> = Vec::new();
    for t in shape.indices.chunks_exact(3) {
        let (a, b, c) = (corner(t[0]), corner(t[1]), corner(t[2]));
        volume += (a - from).dot((b - from).cross(c - from));
        let Some(outward) = (b - a).cross(c - a).try_normalize() else {
            continue;
        };
        if !faces
            .iter()
            .any(|(n, p)| n.dot(outward) > 0.99999 && (a - *p).dot(*n).abs() < 0.05)
        {
            faces.push((outward, a));
            if faces.len() > 96 {
                return None;
            }
        }
    }
    // faces that look inward (a shape written inside out) are turned
    if volume.abs() < 1.0 || faces.len() < 4 {
        return None;
    }
    if volume < 0.0 {
        for face in &mut faces {
            face.0 = -face.0;
        }
    }
    let bulges = (0..shape.positions.len() as u32 / 3)
        .any(|i| faces.iter().any(|(n, p)| (corner(i) - *p).dot(*n) > 0.1));
    (!bulges).then(|| faces.into_iter().map(|(n, p)| (-n, p)).collect())
}

/// A flat polygon in two: what lies against a plane's normal and what lies on its side, by
/// how far each corner is from the plane (0: on it, and then part of both).
fn split(polygon: &[DVec3], side: &[f64]) -> (Vec<DVec3>, Vec<DVec3>) {
    let (mut against, mut along) = (
        Vec::with_capacity(polygon.len() + 2),
        Vec::with_capacity(polygon.len() + 2),
    );
    for k in 0..polygon.len() {
        let (a, b, sa, sb) = (
            polygon[k],
            polygon[(k + 1) % polygon.len()],
            side[k],
            side[(k + 1) % polygon.len()],
        );
        if sa <= 0.0 {
            against.push(a);
        }
        if sa >= 0.0 {
            along.push(a);
        }
        if (sa < 0.0 && sb > 0.0) || (sa > 0.0 && sb < 0.0) {
            let crossing = a + (b - a) * (sa / (sa - sb));
            against.push(crossing);
            along.push(crossing);
        }
    }
    (against, along)
}

/// A mesh (in its own frame, metres) less what lies on the side of a plane its normal points to
/// (`through` in millimetres), with the cut closed, placed by `at`.
#[cfg(test)]
fn cut_off(whole: &Mesh, normal: DVec3, through: DVec3, at: DMat4, out: &mut Mesh) {
    less(whole, &[(normal, through)], at, out);
}

/// How near a plane counts as on it, in millimetres.
const NEAR: f64 = 0.05;

/// How far a point lies on the normal's side of a plane (a normal and a point), nought when it
/// lies on it.
fn side_of(p: DVec3, (normal, through): (DVec3, DVec3)) -> f64 {
    Some((p - through).dot(normal))
        .filter(|s| s.abs() > NEAR)
        .unwrap_or(0.0)
}

/// What of a flat piece (looking along `facing`) lies outside a convex region: what lies
/// against the normal of one of its planes. A piece that lies on a plane stays when it looks
/// along the normal and goes with the region when it looks against it.
fn outside_of(
    region: &[(DVec3, DVec3)],
    piece: Vec<DVec3>,
    facing: DVec3,
    out: &mut Vec<Vec<DVec3>>,
) {
    let sides = |points: &[DVec3], plane: usize| -> Vec<f64> {
        points.iter().map(|p| side_of(*p, region[plane])).collect()
    };
    let clear = |side: &[f64], plane: usize| {
        side.iter().all(|s| *s <= 0.0)
            && (side.iter().any(|s| *s < 0.0) || facing.dot(region[plane].0) > 0.0)
    };
    // most pieces lie clear of a region: that is told without keeping the sides
    let clear_of = |plane: usize| {
        let (mut behind, mut on) = (false, true);
        for p in &piece {
            let s = side_of(*p, region[plane]);
            if s > 0.0 {
                return false;
            }
            behind |= s < 0.0;
            on &= s == 0.0;
        }
        behind || (on && facing.dot(region[plane].0) > 0.0)
    };
    if (0..region.len()).any(clear_of) {
        out.push(piece);
        return;
    }
    let mut rest = piece;
    for plane in 0..region.len() {
        let side = sides(&rest, plane);
        if clear(&side, plane) {
            out.push(rest);
            return;
        }
        if side.iter().all(|s| *s >= 0.0) {
            continue;
        }
        let (against, along) = split(&rest, &side);
        if against.len() >= 3 {
            out.push(against);
        }
        if along.len() < 3 {
            return;
        }
        rest = along;
    }
    // what is left lies within the region
}

/// A mesh (in its own frame, metres) less what lies inside a convex region, placed by `at`.
#[cfg(test)]
fn less(whole: &Mesh, region: &[(DVec3, DVec3)], at: DMat4, out: &mut Mesh) {
    less_all(whole, &[region.to_vec()], at, out);
}

/// A mesh (in its own frame, metres) less what lies inside any of a number of convex regions,
/// placed by `at`. A region is what lies on the normal's side of every one of its planes (a
/// normal and a point, millimetres): one plane cuts a shape off, the faces of an opening cut it
/// out, and an opening that is not convex comes in convex parts. Where a region's faces run
/// through the shape, the cut is closed, as far as no other region takes that away too.
fn less_all(whole: &Mesh, regions: &[Vec<(DVec3, DVec3)>], at: DMat4, out: &mut Mesh) {
    let corner = |i: u32| {
        DVec3::new(
            whole.positions[i as usize * 3] as f64,
            whole.positions[i as usize * 3 + 1] as f64,
            whole.positions[i as usize * 3 + 2] as f64,
        ) * 1000.0
    };
    // a mirrored placement turns faces inside out: their corners are then taken the other way
    let mirrored = at.determinant() < 0.0;
    let placed = |points: &[DVec3]| {
        let mut points: Vec<DVec3> = points.iter().map(|p| at.transform_point3(*p)).collect();
        if mirrored {
            points.reverse();
        }
        points
    };
    // Most of a shape lies clear of what is cut out of it. Where nothing places it, a face
    // that stays whole is taken over as it is.
    let unmoved = at.abs_diff_eq(DMat4::IDENTITY, 1e-12);
    let keep = |triangle: &[u32], corners: &[DVec3; 3], color: [u8; 4], out: &mut Mesh| {
        if unmoved {
            let first = (out.positions.len() / 3) as u32;
            for &i in triangle {
                let i = i as usize;
                out.positions
                    .extend_from_slice(&whole.positions[i * 3..i * 3 + 3]);
                out.normals
                    .extend_from_slice(&whole.normals[i * 3..i * 3 + 3]);
                out.colors.push(color);
            }
            out.indices.extend([first, first + 1, first + 2]);
        } else {
            polygon(&[placed(corners)], color, out);
        }
    };
    let regions: Vec<&Vec<(DVec3, DVec3)>> = regions.iter().filter(|r| !r.is_empty()).collect();
    let key = |p: DVec3| {
        (
            (p.x * 50.0).round() as i64,
            (p.y * 50.0).round() as i64,
            (p.z * 50.0).round() as i64,
        )
    };
    // per region and plane: the edges where the shape's faces pass it, and the edges of faces
    // that lie on it already, looking along its normal
    let mut cuts: Vec<Vec<Vec<(DVec3, DVec3)>>> =
        regions.iter().map(|r| vec![Vec::new(); r.len()]).collect();
    let mut lying: Vec<Vec<Vec<(DVec3, DVec3)>>> = cuts.clone();
    // whether a region takes anything away: one that touches nothing is closed by nothing
    let mut hit = vec![false; regions.len()];
    let mut color = GREY;
    let mut pieces: Vec<Vec<DVec3>> = Vec::new();
    for triangle in whole.indices.chunks_exact(3) {
        let corners = [
            corner(triangle[0]),
            corner(triangle[1]),
            corner(triangle[2]),
        ];
        color = whole
            .colors
            .get(triangle[0] as usize)
            .copied()
            .unwrap_or(GREY);
        let facing = (corners[1] - corners[0]).cross(corners[2] - corners[0]);
        for (r, region) in regions.iter().enumerate() {
            for (plane, &(normal, through)) in region.iter().enumerate() {
                let side = corners.map(|p| side_of(p, (normal, through)));
                if side.iter().all(|s| *s == 0.0) {
                    if facing.dot(normal) > 0.0 {
                        lying[r][plane].extend((0..3).map(|k| (corners[k], corners[(k + 1) % 3])));
                    }
                    continue;
                }
                let mut on = [DVec3::ZERO; 3];
                let mut count = 0;
                for k in 0..3 {
                    let (a, b, sa, sb) =
                        (corners[k], corners[(k + 1) % 3], side[k], side[(k + 1) % 3]);
                    if (sa < 0.0) != (sb < 0.0) {
                        on[count] = a + (b - a) * (sa / (sa - sb));
                        count += 1;
                    }
                }
                if count == 2 {
                    cuts[r][plane].push((on[0], on[1]));
                }
            }
        }
        // what lies outside one region goes on to the next; what is inside one goes
        pieces.clear();
        pieces.push(corners.to_vec());
        let mut changed = false;
        for (r, region) in regions.iter().enumerate() {
            let mut next = Vec::with_capacity(pieces.len());
            for piece in pieces.drain(..) {
                let was = piece.clone();
                let from = next.len();
                outside_of(region, piece, facing, &mut next);
                if next.len() != from + 1 || next[from] != was {
                    changed = true;
                    hit[r] = true;
                }
            }
            pieces = next;
            if pieces.is_empty() {
                break;
            }
        }
        if !changed {
            keep(triangle, &corners, color, out);
        } else {
            for piece in &pieces {
                polygon(&[placed(piece)], color, out);
            }
        }
    }
    // The cut itself: per region and plane the outlines its edges close into, as a face that
    // looks along the normal, as far as it lies within the region's other planes and outside
    // the other regions.
    let mut bits = Vec::new();
    for (r, region) in regions.iter().enumerate() {
        if !hit[r] {
            continue;
        }
        for plane in 0..region.len() {
            if cuts[r][plane].is_empty() && lying[r][plane].is_empty() {
                continue;
            }
            let normal = region[plane].0;
            let mut edges = std::mem::take(&mut cuts[r][plane]);
            if !lying[r][plane].is_empty() {
                // where a face lies on the plane already, no second one comes over it: an edge
                // of both falls away
                edges.append(&mut lying[r][plane]);
                let ends = |e: &(DVec3, DVec3)| {
                    let (a, b) = (key(e.0), key(e.1));
                    if a <= b { (a, b) } else { (b, a) }
                };
                let mut times: HashMap<_, u32> = HashMap::new();
                for edge in &edges {
                    *times.entry(ends(edge)).or_default() += 1;
                }
                edges.retain(|edge| times[&ends(edge)] % 2 == 1);
            }
            let mut loops: Vec<Vec<DVec3>> = closed_loops(&edges);
            if loops.is_empty() {
                continue;
            }
            for outline in &mut loops {
                if newell(outline).dot(normal) < 0.0 {
                    outline.reverse();
                }
            }
            if region.len() == 1 && regions.len() == 1 {
                face_loops(
                    &loops.iter().map(|l| placed(l)).collect::<Vec<_>>(),
                    color,
                    out,
                );
                continue;
            }
            let mut face = Mesh::default();
            face_loops(&loops, color, &mut face);
            let corner = |i: u32| {
                DVec3::new(
                    face.positions[i as usize * 3] as f64,
                    face.positions[i as usize * 3 + 1] as f64,
                    face.positions[i as usize * 3 + 2] as f64,
                ) * 1000.0
            };
            'pieces: for triangle in face.indices.chunks_exact(3) {
                let mut piece = vec![
                    corner(triangle[0]),
                    corner(triangle[1]),
                    corner(triangle[2]),
                ];
                for other in (0..region.len()).filter(|other| *other != plane) {
                    let side: Vec<f64> = piece.iter().map(|p| side_of(*p, region[other])).collect();
                    if side.iter().any(|s| *s < 0.0) {
                        piece = split(&piece, &side).1;
                        if piece.len() < 3 {
                            continue 'pieces;
                        }
                    }
                }
                bits.clear();
                bits.push(piece);
                for (o, other) in regions.iter().enumerate() {
                    if o == r {
                        continue;
                    }
                    let mut next = Vec::new();
                    for bit in bits.drain(..) {
                        outside_of(other, bit, normal, &mut next);
                    }
                    bits = next;
                }
                for bit in &bits {
                    polygon(&[placed(bit)], color, out);
                }
            }
        }
    }
}

/// The closed outlines segments make end to end (millimetres); what closes nothing is left out.
fn closed_loops(segments: &[(DVec3, DVec3)]) -> Vec<Vec<DVec3>> {
    let key = |p: DVec3| {
        (
            (p.x * 50.0).round() as i64,
            (p.y * 50.0).round() as i64,
            (p.z * 50.0).round() as i64,
        )
    };
    let mut at: HashMap<(i64, i64, i64), Vec<usize>> = HashMap::new();
    for (i, (a, b)) in segments.iter().enumerate() {
        if key(*a) != key(*b) {
            at.entry(key(*a)).or_default().push(i);
            at.entry(key(*b)).or_default().push(i);
        }
    }
    let mut used = vec![false; segments.len()];
    let mut loops = Vec::new();
    for first in 0..segments.len() {
        if used[first] || key(segments[first].0) == key(segments[first].1) {
            continue;
        }
        used[first] = true;
        let (start, mut end) = (key(segments[first].0), segments[first].1);
        let mut points = vec![segments[first].0];
        let closed = loop {
            if key(end) == start {
                break true;
            }
            points.push(end);
            let Some(next) = at
                .get(&key(end))
                .and_then(|l| l.iter().copied().find(|&i| !used[i]))
            else {
                break false;
            };
            used[next] = true;
            end = if key(segments[next].0) == key(end) {
                segments[next].1
            } else {
                segments[next].0
            };
        };
        if closed && points.len() >= 3 {
            loops.push(points);
        }
    }
    loops
}

/// A face of several loops, as the file writes it: in any order, a loop being a hole in another
/// or a part of the face beside it. Which it is shows from where it lies: a loop inside an odd
/// number of the others is a hole of the one just round it.
fn face_loops(loops: &[Vec<DVec3>], color: [u8; 4], out: &mut Mesh) {
    if loops.len() < 2 {
        return polygon(loops, color, out);
    }
    // the face's plane is that of its largest loop, which no other encloses
    let Some(normal) = loops
        .iter()
        .map(|l| newell(l))
        .max_by(|a, b| a.length_squared().total_cmp(&b.length_squared()))
        .and_then(|n| n.try_normalize())
    else {
        return;
    };
    let drop = if normal.x.abs() >= normal.y.abs() && normal.x.abs() >= normal.z.abs() {
        0
    } else if normal.y.abs() >= normal.z.abs() {
        1
    } else {
        2
    };
    let (u, v) = [(1, 2), (2, 0), (0, 1)][drop];
    let holds = |outer: &[DVec3], p: DVec3| {
        let mut inside = false;
        for k in 0..outer.len() {
            let (a, b) = (outer[k], outer[(k + 1) % outer.len()]);
            if (a[v] > p[v]) != (b[v] > p[v])
                && p[u] < a[u] + (p[v] - a[v]) / (b[v] - a[v]) * (b[u] - a[u])
            {
                inside = !inside;
            }
        }
        inside
    };
    // a point of each loop that lies on none of the others' edges: the middle of its first edge,
    // a hair inside, is not needed as long as loops of one face do not touch along an edge
    let probe: Vec<DVec3> = loops
        .iter()
        .map(|l| {
            if l.len() >= 2 {
                (l[0] + l[1]) / 2.0
            } else {
                DVec3::ZERO
            }
        })
        .collect();
    let round: Vec<Vec<usize>> = (0..loops.len())
        .map(|i| {
            (0..loops.len())
                .filter(|&j| j != i && loops[i].len() >= 3 && holds(&loops[j], probe[i]))
                .collect()
        })
        .collect();
    for i in (0..loops.len()).filter(|&i| round[i].len() % 2 == 0 && loops[i].len() >= 3) {
        let mut outline = loops[i].clone();
        // every part of the face looks the way the face does
        if newell(&outline).dot(normal) < 0.0 {
            outline.reverse();
        }
        let mut part = vec![outline];
        part.extend(
            (0..loops.len())
                .filter(|&j| round[j].len() == round[i].len() + 1 && round[j].contains(&i))
                .map(|j| loops[j].clone()),
        );
        polygon(&part, color, out);
    }
}

/// A flat face with holes, as triangles.
fn polygon(loops: &[Vec<DVec3>], color: [u8; 4], out: &mut Mesh) {
    let Some(outline) = loops.first().filter(|l| l.len() >= 3) else {
        return;
    };
    let Some(normal) = newell(outline).try_normalize() else {
        return;
    };
    if outline.len() == 3 && loops.len() == 1 {
        let first = out.corners(outline, normal, color);
        out.indices.extend([first, first + 1, first + 2]);
        return;
    }
    // flat: the two axes the face lies along most
    let drop = if normal.x.abs() >= normal.y.abs() && normal.x.abs() >= normal.z.abs() {
        0
    } else if normal.y.abs() >= normal.z.abs() {
        1
    } else {
        2
    };
    let (u, v) = [(1, 2), (2, 0), (0, 1)][drop];
    let points: Vec<DVec3> = loops.iter().flatten().copied().collect();
    let flat: Vec<f64> = points.iter().flat_map(|p| [p[u], p[v]]).collect();
    let mut holes = Vec::new();
    let mut at = 0;
    for l in &loops[..loops.len() - 1] {
        at += l.len();
        holes.push(at);
    }
    let Ok(triangles) = earcutr::earcut(&flat, &holes, 2) else {
        return;
    };
    let first = out.corners(&points, normal, color);
    for t in triangles.chunks_exact(3) {
        let (a, b, c) = (points[t[0]], points[t[1]], points[t[2]]);
        // the corners turn as the face's normal says
        let turned = (b - a).cross(c - a).dot(normal) < 0.0;
        let (second, third) = if turned { (t[2], t[1]) } else { (t[1], t[2]) };
        out.indices.extend([
            first + t[0] as u32,
            first + second as u32,
            first + third as u32,
        ]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_face_with_a_hole_keeps_the_hole_open() {
        let square = |size: f64| {
            vec![
                DVec3::new(-size, -size, 0.0),
                DVec3::new(size, -size, 0.0),
                DVec3::new(size, size, 0.0),
                DVec3::new(-size, size, 0.0),
            ]
        };
        let mut out = Mesh::default();
        polygon(&[square(2000.0), square(1000.0)], GREY, &mut out);
        assert_eq!(out.triangles(), 8);
        // 16 m2 less the 4 m2 of the hole, every triangle facing up
        // the eight corners once, shared by the triangles
        assert_eq!(out.positions.len(), 8 * 3);
        let at = |i: u32| {
            [
                out.positions[i as usize * 3],
                out.positions[i as usize * 3 + 1],
            ]
        };
        let area: f64 = out // OpenCheck: f64 positions
            .indices
            .chunks_exact(3)
            .map(|t| (at(t[0]), at(t[1]), at(t[2])))
            .map(|(a, b, c)| ((b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1])) / 2.0)
            .sum();
        assert!((area - 12.0).abs() < 1e-3, "{area}");
        assert!(out.normals.chunks_exact(3).all(|n| n == [0.0, 0.0, 1.0]));
    }

    /// The volume a mesh encloses, counted from where its faces look: positive when they look
    /// out of it.
    fn volume(m: &Mesh) -> f64 {
        let at = |i: u32| {
            DVec3::new(
                m.positions[i as usize * 3] as f64,
                m.positions[i as usize * 3 + 1] as f64,
                m.positions[i as usize * 3 + 2] as f64,
            )
        };
        m.indices
            .chunks_exact(3)
            .map(|t| at(t[0]).dot(at(t[1]).cross(at(t[2]))) / 6.0)
            .sum()
    }

    #[test]
    fn a_hollow_profile_drawn_along_a_line_is_material_round_a_hole() {
        use crate::stream::{Class, Field, Rec}; // OpenCheck: the crate's own path
        // a profile of 2 m square with a hole of 1 m square, both drawn the same way round, 3 m long
        let mut d = Data::default();
        let point = |d: &mut Data, p: [f32; 3]| {
            d.points.push(p);
            V::Pt(d.points.len() as u32 - 1)
        };
        let list = |d: &mut Data, items: Vec<V>| {
            d.lists.push(items.into_boxed_slice());
            V::List(d.lists.len() as u32 - 1)
        };
        let square = |d: &mut Data, half: f32| {
            let corners = [
                [-half, -half, 0.0],
                [half, -half, 0.0],
                [half, half, 0.0],
                [-half, half, 0.0],
            ]
            .map(|p| point(d, p))
            .to_vec();
            list(d, corners)
        };
        let (outer, inner) = (square(&mut d, 1000.0), square(&mut d, 500.0));
        let holes = list(&mut d, vec![inner]);
        d.classes.push(Class::plain(
            "ArbitraryProfile",
            vec![
                Field {
                    ty: b'[',
                    name: "innerCurves".into(),
                },
                Field {
                    ty: b'[',
                    name: "outerCurve".into(),
                },
            ],
        ));
        d.recs.push(Rec {
            class: 0,
            vals: vec![holes, outer].into_boxed_slice(),
            extras: Box::new([]),
        });
        let ends = [
            point(&mut d, [0.0, 0.0, 0.0]),
            point(&mut d, [0.0, 0.0, 3000.0]),
        ]
        .to_vec();
        let route = list(&mut d, ends);
        let source = HashMap::new();
        for mirror in [1.0, -1.0] {
            let mut out = Mesh::default();
            let at = DMat4::from_scale(DVec3::new(mirror, 1.0, 1.0));
            Shapes::new(&d, &source).extrusion(
                [V::Rec(0), route, V::Null, V::Null, V::Null],
                at,
                GREY,
                &mut out,
            );
            // 4 m2 less the hole of 1 m2, 3 m long, its faces looking out: also when mirrored
            assert!((volume(&out) - 9.0).abs() < 1e-6, "{}", volume(&out));
        }
    }

    #[test]
    fn the_loops_of_a_face_are_holes_or_parts_by_where_they_lie() {
        let square = |x: f64, size: f64| {
            vec![
                DVec3::new(x - size, -size, 0.0),
                DVec3::new(x + size, -size, 0.0),
                DVec3::new(x + size, size, 0.0),
                DVec3::new(x - size, size, 0.0),
            ]
        };
        let area = |loops: &[Vec<DVec3>]| {
            let mut out = Mesh::default();
            face_loops(loops, GREY, &mut out);
            let at = |i: u32| {
                [
                    out.positions[i as usize * 3],
                    out.positions[i as usize * 3 + 1],
                ]
            };
            // every triangle counter-clockwise seen from above: no part of the face lies twice
            let parts: Vec<f64> = out // OpenCheck: f64 positions
                .indices
                .chunks_exact(3)
                .map(|t| (at(t[0]), at(t[1]), at(t[2])))
                .map(|(a, b, c)| {
                    ((b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1])) / 2.0
                })
                .collect();
            assert!(parts.iter().all(|a| *a > 0.0), "{parts:?}");
            parts.iter().sum::<f64>()
        };
        // a hole written before its outline: 16 m2 less the hole of 1 m2
        assert!((area(&[square(0.0, 500.0), square(0.0, 2000.0)]) - 15.0).abs() < 1e-3);
        // a second part beside the first, written the other way round: both are face, 16 + 1 m2
        let mut beside = square(5000.0, 500.0);
        beside.reverse();
        assert!((area(&[square(0.0, 2000.0), beside]) - 17.0).abs() < 1e-3);
        // an island in a hole is face again: 16 - 4 + 0.25 m2
        assert!(
            (area(&[square(0.0, 2000.0), square(0.0, 1000.0), square(0.0, 250.0)]) - 12.25).abs()
                < 1e-3
        );
    }

    #[test]
    fn a_bent_outline_comes_apart_in_convex_pieces_that_cover_it() {
        // an L of 2 x 2 less a square of 1 x 1, with a small hole in its foot
        let l = vec![
            DVec2::new(0.0, 0.0),
            DVec2::new(2.0, 0.0),
            DVec2::new(2.0, 1.0),
            DVec2::new(1.0, 1.0),
            DVec2::new(1.0, 2.0),
            DVec2::new(0.0, 2.0),
        ];
        let hole = vec![
            DVec2::new(1.4, 0.4),
            DVec2::new(1.4, 0.6),
            DVec2::new(1.6, 0.6),
            DVec2::new(1.6, 0.4),
        ];
        for loops in [vec![l.clone()], vec![l, hole]] {
            let pieces = convex_pieces(&loops);
            let area = |p: &Vec<DVec2>| {
                (0..p.len())
                    .map(|i| p[i].perp_dot(p[(i + 1) % p.len()]))
                    .sum::<f64>()
                    / 2.0
            };
            let want = 3.0 - if loops.len() > 1 { 0.04 } else { 0.0 };
            assert!((pieces.iter().map(area).sum::<f64>() - want).abs() < 1e-9);
            assert!(pieces.iter().all(|p| area(p) > 0.0));
            if loops.len() == 1 {
                assert_eq!(pieces.len(), 2, "{pieces:?}");
            }
        }
    }

    #[test]
    fn a_bent_opening_is_cut_out_piece_by_piece() {
        // a wall of 4 x 0.2 x 3 m and an L-shaped opening through it, drawn along y
        let wall = block(DVec3::ZERO, DVec3::new(4000.0, 200.0, 3000.0));
        let l = vec![
            DVec2::new(1000.0, 500.0),
            DVec2::new(3000.0, 500.0),
            DVec2::new(3000.0, 1500.0),
            DVec2::new(2000.0, 1500.0),
            DVec2::new(2000.0, 2500.0),
            DVec2::new(1000.0, 2500.0),
        ];
        // the profile's x and y become the wall's x and z, the line runs across the wall
        let across = DMat4::from_cols(DVec4::X, DVec4::Z, DVec4::NEG_Y, DVec4::W);
        let mut whole = wall;
        let pieces = convex_pieces(&[l]);
        for piece in &pieces {
            let mut solid = Mesh::default();
            prism(
                &[piece.clone()],
                DMat4::IDENTITY,
                DVec3::new(0.0, 0.0, -300.0),
                DVec3::new(0.0, 0.0, 100.0),
                across,
                GREY,
                &mut solid,
            );
            let region = convex(&solid).expect("a piece is convex");
            let mut rest = Mesh::default();
            less(&whole, &region, DMat4::IDENTITY, &mut rest);
            whole = rest;
        }
        assert!(
            (volume(&whole) - (2.4 - 3.0 * 0.2)).abs() < 1e-6,
            "{}",
            volume(&whole)
        );
    }

    #[test]
    fn the_kind_of_a_profile_tells_what_its_measures_mean() {
        let area = |loops: &[Vec<DVec2>]| -> f64 {
            let of = |l: &Vec<DVec2>| {
                ((0..l.len())
                    .map(|i| l[i].perp_dot(l[(i + 1) % l.len()]))
                    .sum::<f64>()
                    / 2.0)
                    .abs()
            };
            of(&loops[0]) - loops[1..].iter().map(of).sum::<f64>()
        };
        // a tube is a ring, not a strip as wide as its radius
        let tube = parametric(Some(11), &[30.15, 1.5]);
        assert_eq!(tube.len(), 2);
        assert!(
            tube[0].iter().all(|p| (p.length() - 30.15).abs() < 1e-9)
                && tube[1].iter().all(|p| (p.length() - 28.65).abs() < 1e-9)
        );
        // the same two measures of a rectangle
        assert!((area(&parametric(Some(7), &[2605.0, 200.0])) - 521000.0).abs() < 1e-6);
        // an angle of 90 x 65 x 2, a channel of 180 x 70 and a T of 670 x 1200
        assert!(
            (area(&parametric(
                Some(5),
                &[90.0, 65.0, 2.0, 0.0, 0.0, 0.0, 0.0, 0.0]
            )) - (65.0 * 2.0 + 88.0 * 2.0))
                .abs()
                < 1e-6
        );
        assert!(
            (area(&parametric(
                Some(6),
                &[180.0, 70.0, 8.0, 11.0, 11.0, 5.5, 0.08, 0.0]
            )) - (2.0 * 70.0 * 11.0 + 158.0 * 8.0))
                .abs()
                < 1e-6
        );
        let t = parametric(
            Some(4),
            &[670.0, 1200.0, 900.0, 395.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        );
        assert!((area(&t) - (1200.0 * 395.0 + 900.0 * 275.0)).abs() < 1e-6);
        assert!(
            t[0].iter()
                .all(|p| p.x.abs() <= 600.0 && p.y.abs() <= 335.0)
        );
        // beams with a ledge on one side and with ledges of two heights
        assert!(
            (area(&parametric(
                Some(17),
                &[860.0, 1200.0, 1050.0, 595.0, 0.0, 0.0, 0.0, 0.0, 0.0]
            )) - (1200.0 * 595.0 + 1050.0 * 265.0))
                .abs()
                < 1e-6
        );
        assert!(
            (area(&parametric(
                Some(16),
                &[
                    670.0, 1200.0, 900.0, 265.0, 395.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0
                ]
            )) - (900.0 * 670.0 + 150.0 * 265.0 + 150.0 * 395.0))
                .abs()
                < 1e-6
        );
        // measures that do not fit their kind give the rectangle around them, the width along x
        assert!(
            (area(&parametric(
                Some(17),
                &[860.0, 1200.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]
            )) - 860.0 * 1200.0)
                .abs()
                < 1e-6
        );
    }

    #[test]
    fn a_shape_cut_off_by_a_plane_is_shorter_and_closed() {
        // a beam of 0.2 x 0.2 m, 3 m long along z, as an extrusion would give it
        let square = |z: f64| {
            vec![
                DVec3::new(-100.0, -100.0, z),
                DVec3::new(100.0, -100.0, z),
                DVec3::new(100.0, 100.0, z),
                DVec3::new(-100.0, 100.0, z),
            ]
        };
        let (low, high) = (square(0.0), square(3000.0));
        let mut beam = Mesh::default();
        polygon(&[low.iter().rev().copied().collect()], GREY, &mut beam);
        polygon(&[high.clone()], GREY, &mut beam);
        for i in 0..4 {
            let j = (i + 1) % 4;
            polygon(&[vec![low[i], low[j], high[j], high[i]]], GREY, &mut beam);
        }
        assert!((volume(&beam) - 0.12).abs() < 1e-6);
        // cut off square at 2 m: what lies beyond goes, the end is closed
        let mut cut = Mesh::default();
        cut_off(
            &beam,
            DVec3::Z,
            DVec3::new(0.0, 0.0, 2000.0),
            DMat4::IDENTITY,
            &mut cut,
        );
        assert!((volume(&cut) - 0.08).abs() < 1e-6, "{}", volume(&cut));
        assert!(cut.positions.chunks_exact(3).all(|p| p[2] <= 2.0 + 1e-6));
        // cut off at a slant through the same point: half of the last 0.2 m more or less, still closed
        let mut mitre = Mesh::default();
        cut_off(
            &beam,
            DVec3::new(1.0, 0.0, 1.0).normalize(),
            DVec3::new(0.0, 0.0, 2000.0),
            DMat4::IDENTITY,
            &mut mitre,
        );
        assert!((volume(&mitre) - 0.08).abs() < 1e-6, "{}", volume(&mitre));
        // a plane the whole beam lies behind cuts nothing off; one it lies before leaves nothing
        let mut all = Mesh::default();
        cut_off(
            &beam,
            DVec3::Z,
            DVec3::new(0.0, 0.0, 5000.0),
            DMat4::IDENTITY,
            &mut all,
        );
        assert!((volume(&all) - 0.12).abs() < 1e-6);
        let mut none = Mesh::default();
        cut_off(
            &beam,
            DVec3::Z,
            DVec3::new(0.0, 0.0, -1.0),
            DMat4::IDENTITY,
            &mut none,
        );
        assert_eq!(none.triangles(), 0);
    }

    /// A block between two corners (millimetres), its faces looking outward.
    fn block(low: DVec3, high: DVec3) -> Mesh {
        let at = |x: bool, y: bool, z: bool| {
            DVec3::new(
                if x { high.x } else { low.x },
                if y { high.y } else { low.y },
                if z { high.z } else { low.z },
            )
        };
        let mut mesh = Mesh::default();
        for (a, b, c, d) in [
            (
                at(false, false, false),
                at(false, true, false),
                at(true, true, false),
                at(true, false, false),
            ),
            (
                at(false, false, true),
                at(true, false, true),
                at(true, true, true),
                at(false, true, true),
            ),
            (
                at(false, false, false),
                at(true, false, false),
                at(true, false, true),
                at(false, false, true),
            ),
            (
                at(false, true, false),
                at(false, true, true),
                at(true, true, true),
                at(true, true, false),
            ),
            (
                at(false, false, false),
                at(false, false, true),
                at(false, true, true),
                at(false, true, false),
            ),
            (
                at(true, false, false),
                at(true, true, false),
                at(true, true, true),
                at(true, false, true),
            ),
        ] {
            polygon(&[vec![a, b, c, d]], GREY, &mut mesh);
        }
        mesh
    }

    #[test]
    fn an_opening_is_cut_out_of_a_wall() {
        // a wall of 4 x 0.2 x 3 m
        let wall = block(DVec3::ZERO, DVec3::new(4000.0, 200.0, 3000.0));
        assert!((volume(&wall) - 2.4).abs() < 1e-6);
        // an opening of 1 x 2 m that sticks out of the wall on both sides
        let through = convex(&block(
            DVec3::new(1000.0, -50.0, 500.0),
            DVec3::new(2000.0, 250.0, 2500.0),
        ))
        .expect("a block is convex");
        assert_eq!(through.len(), 6);
        let mut open = Mesh::default();
        less(&wall, &through, DMat4::IDENTITY, &mut open);
        assert!((volume(&open) - 2.0).abs() < 1e-6, "{}", volume(&open));
        // one exactly as deep as the wall: no skin stays over it
        let flush = convex(&block(
            DVec3::new(1000.0, 0.0, 500.0),
            DVec3::new(2000.0, 200.0, 2500.0),
        ))
        .unwrap();
        let mut open = Mesh::default();
        less(&wall, &flush, DMat4::IDENTITY, &mut open);
        assert!((volume(&open) - 2.0).abs() < 1e-6, "{}", volume(&open));
        let skin = open.indices.chunks_exact(3).any(|t| {
            let p = |i: u32| {
                DVec3::new(
                    open.positions[i as usize * 3] as f64,
                    open.positions[i as usize * 3 + 1] as f64,
                    open.positions[i as usize * 3 + 2] as f64,
                )
            };
            let middle = (p(t[0]) + p(t[1]) + p(t[2])) / 3.0;
            middle.x > 1.01 && middle.x < 1.99 && middle.z > 0.51 && middle.z < 2.49
        });
        assert!(!skin, "a face stays over the opening");
        // a recess half as deep, and a shape that only touches the wall
        let recess = convex(&block(
            DVec3::new(1000.0, 100.0, 500.0),
            DVec3::new(2000.0, 300.0, 2500.0),
        ))
        .unwrap();
        let mut dented = Mesh::default();
        less(&wall, &recess, DMat4::IDENTITY, &mut dented);
        assert!((volume(&dented) - 2.2).abs() < 1e-6, "{}", volume(&dented));
        let beside = convex(&block(
            DVec3::new(1000.0, 200.0, 500.0),
            DVec3::new(2000.0, 400.0, 2500.0),
        ))
        .unwrap();
        let mut same = Mesh::default();
        less(&wall, &beside, DMat4::IDENTITY, &mut same);
        assert!((volume(&same) - 2.4).abs() < 1e-6, "{}", volume(&same));
        assert_eq!(same.triangles(), wall.triangles());
    }

    #[test]
    fn a_plane_with_an_outline_takes_away_only_within_it() {
        let wall = block(DVec3::ZERO, DVec3::new(4000.0, 200.0, 3000.0));
        // the top metre, but only over the first metre of the wall's length
        let outline = [
            DVec3::new(-1.0, -1.0, 2000.0),
            DVec3::new(1000.0, -1.0, 2000.0),
            DVec3::new(1000.0, 201.0, 2000.0),
            DVec3::new(-1.0, 201.0, 2000.0),
        ];
        let mut cut = Mesh::default();
        less(
            &wall,
            &within(DVec3::Z, DVec3::new(0.0, 0.0, 2000.0), &outline),
            DMat4::IDENTITY,
            &mut cut,
        );
        assert!((volume(&cut) - 2.2).abs() < 1e-6, "{}", volume(&cut));
        // a shape that bulges inward is not taken as a region
        let mut bent = block(DVec3::ZERO, DVec3::new(1000.0, 1000.0, 1000.0));
        let more = block(
            DVec3::new(1000.0, 0.0, 0.0),
            DVec3::new(2000.0, 500.0, 1000.0),
        );
        let first = bent.positions.len() as u32 / 3;
        bent.positions.extend(&more.positions);
        bent.normals.extend(&more.normals);
        bent.colors.extend(&more.colors);
        bent.indices.extend(more.indices.iter().map(|i| i + first));
        assert!(convex(&bent).is_none());
    }

    #[test]
    fn the_measures_of_a_profile_give_its_outline() {
        assert_eq!(parametric(None, &[80.0])[0].len(), 24);
        assert_eq!(
            parametric(None, &[2605.0, 200.0])[0][2],
            DVec2::new(1302.5, 100.0)
        );
        assert_eq!(parametric(None, &[180.0, 180.0, 10.0, 10.0, 20.0]).len(), 2);
        assert_eq!(
            parametric(None, &[240.0, 240.0, 10.0, 17.0, 21.0, 0.0, 0.0])[0].len(),
            12
        );
        // a slab with cores: the rectangle around it, its width first
        assert_eq!(
            parametric(
                None,
                &[670.0, 1200.0, 900.0, 395.0, 0.0, 0.0, 0.0, 0.0, 0.0]
            )[0][2],
            DVec2::new(600.0, 335.0)
        );
    }
}
