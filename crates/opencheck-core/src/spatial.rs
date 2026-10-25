/// A node in the spatial structure: project, site, building, storey or space.
#[derive(Debug, Clone)]
pub struct SpatialNode {
    pub express_id: u32,
    pub ifc_class: String,
    pub name: String,
    pub long_name: Option<String>,
    /// Storey elevation in model length units, when the file states one.
    pub elevation: Option<f64>,
    /// Height of the node's origin in the IFC world frame (its placement
    /// followed up to the project), in model length units. Unlike
    /// `elevation`, it includes the height the site or building is placed at.
    pub placement_z: Option<f64>,
    pub children: Vec<SpatialNode>,
    /// Express IDs of the elements contained directly in this node.
    pub elements: Vec<u32>,
}

impl SpatialNode {
    /// Display label, e.g. `Level 1` or `IfcBuildingStorey` when unnamed.
    pub fn label(&self) -> &str {
        if self.name.is_empty() {
            &self.ifc_class
        } else {
            &self.name
        }
    }

    /// Visits this node and every descendant, depth first.
    pub fn walk<'a>(&'a self, visit: &mut impl FnMut(&'a SpatialNode, usize)) {
        self.walk_at(0, visit);
    }

    fn walk_at<'a>(&'a self, depth: usize, visit: &mut impl FnMut(&'a SpatialNode, usize)) {
        visit(self, depth);
        for child in &self.children {
            child.walk_at(depth + 1, visit);
        }
    }

    /// Number of elements in this node and all descendants.
    pub fn element_count(&self) -> usize {
        let mut n = 0;
        self.walk(&mut |node, _| n += node.elements.len());
        n
    }

    pub fn is_storey(&self) -> bool {
        self.ifc_class == "IfcBuildingStorey"
    }
}
