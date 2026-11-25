// This file is from Open BIM Suite by the OpenAEC Foundation
// (https://github.com/OpenAEC-Foundation/open-bim-suite, src/loaders/smc/stream.rs,
// revision eb4142ee12ea28f796c07f475a24a1d0c66aa8f0), licensed under the GNU Lesser General Public
// License, version 3 or later: see ../licenses/open-bim-suite-LICENSE and
// ../licenses/open-bim-suite-NOTICE. OpenCheck's changes are marked "OpenCheck:".
// The file is the upstream one run through rustfmt, with its lints left as
// they are, so a newer revision can be taken in by copying it again.
#![allow(clippy::all, dead_code)]

//! A Java object stream read in one pass.
//!
//! The coordination file of a model checker is a zip around one such stream of several
//! gigabytes: every model, element, property and shape as a serialized object, each referring
//! back to earlier ones by number. Keeping all of them as they are written would take many times
//! the file, so each object is reduced as it ends to what a model needs of it (a number, a
//! text, a matrix, a shape, an element), and the table of back references holds those small
//! values ([`V`], eight bytes).
//!
//! The format is the one of `java.io.ObjectOutputStream` (protocol 2). Classes that write
//! themselves (`writeObject`) are read as their fields followed by what they added, which is
//! how the classes of these files write. Class names are matched on their last part.

// the reader keeps more than the loader takes yet: relations, issues and viewpoints
#![allow(dead_code)]

use std::collections::HashMap;
use std::io::Read;

/// What an object was reduced to. The numbers are places in the lists of [`Data`].
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum V {
    #[default]
    Null,
    /// An object nothing is kept of, or one still being read (a parent seen from its child).
    Other,
    /// Raw bytes a class wrote between its objects.
    Block,
    Bool(bool),
    Int(i32),
    Num(f32),
    Long(u32),
    Str(u32),
    Class(u32),
    /// A colour: red, green, blue and opacity, a byte each.
    Color(u32),
    /// A matrix of 16 numbers, row by row.
    Mat(u32),
    /// A coordinate system: its matrix and the system it stands in.
    Csys(u32),
    Pt(u32),
    F32s(u32),
    I32s(u32),
    /// Bytes as they are (a picture).
    Bytes(u32),
    List(u32),
    /// A property with a name: name and value.
    Named(u32),
    /// A property set: name and its properties.
    Props(u32),
    Geo(u32),
    Elem(u32),
    Model(u32),
    /// An object kept with all its fields ([`Rec`]).
    Rec(u32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GeoKind {
    /// Faces as loops of points.
    Brep,
    /// Several shapes as one.
    Multi,
    /// A shape that is another one's (the relation to it stands at the end of the file).
    Mapped,
    /// A profile along a route.
    Extrusion,
    /// One shape cut from, joined with or crossed with another.
    Csg,
    Halfspace,
    Other,
}

/// A shape. `slots` holds, by kind: Brep and Multi: vertices, loop sizes, loops per face,
/// colours, and for Multi its shapes; Extrusion: profile and route; Csg: first shape, second
/// shape, the second's matrix and the operation (0 cut, 1 join, 2 cross); Halfspace: normal, point,
/// a flag and, when it is bounded, the outline it reaches to (points on the plane).
#[derive(Clone, Debug)]
pub struct Geo {
    pub kind: GeoKind,
    pub matrix: V,
    pub origin: V,
    pub color: V,
    pub children: V,
    pub slots: [V; 5],
}

/// An element of a model (a wall, a beam, a space, a storey).
#[derive(Clone, Debug, Default)]
pub struct Elem {
    pub class: u32,
    pub model: u32,
    pub id: i32,
    pub ifc_type: V,
    pub guid: V,
    pub name: V,
    pub object_type: V,
    pub description: V,
    pub tag: V,
    pub csys: V,
    pub children: V,
    pub psets: V,
    pub kind: V,
    /// The fields of the element's own classes with a plain value: class, field, value.
    pub own: Box<[(u32, u16, V)]>,
}

/// A model in the file: one source file's elements.
#[derive(Clone, Debug, Default)]
pub struct ModelRec {
    pub source: V,
    pub short: V,
    pub csys: V,
    /// OpenCheck: the file the model came from (a `java.io.File` record, when `File` is kept).
    pub file: V,
}

/// An object kept whole: the values of all its fields, class by class from the topmost down.
#[derive(Clone, Debug)]
pub struct Rec {
    pub class: u32,
    pub vals: Box<[V]>,
    pub extras: Box<[V]>,
}

pub struct Field {
    pub ty: u8,
    pub name: Box<str>,
}

pub struct Class {
    pub full: Box<str>,
    pub short: Box<str>,
    pub flags: u8,
    pub fields: Vec<Field>,
    pub parent: Option<u32>,
    /// The class and those it extends, the topmost first.
    pub chain: Vec<u32>,
    role: Role,
}

impl Class {
    /// A class written by hand, for tests: the first of a file, with these fields.
    #[cfg(test)]
    pub fn plain(name: &str, fields: Vec<Field>) -> Class {
        Class {
            full: name.into(),
            short: name.into(),
            flags: 0x02,
            fields,
            parent: None,
            chain: vec![0],
            role: Role::Rec,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Role {
    Skip,
    List,
    Number,
    Boolean,
    /// A moment: milliseconds since 1970, which the class writes itself.
    Date,
    Property,
    PropertySet,
    Csys,
    Matrix,
    Point,
    Color,
    Operation(i32),
    Geo(GeoKind),
    Element,
    Model,
    Rec,
}

/// What a file holds after reading.
#[derive(Default)]
pub struct Data {
    pub classes: Vec<Class>,
    pub strings: Vec<Box<str>>,
    pub longs: Vec<i64>,
    pub mats: Vec<[f64; 16]>,
    pub csys: Vec<(V, V)>,
    pub points: Vec<[f32; 3]>,
    pub f32s: Vec<Vec<f32>>,
    pub i32s: Vec<Vec<i32>>,
    pub blobs: Vec<Vec<u8>>,
    pub lists: Vec<Box<[V]>>,
    pub named: Vec<(V, V)>,
    pub props: Vec<(V, V)>,
    pub geos: Vec<Geo>,
    pub elems: Vec<Elem>,
    pub models: Vec<ModelRec>,
    pub recs: Vec<Rec>,
    /// The objects at the top of the stream.
    pub tops: Vec<V>,
    /// Bytes read, and whether the stream ended before its last object did.
    pub bytes: u64,
    pub cut_short: bool,
    pub objects: u64,
}

impl Data {
    pub fn text(&self, v: V) -> Option<&str> {
        match v {
            V::Str(k) => self.strings.get(k as usize).map(|s| &**s),
            _ => None,
        }
    }

    pub fn list(&self, v: V) -> &[V] {
        match v {
            V::List(k) => self.lists.get(k as usize).map_or(&[], |l| &**l),
            _ => &[],
        }
    }

    /// The value of a field of a kept object, by the last part of its class's name.
    pub fn field(&self, rec: &Rec, class: &str, field: &str) -> V {
        let mut at = 0;
        for &c in &self.classes[rec.class as usize].chain {
            let c = &self.classes[c as usize];
            if &*c.short == class && c.flags & 0x06 == 0x02 {
                return c
                    .fields
                    .iter()
                    .position(|f| &*f.name == field)
                    .and_then(|k| rec.vals.get(at + k))
                    .copied()
                    .unwrap_or(V::Null);
            }
            if c.flags & 0x06 == 0x02 {
                at += c.fields.len();
            }
        }
        V::Null
    }

    /// Any field of that name of a kept object, whichever of its classes has it.
    pub fn any_field(&self, rec: &Rec, field: &str) -> V {
        let mut at = 0;
        for &c in &self.classes[rec.class as usize].chain {
            let c = &self.classes[c as usize];
            if let Some(k) = c
                .fields
                .iter()
                .position(|f| &*f.name == field)
                .filter(|_| c.flags & 0x06 == 0x02)
            {
                return rec.vals.get(at + k).copied().unwrap_or(V::Null);
            }
            if c.flags & 0x06 == 0x02 {
                at += c.fields.len();
            }
        }
        V::Null
    }

    /// A value in words, for finding out how a file is laid out.
    pub fn say(&self, v: V) -> String {
        match v {
            V::Str(_) => format!("{:?}", self.text(v).unwrap_or("")),
            V::Class(k) => format!("class {}", self.classes[k as usize].short),
            V::List(_) => format!(
                "list[{}] {}",
                self.list(v).len(),
                self.list(v)
                    .iter()
                    .take(6)
                    .map(|x| if matches!(x, V::List(_)) {
                        "list".to_string()
                    } else {
                        self.say(*x)
                    })
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            V::Rec(k) => format!(
                "<{} #{k}>",
                self.classes[self.recs[k as usize].class as usize].short
            ),
            V::Elem(k) => format!(
                "<elem #{k} {}>",
                self.classes[self.elems[k as usize].class as usize].short
            ),
            V::Geo(k) => format!("<geo #{k} {:?}>", self.geos[k as usize].kind),
            V::Named(k) => format!(
                "{} = {}",
                self.say(self.named[k as usize].0),
                self.say(self.named[k as usize].1)
            ),
            V::Props(k) => format!(
                "set {} {}",
                self.say(self.props[k as usize].0),
                self.say(self.props[k as usize].1)
            ),
            V::Long(k) => format!("{}L", self.longs[k as usize]),
            V::Bytes(k) => format!("bytes[{}]", self.blobs[k as usize].len()),
            V::Pt(k) => format!("{:?}", self.points[k as usize]),
            other => format!("{other:?}"),
        }
    }
}

#[derive(Debug)]
pub enum Error {
    /// The stream ended inside an object.
    End,
    Bad(String),
}

type Res<T> = Result<T, Error>;

fn bad<T>(what: impl Into<String>) -> Res<T> {
    Err(Error::Bad(what.into()))
}

struct Part {
    class: u32,
    fields_at: usize,
    extras_at: usize,
    end: usize,
}

const BASE: u32 = 0x7e_0000;

pub struct Reader<R: Read> {
    input: R,
    buf: Vec<u8>,
    at: usize,
    end: usize,
    read: u64,
    handles: Vec<V>,
    vals: Vec<V>,
    parts: Vec<Part>,
    models: Vec<u32>,
    /// The last eight raw bytes a class wrote (a date writes its moment so).
    last_block: [u8; 8],
    /// Class names (last part) of which the first objects are printed, with how many are left.
    pub watch: HashMap<String, u32>,
    /// Class names (last part) kept whole besides those the reader knows.
    pub keep: Vec<String>,
    pub data: Data,
}

impl<R: Read> Reader<R> {
    pub fn new(input: R) -> Self {
        Reader {
            input,
            buf: vec![0; 1 << 20],
            at: 0,
            end: 0,
            read: 0,
            handles: Vec::new(),
            vals: Vec::new(),
            parts: Vec::new(),
            models: Vec::new(),
            last_block: [0; 8],
            watch: HashMap::new(),
            keep: Vec::new(),
            data: Data::default(),
        }
    }

    /// Reads the whole stream. A stream that stops inside an object gives what was whole.
    pub fn run(mut self) -> Result<Data, String> {
        let result = (|| -> Res<()> {
            if self.u16()? != 0xaced || self.u16()? != 5 {
                return bad("not a Java object stream");
            }
            loop {
                match self.need(1) {
                    Err(Error::End) => return Ok(()),
                    other => other?,
                }
                let v = self.content()?;
                if v != V::Block {
                    self.data.tops.push(v);
                }
            }
        })();
        self.data.bytes = self.read - (self.end - self.at) as u64;
        match result {
            Ok(()) => {}
            Err(Error::End) => self.data.cut_short = true,
            Err(Error::Bad(e)) => return Err(format!("{e} (at byte {})", self.data.bytes)),
        }
        Ok(self.data)
    }

    // ---- bytes ----

    fn need(&mut self, n: usize) -> Res<()> {
        while self.end - self.at < n {
            if self.at > 0 {
                self.buf.copy_within(self.at..self.end, 0);
                self.end -= self.at;
                self.at = 0;
            }
            if n > self.buf.len() {
                return bad("a value larger than the buffer");
            }
            let got = self
                .input
                .read(&mut self.buf[self.end..])
                .map_err(|e| Error::Bad(e.to_string()))?;
            if got == 0 {
                return Err(Error::End);
            }
            self.end += got;
            self.read += got as u64;
        }
        Ok(())
    }

    fn u8(&mut self) -> Res<u8> {
        self.need(1)?;
        self.at += 1;
        Ok(self.buf[self.at - 1])
    }

    fn peek(&mut self) -> Res<u8> {
        self.need(1)?;
        Ok(self.buf[self.at])
    }

    fn take<const N: usize>(&mut self) -> Res<[u8; N]> {
        self.need(N)?;
        let mut out = [0; N];
        out.copy_from_slice(&self.buf[self.at..self.at + N]);
        self.at += N;
        Ok(out)
    }

    fn u16(&mut self) -> Res<u16> {
        Ok(u16::from_be_bytes(self.take()?))
    }

    fn i32(&mut self) -> Res<i32> {
        Ok(i32::from_be_bytes(self.take()?))
    }

    fn skip(&mut self, mut n: u64) -> Res<()> {
        while n > 0 {
            self.need(1)?;
            let step = ((self.end - self.at) as u64).min(n) as usize;
            self.at += step;
            n -= step as u64;
        }
        Ok(())
    }

    fn string(&mut self, n: u64) -> Res<u32> {
        let mut bytes = Vec::with_capacity(n.min(1 << 16) as usize);
        let mut left = n;
        while left > 0 {
            self.need(1)?;
            let step = ((self.end - self.at) as u64).min(left) as usize;
            bytes.extend_from_slice(&self.buf[self.at..self.at + step]);
            self.at += step;
            left -= step as u64;
        }
        self.data.strings.push(
            String::from_utf8_lossy(&bytes)
                .into_owned()
                .into_boxed_str(),
        );
        Ok(self.data.strings.len() as u32 - 1)
    }

    fn utf(&mut self) -> Res<Box<str>> {
        let n = self.u16()? as usize;
        self.need(n)?;
        let s = String::from_utf8_lossy(&self.buf[self.at..self.at + n]).into_owned();
        self.at += n;
        Ok(s.into_boxed_str())
    }

    fn handle(&mut self, v: V) -> usize {
        self.handles.push(v);
        self.handles.len() - 1
    }

    // ---- the stream ----

    fn class_desc(&mut self) -> Res<Option<u32>> {
        match self.u8()? {
            0x70 => Ok(None),
            0x71 => match self.reference()? {
                V::Class(k) => Ok(Some(k)),
                _ => bad("a reference to a class that is none"),
            },
            0x72 => {
                let full = self.utf()?;
                self.skip(8)?;
                let k = self.data.classes.len() as u32;
                let short: Box<str> = full.rsplit('.').next().unwrap_or(&full).into();
                self.data.classes.push(Class {
                    full,
                    short,
                    flags: 0,
                    fields: Vec::new(),
                    parent: None,
                    chain: Vec::new(),
                    role: Role::Skip,
                });
                self.handle(V::Class(k));
                let flags = self.u8()?;
                let count = self.u16()?;
                let mut fields = Vec::with_capacity(count as usize);
                for _ in 0..count {
                    let ty = self.u8()?;
                    let name = self.utf()?;
                    if ty == b'L' || ty == b'[' {
                        self.content()?;
                    }
                    fields.push(Field { ty, name });
                }
                // what the class was annotated with
                while self.peek()? != 0x78 {
                    self.content()?;
                }
                self.skip(1)?;
                let parent = self.class_desc()?;
                let mut chain = parent
                    .map(|p| self.data.classes[p as usize].chain.clone())
                    .unwrap_or_default();
                chain.push(k);
                let own = self.role(k);
                let role = own
                    .or(parent.map(|p| self.data.classes[p as usize].role))
                    .unwrap_or(Role::Skip);
                let class = &mut self.data.classes[k as usize];
                (
                    class.flags,
                    class.fields,
                    class.parent,
                    class.chain,
                    class.role,
                ) = (flags, fields, parent, chain, role);
                Ok(Some(k))
            }
            0x7d => bad("proxy classes are not read"),
            other => bad(format!("a class was expected, found mark {other:#x}")),
        }
    }

    /// What a class is to the reader, by its own name.
    fn role(&self, k: u32) -> Option<Role> {
        let class = &self.data.classes[k as usize];
        let java = class.full.starts_with("java.");
        Some(match &*class.short {
            "Double" | "Float" | "Integer" | "Short" | "Byte" if java => Role::Number,
            "Boolean" if java => Role::Boolean,
            "Date" if java => Role::Date,
            _ if class.full.starts_with("java.util.") && &*class.short != "UUID" => Role::List,
            "BoundaryRepresentation" => Role::Geo(GeoKind::Brep),
            "MultiGeometry" => Role::Geo(GeoKind::Multi),
            "MappedGeometry" => Role::Geo(GeoKind::Mapped),
            "Extrusion" => Role::Geo(GeoKind::Extrusion),
            "CSGSolid" => Role::Geo(GeoKind::Csg),
            "Halfspace" => Role::Geo(GeoKind::Halfspace),
            "GEntity" => Role::Geo(GeoKind::Other),
            "CoordinateSystem" => Role::Csys,
            "TransformationMatrix" => Role::Matrix,
            "Tuple3d" | "Tuple3f" => Role::Point,
            "MaterialInfo" => Role::Color,
            "Difference" => Role::Operation(0),
            "Union" => Role::Operation(1),
            "Intersection" => Role::Operation(2),
            "PropertySet" => Role::PropertySet,
            "Property" => Role::Property,
            "SEntity" => Role::Element,
            "Model" => Role::Model,
            "Relation" | "Profile" | "SType" | "Issue" | "IssueViewpoint" | "Snapshot"
            | "Category" | "Presentation" | "Slide" => Role::Rec,
            "IssueCoordination"
            | "AnimationKeyFrame"
            | "Visualization"
            | "ClipPlane"
            | "Tuple4d"
            | "ParametricProfileTypes" => Role::Rec,
            short if self.keep.iter().any(|k| k == short) => Role::Rec,
            _ => return None,
        })
    }

    fn reference(&mut self) -> Res<V> {
        let k = (self.i32()? as u32).wrapping_sub(BASE) as usize;
        match self.handles.get(k) {
            Some(v) => Ok(*v),
            None => bad(format!(
                "a reference to object {k}, of {}",
                self.handles.len()
            )),
        }
    }

    fn content(&mut self) -> Res<V> {
        match self.u8()? {
            0x70 => Ok(V::Null),
            0x71 => self.reference(),
            0x72 => {
                self.at -= 1;
                Ok(self.class_desc()?.map_or(V::Null, V::Class))
            }
            0x73 => self.object(),
            0x74 => {
                let n = self.u16()? as u64;
                let v = V::Str(self.string(n)?);
                self.handle(v);
                Ok(v)
            }
            0x7c => {
                let n = u64::from_be_bytes(self.take()?);
                let v = V::Str(self.string(n)?);
                self.handle(v);
                Ok(v)
            }
            0x75 => self.array(),
            0x76 => {
                let v = self.class_desc()?.map_or(V::Null, V::Class);
                self.handle(v);
                Ok(v)
            }
            0x77 => {
                let n = self.u8()? as u64;
                if n == 8 {
                    self.last_block = self.take()?;
                } else {
                    self.skip(n)?;
                }
                Ok(V::Block)
            }
            0x7a => {
                let n = self.i32()? as u32 as u64;
                self.skip(n)?;
                Ok(V::Block)
            }
            0x79 => {
                self.handles.clear();
                Ok(V::Block)
            }
            0x7e => {
                self.class_desc()?;
                let h = self.handle(V::Other);
                let v = self.content()?;
                self.handles[h] = v;
                Ok(v)
            }
            0x7b => bad("the stream holds an exception"),
            other => bad(format!("unknown mark {other:#x}")),
        }
    }

    fn array(&mut self) -> Res<V> {
        let Some(class) = self.class_desc()? else {
            return bad("an array without a class");
        };
        let h = self.handle(V::Other);
        let n = self.i32()?.max(0) as usize;
        let of = self.data.classes[class as usize]
            .full
            .as_bytes()
            .get(1)
            .copied()
            .unwrap_or(0);
        let v = match of {
            b'L' | b'[' => {
                let mut items = Vec::with_capacity(n.min(1 << 16));
                for _ in 0..n {
                    items.push(self.content()?);
                }
                self.data.lists.push(items.into_boxed_slice());
                V::List(self.data.lists.len() as u32 - 1)
            }
            b'F' => {
                let mut out = Vec::with_capacity(n.min(1 << 24));
                for _ in 0..n {
                    out.push(f32::from_be_bytes(self.take()?));
                }
                self.data.f32s.push(out);
                V::F32s(self.data.f32s.len() as u32 - 1)
            }
            b'I' => {
                let mut out = Vec::with_capacity(n.min(1 << 24));
                for _ in 0..n {
                    out.push(self.i32()?);
                }
                self.data.i32s.push(out);
                V::I32s(self.data.i32s.len() as u32 - 1)
            }
            b'D' if n == 16 => {
                let mut m = [0.0; 16];
                for x in &mut m {
                    *x = f64::from_be_bytes(self.take()?);
                }
                self.data.mats.push(m);
                V::Mat(self.data.mats.len() as u32 - 1)
            }
            b'D' => {
                let mut out = Vec::with_capacity(n.min(1 << 24));
                for _ in 0..n {
                    out.push(f64::from_be_bytes(self.take()?) as f32);
                }
                self.data.f32s.push(out);
                V::F32s(self.data.f32s.len() as u32 - 1)
            }
            b'B' => {
                let mut out = Vec::with_capacity(n.min(1 << 24));
                let mut left = n;
                while left > 0 {
                    self.need(1)?;
                    let step = (self.end - self.at).min(left);
                    out.extend_from_slice(&self.buf[self.at..self.at + step]);
                    self.at += step;
                    left -= step;
                }
                self.data.blobs.push(out);
                V::Bytes(self.data.blobs.len() as u32 - 1)
            }
            b'Z' => self.skip(n as u64).map(|_| V::Other)?,
            b'S' | b'C' => self.skip(n as u64 * 2).map(|_| V::Other)?,
            b'J' => self.skip(n as u64 * 8).map(|_| V::Other)?,
            other => return bad(format!("an array of type {}", other as char)),
        };
        self.handles[h] = v;
        Ok(v)
    }

    fn prim(&mut self, ty: u8, keep_long: bool) -> Res<V> {
        Ok(match ty {
            b'B' => V::Int(self.u8()? as i8 as i32),
            b'C' | b'S' => V::Int(i16::from_be_bytes(self.take()?) as i32),
            b'I' => V::Int(self.i32()?),
            b'Z' => V::Bool(self.u8()? != 0),
            b'F' => V::Num(f32::from_be_bytes(self.take()?)),
            b'D' => V::Num(f64::from_be_bytes(self.take()?) as f32),
            b'J' => {
                let v = i64::from_be_bytes(self.take()?);
                if keep_long {
                    self.data.longs.push(v);
                    V::Long(self.data.longs.len() as u32 - 1)
                } else {
                    V::Other
                }
            }
            other => return bad(format!("a field of type {}", other as char)),
        })
    }

    fn object(&mut self) -> Res<V> {
        let Some(class) = self.class_desc()? else {
            return bad("an object without a class");
        };
        let h = self.handle(V::Other);
        let role = self.data.classes[class as usize].role;
        if role == Role::Model {
            self.models.push(self.data.models.len() as u32);
            self.data.models.push(ModelRec::default());
        }
        let (vals_at, parts_at) = (self.vals.len(), self.parts.len());
        for k in 0..self.data.classes[class as usize].chain.len() {
            let c = self.data.classes[class as usize].chain[k];
            let flags = self.data.classes[c as usize].flags;
            let fields_at = self.vals.len();
            let external = flags & 0x04 != 0;
            if external && flags & 0x08 == 0 {
                return bad(format!(
                    "{} writes itself in a form that cannot be skipped",
                    self.data.classes[c as usize].short
                ));
            }
            if !external {
                if flags & 0x02 == 0 {
                    continue;
                }
                for f in 0..self.data.classes[c as usize].fields.len() {
                    let field = &self.data.classes[c as usize].fields[f];
                    let (ty, long) = (
                        field.ty,
                        matches!(&*field.name, "hashValue" | "modDate" | "timeStamp"),
                    );
                    let v = if ty == b'L' || ty == b'[' {
                        self.content()?
                    } else {
                        self.prim(ty, long)?
                    };
                    self.vals.push(v);
                }
            }
            let extras_at = self.vals.len();
            if external || flags & 0x01 != 0 {
                while self.peek()? != 0x78 {
                    let v = self.content()?;
                    self.vals.push(v);
                }
                self.skip(1)?;
            }
            self.parts.push(Part {
                class: c,
                fields_at,
                extras_at,
                end: self.vals.len(),
            });
        }
        self.data.objects += 1;
        let v = self.reduce(class, role, parts_at);
        self.vals.truncate(vals_at);
        self.parts.truncate(parts_at);
        self.handles[h] = v;
        Ok(v)
    }

    // ---- what an object comes to ----

    /// The value of a field of the object being reduced, by its class's last name.
    fn of(&self, parts_at: usize, class: &str, field: &str) -> V {
        for part in &self.parts[parts_at..] {
            let c = &self.data.classes[part.class as usize];
            if &*c.short == class {
                return c
                    .fields
                    .iter()
                    .position(|f| &*f.name == field)
                    .map_or(V::Null, |k| self.vals[part.fields_at + k]);
            }
        }
        V::Null
    }

    fn extras(&self, parts_at: usize, class: &str) -> &[V] {
        self.parts[parts_at..]
            .iter()
            .find(|p| &*self.data.classes[p.class as usize].short == class)
            .map_or(&[], |p| &self.vals[p.extras_at..p.end])
    }

    fn reduce(&mut self, class: u32, role: Role, parts_at: usize) -> V {
        let short = self.data.classes[class as usize].short.clone();
        if let Some(left) = self.watch.get_mut(&*short).filter(|n| **n > 0) {
            *left -= 1;
            println!(
                "--- {} (object {})",
                self.data.classes[class as usize].full, self.data.objects
            );
            for part in &self.parts[parts_at..] {
                let c = &self.data.classes[part.class as usize];
                for (k, f) in c.fields.iter().enumerate() {
                    println!(
                        "    {}.{} = {}",
                        c.short,
                        f.name,
                        self.data.say(self.vals[part.fields_at + k])
                    );
                }
                for v in &self.vals[part.extras_at..part.end] {
                    println!("    {} + {}", c.short, self.data.say(*v));
                }
            }
        }
        match role {
            Role::Skip => V::Other,
            Role::Date => {
                self.data.longs.push(i64::from_be_bytes(self.last_block));
                V::Long(self.data.longs.len() as u32 - 1)
            }
            Role::Number | Role::Boolean => self.parts[parts_at..]
                .last()
                .and_then(|p| self.vals.get(p.fields_at))
                .copied()
                .unwrap_or(V::Null),
            Role::List => {
                let last = &self.parts[parts_at..];
                // what the class wrote itself, else the list among its fields
                let written: Vec<V> = last
                    .iter()
                    .flat_map(|p| &self.vals[p.extras_at..p.end])
                    .copied()
                    .filter(|v| *v != V::Block)
                    .collect();
                let items = if !written.is_empty() {
                    written
                } else {
                    let inner = last
                        .iter()
                        .flat_map(|p| &self.vals[p.fields_at..p.extras_at])
                        .find(|v| matches!(v, V::List(_)))
                        .copied();
                    let count = self.of(parts_at, "Vector", "elementCount");
                    let mut items = inner
                        .map(|v| self.data.list(v).to_vec())
                        .unwrap_or_default();
                    if let V::Int(n) = count {
                        items.truncate(n.max(0) as usize);
                    }
                    items
                };
                self.data.lists.push(items.into_boxed_slice());
                V::List(self.data.lists.len() as u32 - 1)
            }
            Role::Property => {
                let (name, value) = (
                    self.of(parts_at, "Property", "name"),
                    self.of(parts_at, "Property", "value"),
                );
                if name == V::Null {
                    value
                } else {
                    self.data.named.push((name, value));
                    V::Named(self.data.named.len() as u32 - 1)
                }
            }
            Role::PropertySet => {
                self.data.props.push((
                    self.of(parts_at, "Property", "name"),
                    self.of(parts_at, "PropertySet", "propertyList"),
                ));
                V::Props(self.data.props.len() as u32 - 1)
            }
            Role::Csys => {
                self.data.csys.push((
                    self.of(parts_at, "CoordinateSystem", "matrix"),
                    self.of(parts_at, "CoordinateSystem", "origin"),
                ));
                V::Csys(self.data.csys.len() as u32 - 1)
            }
            Role::Matrix => self
                .extras(parts_at, "TransformationMatrix")
                .iter()
                .find(|v| matches!(v, V::Mat(_)))
                .copied()
                .unwrap_or(V::Null),
            Role::Point => {
                let n = |v: V| if let V::Num(x) = v { x } else { 0.0 };
                let part = &self.parts[parts_at];
                let p = [
                    n(self.vals[part.fields_at]),
                    n(self.vals[part.fields_at + 1]),
                    n(self.vals[part.fields_at + 2]),
                ];
                self.data.points.push(p);
                V::Pt(self.data.points.len() as u32 - 1)
            }
            Role::Color => {
                let c = |f: &str| {
                    if let V::Num(x) = self.of(parts_at, "MaterialInfo", f) {
                        (x.clamp(0.0, 1.0) * 255.0).round() as u32
                    } else {
                        0
                    }
                };
                V::Color(
                    c("red") << 24 | c("green") << 16 | c("blue") << 8 | (255 - c("transparency")),
                )
            }
            Role::Operation(code) => V::Int(code),
            Role::Geo(kind) => {
                let f = |class: &str, field: &str| self.of(parts_at, class, field);
                let b = "BoundaryRepresentation";
                let slots = match kind {
                    GeoKind::Brep => [
                        f(b, "vertices"),
                        f(b, "strips"),
                        f(b, "contours"),
                        f(b, "colors"),
                        V::Null,
                    ],
                    GeoKind::Multi => [
                        f(b, "vertices"),
                        f(b, "strips"),
                        f(b, "contours"),
                        f(b, "colors"),
                        f("MultiGeometry", "geometries"),
                    ],
                    GeoKind::Extrusion => [
                        f("Extrusion", "profile"),
                        f("Extrusion", "route"),
                        V::Null,
                        V::Null,
                        V::Null,
                    ],
                    GeoKind::Csg => [
                        f("CSGSolid", "child1"),
                        f("CSGSolid", "child2"),
                        f("CSGSolid", "child2Transformation"),
                        f("CSGSolid", "operation"),
                        V::Null,
                    ],
                    GeoKind::Halfspace => [
                        f("Halfspace", "normal"),
                        f("Halfspace", "pt"),
                        f("Halfspace", "isRelative"),
                        f("BoundedHalfspace", "boundingPolyline"),
                        V::Null,
                    ],
                    GeoKind::Mapped | GeoKind::Other => [V::Null; 5],
                };
                let geo = Geo {
                    kind,
                    matrix: f("CoordinateSystem", "matrix"),
                    origin: f("CoordinateSystem", "origin"),
                    color: f("GEntity", "importedMaterialInfo"),
                    children: f("Entity", "children"),
                    slots,
                };
                self.data.geos.push(geo);
                V::Geo(self.data.geos.len() as u32 - 1)
            }
            Role::Element => {
                let f = |class: &str, field: &str| self.of(parts_at, class, field);
                let mut own = Vec::new();
                let mut below = false;
                for part in &self.parts[parts_at..] {
                    if below {
                        for k in 0..part.extras_at - part.fields_at {
                            let v = self.vals[part.fields_at + k];
                            if matches!(v, V::Num(_) | V::Int(_) | V::Bool(_) | V::Str(_)) {
                                own.push((part.class, k as u16, v));
                            }
                        }
                    }
                    below |= &*self.data.classes[part.class as usize].short == "SEntity";
                }
                let id = if let V::Int(id) = f("Entity", "id") {
                    id
                } else {
                    0
                };
                let elem = Elem {
                    class,
                    model: self.models.last().copied().unwrap_or(0),
                    id,
                    ifc_type: f("SEntity", "ifcType"),
                    guid: f("SEntity", "uniqueId"),
                    name: f("SEntity", "label"),
                    object_type: f("SEntity", "objectType"),
                    description: f("SEntity", "description"),
                    tag: f("SProduct", "bimToolId"),
                    csys: f("SEntity", "coordinateSystem"),
                    children: f("Entity", "children"),
                    psets: f("Entity", "extensionPropertySupport"),
                    kind: f("SEntity", "type"),
                    own: own.into_boxed_slice(),
                };
                self.data.elems.push(elem);
                V::Elem(self.data.elems.len() as u32 - 1)
            }
            Role::Model => {
                let Some(k) = self.models.pop() else {
                    return V::Other;
                };
                self.data.models[k as usize] = ModelRec {
                    source: self.of(parts_at, "Model", "serializedResource"),
                    short: self.of(parts_at, "Model", "shortName"),
                    csys: self.of(parts_at, "SModel", "coordinateSystem"),
                    file: self.of(parts_at, "Model", "file"),
                }; // OpenCheck: `file` added
                V::Model(k)
            }
            Role::Rec => {
                let parts = &self.parts[parts_at..];
                let vals: Vec<V> = parts
                    .iter()
                    .flat_map(|p| &self.vals[p.fields_at..p.extras_at])
                    .copied()
                    .collect();
                let extras: Vec<V> = parts
                    .iter()
                    .flat_map(|p| &self.vals[p.extras_at..p.end])
                    .copied()
                    .filter(|v| *v != V::Block)
                    .collect();
                self.data.recs.push(Rec {
                    class,
                    vals: vals.into_boxed_slice(),
                    extras: extras.into_boxed_slice(),
                });
                V::Rec(self.data.recs.len() as u32 - 1)
            }
        }
    }
}
