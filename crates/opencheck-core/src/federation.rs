use std::sync::Arc;

use glam::{DMat4, DVec3, Mat4};

use crate::{Aabb, IfcModel};

/// Stable identifier of a model inside a [`Federation`]. Never reused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ModelId(pub u32);

/// A set of IFC models viewed and checked together.
///
/// Models keep their own local frames. The federation picks one shared
/// origin (the world position of the first model's frame) so that all models
/// can be rendered and compared in `f32` without losing precision on
/// georeferenced coordinates.
#[derive(Debug, Default)]
pub struct Federation {
    models: Vec<(ModelId, Arc<IfcModel>)>,
    next_id: u32,
    origin: Option<DVec3>,
}

impl Federation {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add(&mut self, model: IfcModel) -> ModelId {
        let id = ModelId(self.next_id);
        self.next_id += 1;
        if self.origin.is_none() {
            self.origin = Some(model.local_to_world.transform_point3(DVec3::ZERO));
        }
        self.models.push((id, Arc::new(model)));
        id
    }

    pub fn remove(&mut self, id: ModelId) -> Option<Arc<IfcModel>> {
        let index = self.models.iter().position(|(m, _)| *m == id)?;
        let (_, model) = self.models.remove(index);
        if self.models.is_empty() {
            self.origin = None;
        }
        Some(model)
    }

    pub fn get(&self, id: ModelId) -> Option<&IfcModel> {
        self.models
            .iter()
            .find(|(m, _)| *m == id)
            .map(|(_, m)| m.as_ref())
    }

    /// Mutable access; None while a background check still shares the model.
    pub fn get_mut(&mut self, id: ModelId) -> Option<&mut IfcModel> {
        self.models
            .iter_mut()
            .find(|(m, _)| *m == id)
            .and_then(|(_, m)| Arc::get_mut(m))
    }

    pub fn iter(&self) -> impl Iterator<Item = (ModelId, &IfcModel)> {
        self.models.iter().map(|(id, m)| (*id, m.as_ref()))
    }

    /// A shared handle for work on other threads.
    pub fn shared(&self, id: ModelId) -> Option<Arc<IfcModel>> {
        self.models
            .iter()
            .find(|(m, _)| *m == id)
            .map(|(_, m)| Arc::clone(m))
    }

    pub fn ids(&self) -> Vec<ModelId> {
        self.models.iter().map(|(id, _)| *id).collect()
    }

    pub fn len(&self) -> usize {
        self.models.len()
    }

    pub fn is_empty(&self) -> bool {
        self.models.is_empty()
    }

    /// World position of the federation origin, in metres.
    pub fn origin(&self) -> DVec3 {
        self.origin.unwrap_or(DVec3::ZERO)
    }

    /// Transform from a model's mesh frame into the federation frame
    /// (world coordinates relative to [`Federation::origin`]).
    pub fn model_transform(&self, id: ModelId) -> Option<Mat4> {
        let model = self.get(id)?;
        let shift = DMat4::from_translation(-self.origin());
        Some((shift * model.local_to_world).as_mat4())
    }

    /// Bounds of all models in the federation frame.
    pub fn bounds(&self) -> Aabb {
        self.models.iter().fold(Aabb::EMPTY, |acc, (id, model)| {
            let m = self
                .model_transform(*id)
                .expect("id comes from the model list")
                .as_dmat4();
            acc.union(&model.bounds.transformed(&m))
        })
    }
}
