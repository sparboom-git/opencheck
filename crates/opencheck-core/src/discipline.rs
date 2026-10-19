use std::fmt;

/// The design discipline a model belongs to.
///
/// Rules filter on discipline (for example "MEP against Structural"), so every
/// loaded model gets one. It is guessed on load and can be changed by the user.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum Discipline {
    Architectural,
    Structural,
    Mechanical,
    Plumbing,
    Electrical,
    FireProtection,
    Landscape,
    Infrastructure,
    #[default]
    Unknown,
}

impl Discipline {
    pub const ALL: [Discipline; 9] = [
        Discipline::Architectural,
        Discipline::Structural,
        Discipline::Mechanical,
        Discipline::Plumbing,
        Discipline::Electrical,
        Discipline::FireProtection,
        Discipline::Landscape,
        Discipline::Infrastructure,
        Discipline::Unknown,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Discipline::Architectural => "Architectural",
            Discipline::Structural => "Structural",
            Discipline::Mechanical => "Mechanical (HVAC)",
            Discipline::Plumbing => "Plumbing",
            Discipline::Electrical => "Electrical",
            Discipline::FireProtection => "Fire protection",
            Discipline::Landscape => "Landscape",
            Discipline::Infrastructure => "Infrastructure",
            Discipline::Unknown => "Unknown",
        }
    }

    /// Short code used in compact UI and in rule files.
    pub fn code(self) -> &'static str {
        match self {
            Discipline::Architectural => "ARC",
            Discipline::Structural => "STR",
            Discipline::Mechanical => "MEC",
            Discipline::Plumbing => "PLU",
            Discipline::Electrical => "ELE",
            Discipline::FireProtection => "FIR",
            Discipline::Landscape => "LAN",
            Discipline::Infrastructure => "INF",
            Discipline::Unknown => "UNK",
        }
    }

    /// Guesses the discipline from the file name and the element classes in the model.
    ///
    /// File names win because offices name their files by discipline
    /// (`K_Building.ifc`, `ARK-model.ifc`, `Building-Hvac.ifc`). When the name gives
    /// nothing away, the most common discipline-specific element class decides.
    pub fn guess<'a>(file_name: &str, ifc_classes: impl IntoIterator<Item = &'a str>) -> Self {
        if let Some(d) = Self::guess_from_file_name(file_name) {
            return d;
        }
        Self::guess_from_classes(ifc_classes)
    }

    fn guess_from_file_name(file_name: &str) -> Option<Self> {
        let stem = file_name
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or(file_name)
            .to_ascii_lowercase();
        let tokens: Vec<&str> = stem
            .split(|c: char| !c.is_ascii_alphanumeric())
            .filter(|t| !t.is_empty())
            .collect();
        let has = |words: &[&str]| tokens.iter().any(|t| words.contains(t));
        let contains = |words: &[&str]| words.iter().any(|w| stem.contains(w));

        // Order matters: "hvac" must be checked before shorter tokens such as "el".
        if contains(&["hvac", "mechanical", "ventilation"]) || has(&["mep", "vent", "ven", "mec"]) {
            return Some(Discipline::Mechanical);
        }
        if contains(&["plumbing", "sanitary", "sprinkler"]) || has(&["vvs", "plu", "san"]) {
            return Some(Discipline::Plumbing);
        }
        if contains(&["electrical"]) || has(&["el", "ele", "elec"]) {
            return Some(Discipline::Electrical);
        }
        if contains(&["structur", "konstruktion"]) || has(&["str", "kon", "k", "rib"]) {
            return Some(Discipline::Structural);
        }
        if contains(&["architect", "arkitekt"]) || has(&["arc", "ark", "arch", "a"]) {
            return Some(Discipline::Architectural);
        }
        if contains(&["landscap", "landskab"]) || has(&["lan", "land"]) {
            return Some(Discipline::Landscape);
        }
        if contains(&["infra", "road", "rail", "bridge"]) {
            return Some(Discipline::Infrastructure);
        }
        None
    }

    fn guess_from_classes<'a>(ifc_classes: impl IntoIterator<Item = &'a str>) -> Self {
        let mut votes = [0usize; 9];
        for class in ifc_classes {
            let d = match class {
                "IfcBeam" | "IfcColumn" | "IfcFooting" | "IfcPile" | "IfcReinforcingBar"
                | "IfcReinforcingMesh" | "IfcTendon" => Discipline::Structural,
                "IfcDuctSegment"
                | "IfcDuctFitting"
                | "IfcAirTerminal"
                | "IfcFan"
                | "IfcUnitaryEquipment"
                | "IfcDamper" => Discipline::Mechanical,
                "IfcPipeSegment"
                | "IfcPipeFitting"
                | "IfcSanitaryTerminal"
                | "IfcValve"
                | "IfcPump"
                | "IfcWasteTerminal" => Discipline::Plumbing,
                "IfcCableSegment"
                | "IfcCableCarrierSegment"
                | "IfcCableFitting"
                | "IfcLightFixture"
                | "IfcOutlet"
                | "IfcSwitchingDevice" => Discipline::Electrical,
                "IfcFireSuppressionTerminal" | "IfcAlarm" => Discipline::FireProtection,
                "IfcDoor" | "IfcWindow" | "IfcCovering" | "IfcCurtainWall" | "IfcRailing"
                | "IfcStair" | "IfcSpace" => Discipline::Architectural,
                "IfcGeographicElement" => Discipline::Landscape,
                "IfcRoad" | "IfcRailway" | "IfcBridge" | "IfcAlignment" => {
                    Discipline::Infrastructure
                }
                _ => continue,
            };
            votes[d as usize] += 1;
        }
        let (best, count) = votes
            .iter()
            .enumerate()
            .max_by_key(|(_, c)| **c)
            .expect("votes is non-empty");
        if *count == 0 {
            Discipline::Unknown
        } else {
            Discipline::ALL[best]
        }
    }
}

impl fmt::Display for Discipline {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guesses_from_file_name() {
        assert_eq!(
            Discipline::guess("Building-Hvac.ifc", []),
            Discipline::Mechanical
        );
        assert_eq!(
            Discipline::guess("Building-Structural.ifc", []),
            Discipline::Structural
        );
        assert_eq!(
            Discipline::guess("C:\\proj\\K_Hus.ifc", []),
            Discipline::Structural
        );
        assert_eq!(
            Discipline::guess("ARK_model.ifc", []),
            Discipline::Architectural
        );
        assert_eq!(Discipline::guess("VVS-stue.ifc", []), Discipline::Plumbing);
        assert_eq!(
            Discipline::guess("Projekt_EL.ifc", []),
            Discipline::Electrical
        );
    }

    #[test]
    fn falls_back_to_element_classes() {
        let classes = ["IfcBeam", "IfcBeam", "IfcColumn", "IfcDoor"];
        assert_eq!(
            Discipline::guess("model.ifc", classes),
            Discipline::Structural
        );
        assert_eq!(Discipline::guess("model.ifc", []), Discipline::Unknown);
    }
}
